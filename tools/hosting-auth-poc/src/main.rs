//! Disposable protocol/credential-store probe, not the app's provider contract.
mod callback;
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
    /// GitHub App device flow; requires a registered client ID with device flow enabled.
    GithubConnect { client_id: Option<String> },
    /// Open browser-assisted development GitHub App registration and save its client ID.
    GithubRegister,
    /// Recover an explicitly supplied browser callback after the local listener timed out.
    GithubRecoverRegistration,
    /// Open the saved development app's settings to enable Device Flow.
    GithubSetup,
    /// Open installation for the saved development app; select test repositories yourself.
    GithubInstall,
    /// Forget the saved development registration locally (does not delete the GitHub App).
    GithubForgetRegistration,
    /// Development consumer only. Prompts locally for its secret; not a shipping design.
    BitbucketConnect { client_id: String },
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
    // A developer's own Bitbucket consumer secret, never a distributed app secret.
    developer_secret: Option<String>,
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
        Action::GithubConnect { client_id } => {
            let client_id = match client_id {
                Some(id) => id,
                None => registration::client_id()?,
            };
            ensure_disconnected(Provider::Github)?;
            let session = github_connect(&client, &client_id, &cancelled)?;
            check_cancelled(&cancelled)?;
            identity(&client, Provider::Github, &session)?;
            check_cancelled(&cancelled)?;
            save(Provider::Github, &session)?;
            println!(
                "Connected; credentials saved in the OS store. Run status in a fresh process."
            );
        }
        Action::BitbucketConnect { client_id } => {
            ensure_disconnected(Provider::Bitbucket)?;
            let session = bitbucket_connect(&client, &client_id, &cancelled)?;
            check_cancelled(&cancelled)?;
            identity(&client, Provider::Bitbucket, &session)?;
            check_cancelled(&cancelled)?;
            save(Provider::Bitbucket, &session)?;
            println!(
                "Development-consumer flow connected. This does not prove a distributable native flow."
            );
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
            let mut request = client.post(provider.token_url()).form(&[
                ("client_id", session.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
            ]);
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
            for pointer in [left, right] {
                let hash = response
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .ok_or("Review response is missing a published commit identity")?;
                if hash.len() != 40 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err("Review response has an unsupported commit identity".into());
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

fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

fn github_connect(client: &Client, client_id: &str, cancelled: &AtomicBool) -> Result<Session> {
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
                });
            }
        }
    }
}

fn bitbucket_connect(client: &Client, client_id: &str, cancelled: &AtomicBool) -> Result<Session> {
    println!("Development-only consumer test. Enter your own consumer secret locally.");
    let secret = rpassword::prompt_password("Consumer secret (hidden): ")
        .map_err(|_| "Cannot read secret from the terminal")?;
    if secret.is_empty() {
        return Err("Consumer secret is required for this development test".into());
    }
    let listener = callback::Listener::bind()?;
    let state = random_string();
    let mut url = reqwest::Url::parse("https://bitbucket.org/site/oauth2/authorize")
        .map_err(|_| "Invalid built-in authorization URL")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("response_type", "code"),
        ("state", &state),
        ("redirect_uri", callback::REDIRECT),
    ]);
    println!("Waiting for browser authorization (Ctrl+C cancels).");
    open_browser(url.as_str())?;
    let code = listener.receive(&state, Duration::from_secs(180), cancelled)?;
    let response = json(
        client
            .post(BITBUCKET_TOKEN)
            .basic_auth(client_id, Some(&secret))
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code.as_str()),
                ("redirect_uri", callback::REDIRECT),
            ]),
    )?;
    Ok(Session {
        client_id: client_id.into(),
        tokens: Tokens::parse(&response)?,
        developer_secret: Some(secret),
    })
}

fn json(request: RequestBuilder) -> Result<Value> {
    let mut response = request
        .header("Accept", "application/json")
        .send()
        .map_err(
            |_| "HTTPS request failed (transport details suppressed to protect credentials)",
        )?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 => "Authentication rejected; reconnect or refresh",
            403 => "Access forbidden; check app installation, permissions, SSO or rate limits",
            404 => "Resource unavailable; check identity and access",
            429 => "Provider rate limit reached; retry later",
            _ => "Provider request failed; response body suppressed",
        }
        .into());
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read provider response")?;
    if bytes.len() > 65_536 {
        return Err("Provider response exceeded probe size limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "Provider returned invalid JSON".into())
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
