//! Disposable protocol/credential-store probe, not the app's provider contract.
mod bitbucket;
mod callback;
mod github_browser;
mod protocol;
mod registration;

use clap::{Parser, Subcommand, ValueEnum};
use keyring::Entry;
use rand::RngCore;
use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use protocol::{Poll, Tokens};

type Result<T> = std::result::Result<T, String>;
const SERVICE: &str = "jaywalk.hosting-auth-poc.v1";
const GITHUB_TOKEN: &str = "https://github.com/login/oauth/access_token";
const BITBUCKET_TOKEN: &str = "https://bitbucket.org/site/oauth2/access_token";

#[derive(Parser)]
#[command(about = "Isolated browser-auth feasibility probe; never prints tokens")]
struct Args {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Browser sign-in by default; --flow device is the optional device-code experiment.
    GithubConnect {
        client_id: Option<String>,
        #[arg(long, value_enum, default_value = "browser")]
        flow: GithubFlow,
        /// Replace the existing session only after new authorization and identity checks succeed.
        #[arg(long)]
        reauthorize: bool,
    },
    /// Open browser-assisted development GitHub App registration and save its client ID.
    GithubRegister,
    /// Recover an explicitly supplied browser callback after the local listener timed out.
    GithubRecoverRegistration,
    /// Open one-time development registration settings (callback URL or optional device flow).
    GithubSetup,
    /// Open installation for the saved development app; select test repositories yourself.
    GithubInstall,
    /// Forget the saved development registration locally (does not delete the GitHub App).
    GithubForgetRegistration,
    /// One-time developer setup; keep the client registration separate from account sessions.
    BitbucketSetup {
        #[arg(
            required_unless_present = "from_session",
            conflicts_with = "from_session"
        )]
        client_id: Option<String>,
        /// Recover only app credentials from this probe's existing Bitbucket session.
        #[arg(long)]
        from_session: bool,
    },
    /// Browser sign-in using the saved development client; never prompts for its secret.
    BitbucketConnect {
        /// Keep the old session until replacement authorization and identity checks succeed.
        #[arg(long)]
        reauthorize: bool,
    },
    /// Forget only the local development registration; does not delete it online.
    BitbucketForgetRegistration,
    /// Discover available workspaces; selection is independent of the client's owner.
    BitbucketWorkspaces,
    /// Select a workspace from the current account's authenticated discovery results.
    BitbucketSelectWorkspace { workspace: String },
    /// Load from the OS store in a fresh process and verify the account via the API.
    Status { provider: Provider },
    /// Explicitly rotate tokens and replace the stored pair (including refresh token).
    Refresh { provider: Provider },
    /// Forget this probe's local credentials. Does not revoke authorization remotely.
    Disconnect { provider: Provider },
    /// Read one PR and check that both published endpoint IDs are present.
    ProbeReview {
        provider: Provider,
        owner: String,
        repo: String,
        number: u64,
    },
    /// Write/read a random synthetic credential; use read/delete in separate processes.
    Store { action: StoreAction },
}

#[derive(Clone, Copy, ValueEnum)]
enum GithubFlow {
    Browser,
    Device,
}

#[derive(Clone, Copy, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum Provider {
    Github,
    Bitbucket,
}

#[derive(Clone, Copy, ValueEnum)]
enum StoreAction {
    Write,
    Read,
    Delete,
}

impl Provider {
    fn name(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Bitbucket => "bitbucket",
        }
    }
    fn token_url(self) -> &'static str {
        match self {
            Self::Github => GITHUB_TOKEN,
            Self::Bitbucket => BITBUCKET_TOKEN,
        }
    }
    fn identity_url(self) -> &'static str {
        match self {
            Self::Github => "https://api.github.com/user",
            Self::Bitbucket => "https://api.bitbucket.org/2.0/user",
        }
    }
}

