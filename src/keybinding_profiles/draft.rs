//! Comment-preserving profile drafts. No filesystem writes until Settings Save.
use super::{Profile, path};
use crate::commands::CommandId;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Draft {
    pub path: PathBuf,
    pub original: String,
    pub text: String,
    pub profile: Profile,
}
impl Draft {
    pub fn load(config: &Path, reference: &str) -> Result<Self, String> {
        let path = path(config, reference)?
            .canonicalize()
            .map_err(|e| format!("Could not resolve keyboard profile: {e}"))?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        let profile = Profile::parse(&text)?;
        Ok(Self {
            path,
            original: text.clone(),
            text,
            profile,
        })
    }
    pub fn changed(&self) -> bool {
        self.text != self.original
    }
    /// None removes both common and native-platform overrides; other platforms
    /// and toolkit action/context rules are left intact.
    pub fn remap(&self, command: CommandId, bindings: Option<&[String]>) -> Result<Self, String> {
        let mut doc: toml_edit::DocumentMut = self
            .text
            .parse()
            .map_err(|e| format!("Could not edit keyboard profile: {e}"))?;
        let key = command.key();
        let os = std::env::consts::OS;
        if doc
            .get("platform")
            .and_then(|p| p.get(os))
            .and_then(|p| p.get(&key))
            .is_some()
        {
            // Normalize inline tables so commenting one assignment cannot remove
            // another command. Keep the old value/comments as a reset example.
            let platform = crate::settings_reload::standard_section(&mut doc, "platform");
            crate::settings_reload::standard_table(platform, os);
            let text =
                crate::preferences::remove(&doc.to_string(), &format!("platform.{os}.{key}"))?;
            doc = text
                .parse()
                .map_err(|e| format!("Could not edit keyboard profile: {e}"))?;
        }
        let table = crate::settings_reload::standard_section(&mut doc, "keybindings");
        if let Some(bindings) = bindings {
            let mut value = toml_edit::Value::Array(bindings.iter().map(String::as_str).collect());
            if let Some(previous) = table.get(&key).and_then(toml_edit::Item::as_value) {
                *value.decor_mut() = previous.decor().clone();
            }
            table[&key] = toml_edit::Item::Value(value);
        }
        let text = if bindings.is_some() {
            doc.to_string()
        } else {
            crate::preferences::remove(&doc.to_string(), &format!("keybindings.{key}"))?
        };
        let profile = Profile::parse(&text)?;
        Ok(Self {
            text,
            profile,
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Draft;
    use crate::commands::CommandId;
    #[test]
    fn drafts_preserve_comments_other_platforms_and_toolkit_rules_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.toml");
        let original = format!(
            "# mine\n[keybindings]\nfind_file=['alt-p'] # keep\n[platform.{}]\nfind_file=['alt-shift-p']\n[platform.other]\nfind_file=['ctrl-p']\n",
            std::env::consts::OS
        );
        std::fs::write(&path, &original).unwrap();
        let draft = Draft::load(&dir.path().join("config.toml"), "keys.toml").unwrap();
        let edit = draft
            .remap(CommandId::FindFile, Some(&["alt-shift-j".into()]))
            .unwrap();
        assert_eq!(
            edit.profile.keybindings[&CommandId::FindFile],
            ["alt-shift-j"]
        );
        assert!(edit.text.contains("# keep"));
        assert!(edit.text.contains("[platform.other]"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let reset = edit.remap(CommandId::FindFile, None).unwrap();
        assert!(reset.text.contains("# keep"));
        assert_eq!(
            reset.profile.keybindings[&CommandId::FindFile],
            super::super::default_bindings()[&CommandId::FindFile]
        );
        assert!(
            draft
                .remap(CommandId::FindFile, Some(&["secondary-b".into()]))
                .is_err()
        );
    }
}
