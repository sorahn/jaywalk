//! Developer-owned Bitbucket registration and browser connection feasibility probe.
use crate::{
    BITBUCKET_TOKEN, Provider, Result, Session, callback, check_cancelled, delete, entry, json,
    load, open_browser, protocol::Tokens, random_string, valid_component,
};
use reqwest::{Url, blocking::Client};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::atomic::AtomicBool, time::Duration};

const REGISTRATION: &str = "bitbucket-development-registration";
const WORKSPACES: &str = "https://api.bitbucket.org/2.0/user/workspaces";

// Deliberately contains no user tokens; disconnecting an account retains publisher setup.
#[derive(Serialize, Deserialize)]
struct Registration {
    client_id: String,
    secret: String,
}

impl Registration {
    fn validate(&self) -> Result<()> {
        if self.client_id.trim().is_empty() || self.secret.trim().is_empty() {
            return Err("Development client ID and secret must both be nonempty".into());
        }
        Ok(())
    }
}

pub(crate) fn setup(client_id: Option<&str>, from_session: bool) -> Result<()> {
    let registration = if from_session {
        let session = load(Provider::Bitbucket)?;
        Registration {
            client_id: session.client_id,
            secret: session
                .developer_secret
                .ok_or("Existing session has no development secret")?,
        }
    } else {
        let client_id = client_id.ok_or("Provide a client ID or --from-session")?;
        let secret = rpassword::prompt_password("Bitbucket development client secret (hidden): ")
            .map_err(|_| "Cannot read secret from the terminal")?;
        Registration {
            client_id: client_id.into(),
            secret,
        }
    };
    registration.validate()?;
    let encoded = serde_json::to_string(&registration)
        .map_err(|_| "Cannot encode development registration")?;
    entry(REGISTRATION)?
        .set_password(&encoded)
        .map_err(|_| "Cannot save development registration in OS store")?;
    println!("Development client saved separately; account disconnect will retain this setup.");
    Ok(())
}

pub(crate) fn forget_registration() -> Result<()> {
    delete(&entry(REGISTRATION)?)?;
    println!(
        "Local development registration removed; account session and online client unchanged."
    );
    Ok(())
}

fn registration() -> Result<Registration> {
    let encoded = entry(REGISTRATION)?.get_password().map_err(
        |_| "Cannot load development client; run bitbucket-setup first or unlock the OS store",
    )?;
    let registration: Registration = serde_json::from_str(&encoded)
        .map_err(|_| "Invalid development registration; run bitbucket-setup again")?;
    registration.validate()?;
    Ok(registration)
}

pub(crate) fn connect(client: &Client, cancelled: &AtomicBool) -> Result<Session> {
    let registration = registration()?;
    let listener = callback::Listener::bind()?;
    check_cancelled(cancelled)?;
    let state = random_string();
    let mut url = Url::parse("https://bitbucket.org/site/oauth2/authorize")
        .map_err(|_| "Invalid built-in authorization URL")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", registration.client_id.as_str()),
        ("response_type", "code"),
        ("state", state.as_str()),
        ("redirect_uri", callback::REDIRECT),
    ]);
    println!("Opening Bitbucket sign-in in your browser (Ctrl+C cancels).");
    open_browser(url.as_str())?;
    let code = listener.receive(&state, Duration::from_secs(3600), cancelled)?;
    check_cancelled(cancelled)?;
    let response = json(
        client
            .post(BITBUCKET_TOKEN)
            .basic_auth(&registration.client_id, Some(&registration.secret))
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("redirect_uri", callback::REDIRECT),
            ]),
    )?;
    check_cancelled(cancelled)?;
    Ok(Session {
        client_id: registration.client_id,
        tokens: Tokens::parse(&response)?,
        developer_secret: Some(registration.secret),
        // A replacement may belong to another account; do not inherit its selection.
        workspace: None,
    })
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Workspace {
    pub(crate) uuid: String,
    pub(crate) slug: String,
}

#[derive(Deserialize)]
struct WorkspaceAccess {
    workspace: Workspace,
}

#[derive(Deserialize)]
struct Page {
    values: Vec<WorkspaceAccess>,
    next: Option<String>,
}

