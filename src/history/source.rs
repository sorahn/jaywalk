use super::{
    History, HistoryQuery, Revision,
    process::{self, Format},
};
use crate::{accounts::Control, references::ReferenceSnapshot, repository::Comparison};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
enum Backend {
    Git,
    Jj { operation: String, revset: String },
}
/// Stable roots/operation and reference labels, shared by paging and search.
#[derive(Clone, Debug)]
pub struct HistorySource {
    root: PathBuf,
    backend: Backend,
    roots: Vec<String>,
    pub(super) references: Arc<ReferenceSnapshot>,
    working_copy: Option<Revision>,
    context: Option<String>,
    origin: Option<Arc<HistorySource>>,
}
#[derive(Debug)]
pub struct Pagination {
    pub(super) source: Arc<HistorySource>,
    pub(super) has_more: bool,
}

impl HistorySource {
    /// Read-only capture; never runs jj or modifies an index/checkout.
    pub fn git(root: &Path, query: &HistoryQuery) -> Result<Self, String> {
        let references = crate::git::references(root)?;
        let mut roots = query.roots(&references);
        let working_copy = if query.includes_working_copy() && !roots.is_empty() {
            let head = crate::git::resolve(root, "HEAD")?;
            let empty = crate::git::output(
                root,
                &[
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "status",
                    "--porcelain",
                    "--untracked-files=normal",
                ],
            )?
            .is_empty();
            Some(Revision {
                commit_id: "working tree".into(),
                change_id: String::new(),
                description: String::new(),
                author: String::new(),
                timestamp: String::new(),
                references: vec![],
                parents: vec![head],
                working_copy: true,
                empty,
            })
        } else {
            None
        };
        if let Some(revision) = &working_copy {
            roots.extend(revision.parents.iter().cloned());
            roots.sort();
            roots.dedup();
        }
        Ok(Self {
            root: root.to_owned(),
            backend: Backend::Git,
            roots,
            references: Arc::new(references),
            working_copy,
            context: None,
            origin: None,
        })
    }
    pub(crate) fn recorded(
        comparison: &Comparison,
        query: &HistoryQuery,
        current: bool,
    ) -> Result<Self, String> {
        let operation = if current {
            Comparison::current_operation(comparison.root())?
        } else {
            comparison.operation().to_owned()
        };
        let references = crate::references::recorded(comparison.root(), &operation)?;
        let revset = query.jj_revset(comparison.tip(), &references);
        Ok(Self {
            root: comparison.root().to_owned(),
            backend: Backend::Jj { operation, revset },
            roots: query.roots(&references),
            references: Arc::new(references),
            working_copy: None,
            context: None,
            origin: None,
        })
    }
    pub fn repository_source(self: &Arc<Self>) -> Arc<Self> {
        self.origin.clone().unwrap_or_else(|| self.clone())
    }
    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }
    pub fn around(
        self: &Arc<Self>,
        commit: &str,
        size: usize,
        control: &Control,
    ) -> Result<History, String> {
        if commit.is_empty() || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("History target must be a full resolved commit ID".into());
        }
        let mut source = self.as_ref().clone();
        source.roots = vec![commit.to_owned()];
        source.working_copy = None;
        source.context = Some(commit.to_owned());
        source.origin = Some(self.repository_source());
        if let Backend::Jj { revset, .. } = &mut source.backend {
            *revset = format!("ancestors({commit})");
        }
        Arc::new(source).first_page(size, control)
    }
    pub fn first_page(self: &Arc<Self>, size: usize, control: &Control) -> Result<History, String> {
        let (revisions, more) = self.page(0, size, control)?;
        Ok(self.assemble(revisions, more))
    }
    fn assemble(self: &Arc<Self>, revisions: Vec<Revision>, more: bool) -> History {
        let mut history = History::new(revisions).with_references(self.references.as_ref().clone());
        history.references = self.references.clone();
        history.pagination = Some(Arc::new(Pagination {
            source: self.clone(),
            has_more: more,
        }));
        history
    }
    fn page(
        &self,
        skip: usize,
        size: usize,
        control: &Control,
    ) -> Result<(Vec<Revision>, bool), String> {
        let requested = size
            .checked_add(1)
            .filter(|_| size > 0)
            .ok_or("History page size must be positive and representable")?;
        let mut rows = Vec::with_capacity(requested.min(5001));
        self.scan(skip, Some(requested), control, |revision| {
            rows.push(revision);
            rows.len() <= size
        })?;
        let more = rows.len() > size;
        rows.truncate(size);
        Ok((rows, more))
    }
    pub(super) fn scan(
        &self,
        skip: usize,
        count: Option<usize>,
        control: &Control,
        mut consume: impl FnMut(Revision) -> bool,
    ) -> Result<(), String> {
        control.check().map_err(|e| e.to_string())?;
        if skip == 0
            && let Some(revision) = &self.working_copy
            && !consume(revision.clone())
        {
            return Ok(());
        }
        if self.roots.is_empty() {
            return Ok(());
        }
        let virtual_count = usize::from(self.working_copy.is_some());
        let real_skip = skip.saturating_sub(virtual_count);
        let mut command = match &self.backend {
            Backend::Git => {
                let mut command = crate::tools::command("git")?;
                command
                    .current_dir(&self.root)
                    .env("GIT_OPTIONAL_LOCKS", "0")
                    .args([
                        "--no-pager",
                        "--no-replace-objects",
                        "log",
                        "--topo-order",
                        "-z",
                        "--format=%H%x00%P%x00%B%x00%an%x00%aI",
                    ])
                    .arg(format!("--skip={real_skip}"));
                if let Some(count) = count {
                    command.arg(format!("--max-count={count}"));
                }
                command.args(&self.roots).arg("--");
                command
            }
            Backend::Jj { operation, revset } => {
                let mut command = crate::tools::command("jj")?;
                command.current_dir(&self.root).args([
                    "--ignore-working-copy",
                    "--no-pager",
                    "--color",
                    "never",
                    "--at-op",
                    operation,
                    "log",
                    "--no-graph",
                    "-r",
                    revset,
                    "-T",
                    super::TEMPLATE,
                ]);
                if let Some(count) = count {
                    command.args(["-n", &(real_skip.saturating_add(count)).to_string()]);
                }
                command
            }
        };
        command.env("GIT_OPTIONAL_LOCKS", "0");
        let mut skipped = 0;
        let jj = matches!(self.backend, Backend::Jj { .. });
        process::scan(
            command,
            if jj { Format::Jj } else { Format::Git },
            control,
            |revision| {
                if jj && skipped < real_skip {
                    skipped += 1;
                    return true;
                }
                consume(revision)
            },
        )
    }
}
impl History {
    pub fn source(&self) -> Option<Arc<HistorySource>> {
        self.pagination.as_ref().map(|page| page.source.clone())
    }
    pub fn has_more(&self) -> bool {
        self.pagination.as_ref().is_some_and(|page| page.has_more)
    }
    pub fn load_more(&self, size: usize, control: &Control) -> Result<Self, String> {
        let page = self
            .pagination
            .as_ref()
            .filter(|page| page.has_more)
            .ok_or("No older history available")?;
        let (next, more) = page.source.page(self.revisions.len(), size, control)?;
        let mut rows = self.revisions.clone();
        rows.extend(next);
        Ok(page.source.assemble(rows, more))
    }
}
