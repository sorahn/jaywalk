//! GitHub's manifest bootstrap creates an app owned by the approving developer.
//! It is separate from a user's authorization of an existing Jaywalk app.
use crate::{Result, callback, check_cancelled, delete, entry, json, open_browser, random_string};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const ACCOUNT: &str = "github-development-registration";
const HOMEPAGE: &str = "https://github.com/sorahn/jaywalk";

#[derive(Serialize, Deserialize)]
struct Registration {
    id: u64,
    client_id: String,
    slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
}
impl Registration {
    fn parse(value: &Value) -> Result<Self> {
        let parsed: Self = serde_json::from_value(value.clone())
            .map_err(|_| "GitHub returned an incomplete app registration; details suppressed")?;
        if parsed.id == 0
            || parsed.client_id.is_empty()
            || parsed.client_id.len() > 256
            || !parsed
                .client_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.')
            || parsed.slug.is_empty()
            || parsed.slug.len() > 256
            || !parsed
                .slug
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("GitHub returned invalid registration identifiers".into());
        }
        if parsed.client_secret.as_ref().is_some_and(|secret| {
            secret.is_empty() || secret.len() > 4096 || secret.chars().any(char::is_control)
        }) {
            return Err("GitHub returned an invalid app client secret".into());
        }
        let permissions = value
            .get("permissions")
            .and_then(Value::as_object)
            .ok_or("GitHub registration is missing permissions")?;
        if permissions.get("metadata") != Some(&json!("read"))
            || permissions.get("pull_requests") != Some(&json!("read"))
            || permissions.iter().any(|(name, level)| {
                !matches!(
                    name.as_str(),
                    "metadata" | "pull_requests" | "contents" | "checks" | "statuses"
                ) || level != "read"
            })
        {
            return Err(
                "Created app has unexpected permissions; inspect it in GitHub settings".into(),
            );
        }
        Ok(parsed)
    }
    fn settings_url(&self) -> String {
        format!("https://github.com/settings/apps/{}", self.slug)
    }
    fn install_url(&self) -> String {
        format!("https://github.com/apps/{}/installations/new", self.slug)
    }
}

pub(crate) fn register(client: &Client, cancelled: &AtomicBool) -> Result<()> {
    match entry(ACCOUNT)?.get_password() {
        Err(keyring::Error::NoEntry) => (),
        Ok(_) => return Err("A development registration is already saved; use github-setup or github-connect. Use github-forget-registration only to forget it locally.".into()),
        Err(_) => return Err("Cannot access OS credential store; unlock it before registering".into()),
    }
    let listener = callback::Listener::bind_registration()?;
    let origin = listener.origin()?;
    let state = random_string();
    let page_path = format!("/register/{}", random_string());
    let name = format!("Jaywalk Dev {}", &random_string()[..10]);
    let manifest = manifest(&name, &format!("{origin}/callback"));
    let html = page(&manifest, &state)?;
    println!(
        "Opening GitHub development registration. Confirm the app name and read-only permissions in your browser."
    );
    open_browser(&format!("{origin}{page_path}"))?;
    let code = listener.receive_with_page(
        &state,
        Duration::from_secs(3600),
        cancelled,
        Some((&page_path, &html)),
    )?;
    check_cancelled(cancelled)?;
    complete(client, &code)
}

pub(crate) fn recover(client: &Client) -> Result<()> {
    // This is a deliberate developer recovery operation, not an automatic callback.
    // The old listener's state is gone; the developer explicitly supplies their own
    // browser return URL via hidden terminal input or a pipe, never argv or logs.
    match entry(ACCOUNT)?.get_password() {
        Err(keyring::Error::NoEntry) => (),
        Ok(_) => return Err("Registration is already saved; use github-setup".into()),
        Err(_) => {
            return Err("Cannot access OS credential store; unlock it before recovery".into());
        }
    }
    use std::io::{IsTerminal, Read};
    let input = if std::io::stdin().is_terminal() {
        rpassword::prompt_password("Paste your own GitHub return URL (hidden): ")
            .map_err(|_| "Cannot read recovery URL")?
    } else {
        let mut input = String::new();
        std::io::stdin()
            .take(8193)
            .read_to_string(&mut input)
            .map_err(|_| "Cannot read recovery URL from stdin")?;
        input
    };
    let code = recovery_code(input.trim())?;
    complete(client, &code)
}