fn parse_page(value: serde_json::Value) -> Result<Page> {
    let page: Page =
        serde_json::from_value(value).map_err(|_| "Malformed workspace discovery response")?;
    for access in &page.values {
        if access.workspace.uuid.is_empty() || !valid_component(&access.workspace.slug) {
            return Err("Workspace response missing a valid ID or slug".into());
        }
    }
    Ok(page)
}

fn page_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| "Invalid workspace pagination URL")?;
    if url.scheme() != "https"
        || url.host_str() != Some("api.bitbucket.org")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != "/2.0/user/workspaces"
    {
        return Err("Rejected workspace pagination URL outside the discovery endpoint".into());
    }
    Ok(url)
}

pub(crate) fn workspaces(
    client: &Client,
    session: &Session,
    cancelled: &AtomicBool,
) -> Result<Vec<Workspace>> {
    let mut next = Some(format!("{WORKSPACES}?pagelen=20"));
    let mut visited = HashSet::new();
    let mut ids = HashSet::new();
    let mut workspaces = Vec::new();
    while let Some(value) = next {
        check_cancelled(cancelled)?;
        let url = page_url(&value)?;
        if visited.len() >= 100 || !visited.insert(url.to_string()) {
            return Err("Workspace pagination repeated or exceeded the probe limit".into());
        }
        let page = parse_page(json(
            client.get(url).bearer_auth(&session.tokens.access_token),
        )?)?;
        for access in page.values {
            if ids.insert(access.workspace.uuid.clone()) {
                workspaces.push(access.workspace);
            }
        }
        next = page.next;
    }
    check_cancelled(cancelled)?;
    workspaces.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(workspaces)
}

pub(crate) fn select_workspace(workspaces: Vec<Workspace>, slug: &str) -> Result<Workspace> {
    workspaces
        .into_iter()
        .find(|workspace| workspace.slug == slug)
        .ok_or("Choose a workspace returned by the current account's discovery".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pagination_never_sends_bearer_credentials_to_another_endpoint() {
        assert!(page_url(&format!("{WORKSPACES}?page=2")).is_ok());
        for url in [
            "https://example.com/2.0/user/workspaces",
            "http://api.bitbucket.org/2.0/user/workspaces",
            "https://api.bitbucket.org:444/2.0/user/workspaces",
            "https://user:pass@api.bitbucket.org/2.0/user/workspaces",
            "https://api.bitbucket.org/2.0/user/workspaces#fragment",
            "https://api.bitbucket.org/2.0/repositories",
        ] {
            assert!(page_url(url).is_err());
        }
    }

    #[test]
    fn discovery_requires_complete_entries_and_selection_uses_account_results() {
        let page = parse_page(
            json!({"values":[{"workspace":{"uuid":"{synthetic}","slug":"example-team"}}]}),
        )
        .unwrap();
        assert!(select_workspace(vec![], "example-team").is_err());
        assert!(
            select_workspace(
                page.values.into_iter().map(|a| a.workspace).collect(),
                "example-team"
            )
            .is_ok()
        );
        for value in [
            json!({}),
            json!({"values":[{}]}),
            json!({"values":[{"workspace":{"uuid":"id","slug":"../escape"}}]}),
        ] {
            assert!(parse_page(value).is_err());
        }
    }

    #[test]
    fn registration_contains_no_session_credentials_and_requires_both_fields() {
        let registration = Registration {
            client_id: "synthetic-client".into(),
            secret: "synthetic-secret".into(),
        };
        registration.validate().unwrap();
        let encoded = serde_json::to_value(&registration).unwrap();
        assert_eq!(encoded.as_object().unwrap().len(), 2);
        assert!(encoded.get("tokens").is_none());
        assert!(
            Registration {
                client_id: " ".into(),
                secret: "secret".into()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn older_sessions_load_without_workspace_and_refresh_keeps_selection() {
        let mut session: Session = serde_json::from_value(json!({
            "client_id":"synthetic", "developer_secret":"secret",
            "tokens":{"access_token":"old","refresh_token":"refresh","expires_in":3600}
        }))
        .unwrap();
        assert!(session.workspace.is_none());
        session.workspace = Some(Workspace {
            uuid: "{synthetic}".into(),
            slug: "example-team".into(),
        });
        session.tokens =
            Tokens::parse(&json!({"access_token":"new","token_type":"bearer"})).unwrap();
        let recovered: Session =
            serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
        assert_eq!(recovered.workspace.unwrap().slug, "example-team");
    }
}