// No Debug implementations: neither credentials nor provider error bodies enter logs.
#[derive(Serialize, Deserialize)]
struct Session {
    client_id: String,
    tokens: Tokens,
    // Developer registration secrets for browser flows; never printed or shipped.
    developer_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<bitbucket::Workspace>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancelled);
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
        .map_err(|_| "Cannot install cancellation handler")?;
    let client = Client::builder()
        .user_agent("jaywalk-hosting-auth-poc/0.0.0")
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Cannot initialize HTTPS client")?;
    match args.command {
        Action::GithubRegister => registration::register(&client, &cancelled)?,
        Action::GithubRecoverRegistration => registration::recover(&client)?,
        Action::GithubSetup => registration::open_settings()?,
        Action::GithubInstall => registration::open_installation()?,
        Action::GithubForgetRegistration => registration::forget()?,
        Action::GithubConnect {
            client_id,
            flow,
            reauthorize,
        } => {
            let client_id = match client_id {
                Some(id) => id,
                None => registration::client_id()?,
            };
            if !reauthorize {
                ensure_disconnected(Provider::Github)?;
            }
            let session = match flow {
                GithubFlow::Browser => github_browser::connect(&client, &client_id, &cancelled)?,
                GithubFlow::Device => github_device_connect(&client, &client_id, &cancelled)?,
            };
            check_cancelled(&cancelled)?;
            identity(&client, Provider::Github, &session)?;
            check_cancelled(&cancelled)?;
            save(Provider::Github, &session)?;
            if let Some(secret) = &session.developer_secret {
                registration::remember_browser_secret(&client_id, secret)?;
            }
            println!(
                "Connected; credentials saved in the OS store. Run status in a fresh process."
            );
        }
        Action::BitbucketSetup {
            client_id,
            from_session,
        } => {
            bitbucket::setup(client_id.as_deref(), from_session)?;
        }
        Action::BitbucketForgetRegistration => bitbucket::forget_registration()?,
        Action::BitbucketConnect { reauthorize } => {
            if !reauthorize {
                ensure_disconnected(Provider::Bitbucket)?;
            }
            let session = bitbucket::connect(&client, &cancelled)?;
            check_cancelled(&cancelled)?;
            identity(&client, Provider::Bitbucket, &session)?;
            check_cancelled(&cancelled)?;
            save(Provider::Bitbucket, &session)?;
            println!("Connected using the saved development client; no secret entry required.");
        }
        Action::BitbucketWorkspaces => {
            let session = load(Provider::Bitbucket)?;
            let workspaces = bitbucket::workspaces(&client, &session, &cancelled)?;
            for workspace in &workspaces {
                let marker = if session
                    .workspace
                    .as_ref()
                    .is_some_and(|selected| selected.uuid == workspace.uuid)
                {
                    "selected"
                } else {
                    "available"
                };
                println!("{} ({marker})", workspace.slug);
            }
            println!(
                "{} accessible workspaces; choose with bitbucket-select-workspace SLUG.",
                workspaces.len()
            );
        }
        Action::BitbucketSelectWorkspace { workspace } => {
            let mut session = load(Provider::Bitbucket)?;
            let available = bitbucket::workspaces(&client, &session, &cancelled)?;
            session.workspace = Some(bitbucket::select_workspace(available, &workspace)?);
            check_cancelled(&cancelled)?;
            save(Provider::Bitbucket, &session)?;
            println!("Workspace selection saved for this account session.");
        }
        Action::Status { provider } => {
            let session = load(provider)?;
            identity(&client, provider, &session)?;
            println!("Stored credentials recovered; authenticated identity verified.");
        }
        Action::Refresh { provider } => {
            let mut session = load(provider)?;
            let refresh = session
                .tokens
                .refresh_token
                .as_deref()
                .ok_or("No refresh token; reconnect with expiring tokens enabled")?;
            let parameters = refresh_parameters(provider, &session, refresh);
            let mut request = client.post(provider.token_url()).form(&parameters);
            if let Provider::Bitbucket = provider {
                request =
                    request.basic_auth(&session.client_id, session.developer_secret.as_deref());
            }
            let response = json(request)?;
            let mut rotated = Tokens::parse(&response)?;
            // Some providers may retain the old refresh token; replace when one is returned.
            if rotated.refresh_token.is_none() {
                rotated.refresh_token = session.tokens.refresh_token.take();
            }
            session.tokens = rotated;
            // Save before validation: rotation may already have invalidated the previous pair.
            save(provider, &session)?;
            identity(&client, provider, &session)?;
            println!("Rotated token pair stored and authenticated identity verified.");
        }
        Action::Disconnect { provider } => {
            delete(&entry(provider.name())?)?;
            println!("Local credentials removed. Revoke access in provider settings separately.");
        }
        Action::ProbeReview {
            provider,
            owner,
            repo,
            number,
        } => {
            if !valid_component(&owner) || !valid_component(&repo) || number == 0 {
                return Err("Use plain owner/repository names and a positive PR number".into());
            }
            let session = load(provider)?;
            let (url, left, right) = match provider {
                Provider::Github => (
                    format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}"),
                    "/base/sha",
                    "/head/sha",
                ),
                Provider::Bitbucket => (
                    format!(
                        "https://api.bitbucket.org/2.0/repositories/{owner}/{repo}/pullrequests/{number}"
                    ),
                    "/destination/commit/hash",
                    "/source/commit/hash",
                ),
            };
            let response = json(client.get(url).bearer_auth(&session.tokens.access_token))?;
            match provider {
                Provider::Github => {
                    for pointer in [left, right] {
                        require_full_commit(response.pointer(pointer).and_then(Value::as_str))?;
                    }
                }
                Provider::Bitbucket => {
                    for side in ["source", "destination"] {
                        let (url, prefix) = bitbucket_commit_lookup(&response, side)?;
                        let commit = json(
                            client
                                .get(url)
                                .bearer_auth(&session.tokens.access_token)
                                .query(&[("fields", "hash")]),
                        )?;
                        validate_resolved_commit(&prefix, &commit)?;
                    }
                }
            }
            println!(
                "PR read access verified; source and target commits present. Review diff semantics still need validation."
            );
        }
        Action::Store { action } => store_check(action)?,
    }
    Ok(())
}

