//! Read-only provider smoke test. Credentials stay in the OS store; output is
//! counts only. Usage: hosted-review-probe PR_URL [FILE_COUNT].
use jaywalk::{
    accounts::{Control, HttpProvider, OsCredentialStore, Service},
    hosting::EntryInput,
};
use std::{sync::Arc, time::Duration};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let EntryInput::Url(locator) =
        EntryInput::parse(&std::env::args().nth(1).ok_or("PR URL required")?)?
    else {
        return Err("Full PR URL required".into());
    };
    let count = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "1".into())
        .parse::<usize>()?;
    let service = Service::new(
        Arc::new(OsCredentialStore::new()),
        Arc::new(HttpProvider::new()?),
    );
    service.restore()?;
    let key = service
        .accounts_snapshot()
        .accounts
        .into_iter()
        .find(|a| a.identity.key.provider == locator.provider)
        .ok_or("No matching account")?
        .identity
        .key;
    let review = service.open_review(
        &key,
        &locator,
        Default::default(),
        Default::default(),
        &Control::new(Duration::from_secs(180)),
    )?;
    println!("Review opened: {} files", review.comparison().files().len());
    for file in review.comparison().files().iter().take(count) {
        let (before, after) = review.comparison().contents(file)?;
        println!("Read text: {} / {} bytes", before.len(), after.len());
    }
    let history = review
        .comparison()
        .history(&jaywalk::history::HistoryQuery::sources(
            Default::default(),
            None,
        )?)?;
    println!("History: {} commits", history.revisions.len());
    if std::env::args().nth(3).as_deref() == Some("commit")
        && let Some(revision) = history.revisions.first()
    {
        let commit = review.comparison().for_revision(revision)?;
        println!("Commit opened: {} files", commit.files().len());
        if let Some(file) = commit.files().first() {
            let (before, after) = commit.contents(file)?;
            println!("Commit text: {} / {} bytes", before.len(), after.len());
        }
    }
    if std::env::args().any(|arg| arg == "status") {
        use jaywalk::hosting::status::Availability;
        let snapshot = review
            .comparison()
            .hosted()
            .expect("hosted review")
            .status(&Control::new(Duration::from_secs(90)))?;
        println!("Status version: {:?}", snapshot.freshness);
        for (index, group) in snapshot.checks.iter().enumerate() {
            match &group.checks {
                Availability::Known(list) => println!(
                    "Check group {index}: {} observations, partial={}",
                    list.items.len(),
                    list.partial
                ),
                Availability::Unavailable(issue) => {
                    println!("Check group {index}: {}", issue.label())
                }
            }
        }
        match &snapshot.reviews {
            Availability::Known(list) => println!(
                "Reviews: {} observations, partial={}",
                list.items.len(),
                list.partial
            ),
            Availability::Unavailable(issue) => println!("Reviews: {}", issue.label()),
        }
    }
    let metadata = review.metadata();
    println!("History limited: {}", metadata.history_partial);
    Ok(())
}
