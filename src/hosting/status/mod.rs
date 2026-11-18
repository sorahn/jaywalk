//! Read-only, in-memory observations for an already pinned published review.
//! These results never authorize a merge or change a comparison's endpoints.
mod bitbucket;
mod github;
use super::review::{Metadata, Reply, Request, json, oid, text, url};
use crate::accounts::{Control, Error, Provider};
use reqwest::Url;
use serde_json::Value;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Availability<T> {
    Known(Listing<T>),
    Unavailable(Issue),
}
impl<T> Availability<T> {
    fn from_result(result: Result<Listing<T>, Error>) -> Self {
        match result {
            Ok(list) => Self::Known(list),
            Err(error) => Self::Unavailable(Issue::Provider(error)),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing<T> {
    pub items: Vec<T>,
    pub partial: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    Provider(Error),
    NewVersion,
}
impl Issue {
    pub fn label(&self) -> &'static str {
        match self {
            Self::NewVersion => "Unavailable for this pinned version; update the comparison",
            Self::Provider(Error::NoAccess) => {
                "Unavailable: access denied; check account/app permissions"
            }
            Self::Provider(Error::NotFound) => "Unavailable: repository or revision not accessible",
            Self::Provider(Error::Unsupported) => "Unsupported by this provider",
            Self::Provider(Error::RateLimited) => "Unavailable: provider rate limit",
            Self::Provider(Error::NeedsSignIn | Error::NotConnected | Error::Superseded) => {
                "Unavailable: account changed or sign-in required"
            }
            Self::Provider(Error::TimedOut) => "Unavailable: status read timed out",
            Self::Provider(Error::Network) => {
                "Unavailable: network request failed; refresh to retry"
            }
            Self::Provider(_) => "Unavailable: provider response could not be verified",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckState {
    Passed,
    Failed,
    Pending,
    Neutral,
    Skipped,
    Cancelled,
    Unknown(String),
}
impl CheckState {
    pub fn label(&self) -> &str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Pending => "pending",
            Self::Neutral => "neutral",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
            Self::Unknown(_) => "unknown",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub id: String,
    pub name: String,
    pub state: CheckState,
    pub url: Option<Url>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckGroup {
    pub label: String,
    pub checks: Availability<Check>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Awaiting,
    Unknown(String),
}
impl Decision {
    pub fn label(&self) -> &str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes requested",
            Self::Commented => "commented",
            Self::Dismissed => "dismissed",
            Self::Awaiting => "awaiting review",
            Self::Unknown(_) => "unknown",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reviewer {
    pub id: String,
    pub name: String,
    pub decision: Decision,
    pub commit: Option<String>,
    pub requested: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Current,
    NewVersion,
    Unavailable(Error),
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub head: String,
    pub fetched_at: SystemTime,
    pub freshness: Freshness,
    pub checks: Vec<CheckGroup>,
    pub reviews: Availability<Reviewer>,
}
impl CheckGroup {
    pub fn summary(&self) -> String {
        match &self.checks {
            Availability::Unavailable(issue) => format!("{} · {}", self.label, issue.label()),
            Availability::Known(list) => {
                let mut counts = [0usize; 7];
                for check in &list.items {
                    counts[match &check.state {
                        CheckState::Passed => 0,
                        CheckState::Failed => 1,
                        CheckState::Pending => 2,
                        CheckState::Neutral => 3,
                        CheckState::Skipped => 4,
                        CheckState::Cancelled => 5,
                        CheckState::Unknown(_) => 6,
                    }] += 1;
                }
                let labels = [
                    "passed",
                    "failed",
                    "pending",
                    "neutral",
                    "skipped",
                    "cancelled",
                    "unknown",
                ];
                let summary = counts
                    .into_iter()
                    .zip(labels)
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, label)| format!("{n} {label}"))
                    .collect::<Vec<_>>()
                    .join(" · ");
                format!(
                    "{} · {}{}",
                    self.label,
                    if summary.is_empty() {
                        "None reported"
                    } else {
                        &summary
                    },
                    if list.partial {
                        " · partial results"
                    } else {
                        ""
                    }
                )
            }
        }
    }
}
impl Snapshot {
    pub fn review_summary(&self) -> String {
        match &self.reviews {
            Availability::Unavailable(issue) => format!("Reviews · {}", issue.label()),
            Availability::Known(list) => {
                if list.items.is_empty() {
                    return "Reviews · None reported · merge requirements not evaluated".into();
                }
                let approved = list
                    .items
                    .iter()
                    .filter(|r| r.decision == Decision::Approved)
                    .count();
                let changes = list
                    .items
                    .iter()
                    .filter(|r| r.decision == Decision::ChangesRequested)
                    .count();
                let awaiting = list
                    .items
                    .iter()
                    .filter(|r| r.decision == Decision::Awaiting)
                    .count();
                let unknown = list
                    .items
                    .iter()
                    .filter(|r| matches!(r.decision, Decision::Unknown(_)))
                    .count();
                let earlier = list
                    .items
                    .iter()
                    .filter(|r| {
                        matches!(r.decision, Decision::Approved | Decision::ChangesRequested)
                            && r.commit.as_ref().is_some_and(|commit| commit != &self.head)
                    })
                    .count();
                let unpinned = list.items.iter().any(|r| {
                    matches!(r.decision, Decision::Approved | Decision::ChangesRequested)
                        && r.commit.is_none()
                });
                let mut parts = vec![
                    format!(
                        "{approved} approval{}",
                        if approved == 1 { "" } else { "s" }
                    ),
                    format!("{changes} changes requested"),
                    format!("{awaiting} awaiting"),
                ];
                if unknown > 0 {
                    parts.push(format!("{unknown} unknown"));
                }
                if earlier > 0 {
                    parts.push(format!("{earlier} earlier-commit decisions"));
                }
                if unpinned {
                    parts.push("reviewed revisions not supplied".into());
                }
                if list.partial {
                    parts.push("partial results".into());
                }
                parts.push("merge requirements not evaluated".into());
                format!("Reviews · {}", parts.join(" · "))
            }
        }
    }
}
pub(crate) fn load(
    metadata: &Metadata,
    control: &Control,
    request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Snapshot, Error> {
    control.check()?;
    let snapshot = match metadata.locator.provider {
        Provider::Github => github::load(metadata, control, request)?,
        Provider::Bitbucket => bitbucket::load(metadata, control, request)?,
    };
    control.check()?;
    Ok(snapshot)
}
fn safe_url(value: Option<&str>) -> Option<Url> {
    let value = value.filter(|value| value.len() <= 8192)?;
    let url = Url::parse(value).ok()?;
    (url.scheme() == "https" && url.username().is_empty() && url.password().is_none())
        .then_some(url)
}
fn get(
    url: Url,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Value, Error> {
    json(request(Request::Json(url))?)
}
fn bounded_rows<'a>(value: &'a Value, pointer: &str, max: usize) -> Result<&'a [Value], Error> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .filter(|rows| rows.len() <= max)
        .map(Vec::as_slice)
        .ok_or(Error::InvalidResponse)
}
fn freshness(metadata: &Metadata, value: &Value) -> Result<Freshness, Error> {
    let (source, target, source_repo, target_repo, id) = match metadata.locator.provider {
        Provider::Github => (
            "/head/sha",
            "/base/sha",
            "/head/repo/node_id",
            "/base/repo/node_id",
            text(value, "/node_id", 512)?,
        ),
        Provider::Bitbucket => (
            "/source/commit/hash",
            "/destination/commit/hash",
            "/source/repository/uuid",
            "/destination/repository/uuid",
            format!(
                "{}:{}",
                text(value, "/destination/repository/uuid", 512)?,
                value["id"].as_u64().ok_or(Error::InvalidResponse)?
            ),
        ),
    };
    if id != metadata.id
        || text(value, source_repo, 512)? != metadata.source.repository.id
        || text(value, target_repo, 512)? != metadata.target.repository.id
    {
        return Err(Error::InvalidResponse);
    }
    let source = text(value, source, 40)?;
    let target = text(value, target, 40)?;
    let same = |full: &str, reported: &str| -> Result<bool, Error> { Ok(full == oid(reported)?) };
    Ok(
        if same(&metadata.source.commit, &source)? && same(&metadata.target.commit, &target)? {
            Freshness::Current
        } else {
            Freshness::NewVersion
        },
    )
}
fn finish(
    metadata: &Metadata,
    detail: &Result<Value, Error>,
    verify: Result<Value, Error>,
    checks: Vec<CheckGroup>,
    mut reviews: Availability<Reviewer>,
) -> Snapshot {
    let first = detail
        .as_ref()
        .map_err(|e| *e)
        .and_then(|v| freshness(metadata, v));
    let last = verify.and_then(|v| freshness(metadata, &v));
    let current = match (first, last) {
        (Ok(Freshness::Current), Ok(Freshness::Current)) => Freshness::Current,
        (Ok(Freshness::NewVersion), _) | (_, Ok(Freshness::NewVersion)) => Freshness::NewVersion,
        (Err(e), _) | (_, Err(e)) => Freshness::Unavailable(e),
        _ => Freshness::Unavailable(Error::InvalidResponse),
    };
    if current != Freshness::Current {
        reviews = Availability::Unavailable(match current {
            Freshness::NewVersion => Issue::NewVersion,
            Freshness::Unavailable(error) => Issue::Provider(error),
            Freshness::Current => unreachable!(),
        });
    }
    Snapshot {
        head: metadata.source.commit.clone(),
        fetched_at: SystemTime::now(),
        freshness: current,
        checks,
        reviews,
    }
}
#[cfg(test)]
mod tests;