fn require_full_commit(value: Option<&str>) -> Result<&str> {
    let hash = value.ok_or("Review response is missing a published commit identity")?;
    if hash.len() != 40 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Review response has an unsupported commit identity".into());
    }
    Ok(hash)
}

fn bitbucket_commit_lookup(review: &Value, side: &str) -> Result<(String, String)> {
    let endpoint = review.get(side).ok_or("Review endpoint is unavailable")?;
    let prefix = endpoint
        .pointer("/commit/hash")
        .and_then(Value::as_str)
        .ok_or("Review endpoint is missing its commit identity")?;
    if !(12..=40).contains(&prefix.len()) || !prefix.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Review endpoint has an unsupported abbreviated commit identity".into());
    }
    let repo = endpoint
        .pointer("/repository/full_name")
        .and_then(Value::as_str)
        .ok_or("Cannot resolve commit without the endpoint's repository identity")?;
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2 || !parts.iter().all(|part| valid_component(part)) {
        return Err("Review endpoint has an invalid repository identity".into());
    }
    Ok((
        format!("https://api.bitbucket.org/2.0/repositories/{repo}/commit/{prefix}"),
        prefix.to_owned(),
    ))
}

fn validate_resolved_commit(prefix: &str, commit: &Value) -> Result<()> {
    let full = require_full_commit(commit.get("hash").and_then(Value::as_str))?;
    if !full
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
    {
        return Err("Resolved commit does not match the published review endpoint".into());
    }
    Ok(())
}

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

fn refresh_parameters<'a>(
    provider: Provider,
    session: &'a Session,
    refresh: &'a str,
) -> Vec<(&'static str, &'a str)> {
    let mut parameters = vec![
        ("client_id", session.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh),
    ];
    // Bitbucket authenticates its client with HTTP Basic instead.
    if let Provider::Github = provider
        && let Some(secret) = &session.developer_secret
    {
        parameters.push(("client_secret", secret));
    }
    parameters
}

