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
        let permissions = value
            .get("permissions")
            .and_then(Value::as_object)
            .ok_or("GitHub registration is missing permissions")?;
        if permissions.get("metadata") != Some(&json!("read"))
            || permissions.get("pull_requests") != Some(&json!("read"))
            || permissions.iter().any(|(name, level)| {
                !matches!(name.as_str(), "metadata" | "pull_requests") || level != "read"
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
        Duration::from_secs(900),
        cancelled,
        Some((&page_path, &html)),
    )?;
    check_cancelled(cancelled)?;
    let url = conversion_url(&code)?;
    // No token/private key required: the temporary manifest code authorizes this conversion.
    let response = json(client.post(url))?;
    // Keep only identifiers needed by device flow. Generated private key, client secret,
    // and webhook secret are deliberately not persisted or printed.
    let registration = Registration::parse(&response)?;
    let encoded = serde_json::to_string(&registration).map_err(|_| "Cannot encode registration")?;
    // Conversion is one-shot: save before any optional browser operation can fail.
    entry(ACCOUNT)?.set_password(&encoded)
        .map_err(|_| "App created but local save failed. Find it in GitHub Developer settings; use its client ID with github-connect.")?;
    println!("Development app registered; client ID saved in OS credential store.");
    println!(
        "In the app settings, enable Device Flow and save. Then use github-install to choose test repositories, followed by github-connect (no client ID argument needed)."
    );
    open_browser(&registration.settings_url())
}

fn manifest(name: &str, redirect_url: &str) -> Value {
    json!({
        "name": name,
        "url": HOMEPAGE,
        "description": "Local Jaywalk read-only hosting authentication experiment",
        "redirect_url": redirect_url,
        "public": false,
        // GitHub validates public reachability even for an inactive hook. This
        // existing project URL is a placeholder only; no events or delivery enabled.
        "hook_attributes": {"url": HOMEPAGE, "active": false},
        "default_permissions": {"metadata": "read", "pull_requests": "read"},
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
<p>GitHub will let you confirm the name. After registration, return here automatically; the probe saves the client ID locally. Repository installation and account sign-in follow separately.</p>
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
        &json!({"id":saved.id,"client_id":saved.client_id,"slug":saved.slug,
        "permissions":{"metadata":"read","pull_requests":"read"}}),
    )
}
pub(crate) fn client_id() -> Result<String> {
    Ok(load()?.client_id)
}
pub(crate) fn open_settings() -> Result<()> {
    open_browser(&load()?.settings_url())
}
pub(crate) fn open_installation() -> Result<()> {
    open_browser(&load()?.install_url())
}
pub(crate) fn forget() -> Result<()> {
    delete(&entry(ACCOUNT)?)?;
    println!(
        "Local registration identifiers removed. The GitHub App still exists; delete it in GitHub Developer settings if no longer needed."
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
        assert_eq!(manifest["default_events"], json!([]));
        assert_eq!(
            manifest["default_permissions"],
            json!({"metadata":"read","pull_requests":"read"})
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
    fn response_keeps_only_safe_identifiers_and_rejects_extra_permissions() {
        let mut value = json!({"id":123,"client_id":"Iv1.synthetic","slug":"synthetic-app","pem":"SECRET","client_secret":"SECRET","webhook_secret":"SECRET", "permissions":{"metadata":"read","pull_requests":"read"}});
        let registration = Registration::parse(&value).unwrap();
        assert!(
            !serde_json::to_string(&registration)
                .unwrap()
                .contains("SECRET")
        );
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