fn recovery_code(input: &str) -> Result<String> {
    if input.len() > 8192 {
        return Err("Recovery URL exceeds size limit".into());
    }
    let url = reqwest::Url::parse(input).map_err(|_| "Invalid recovery URL")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path() != "/callback"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Recovery expects your own loopback registration callback URL".into());
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    if pairs.len() != 2
        || pairs
            .iter()
            .filter(|(key, value)| key == "state" && !value.is_empty())
            .count()
            != 1
    {
        return Err("Recovery URL must have exactly one code and one state".into());
    }
    let codes: Vec<_> = pairs
        .iter()
        .filter(|(key, value)| key == "code" && !value.is_empty() && value.len() <= 2048)
        .collect();
    if codes.len() != 1 {
        return Err("Recovery URL missing a unique code".into());
    }
    Ok(codes[0].1.to_string())
}

fn complete(client: &Client, code: &str) -> Result<()> {
    let url = conversion_url(code)?;
    // No token/private key required: the temporary manifest code authorizes this conversion.
    let response = json(client.post(url))?;
    // Keep browser client credentials only in the OS store. Generated private keys
    // and webhook secrets are deliberately not persisted; no secrets are printed.
    let registration = Registration::parse(&response)?;
    let encoded = serde_json::to_string(&registration).map_err(|_| "Cannot encode registration")?;
    // Conversion is one-shot: save before any optional browser operation can fail.
    entry(ACCOUNT)?.set_password(&encoded)
        .map_err(|_| "App created but local save failed. Find it in GitHub Developer settings; use its client ID with github-connect.")?;
    println!("Development app registered; client ID saved in OS credential store.");
    println!(
        "Use github-install to choose test repositories, then github-connect for browser sign-in. Device Flow is not required. This setup is for the app developer, not ordinary users."
    );
    Ok(())
}