fn github_device_connect(
    client: &Client,
    client_id: &str,
    cancelled: &AtomicBool,
) -> Result<Session> {
    let response = json(
        client
            .post("https://github.com/login/device/code")
            .form(&[("client_id", client_id), ("scope", "offline_access")]),
    )?;
    let device = protocol::Device::parse(&response)?;
    println!(
        "Enter code {} at https://github.com/login/device (Ctrl+C cancels).",
        device.user_code
    );
    open_browser("https://github.com/login/device")?;
    let deadline = Instant::now() + Duration::from_secs(device.expires_in);
    let mut interval = device.interval;
    loop {
        pause(interval, deadline, cancelled)?;
        let response = json(client.post(GITHUB_TOKEN).form(&[
            ("client_id", client_id),
            ("device_code", device.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]))?;
        check_cancelled(cancelled)?;
        match protocol::poll(&response, interval)? {
            Poll::Wait(next) => interval = next,
            Poll::Authorized(tokens) => {
                return Ok(Session {
                    client_id: client_id.into(),
                    tokens,
                    developer_secret: None,
                    workspace: None,
                });
            }
        }
    }
}

fn json(request: RequestBuilder) -> Result<Value> {
    let mut response = request
        .header("Accept", "application/json")
        .send()
        .map_err(
            |_| "HTTPS request failed (transport details suppressed to protect credentials)",
        )?;
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read provider response")?;
    if bytes.len() > 65_536 {
        return Err("Provider response exceeded probe size limit".into());
    }
    decode_response(status.as_u16(), &bytes)
}

fn decode_response(status: u16, bytes: &[u8]) -> Result<Value> {
    if !(200..300).contains(&status) {
        // OAuth endpoints often return useful protocol errors with HTTP 400/401.
        // Only expose the existing static whitelist, never arbitrary response text.
        if matches!(status, 400 | 401)
            && let Ok(value) = serde_json::from_slice::<Value>(bytes)
            && value.get("error").and_then(Value::as_str).is_some()
        {
            return Err(format!("{} (HTTP {status})", protocol::oauth_error(&value)));
        }
        let message = match status {
            401 => "Authentication rejected; reconnect or refresh",
            403 => "Access forbidden; check app installation, permissions, SSO or rate limits",
            404 => "Resource unavailable; check identity and access",
            429 => "Provider rate limit reached; retry later",
            _ => "Provider request failed; response body suppressed",
        };
        return Err(format!("{message} (HTTP {status})"));
    }
    serde_json::from_slice(bytes).map_err(|_| "Provider returned invalid JSON".into())
}

fn identity(client: &Client, provider: Provider, session: &Session) -> Result<()> {
    let value = json(
        client
            .get(provider.identity_url())
            .bearer_auth(&session.tokens.access_token),
    )?;
    let id = match provider {
        Provider::Github => value.get("id"),
        Provider::Bitbucket => value.get("uuid"),
    };
    if id.is_none_or(Value::is_null) {
        return Err("Identity response missing account ID".into());
    }
    Ok(())
}

fn entry(account: &str) -> Result<Entry> {
    Entry::new(SERVICE, account).map_err(|_| "Cannot initialize OS credential store".into())
}
fn ensure_disconnected(provider: Provider) -> Result<()> {
    match entry(provider.name())?.get_password() {
        Err(keyring::Error::NoEntry) => Ok(()),
        Ok(_) => Err("Already connected; disconnect before connecting another account".into()),
        Err(_) => Err("OS credential store unavailable or locked; unlock it first".into()),
    }
}
fn save(provider: Provider, session: &Session) -> Result<()> {
    let encoded = serde_json::to_string(session).map_err(|_| "Cannot encode credentials")?;
    entry(provider.name())?
        .set_password(&encoded)
        .map_err(|_| "Cannot save credentials in OS store".into())
}
fn load(provider: Provider) -> Result<Session> {
    let encoded = entry(provider.name())?
        .get_password()
        .map_err(|_| "Cannot load credentials; connect first or unlock the OS store")?;
    serde_json::from_str(&encoded)
        .map_err(|_| "Stored credentials are invalid; disconnect and reconnect".into())
}
fn delete(entry: &Entry) -> Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Cannot remove credential from OS store".into()),
    }
}
fn store_check(action: StoreAction) -> Result<()> {
    let entry = entry("synthetic-store-check")?;
    match action {
        StoreAction::Write => {
            let value = random_string();
            entry
                .set_password(&value)
                .map_err(|_| "Cannot write synthetic credential")?;
            if entry
                .get_password()
                .map_err(|_| "Cannot read synthetic credential")?
                != value
            {
                return Err("Credential store round trip mismatch".into());
            }
            println!("Synthetic credential round trip passed. Run store read in a fresh process.");
        }
        StoreAction::Read => {
            let value = entry
                .get_password()
                .map_err(|_| "Cannot recover synthetic credential")?;
            if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Recovered synthetic credential is malformed".into());
            }
            println!("Synthetic credential recovered from OS store.");
        }
        StoreAction::Delete => {
            delete(&entry)?;
            println!("Synthetic credential removed.");
        }
    }
    Ok(())
}
fn random_string() -> String {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn open_browser(url: &str) -> Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if !Command::new(opener)
        .arg(url)
        .status()
        .map_err(|_| "Cannot launch default browser")?
        .success()
    {
        return Err("Default browser launcher failed".into());
    }
    Ok(())
}
fn check_cancelled(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Authorization cancelled".into())
    } else {
        Ok(())
    }
}
fn pause(seconds: u64, deadline: Instant, cancelled: &AtomicBool) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(seconds);
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Authorization cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("Authorization timed out; start again".into());
        }
        if Instant::now() >= end {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_preserves_device_flow_and_uses_browser_client_authentication_once() {
        let mut session = Session {
            client_id: "synthetic".into(),
            tokens: Tokens::parse(&serde_json::json!({"access_token":"access","token_type":"bearer","refresh_token":"refresh"})).unwrap(),
            developer_secret: None,
            workspace: None,
        };
        let device = refresh_parameters(Provider::Github, &session, "refresh");
        assert!(!device.iter().any(|(name, _)| *name == "client_secret"));
        session.developer_secret = Some("synthetic-secret".into());
        let browser = refresh_parameters(Provider::Github, &session, "refresh");
        assert_eq!(
            browser
                .iter()
                .filter(|(name, _)| *name == "client_secret")
                .count(),
            1
        );
        let bitbucket = refresh_parameters(Provider::Bitbucket, &session, "refresh");
        assert!(!bitbucket.iter().any(|(name, _)| *name == "client_secret"));
    }

    #[test]
    fn bitbucket_fork_endpoints_resolve_in_their_own_repositories() {
        let review = serde_json::json!({
            "source":{"repository":{"full_name":"fork/project"},"commit":{"hash":"123456789abc"}},
            "destination":{"repository":{"full_name":"upstream/project"},"commit":{"hash":"abcdef123456"}}
        });
        let (source, _) = bitbucket_commit_lookup(&review, "source").unwrap();
        let (target, _) = bitbucket_commit_lookup(&review, "destination").unwrap();
        assert!(source.contains("/repositories/fork/project/commit/123456789abc"));
        assert!(target.contains("/repositories/upstream/project/commit/abcdef123456"));
        let mut invalid = review;
        invalid["source"]["repository"]["full_name"] = serde_json::json!("other/../../escape");
        assert!(bitbucket_commit_lookup(&invalid, "source").is_err());
        invalid["source"]["repository"] = Value::Null;
        assert!(bitbucket_commit_lookup(&invalid, "source").is_err());
    }

    #[test]
    fn abbreviated_hash_is_not_accepted_as_a_full_published_identity() {
        let prefix = "123456789abc";
        assert!(require_full_commit(Some(prefix)).is_err());
        let full = format!("{prefix}{}", "0".repeat(28));
        assert!(validate_resolved_commit(prefix, &serde_json::json!({"hash":full})).is_ok());
        assert!(
            validate_resolved_commit(prefix, &serde_json::json!({"hash":"f".repeat(40)})).is_err()
        );
        assert!(validate_resolved_commit(prefix, &serde_json::json!({"hash":prefix})).is_err());
    }

    #[test]
    fn non_success_oauth_responses_keep_safe_protocol_errors() {
        let error = decode_response(
            400,
            br#"{"error":"device_flow_disabled","error_description":"SENSITIVE"}"#,
        )
        .unwrap_err();
        assert!(error.contains("Enable device flow"));
        assert!(error.contains("HTTP 400"));
        assert!(!error.contains("SENSITIVE"));
        let error = decode_response(
            401,
            br#"{"error":"invalid_client","error_description":"SENSITIVE"}"#,
        )
        .unwrap_err();
        assert!(error.contains("client registration"));
        assert!(!error.contains("SENSITIVE"));
    }

    #[test]
    fn provider_failures_do_not_reflect_arbitrary_json_or_html() {
        for (status, body) in [
            (
                400,
                br#"{"error":"SENSITIVE","error_description":"SENSITIVE"}"#.as_slice(),
            ),
            (403, b"<html>SENSITIVE</html>".as_slice()),
            (429, br#"{"access_token":"SENSITIVE"}"#.as_slice()),
        ] {
            let error = decode_response(status, body).unwrap_err();
            assert!(error.contains(&format!("HTTP {status}")));
            assert!(!error.contains("SENSITIVE"));
        }
        assert_eq!(
            decode_response(200, br#"{"ok":true}"#).unwrap(),
            serde_json::json!({"ok":true})
        );
    }
    #[test]
    fn cancellation_and_deadline_prevent_polling() {
        assert!(
            pause(
                5,
                Instant::now() + Duration::from_secs(30),
                &AtomicBool::new(true)
            )
            .unwrap_err()
            .contains("cancelled")
        );
        assert!(
            pause(5, Instant::now(), &AtomicBool::new(false))
                .unwrap_err()
                .contains("timed out")
        );
    }

    #[test]
    fn review_paths_cannot_escape_the_fixed_provider_origin() {
        for value in [
            "",
            ".",
            "..",
            "owner/repo",
            "a?token=b",
            "evil.example/#",
            "%2f",
            "a\\b",
        ] {
            assert!(!valid_component(value));
        }
        assert!(valid_component("some-owner"));
        assert!(valid_component("some.repo_2"));
    }
}
