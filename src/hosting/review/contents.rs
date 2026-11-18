use super::*;
use crate::{
    overview::{CommitDetails, OverviewSeries, SeriesCommit},
    repository::{ComparisonMetadata, ContentSource, FileSide},
};
use std::sync::Mutex;
#[derive(Default, Debug)]
struct Cache {
    text: BTreeMap<String, Arc<str>>,
    bytes: usize,
}
pub struct Contents {
    pub data: Data,
    access: Access,
    options: Options,
    limit: TextLimit,
    cache: Arc<Mutex<Cache>>,
}
impl std::fmt::Debug for Contents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedContents")
            .field("files", &self.data.files.len())
            .finish_non_exhaustive()
    }
}
impl Contents {
    pub(crate) fn new(data: Data, access: Access, options: Options, limit: TextLimit) -> Arc<Self> {
        Arc::new(Self {
            data,
            access,
            options,
            limit,
            cache: Arc::default(),
        })
    }
    pub(crate) fn comparison(self: &Arc<Self>) -> Result<Comparison, String> {
        let m = &self.data.metadata;
        Comparison::assemble(
            ComparisonMetadata {
                root: std::path::PathBuf::from(format!(
                    "hosted/{}/{}",
                    m.locator.provider.name(),
                    m.locator.repository
                )),
                operation: format!("{}:{}:{}", m.id, m.target.commit, m.source.commit),
                base: m.base.commit.clone(),
                tip: m.source.commit.clone(),
                capture: crate::repository::next_capture(),
            },
            self.data
                .files
                .iter()
                .map(|file| file.change.clone())
                .collect(),
            ContentSource::Hosted(self.clone()),
            self.limit,
        )
    }
    pub(crate) fn paths(&self, side: FileSide) -> BTreeMap<String, FileKind> {
        self.data
            .files
            .iter()
            .filter_map(|file| match side {
                FileSide::Before => file
                    .before
                    .as_ref()
                    .map(|blob| (file.change.before_path().to_owned(), blob.kind.clone())),
                FileSide::After => file
                    .after
                    .as_ref()
                    .map(|blob| (file.change.after_path().to_owned(), blob.kind.clone())),
            })
            .collect()
    }
    pub(crate) fn executable_changes(&self) -> impl Iterator<Item = (&str, bool, bool)> {
        self.data.files.iter().filter_map(|file| {
            let before = file.before.as_ref()?;
            let after = file.after.as_ref()?;
            (before.kind == FileKind::Regular
                && after.kind == FileKind::Regular
                && before.executable != after.executable)
                .then_some((
                    file.change.after_path(),
                    before.executable,
                    after.executable,
                ))
        })
    }
    pub(crate) fn read_path(&self, path: &str, side: FileSide) -> Result<String, String> {
        let file = self
            .data
            .files
            .iter()
            .find(|file| match side {
                FileSide::Before => file.change.before_path() == path,
                FileSide::After => file.change.after_path() == path,
            })
            .ok_or("File is outside this PR's change list")?;
        let blob = match side {
            FileSide::Before => &file.before,
            FileSide::After => &file.after,
        };
        let Some(blob) = blob else {
            return Ok(String::new());
        };
        if blob.kind == FileKind::Submodule {
            return Err("Submodule change: text contents are unavailable".into());
        }
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "Review content cache lock poisoned")?;
        if let Some(text) = cache.text.get(blob.url.as_str()) {
            return Ok(text.to_string());
        }
        let budget = (self.options.max_content_mb * 1_048_576) as usize;
        let remaining = budget.saturating_sub(cache.bytes);
        if remaining == 0 {
            return Err("Review memory limit reached; increase hosting.max_content_mb and reopen the review".into());
        }
        let limit = self.limit.bytes().min(remaining);
        let bytes = match self
            .access
            .request(Request::Bytes(blob.url.clone(), limit), &control())
            .map_err(|error| error.to_string())?
        {
            Reply::Bytes(bytes) => bytes,
            _ => return Err("Provider returned unsupported file content".into()),
        };
        if bytes.len() > limit {
            return Err("Review content exceeds configured limits".into());
        }
        let text = self.limit.text(bytes)?;
        cache.bytes += text.len();
        cache
            .text
            .insert(blob.url.to_string(), Arc::from(text.as_str()));
        Ok(text)
    }
    pub(crate) fn history(&self) -> crate::history::History {
        history(self.data.commits.clone())
    }
    pub(crate) fn series(&self) -> OverviewSeries {
        let mut commits = self.data.commits.clone();
        commits.reverse();
        OverviewSeries {
            commits: commits
                .into_iter()
                .map(|revision| SeriesCommit {
                    details: CommitDetails {
                        id: revision.commit_id.clone(),
                        message: revision.description.clone(),
                        author: revision.author.clone(),
                        timestamp: revision.timestamp.clone(),
                        working_copy: false,
                    },
                    revision: Some(revision),
                })
                .collect(),
        }
    }
    pub(crate) fn commit_details(&self, commit: &str) -> Option<CommitDetails> {
        self.data
            .commits
            .iter()
            .find(|revision| revision.commit_id == commit)
            .map(|r| CommitDetails {
                id: r.commit_id.clone(),
                message: r.description.clone(),
                author: r.author.clone(),
                timestamp: r.timestamp.clone(),
                working_copy: false,
            })
    }
    pub(crate) fn for_revision(&self, revision: &Revision) -> Result<Comparison, String> {
        if !self
            .data
            .commits
            .iter()
            .any(|r| r.commit_id == revision.commit_id)
        {
            return Err("Commit is outside this published review snapshot".into());
        }
        let control = control();
        let data = super::for_revision(&self.data, revision, self.options, &control, |request| {
            self.access.request(request, &control)
        })
        .map_err(|error| error.to_string())?;
        Arc::new(Self {
            data,
            access: self.access.clone(),
            options: self.options,
            limit: self.limit,
            cache: self.cache.clone(),
        })
        .comparison()
    }
    pub fn status(&self, control: &Control) -> Result<crate::hosting::status::Snapshot, Error> {
        crate::hosting::status::load(&self.data.metadata, control, |request| {
            self.access.request(request, control)
        })
    }
    pub fn newer(
        &self,
        options: Options,
        limit: TextLimit,
        control: &Control,
    ) -> Result<Review, Error> {
        let service = self.access.service.upgrade().ok_or(Error::NotConnected)?;
        service.open_review(
            &self.access.account,
            &self.data.metadata.locator,
            options,
            limit,
            control,
        )
    }
}