fn manifest(name: &str, redirect_url: &str) -> Value {
    json!({
        "name": name,
        "url": HOMEPAGE,
        "description": "Local Jaywalk read-only hosting authentication experiment",
        "redirect_url": redirect_url,
        "callback_urls": [crate::github_browser::REDIRECT],
        "public": false,
        // GitHub validates public reachability even for an inactive hook. This
        // existing project URL is a placeholder only; no events or delivery enabled.
        "hook_attributes": {"url": HOMEPAGE, "active": false},
        "default_permissions": {"metadata": "read", "pull_requests": "read", "contents":"read", "checks":"read", "statuses":"read"},
        "default_events": [],
        "request_oauth_on_install": false
    })
}
fn conversion_url(code: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse("https://api.github.com/app-manifests/")
        .map_err(|_| "Invalid built-in conversion URL")?;
    url.path_segments_mut()
        .map_err(|_| "Invalid conversion URL path")?
        .pop_if_empty()
        .push(code)
        .push("conversions");
    Ok(url)
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn page(manifest: &Value, state: &str) -> Result<String> {
    let encoded =
        serde_json::to_string(manifest).map_err(|_| "Cannot encode GitHub App manifest")?;
    Ok(format!(
        r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Jaywalk development setup</title>
<style>body{{font:17px system-ui;max-width:38rem;margin:12vh auto;padding:24px;color-scheme:light dark}}button{{font:inherit;padding:12px 18px;cursor:pointer}}p{{line-height:1.5}}</style>
<h1>Set up a Jaywalk development app</h1>
<p>This creates a private GitHub App owned by your account, with read-only repository metadata and pull request access. Webhooks are disabled.</p>
<p>GitHub will let you confirm the name. After registration, return here automatically; the probe saves development client credentials in the OS credential store. Repository installation and account sign-in follow separately.</p>
<form action="https://github.com/settings/apps/new?state={}" method="post"><input type="hidden" name="manifest" value="{}"><button type="submit">Continue to GitHub</button></form>
<p>Keep the probe running until registration finishes.</p></html>"#,
        escape(state),
        escape(&encoded)
    ))
}
fn load() -> Result<Registration> {
    let encoded = entry(ACCOUNT)?.get_password().map_err(
        |_| "No saved development registration; run github-register first or unlock the OS store",
    )?;
    // Revalidate persisted identifiers before constructing browser URLs.
    let saved: Registration = serde_json::from_str(&encoded)
        .map_err(|_| "Invalid saved registration; forget it locally and register again")?;
    Registration::parse(
        &json!({"id":saved.id,"client_id":saved.client_id,"slug":saved.slug,"client_secret":saved.client_secret,
        "permissions":{"metadata":"read","pull_requests":"read"}}),
    )
}
pub(crate) fn client_id() -> Result<String> {
    Ok(load()?.client_id)
}
pub(crate) fn browser_secret(client_id: &str) -> Result<Option<String>> {
    match entry(ACCOUNT)?.get_password() {
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Cannot read developer registration; unlock the OS store".into()),
        Ok(_) => {
            let registration = load()?;
            Ok((registration.client_id == client_id)
                .then_some(registration.client_secret)
                .flatten())
        }
    }
}

pub(crate) fn remember_browser_secret(client_id: &str, secret: &str) -> Result<()> {
    match entry(ACCOUNT)?.get_password() {
        Err(keyring::Error::NoEntry) => return Ok(()),
        Err(_) => {
            return Err("Connected, but cannot cache developer setup; unlock the OS store".into());
        }
        Ok(_) => (),
    }
    let mut registration = load()?;
    if registration.client_id != client_id || registration.client_secret.as_deref() == Some(secret)
    {
        return Ok(());
    }
    registration.client_secret = Some(secret.into());
    let encoded = serde_json::to_string(&registration).map_err(|_| "Cannot encode registration")?;
    entry(ACCOUNT)?
        .set_password(&encoded)
        .map_err(|_| "Connected, but cannot cache developer setup in the OS store".into())
}

pub(crate) fn open_settings() -> Result<()> {
    println!(
        "Developer setup only: add {} as a Redirect URI under General > Identifying and authorizing users and save. Device Flow is needed only for github-connect --flow device. Existing registrations can generate a client secret here and enter it at the probe's hidden prompt.",
        crate::github_browser::REDIRECT
    );
    open_browser(&load()?.settings_url())
}
pub(crate) fn open_installation() -> Result<()> {
    open_browser(&load()?.install_url())
}
pub(crate) fn forget() -> Result<()> {
    delete(&entry(ACCOUNT)?)?;
    println!(
        "Local development registration credentials removed. The GitHub App still exists; delete it in GitHub Developer settings if no longer needed."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_is_private_read_only_and_has_no_active_hooks() {
        let manifest = manifest("test", "http://127.0.0.1:1234/callback");
        assert_eq!(manifest["public"], false);
        assert_eq!(manifest["hook_attributes"]["active"], false);
        assert_eq!(manifest["hook_attributes"]["url"], HOMEPAGE);
        assert_eq!(manifest["redirect_url"], "http://127.0.0.1:1234/callback");
        assert_eq!(
            manifest["callback_urls"],
            json!([crate::github_browser::REDIRECT])
        );
        assert_ne!(manifest["redirect_url"], manifest["callback_urls"][0]);
        assert_eq!(manifest["default_events"], json!([]));
        assert_eq!(
            manifest["default_permissions"],
            json!({"metadata":"read","pull_requests":"read","contents":"read","checks":"read","statuses":"read"})
        );
        assert!(manifest.get("client_secret").is_none());
    }
    #[test]
    fn form_escapes_manifest_instead_of_interpreting_html() {
        let manifest = manifest("\"><script>bad</script>", "http://127.0.0.1:1234/callback");
        let html = page(&manifest, "state").unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains(
            "action=\"https://github.com/settings/apps/new?state=state\" method=\"post\""
        ));
    }
    #[test]
    fn response_keeps_browser_credentials_but_excludes_private_keys_and_webhook_secrets() {
        let mut value = json!({"id":123,"client_id":"Iv1.synthetic","slug":"synthetic-app","pem":"SECRET","client_secret":"SECRET","webhook_secret":"SECRET", "permissions":{"metadata":"read","pull_requests":"read"}});
        let registration = Registration::parse(&value).unwrap();
        let encoded: Value =
            serde_json::from_str(&serde_json::to_string(&registration).unwrap()).unwrap();
        assert_eq!(encoded["client_secret"], "SECRET");
        assert!(encoded.get("pem").is_none());
        assert!(encoded.get("webhook_secret").is_none());
        value["permissions"]["contents"] = json!("write");
        assert!(Registration::parse(&value).is_err());
        value["permissions"]
            .as_object_mut()
            .unwrap()
            .remove("contents");
        value["slug"] = json!("evil/../../somewhere");
        assert!(Registration::parse(&value).is_err());
    }
    #[test]
    fn manual_recovery_accepts_only_explicit_loopback_callbacks_without_duplicates() {
        assert_eq!(
            recovery_code("http://127.0.0.1:1234/callback?code=synthetic&state=approved").unwrap(),
            "synthetic"
        );
        for url in [
            "https://evil.example/callback?code=x&state=y",
            "http://user@127.0.0.1:1234/callback?code=x&state=y",
            "http://127.0.0.1:1234/wrong?code=x&state=y",
            "http://127.0.0.1:1234/callback?code=x&code=y",
            "http://127.0.0.1:1234/callback?code=x&state=y&state=z",
            "http://127.0.0.1:1234/callback?code=x&state=y#fragment",
        ] {
            assert!(recovery_code(url).is_err());
        }
    }
    #[test]
    fn opaque_code_cannot_change_conversion_origin_or_path() {
        let url = conversion_url("x/y?token=z#fragment").unwrap();
        assert_eq!(url.host_str(), Some("api.github.com"));
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
        assert_eq!(
            url.path(),
            "/app-manifests/x%2Fy%3Ftoken=z%23fragment/conversions"
        );
    }
}
