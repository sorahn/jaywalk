//! Explicit, isolated token feasibility probe. Never changes application accounts.
use clap::{Parser, Subcommand};
use jaywalk::{
    accounts::{Control, HostingProvider, HttpProvider, Identity, Provider, Secret},
    hosting::{
        EntryInput, Locator, Scope,
        review::{Reply, Request},
    },
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const SLOT: &str = "github-token-validation";
const SERVICE: &str = "jaywalk.hosting-auth-poc.v1";
#[derive(Parser)]
#[command(about = "Isolated GitHub token probe; credentials are never printed")]
struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Explicitly copy github.com's active stored gh credential into a separate PoC slot.
    ImportGh {
        #[arg(long)]
        pull_request: String,
    },
    /// Recover the isolated credential without invoking gh or reading OAuth registration.
    Restore {
        #[arg(long)]
        pull_request: String,
    },
    /// Delete only the isolated PoC credential; does not revoke the source token.
    Forget,
}
#[derive(Serialize, Deserialize)]
struct SavedToken {
    version: u8,
    identity: Identity,
    token: Secret,
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn import_gh() -> Result<Secret, String> {
    let path = jaywalk::accounts::token::discover().ok_or("GitHub CLI was not found")?;
    jaywalk::accounts::token::import_gh(&path, &Control::new(Duration::from_secs(15)))
        .map_err(|error| error.to_string())
}
fn locator(value: &str) -> Result<Locator, String> {
    match EntryInput::parse(value).map_err(|_| "Supply a canonical github.com pull request URL")? {
        EntryInput::Url(locator) if locator.provider == Provider::Github => Ok(locator),
        _ => Err("Supply a canonical github.com pull request URL".into()),
    }
}
fn json(
    provider: &HttpProvider,
    token: &Secret,
    path: &str,
    control: &Control,
) -> Result<serde_json::Value, String> {
    let url = format!("https://api.github.com/{path}")
        .parse()
        .map_err(|_| "Invalid probe API URL")?;
    match provider
        .review_request(token, Request::Json(url), control)
        .map_err(|e| format!("API read: {e}"))?
    {
        Reply::Json(value) => Ok(value),
        _ => Err("Unexpected API response kind".into()),
    }
}
fn probe(
    provider: &HttpProvider,
    token: &Secret,
    identity: &Identity,
    pr: &Locator,
    control: &Control,
) -> Result<(), String> {
    for (label, scope) in [
        ("authored discovery", Scope::Authored),
        ("requested reviews", Scope::ReviewRequested),
        ("direct PR lookup", Scope::Direct(pr.clone())),
    ] {
        match provider.pull_requests(token, identity, &scope, &[], control) {
            Ok(batch) => println!(
                "{label}: OK ({} results; partial={})",
                batch.items.len(),
                batch.partial
            ),
            Err(error) => println!("{label}: unavailable ({error})"),
        }
    }
    let repository = pr.repository.to_string();
    let detail = json(
        provider,
        token,
        &format!("repos/{repository}/pulls/{}", pr.number),
        control,
    )?;
    let head = detail
        .pointer("/head/sha")
        .and_then(|v| v.as_str())
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("Missing pinned PR head")?;
    let source = detail
        .pointer("/head/repo/full_name")
        .and_then(|v| v.as_str())
        .ok_or("Missing source repository")?;
    let source =
        jaywalk::hosting::RepositoryName::parse(source).map_err(|_| "Invalid source repository")?;
    for (label, path) in [
        (
            "pinned source tree",
            format!("repos/{source}/git/trees/{head}"),
        ),
        (
            "changed files",
            format!("repos/{repository}/pulls/{}/files?per_page=1", pr.number),
        ),
        (
            "checks",
            format!("repos/{repository}/commits/{head}/check-runs?per_page=1"),
        ),
        (
            "statuses",
            format!("repos/{repository}/commits/{head}/status?per_page=1"),
        ),
        (
            "reviews",
            format!("repos/{repository}/pulls/{}/reviews?per_page=1", pr.number),
        ),
    ] {
        match json(provider, token, &path, control) {
            Ok(value) => {
                println!("{label}: OK");
                if label == "pinned source tree" {
                    // Read one pinned blob, without printing or writing repository content.
                    if let Some(sha) = value["tree"]
                        .as_array()
                        .and_then(|rows| rows.iter().find(|row| row["type"] == "blob"))
                        .and_then(|row| row["sha"].as_str())
                        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
                    {
                        match json(
                            provider,
                            token,
                            &format!("repos/{source}/git/blobs/{sha}"),
                            control,
                        ) {
                            Ok(_) => println!("pinned source blob: OK"),
                            Err(error) => println!("pinned source blob: unavailable ({error})"),
                        }
                    }
                }
            }
            Err(error) => println!("{label}: unavailable ({error})"),
        }
    }
    Ok(())
}
fn run() -> Result<(), String> {
    let args = Args::parse();
    let entry = keyring::Entry::new(SERVICE, SLOT)
        .map_err(|_| "Cannot initialize isolated credential slot")?;
    let (token, expected, pr, save) = match args.action {
        Action::Forget => {
            entry
                .delete_credential()
                .map_err(|_| "Cannot delete isolated credential")?;
            println!("Deleted only the isolated token copy.");
            return Ok(());
        }
        Action::ImportGh { pull_request } => {
            let pr = locator(&pull_request)?;
            (import_gh()?, None, pr, true)
        }
        Action::Restore { pull_request } => {
            let pr = locator(&pull_request)?;
            let encoded = entry
                .get_password()
                .map_err(|_| "No isolated token, or Keychain access denied")?;
            let saved: SavedToken =
                serde_json::from_str(&encoded).map_err(|_| "Invalid isolated token record")?;
            if saved.version != 1 {
                return Err("Unsupported isolated token record".into());
            }
            (saved.token, Some(saved.identity), pr, false)
        }
    };
    let control = Control::new(Duration::from_secs(180));
    let provider = HttpProvider::new().map_err(|e| format!("Initialize API adapter: {e}"))?;
    let identity = provider
        .identity(Provider::Github, &token, &control)
        .map_err(|e| format!("Identity validation: {e}"))?;
    if expected.as_ref().is_some_and(|old| old.key != identity.key) {
        return Err("Restored token identifies a different account".into());
    }
    println!("GitHub identity: verified");
    let kind = if token.expose().starts_with("gho_") {
        "GitHub CLI OAuth credential"
    } else if token.expose().starts_with("github_pat_") {
        "fine-grained personal token"
    } else if token.expose().starts_with("ghp_") {
        "classic personal token"
    } else {
        "unclassified bearer credential"
    };
    println!("Credential kind (prefix hint only): {kind}");
    probe(&provider, &token, &identity, &pr, &control)?;
    if save {
        let encoded = serde_json::to_string(&SavedToken {
            version: 1,
            identity,
            token,
        })
        .map_err(|_| "Cannot encode isolated token")?;
        entry
            .set_password(&encoded)
            .map_err(|_| "Cannot save isolated token")?;
        println!("Validated credential saved only in the isolated PoC Keychain slot.");
    } else {
        println!(
            "Fresh-process recovery verified; no gh invocation or OAuth registration was used."
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::locator;
    #[test]
    fn only_canonical_public_github_pr_urls_are_accepted() {
        assert!(locator("https://github.com/owner/repo/pull/1").is_ok());
        assert!(locator("https://example.org/owner/repo/pull/1").is_err());
    }
}
