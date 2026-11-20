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
    local: Option<Arc<crate::local_review::Workspace>>,
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
            local: None,
        })
    }
    pub fn is_local(&self) -> bool {
        self.local.is_some()
    }
    pub(super) fn with_local(
        &self,
        workspace: Arc<crate::local_review::Workspace>,
    ) -> Result<Arc<Self>, String> {
        let control = control();
        workspace.ensure(&self.data.metadata.base, &control)?;
        workspace.ensure(&self.data.metadata.source, &control)?;
        // Catalog preparation and object reads are worker-only.
        workspace.paths(&self.data.metadata.base.commit)?;
        workspace.paths(&self.data.metadata.source.commit)?;
        Ok(Arc::new(Self {
            data: self.data.clone(),
            access: self.access.clone(),
            options: self.options,
            limit: self.limit,
            cache: self.cache.clone(),
            local: Some(workspace),
        }))
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
    pub(crate) fn paths(&self, side: FileSide) -> Result<BTreeMap<String, FileKind>, String> {
        if let Some(local) = &self.local {
            return local.paths(match side {
                FileSide::Before => &self.data.metadata.base.commit,
                FileSide::After => &self.data.metadata.source.commit,
            });
        }
        Ok(self
            .data
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
            .collect())
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
        if let Some(local) = &self.local {
            let commit = match side {
                FileSide::Before => &self.data.metadata.base.commit,
                FileSide::After => &self.data.metadata.source.commit,
            };
            let key = format!("local:{commit}:{path}");
            let mut cache = self
                .cache
                .lock()
                .map_err(|_| "Review content cache lock poisoned")?;
            if let Some(text) = cache.text.get(&key) {
                return Ok(text.to_string());
            }
            let text = local.read(commit, path, self.limit)?;
            let maximum = self.options.max_content_mb * 1024 * 1024;
            if (cache.bytes as u64).saturating_add(text.len() as u64) > maximum {
                return Err("Review content exceeds configured limits".into());
            }
            cache.bytes += text.len();
            cache.text.insert(key, Arc::from(text.as_str()));
            return Ok(text);
        }
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
        let contents = Arc::new(Self {
            data,
            access: self.access.clone(),
            options: self.options,
            limit: self.limit,
            cache: self.cache.clone(),
            local: None,
        });
        let contents = if let Some(local) = &self.local {
            contents.with_local(local.clone())?
        } else {
            contents
        };
        contents.comparison()
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
        let mut review = service.open_review(
            &self.access.account,
            &self.data.metadata.locator,
            options,
            limit,
            control,
        )?;
        if let Some(local) = &self.local {
            let contents = review
                .comparison
                .hosted()
                .expect("a published review has hosted contents");
            match contents
                .with_local(local.clone())
                .and_then(|contents| contents.comparison())
            {
                Ok(comparison) => {
                    review.comparison = comparison;
                    review.local_notice = Some(
                        "Published PR updated with local objects · checkout was not switched"
                            .into(),
                    );
                }
                Err(error) => {
                    review.local_notice = Some(format!(
                        "Local update unavailable: {error} · Updated hosted snapshot retained"
                    ));
                }
            }
        }
        Ok(review)
    }
}

#[cfg(test)]
mod local_tests {
    use super::*;
    #[test]
    fn local_source_keeps_published_identity_and_opens_unchanged_files_without_api_content_reads() {
        let fixture = crate::local_review::tests::Fixture::new();
        let metadata = fixture.metadata();
        let blob = |commit: &str| Blob {
            url: Url::parse(&format!(
                "https://api.github.com/repos/fixture/review/git/blobs/{commit}"
            ))
            .unwrap(),
            kind: FileKind::Regular,
            executable: false,
        };
        let data = Data {
            metadata: metadata.clone(),
            commits: vec![super::super::revision(
                fixture.head.clone(),
                "A coherent review commit".into(),
                "Fixture".into(),
                String::new(),
                vec![fixture.base.clone()],
            )],
            files: vec![
                File {
                    change: ChangedFile::new(
                        crate::repository_file::ChangeKind::Modified,
                        "changed.rs".into(),
                        "changed.rs".into(),
                    ),
                    before: Some(blob(&fixture.base)),
                    after: Some(blob(&fixture.head)),
                },
                File {
                    change: ChangedFile::new(
                        crate::repository_file::ChangeKind::Added,
                        "new.rs".into(),
                        "new.rs".into(),
                    ),
                    before: None,
                    after: Some(blob(&fixture.head)),
                },
            ],
        };
        let contents = Contents::new(
            data,
            Access {
                service: Weak::new(),
                account: crate::accounts::AccountKey {
                    provider: crate::accounts::Provider::Github,
                    subject: "fixture".into(),
                },
                generation: 0,
            },
            Options::default(),
            TextLimit::default(),
        );
        let review = Review {
            comparison: contents.comparison().unwrap(),
            checkout: None,
            local_notice: None,
        };
        let receipts = tempfile::tempdir().unwrap();
        let local = review
            .locally(
                fixture.directory.path(),
                crate::local_review::Options {
                    mode: crate::local_review::Mode::Isolated,
                    max_repository_mb: 16,
                    fetch_missing: false,
                    ..Default::default()
                },
                receipts.path(),
                &control(),
            )
            .unwrap();
        assert_eq!(local.metadata(), &metadata);
        assert_eq!(
            local
                .comparison()
                .history(&crate::history::HistoryQuery::default())
                .unwrap()
                .revisions[0]
                .commit_id,
            fixture.head
        );
        assert!(local.comparison().hosted().unwrap().is_local());
        let stable = local.comparison().catalog().find("stable.rs").unwrap();
        assert!(stable.kind().is_none());
        assert_eq!(
            local.comparison().read_file(stable).unwrap().1,
            "fn stable() {}\n"
        );
        let changed = local.comparison().catalog().find("changed.rs").unwrap();
        assert_eq!(
            local.comparison().read_file(changed).unwrap(),
            ("fn before() {}\n".into(), "fn after() {}\n".into())
        );
        let checked = review
            .locally(
                fixture.directory.path(),
                crate::local_review::Options {
                    mode: crate::local_review::Mode::CheckoutIfClean,
                    max_repository_mb: 16,
                    fetch_missing: false,
                    ..Default::default()
                },
                receipts.path(),
                &control(),
            )
            .unwrap();
        assert_eq!(checked.metadata(), &metadata);
        assert!(checked.checkout().unwrap().active());
        assert_eq!(
            crate::local_review::tests::git(fixture.directory.path(), &["rev-parse", "HEAD"]),
            fixture.head
        );
        let receipt = crate::local_review::checkout::recoveries(receipts.path())
            .unwrap()
            .pop()
            .unwrap();
        crate::local_review::checkout::restore(&receipt.path).unwrap();
        assert_eq!(
            crate::local_review::tests::git(fixture.directory.path(), &["symbolic-ref", "HEAD"]),
            "refs/heads/main"
        );
        let status = local
            .comparison()
            .hosted()
            .unwrap()
            .status(&control())
            .unwrap();
        assert!(
            matches!(
                status.reviews,
                crate::hosting::status::Availability::Unavailable(_)
            ),
            "status still uses the original account-bound provider, never a Git guess"
        );
    }
}
