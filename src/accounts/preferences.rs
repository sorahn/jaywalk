//! Public connection preferences. Tokens and client secrets remain in the OS store.
use super::{AccountKey, Authentication, Provider, StoredSession, Workspace};
use serde::Deserialize;
use std::{collections::HashSet, io::Write, path::Path};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    pub provider: Provider,
    pub id: String,
    #[serde(default)]
    pub authentication: Authentication,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub workspace: Option<Workspace>,
}
impl AccountConfig {
    pub fn key(&self) -> AccountKey {
        AccountKey {
            provider: self.provider,
            subject: self.id.clone(),
        }
    }
    pub(super) fn from_session(session: &StoredSession) -> Self {
        Self {
            provider: session.identity.key.provider,
            id: session.identity.key.subject.clone(),
            name: Some(session.identity.label.clone()),
            authentication: session.authentication,
            client_id: session.client_id.clone(),
            workspace: session.workspace.clone(),
        }
    }
}
pub(crate) fn validate(entries: &[AccountConfig]) -> Result<(), String> {
    if entries.len() > 256 {
        return Err("accounts must contain at most 256 entries".into());
    }
    let mut seen = HashSet::new();
    for entry in entries {
        for (name, value) in [
            ("id", Some(entry.id.as_str())),
            ("name", entry.name.as_deref()),
            ("client_id", entry.client_id.as_deref()),
        ] {
            if let Some(value) = value
                && (value.trim().is_empty()
                    || value.len() > 256
                    || value.chars().any(char::is_control))
            {
                return Err(format!(
                    "accounts.{name} must contain 1–256 bytes without control characters"
                ));
            }
        }
        if !seen.insert(entry.key()) {
            return Err("accounts contains duplicate provider/id entries".into());
        }
        if entry.authentication == Authentication::Token && entry.client_id.is_some() {
            return Err("token accounts do not use an OAuth client_id".into());
        }
        if let Some(workspace) = &entry.workspace {
            if entry.provider != Provider::Bitbucket {
                return Err("accounts.workspace is only supported for Bitbucket".into());
            }
            workspace
                .validate()
                .map_err(|_| "Invalid accounts.workspace".to_owned())?;
        }
    }
    Ok(())
}
fn table(entry: &AccountConfig) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    table["provider"] = toml_edit::value(entry.provider.name());
    table["id"] = toml_edit::value(entry.id.clone());
    table["authentication"] = toml_edit::value(if entry.authentication == Authentication::Token {
        "token"
    } else {
        "oauth"
    });
    if let Some(name) = &entry.name {
        table["name"] = toml_edit::value(name.clone());
    }
    if let Some(id) = &entry.client_id {
        table["client_id"] = toml_edit::value(id.clone());
    }
    if let Some(workspace) = &entry.workspace {
        table["workspace"] = workspace_value(workspace);
    }
    table
}
fn workspace_value(workspace: &Workspace) -> toml_edit::Item {
    let mut value = toml_edit::InlineTable::new();
    value.insert("uuid", workspace.uuid.clone().into());
    value.insert("slug", workspace.slug.clone().into());
    toml_edit::Item::Value(toml_edit::Value::InlineTable(value))
}
/// Append newly connected accounts without rewriting existing preferences or comments.
/// An explicit workspace selection is the only operation that updates an existing row.
pub(super) fn synchronize(
    path: &Path,
    known: &[AccountConfig],
    workspace: Option<(&AccountKey, &Workspace)>,
) -> Result<Vec<AccountConfig>, String> {
    let _writer = crate::settings_reload::CONFIG_WRITER
        .lock()
        .expect("config writer mutex poisoned");
    let target = std::fs::canonicalize(path)
        .map_err(|error| format!("Could not resolve account config: {error}"))?;
    let original = std::fs::read_to_string(&target)
        .map_err(|error| format!("Could not read account config: {error}"))?;
    let config = crate::settings::Config::parse(&original)?;
    let mut document: toml_edit::DocumentMut = original
        .parse()
        .map_err(|error| format!("Could not edit account config: {error}"))?;
    let missing = known
        .iter()
        .filter(|entry| !config.accounts.iter().any(|old| old.key() == entry.key()))
        .collect::<Vec<_>>();
    let changed_auth = known.iter().any(|entry| {
        config
            .accounts
            .iter()
            .any(|old| old.key() == entry.key() && old.authentication != entry.authentication)
    });
    if missing.is_empty() && workspace.is_none() && !changed_auth {
        return Ok(config.accounts);
    }
    if document
        .get("accounts")
        .is_none_or(|item| item.as_array().is_some_and(|array| array.is_empty()))
    {
        document["accounts"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let rows = document["accounts"]
        .as_array_of_tables_mut()
        .ok_or("accounts must use [[accounts]] tables when Jaywalk adds connection metadata")?;
    for entry in missing {
        rows.push(table(entry));
    }
    for entry in known {
        if let Some(old) = config.accounts.iter().find(|old| old.key() == entry.key())
            && old.authentication != entry.authentication
            && let Some(row) = rows.iter_mut().find(|row| {
                row.get("provider").and_then(toml_edit::Item::as_str) == Some(entry.provider.name())
                    && row.get("id").and_then(toml_edit::Item::as_str) == Some(&entry.id)
            })
        {
            row["authentication"] =
                toml_edit::value(if entry.authentication == Authentication::Token {
                    "token"
                } else {
                    "oauth"
                });
            if let Some(id) = &entry.client_id {
                row["client_id"] = toml_edit::value(id.clone());
            } else {
                row.remove("client_id");
            }
        }
    }
    if let Some((key, value)) = workspace {
        let row = rows
            .iter_mut()
            .find(|row| {
                row.get("provider").and_then(toml_edit::Item::as_str) == Some(key.provider.name())
                    && row.get("id").and_then(toml_edit::Item::as_str) == Some(&key.subject)
            })
            .ok_or("The account is no longer configured")?;
        let mut replacement = workspace_value(value);
        if let Some(decor) = row
            .get("workspace")
            .and_then(toml_edit::Item::as_value)
            .map(|value| value.decor().clone())
        {
            *replacement
                .as_value_mut()
                .expect("workspace is an inline value")
                .decor_mut() = decor;
        }
        row["workspace"] = replacement;
    }
    let updated = document.to_string();
    let entries = crate::settings::Config::parse(&updated)?.accounts;
    if original == updated {
        return Ok(entries);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(
        target
            .parent()
            .ok_or("Account config has no parent directory")?,
    )
    .map_err(|error| format!("Could not create account config temporary file: {error}"))?;
    temporary
        .as_file()
        .set_permissions(
            std::fs::metadata(&target)
                .map_err(|error| error.to_string())?
                .permissions(),
        )
        .map_err(|error| error.to_string())?;
    temporary
        .write_all(updated.as_bytes())
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    if std::fs::read_to_string(&target).map_err(|error| error.to_string())? != original {
        return Err("Config changed while updating account metadata; keeping your edit".into());
    }
    temporary
        .persist(&target)
        .map_err(|error| format!("Could not save account config: {}", error.error))?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(provider: Provider, id: &str) -> AccountConfig {
        AccountConfig {
            provider,
            authentication: Authentication::Oauth,
            id: id.into(),
            name: Some(format!("Display {id}")),
            client_id: Some("public-client".into()),
            workspace: None,
        }
    }
    #[test]
    fn synchronization_appends_metadata_and_preserves_order_comments_and_aliases() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        let original = "# keep this\n[editor]\nfont_family='Example' # font\n\n[[accounts]]\nprovider='github'\nid='second'\nname='Work' # my label\nclient_id='custom-client'\n";
        std::fs::write(&path, original).unwrap();
        let entries = synchronize(
            &path,
            &[
                entry(Provider::Github, "first"),
                entry(Provider::Github, "second"),
            ],
            None,
        )
        .unwrap();
        assert_eq!(
            entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert_eq!(entries[0].name.as_deref(), Some("Work"));
        assert_eq!(entries[0].client_id.as_deref(), Some("custom-client"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains(original));
        synchronize(&path, &[entry(Provider::Github, "first")], None).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            saved,
            "no churn or removal of disconnected preferences"
        );
        #[cfg(unix)]
        {
            let alias = fixture.path().join("alias.toml");
            std::os::unix::fs::symlink(&path, &alias).unwrap();
            synchronize(&alias, &[entry(Provider::Bitbucket, "third")], None).unwrap();
            assert!(alias.is_symlink());
        }
    }
    #[test]
    fn empty_defaults_convert_to_tables_and_workspace_edits_preserve_other_values() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path, "accounts = []\n[ui]\nfont_size=15\n").unwrap();
        let account = entry(Provider::Bitbucket, "person");
        let workspace = Workspace {
            uuid: "{team}".into(),
            slug: "team".into(),
        };
        synchronize(
            &path,
            std::slice::from_ref(&account),
            Some((&account.key(), &workspace)),
        )
        .unwrap();
        let config =
            crate::settings::Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config.accounts[0].workspace, Some(workspace));
        assert_eq!(config.accounts[0].client_id, account.client_id);
        assert_eq!(config.ui.font_size, 15.0);
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("access_token")
        );
    }
    #[test]
    fn switching_authentication_updates_binding_without_changing_alias_or_order() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        std::fs::write(&path,"# keep\n[[accounts]]\nprovider='github'\nid='one'\nname='Work' # alias\nclient_id='old-client'\n").unwrap();
        let mut token = entry(Provider::Github, "one");
        token.authentication = Authentication::Token;
        token.client_id = None;
        let entries = synchronize(&path, &[token], None).unwrap();
        assert_eq!(entries[0].authentication, Authentication::Token);
        assert_eq!(entries[0].name.as_deref(), Some("Work"));
        assert!(entries[0].client_id.is_none());
        assert!(std::fs::read_to_string(&path).unwrap().contains("# alias"));
        let browser = entry(Provider::Github, "one");
        let entries = synchronize(&path, &[browser], None).unwrap();
        assert_eq!(entries[0].authentication, Authentication::Oauth);
        assert_eq!(entries[0].client_id.as_deref(), Some("public-client"));
        assert_eq!(entries[0].name.as_deref(), Some("Work"));
    }
    #[test]
    fn invalid_configuration_is_never_overwritten_and_sensitive_fields_are_rejected() {
        let fixture = tempfile::tempdir().unwrap();
        let path = fixture.path().join("config.toml");
        let invalid = "[ui]\nfont_size =";
        std::fs::write(&path, invalid).unwrap();
        assert!(synchronize(&path, &[entry(Provider::Github, "first")], None).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
        for field in ["password", "access_token", "refresh_token", "client_secret"] {
            assert!(
                crate::settings::Config::parse(&format!(
                    "[[accounts]]\nprovider='github'\nid='one'\n{field}='secret'\n"
                ))
                .is_err()
            );
        }
        let duplicate =
            "[[accounts]]\nprovider='github'\nid='one'\n[[accounts]]\nprovider='github'\nid='one'";
        assert!(crate::settings::Config::parse(duplicate).is_err());
        assert!(crate::settings::Config::parse("[hosting]\nconcurrent_accounts=0").is_err());
    }
}
