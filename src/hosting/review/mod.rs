//! Immutable, account-bound hosted review snapshots. Provider API requests are
//! read-only; optional Git acquisition/checkout is owned by `local_review`.
mod bitbucket;
mod contents;
mod github;
use super::{Locator, RepositoryName};
use crate::{
    accounts::{AccountKey, Control, Error, Service},
    history::Revision,
    repository::{ChangedFile, Comparison},
    repository_file::FileKind,
    text_content::TextLimit,
};
pub use contents::Contents;
use reqwest::Url;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    time::Duration,
};
#[derive(Clone, Copy, Debug, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub concurrent_accounts: usize,
    pub history_limit: usize,
    pub max_files: usize,
    pub max_content_mb: u64,
}
impl Default for Options {
    fn default() -> Self {
        crate::settings::Config::default().hosting
    }
}
impl Options {
    pub fn validate(self) -> Result<(), String> {
        if !(1..=8).contains(&self.concurrent_accounts) {
            return Err("hosting.concurrent_accounts must be 1..8".into());
        }
        if !(1..=1000).contains(&self.history_limit)
            || !(1..=3000).contains(&self.max_files)
            || !(1..=1024).contains(&self.max_content_mb)
        {
            Err("hosting history_limit must be 1..1000, max_files 1..3000 and max_content_mb 1..1024".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub name: RepositoryName,
    pub id: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub repository: Repository,
    pub commit: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub locator: Locator,
    pub id: String,
    pub title: String,
    pub body: String,
    pub source_branch: Option<String>,
    pub target_branch: Option<String>,
    pub draft: Option<bool>,
    pub author: String,
    pub state: String,
    pub source: Endpoint,
    pub target: Endpoint,
    pub base: Endpoint,
    pub history_partial: bool,
}
impl Metadata {
    pub fn label(&self) -> String {
        format!(
            "{} · PR {} · {}",
            self.locator.provider.name(),
            self.locator.number,
            self.title
        )
    }
    pub fn version(&self) -> (&str, &str, &str) {
        (&self.source.commit, &self.target.commit, &self.base.commit)
    }
}
#[derive(Clone, Debug)]
pub struct File {
    pub change: ChangedFile,
    pub before: Option<Blob>,
    pub after: Option<Blob>,
}
#[derive(Clone, Debug)]
pub struct Blob {
    pub url: Url,
    pub kind: FileKind,
    pub executable: bool,
}
#[derive(Clone, Debug)]
pub struct Data {
    pub metadata: Metadata,
    pub files: Vec<File>,
    pub commits: Vec<Revision>,
}
#[derive(Clone)]
pub struct Review {
    pub(crate) comparison: Comparison,
    pub(crate) checkout: Option<Arc<crate::local_review::checkout::Lease>>,
    pub(crate) local_notice: Option<String>,
}
impl Review {
    pub fn retained(comparison: &Comparison) -> Option<Self> {
        comparison.hosted().map(|_| Self {
            comparison: comparison.clone(),
            checkout: None,
            local_notice: None,
        })
    }
    pub fn checkout(&self) -> Option<&Arc<crate::local_review::checkout::Lease>> {
        self.checkout.as_ref()
    }
    pub fn local_notice(&self) -> Option<&str> {
        self.local_notice.as_deref()
    }
    pub fn with_notice(mut self, notice: String) -> Self {
        self.local_notice = Some(notice);
        self
    }
    pub fn locally(
        &self,
        known: &std::path::Path,
        options: crate::local_review::Options,
        receipts: &std::path::Path,
        control: &Control,
    ) -> Result<Self, String> {
        let checkout_snapshot = if options.mode == crate::local_review::Mode::CheckoutIfClean {
            Some(crate::local_review::checkout::preflight(known, control)?)
        } else {
            None
        };
        let workspace =
            crate::local_review::Workspace::prepare(known, self.metadata(), options, control)?;
        let contents = self
            .comparison
            .hosted()
            .expect("a review has hosted contents");
        let comparison = contents.with_local(workspace.clone())?.comparison()?;
        let checkout = if options.mode == crate::local_review::Mode::CheckoutIfClean {
            Some(crate::local_review::checkout::start(
                &workspace,
                receipts,
                checkout_snapshot
                    .as_ref()
                    .expect("checkout policy has a preflight snapshot"),
                control,
            )?)
        } else {
            None
        };
        control.check().map_err(|error| error.to_string())?;
        let checkout_changed = checkout.as_ref().is_some_and(|lease| lease.active());
        Ok(Self {
            comparison,
            checkout,
            local_notice: Some(
                if options.mode == crate::local_review::Mode::CheckoutIfClean && !checkout_changed {
                    "Local PR objects · existing clean checkout already matches; nothing switched"
                        .into()
                } else if options.mode == crate::local_review::Mode::CheckoutIfClean {
                    "Local PR objects · clean-only checkout enabled · use Restore review checkout to finish".into()
                } else {
                    "Local PR objects · known repository checkout unchanged".into()
                },
            ),
        })
    }
    pub fn comparison(&self) -> &Comparison {
        &self.comparison
    }
    pub fn into_comparison(self) -> Comparison {
        self.comparison
    }
    pub fn metadata(&self) -> &Metadata {
        &self
            .comparison
            .hosted()
            .expect("review comparison is hosted")
            .data
            .metadata
    }
}
#[derive(Clone, Debug)]
pub enum Request {
    Json(Url),
    Bytes(Url, usize),
}
pub enum Reply {
    Json(Value),
    Bytes(Vec<u8>),
    Redirect(Url),
}
#[derive(Clone)]
pub struct Access {
    pub(crate) service: Weak<Service>,
    pub(crate) account: AccountKey,
    pub(crate) generation: u64,
}
impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HostedAccess")
    }
}
impl Access {
    pub fn request(&self, request: Request, control: &Control) -> Result<Reply, Error> {
        self.service
            .upgrade()
            .ok_or(Error::NotConnected)?
            .review_request(&self.account, self.generation, request, control)
    }
}
pub(crate) fn resolve(
    locator: &Locator,
    options: Options,
    control: &Control,
    request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Data, Error> {
    control.check()?;
    match locator.provider {
        crate::accounts::Provider::Github => github::resolve(locator, options, control, request),
        crate::accounts::Provider::Bitbucket => {
            bitbucket::resolve(locator, options, control, request)
        }
    }
}
pub(crate) fn url(
    provider: crate::accounts::Provider,
    repo: &RepositoryName,
    parts: &[&str],
) -> Result<Url, Error> {
    let mut url = Url::parse(match provider {
        crate::accounts::Provider::Github => "https://api.github.com",
        crate::accounts::Provider::Bitbucket => "https://api.bitbucket.org",
    })
    .map_err(|_| Error::InvalidResponse)?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| Error::InvalidResponse)?;
        if provider == crate::accounts::Provider::Bitbucket {
            path.push("2.0");
        }
        path.push(if provider == crate::accounts::Provider::Github {
            "repos"
        } else {
            "repositories"
        })
        .push(&repo.owner)
        .push(&repo.name)
        .extend(parts);
    }
    Ok(url)
}
pub(crate) fn json(reply: Reply) -> Result<Value, Error> {
    if let Reply::Json(value) = reply {
        Ok(value)
    } else {
        Err(Error::InvalidResponse)
    }
}
pub(crate) fn text(value: &Value, pointer: &str, limit: usize) -> Result<String, Error> {
    super::model::bounded_text(value.pointer(pointer).and_then(Value::as_str), limit)
}
pub(crate) fn oid(value: &str) -> Result<String, Error> {
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(Error::InvalidResponse)
    }
}
pub(crate) fn path(value: &str) -> Result<String, Error> {
    if value.is_empty()
        || value.len() > 4096
        || value.starts_with('/')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
        || value.chars().any(char::is_control)
    {
        Err(Error::InvalidResponse)
    } else {
        Ok(value.into())
    }
}
pub(crate) fn revision(
    id: String,
    message: String,
    author: String,
    date: String,
    parents: Vec<String>,
) -> Revision {
    Revision {
        change_id: id.clone(),
        commit_id: id,
        description: message,
        author,
        timestamp: date,
        parents,
        references: vec![],
        working_copy: false,
        empty: false,
    }
}
pub(crate) fn history(revisions: Vec<Revision>) -> crate::history::History {
    crate::history::History::new(revisions)
}
pub(crate) fn control() -> Control {
    Control::new(Duration::from_secs(180))
}

pub(crate) fn multiline(value: &str, limit: usize) -> Result<String, Error> {
    if value.len() > limit
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        Err(Error::InvalidResponse)
    } else {
        Ok(value.into())
    }
}

pub(crate) fn for_revision(
    data: &Data,
    revision: &Revision,
    options: Options,
    control: &Control,
    request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Data, Error> {
    match data.metadata.locator.provider {
        crate::accounts::Provider::Github => {
            github::for_revision(data, revision, options, control, request)
        }
        crate::accounts::Provider::Bitbucket => {
            bitbucket::for_revision(data, revision, options, control, request)
        }
    }
}

#[cfg(test)]
mod tests;
