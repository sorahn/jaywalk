use crate::repository_file::{ChangeKind, FileCatalog, FileEntry, FileKind};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileSide {
    Before,
    After,
}

/// Equality identifies one coherent read source, not just mutable endpoint names.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComparisonIdentity {
    root: PathBuf,
    operation: String,
    base: String,
    tip: String,
    source: SourceIdentity,
    capture: u64,
    text_limit: crate::text_content::TextLimit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SourceIdentity {
    Recorded,
    Git,
    External,
    Evolution,
}

pub(crate) fn next_capture() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Debug)]
pub struct ChangedFile {
    kind: ChangeKind,
    before_path: String,
    after_path: String,
}
impl ChangedFile {
    pub fn new(kind: ChangeKind, before_path: String, after_path: String) -> Self {
        Self {
            kind,
            before_path,
            after_path,
        }
    }
    pub fn kind(&self) -> &ChangeKind {
        &self.kind
    }
    /// Original unknown statuses remain available for diagnostics and display.
    pub fn status(&self) -> &str {
        self.kind.as_str()
    }
    pub fn before_path(&self) -> &str {
        &self.before_path
    }
    pub fn after_path(&self) -> &str {
        &self.after_path
    }
}

/// A fully assembled read snapshot. Endpoints, source and catalog cannot be
/// independently replaced after construction; clones share immutable contents.
#[derive(Clone, Debug)]
pub struct Comparison {
    root: PathBuf,
    operation: String,
    base: String,
    tip: String,
    files: Arc<[ChangedFile]>,
    catalog: Arc<FileCatalog>,
    content: ContentSource,
    capture: u64,
    text_limit: crate::text_content::TextLimit,
}

/// Resolved backend metadata, consumed only while assembling a snapshot.
/// Captures distinguish separate reads of mutable Git/external sources.
pub(crate) struct ComparisonMetadata {
    pub root: PathBuf,
    pub operation: String,
    pub base: String,
    pub tip: String,
    pub capture: u64,
}

#[derive(Clone, Debug)]
pub(crate) enum ContentSource {
    Recorded,
    Git(Arc<crate::git::GitContents>),
    External(Arc<crate::external::ExternalContents>),
    Evolution(crate::evolution::EvolutionContents),
}

pub(crate) fn query(root: &Path, operation: &str, arguments: &[&str]) -> Result<String, String> {
    let result = crate::tools::command("jj")?
        .current_dir(root)
        .args([
            "--ignore-working-copy",
            "--no-pager",
            "--color",
            "never",
            "--at-op",
            operation,
        ])
        .args(arguments)
        .output()
        .map_err(|error| format!("Could not execute jj: {error}"))?;

    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }

    String::from_utf8(result.stdout).map_err(|_| "This file contains non-UTF-8 data.".to_owned())
}

fn single_revision(root: &Path, operation: &str, revset: &str) -> Result<String, String> {
    let result = query(
        root,
        operation,
        &[
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            "commit_id ++ \"\\n\"",
        ],
    )?;
    let commits: Vec<_> = result.lines().collect();

    if commits.len() != 1 {
        return Err(format!(
            "Comparison endpoint {revset:?} resolved to {} revisions; supply --base and --tip explicitly.",
            commits.len()
        ));
    }

    Ok(commits[0].to_owned())
}

pub fn parse_changed_files(output: &str) -> Result<Vec<ChangedFile>, String> {
    output
        .lines()
        .map(|line| {
            let fields: [String; 3] = serde_json::from_str(line)
                .map_err(|error| format!("Invalid jj diff entry: {error}"))?;
            let [status, before_path, after_path] = fields;
            Ok(ChangedFile::new(
                status.as_str().into(),
                before_path,
                after_path,
            ))
        })
        .collect()
}

impl Comparison {
    /// Reverse an immutable capture without reading a later working copy.
    pub fn swapped(&self) -> Result<Self, String> {
        let content = match &self.content {
            ContentSource::Recorded => {
                return Self::between(
                    self.root.clone(),
                    self.operation.clone(),
                    self.tip.clone(),
                    self.base.clone(),
                    self.text_limit,
                );
            }
            ContentSource::Git(contents) => ContentSource::Git(Arc::new(contents.swapped())),
            ContentSource::External(contents) => {
                ContentSource::External(Arc::new(contents.swapped()))
            }
            ContentSource::Evolution(_) => {
                return Err("Swap is unavailable for patch interdiffs".into());
            }
        };
        let files = self
            .files
            .iter()
            .map(|file| {
                ChangedFile::new(
                    match file.kind() {
                        ChangeKind::Added => ChangeKind::Removed,
                        ChangeKind::Removed => ChangeKind::Added,
                        ChangeKind::Copied => ChangeKind::Modified,
                        kind => kind.clone(),
                    },
                    file.after_path().into(),
                    file.before_path().into(),
                )
            })
            .collect();
        Self::assemble(
            ComparisonMetadata {
                root: self.root.clone(),
                operation: self.operation.clone(),
                base: self.tip.clone(),
                tip: self.base.clone(),
                capture: self.capture,
            },
            files,
            content,
            self.text_limit,
        )
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn operation(&self) -> &str {
        &self.operation
    }
    pub fn base(&self) -> &str {
        &self.base
    }
    pub fn tip(&self) -> &str {
        &self.tip
    }
    pub fn files(&self) -> &[ChangedFile] {
        &self.files
    }
    pub fn catalog(&self) -> &FileCatalog {
        &self.catalog
    }
    pub(crate) fn content(&self) -> &ContentSource {
        &self.content
    }
    pub(crate) fn text_limit(&self) -> crate::text_content::TextLimit {
        self.text_limit
    }

    pub(crate) fn assemble(
        metadata: ComparisonMetadata,
        files: Vec<ChangedFile>,
        content: ContentSource,
        text_limit: crate::text_content::TextLimit,
    ) -> Result<Self, String> {
        let ComparisonMetadata {
            root,
            operation,
            base,
            tip,
            capture,
        } = metadata;
        Self {
            root,
            operation,
            base,
            tip,
            capture,
            files: files.into(),
            content,
            text_limit,
            catalog: Arc::default(),
        }
        .with_catalog()
    }

    pub fn current_operation(directory: &Path) -> Result<String, String> {
        query(
            directory,
            "@",
            &["op", "log", "--no-graph", "-n", "1", "-T", "id"],
        )
        .map(|id| id.trim().to_owned())
    }

    /// Resolve visible versions only. Ambiguity is an error, never a newest-time guess.
    pub fn revision(&self, expression: &str) -> Result<crate::history::Revision, String> {
        let commit = single_revision(&self.root, &self.operation, expression)?;
        let output = query(
            &self.root,
            &self.operation,
            &[
                "log",
                "--no-graph",
                "-r",
                &commit,
                "-T",
                crate::history::TEMPLATE,
            ],
        )?;
        crate::history::History::parse(&output)?
            .revisions
            .into_iter()
            .next()
            .ok_or_else(|| "Selected change is unavailable".into())
    }

    pub fn load(directory: &Path, base: Option<&str>, tip: &str) -> Result<Self, String> {
        Self::load_with_limit(
            directory,
            base,
            tip,
            crate::text_content::TextLimit::default(),
        )
    }

    pub fn load_with_limit(
        directory: &Path,
        base: Option<&str>,
        tip: &str,
        limit: crate::text_content::TextLimit,
    ) -> Result<Self, String> {
        // Prefer jj in colocated repositories. Inspect metadata rather than
        // treating an arbitrary jj error as permission to change backends.
        let directory = std::fs::canonicalize(directory)
            .map_err(|error| format!("Could not open {}: {error}", directory.display()))?;
        if !directory.ancestors().any(|path| path.join(".jj").is_dir()) {
            return crate::git::load(&directory, base, tip, limit);
        }
        let root = PathBuf::from(query(&directory, "@", &["root"])?.trim());
        let operation = query(
            &root,
            "@",
            &["op", "log", "--no-graph", "-n", "1", "-T", "id"],
        )?;
        let operation = operation.trim().to_owned();
        Self::recorded_at(root, operation, base, tip, limit)
    }

    pub(crate) fn recorded_at(
        root: PathBuf,
        operation: String,
        base: Option<&str>,
        tip: &str,
        limit: crate::text_content::TextLimit,
    ) -> Result<Self, String> {
        let tip = single_revision(&root, &operation, tip)?;
        let default_base = format!("heads(::{tip} & immutable())");
        let base = single_revision(&root, &operation, base.unwrap_or(&default_base))?;
        Self::between(root, operation, base, tip, limit)
    }

    /// Resolve a followed change without confusing absence with a query failure.
    pub(crate) fn visible_change(
        &self,
        change: &str,
    ) -> Result<Option<crate::history::Revision>, String> {
        let expression = format!("all() & present({change})");
        let output = query(
            &self.root,
            &self.operation,
            &[
                "log",
                "--no-graph",
                "-r",
                &expression,
                "-T",
                crate::history::TEMPLATE,
            ],
        )?;
        let mut revisions = crate::history::History::parse(&output)?.revisions;
        match revisions.len() {
            0 => Ok(None),
            1 => Ok(revisions.pop()),
            _ => Err("The selected change has divergent visible versions; select a specific history row.".into()),
        }
    }

    pub(crate) fn between(
        root: PathBuf,
        operation: String,
        base: String,
        tip: String,
        limit: crate::text_content::TextLimit,
    ) -> Result<Self, String> {
        let template = "\"[\" ++ json(self.status()) ++ \",\" ++ json(self.source().path()) ++ \",\" ++ json(self.target().path()) ++ \"]\\n\"";
        let output = query(
            &root,
            &operation,
            &["diff", "--from", &base, "--to", &tip, "-T", template],
        )?;
        let files = parse_changed_files(&output)
            .map_err(|error| format!("Could not read changed files: {error}"))?;

        Self::assemble(
            ComparisonMetadata {
                root,
                operation,
                base,
                tip,
                capture: 0,
            },
            files,
            ContentSource::Recorded,
            limit,
        )
    }

    /// Compare a recorded revision against its first parent. Merge revisions
    /// deliberately use the first parent, and the UI labels this choice.
    pub fn for_revision(&self, revision: &crate::history::Revision) -> Result<Self, String> {
        self.for_history_revision(revision, None)
    }

    pub fn for_history_revision(
        &self,
        revision: &crate::history::Revision,
        operation: Option<&str>,
    ) -> Result<Self, String> {
        if matches!(self.content, ContentSource::Git(_)) {
            return crate::git::for_revision(self, revision);
        }
        let base = revision
            .parents
            .first()
            .unwrap_or(&revision.commit_id)
            .clone();
        Self::between(
            self.root.clone(),
            operation.unwrap_or(&self.operation).to_owned(),
            base,
            revision.commit_id.clone(),
            self.text_limit,
        )
    }

    pub fn supports_evolution(&self) -> bool {
        matches!(
            self.content,
            ContentSource::Recorded | ContentSource::Evolution(_)
        )
    }

    pub fn history(
        &self,
        history: &crate::history::HistoryQuery,
    ) -> Result<crate::history::History, String> {
        if matches!(self.content, ContentSource::Git(_)) {
            return crate::git::history(&self.root, history);
        }
        let references = self.reference_snapshot()?;
        let revset = history.jj_revset(&self.tip, &references);
        let limit = history.limit().to_string();
        let output = query(
            &self.root,
            &self.operation,
            &[
                "log",
                "--no-graph",
                "-n",
                &limit,
                "-r",
                &revset,
                "-T",
                crate::history::TEMPLATE,
            ],
        )?;
        Ok(crate::history::History::parse(&output)?.with_references(references))
    }

    pub fn current_history(
        &self,
        history: &crate::history::HistoryQuery,
    ) -> Result<crate::history::History, String> {
        if !self.supports_evolution() {
            return self.history(history);
        }
        let operation = Self::current_operation(&self.root)?;
        let references = crate::references::recorded(&self.root, &operation)?;
        let revset = history.jj_revset(&self.tip, &references);
        let output = query(
            &self.root,
            &operation,
            &[
                "log",
                "--no-graph",
                "-n",
                &history.limit().to_string(),
                "-r",
                &revset,
                "-T",
                crate::history::TEMPLATE,
            ],
        )?;
        Ok(crate::history::History::parse(&output)?.with_references(references))
    }

    fn with_catalog(mut self) -> Result<Self, String> {
        // Comparisons are assembled on the background executor. Clone/selection
        // paths share the index; a bounded cache also reuses immutable snapshots.
        const MAX_CACHED_CATALOGS: usize = 16;
        type Cache = std::collections::VecDeque<(ComparisonIdentity, Arc<FileCatalog>)>;
        static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
        let key = self.identity();
        let cache = CACHE.get_or_init(Default::default);
        if let Some(catalog) = cache
            .lock()
            .map_err(|_| "File catalog cache lock poisoned")?
            .iter()
            .find(|(id, _)| id == &key)
            .map(|(_, catalog)| catalog.clone())
        {
            self.catalog = catalog;
            return Ok(self);
        }
        let (before, after) = match &self.content {
            ContentSource::Git(contents) => (
                contents.paths(FileSide::Before),
                contents.paths(FileSide::After),
            ),
            ContentSource::External(contents) => (
                contents.paths(FileSide::Before)?,
                contents.paths(FileSide::After)?,
            ),
            ContentSource::Recorded => (self.file_kinds(&self.base)?, self.file_kinds(&self.tip)?),
            ContentSource::Evolution(contents) => {
                let target = self.file_kinds(&self.tip)?;
                (
                    contents.paths(FileSide::Before, target.clone()),
                    contents.paths(FileSide::After, target),
                )
            }
        };
        let catalog = FileCatalog::new(before, after, &self.files);
        self.catalog = Arc::new(catalog);
        let mut cached = cache
            .lock()
            .map_err(|_| "File catalog cache lock poisoned")?;
        cached.push_back((key, self.catalog.clone()));
        while cached.len() > MAX_CACHED_CATALOGS {
            cached.pop_front();
        }
        drop(cached);
        Ok(self)
    }

    pub fn identity(&self) -> ComparisonIdentity {
        ComparisonIdentity {
            root: self.root.clone(),
            operation: self.operation.clone(),
            base: self.base.clone(),
            tip: self.tip.clone(),
            source: match self.content {
                ContentSource::Recorded => SourceIdentity::Recorded,
                ContentSource::Git(_) => SourceIdentity::Git,
                ContentSource::External(_) => SourceIdentity::External,
                ContentSource::Evolution(_) => SourceIdentity::Evolution,
            },
            capture: self.capture,
            text_limit: self.text_limit,
        }
    }

    /// Metadata lookup remains changed-only until full-tree enumeration is added.
    pub fn changed_file(&self, path: &str) -> Option<FileEntry> {
        crate::file_browser::find_file(&self.files, path).map(|index| {
            self.catalog
                .find(&self.files[index].after_path)
                .cloned()
                .unwrap_or_else(|| FileEntry::from(&self.files[index]))
        })
    }

    /// Read by source-side path, independent of a change-list position or status.
    /// Captured Git worktree sources deliberately reject paths not captured when
    /// the comparison loaded; a later implementation must not read them live.
    pub fn read_path(&self, path: &str, side: FileSide) -> Result<String, String> {
        match &self.content {
            ContentSource::Git(contents) => contents.read_path(path, side),
            ContentSource::External(contents) => contents.read_path(path, side),
            ContentSource::Evolution(contents) => {
                if contents.contains(path, side) {
                    contents.read_path(path, side, self.text_limit)
                } else {
                    self.file_contents(&self.tip, path)
                }
            }
            ContentSource::Recorded => self.file_contents(
                match side {
                    FileSide::Before => &self.base,
                    FileSide::After => &self.tip,
                },
                path,
            ),
        }
    }

    pub fn read_file(&self, file: &FileEntry) -> Result<(String, String), String> {
        if matches!(
            file.content_kind(),
            FileKind::Submodule | FileKind::Other(_)
        ) {
            return Err(format!(
                "Text view is unavailable for {:?} entries",
                file.content_kind()
            ));
        }
        if file.kind().is_none() {
            let after = self.read_path(file.path(), FileSide::After)?;
            return Ok((after.clone(), after));
        }
        let before = if file.kind() == Some(&ChangeKind::Added) {
            String::new()
        } else {
            self.read_path(file.before_path(), FileSide::Before)?
        };
        let after = if file.kind() == Some(&ChangeKind::Removed) {
            String::new()
        } else {
            self.read_path(file.path(), FileSide::After)?
        };
        Ok((before, after))
    }

    pub fn contents(&self, file: &ChangedFile) -> Result<(String, String), String> {
        match self.catalog.find(&file.after_path) {
            Some(entry) => self.read_file(entry),
            None => self.read_file(&FileEntry::from(file)),
        }
    }

    fn file_kinds(
        &self,
        revision: &str,
    ) -> Result<std::collections::BTreeMap<String, FileKind>, String> {
        let template = r#""[" ++ json(path) ++ "," ++ json(file_type) ++ "]\n""#;
        let output = query(
            &self.root,
            &self.operation,
            &["file", "list", "-r", revision, "-T", template],
        )?;
        output
            .lines()
            .map(|line| {
                let [path, kind]: [String; 2] = serde_json::from_str(line)
                    .map_err(|error| format!("Invalid revision file entry: {error}"))?;
                Ok((path, FileKind::from(kind.as_str())))
            })
            .collect()
    }

    fn file_contents(&self, revision: &str, path: &str) -> Result<String, String> {
        let fileset = format!(
            "root-file:{}",
            serde_json::to_string(path).map_err(|error| error.to_string())?
        );
        let template = r#""[" ++ json(file_type) ++ "," ++ if(normal_value, json(normal_value.object_id()), "null") ++ "]""#;
        let metadata = query(
            &self.root,
            &self.operation,
            &[
                "file", "list", "-r", revision, "-T", template, "--", &fileset,
            ],
        )?;
        let (kind, object): (String, Option<String>) = serde_json::from_str(&metadata)
            .map_err(|error| format!("Could not inspect {path:?} at {revision}: {error}"))?;
        if kind != "file" && kind != "symlink" && kind != "conflict" {
            return Err(format!("Text view unavailable for {kind} entries"));
        }
        if let Some(object) = object {
            if !object.bytes().all(|byte| byte.is_ascii_hexdigit()) || object.is_empty() {
                return Err("Invalid recorded object ID".into());
            }
            let directory = query(&self.root, &self.operation, &["git", "root"])?;
            crate::git::read_blob(
                &self.root,
                Some(Path::new(directory.trim())),
                &object,
                self.text_limit,
            )
        } else {
            // Conflicted/materialized jj content has no single object size.
            let mut command = crate::tools::command("jj")?;
            command.current_dir(&self.root).args([
                "--ignore-working-copy",
                "--no-pager",
                "--color",
                "never",
                "--at-op",
                &self.operation,
                "file",
                "show",
                "-r",
                revision,
                "--",
                &fileset,
            ]);
            self.text_limit.text(self.text_limit.command(&mut command)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_renames_and_escaped_paths() {
        let output = "[\"renamed\",\"old file.ts\",\"new\\tfile.ts\"]\n[\"removed\",\"gone.ts\",\"gone.ts\"]\n";

        let files = parse_changed_files(output).unwrap();

        assert_eq!(files[0].before_path, "old file.ts");
        assert_eq!(files[0].after_path, "new\tfile.ts");
        assert_eq!(files[1].status(), "removed");
    }

    #[test]
    fn rejects_incomplete_diff_entries() {
        assert!(parse_changed_files("[\"added\",\"only-one-path\"]").is_err());
    }

    #[test]
    fn reads_pinned_contents_without_snapshotting_working_files() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("jaywalk-test-{}-{unique}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let run = |arguments: &[&str]| {
            let result = Command::new("jj")
                .current_dir(&root)
                .args([
                    "--config",
                    "user.name=Viewer Test",
                    "--config",
                    "user.email=viewer@example.invalid",
                ])
                .args(arguments)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        run(&["git", "init"]);
        std::fs::write(root.join("old file.ts"), "const value = 1;\n").unwrap();
        std::fs::write(root.join("removed.ts"), "removed\n").unwrap();
        std::fs::write(root.join("unchanged 漢\n.txt"), "pinned unchanged\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("not-a-real-target", root.join("symbolic")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("deleted-target", root.join("z-symbolic")).unwrap();
        run(&["describe", "-m", "baseline"]);
        run(&["bookmark", "create", "main"]);
        run(&["new", "main", "-m", "change files"]);
        std::fs::rename(root.join("old file.ts"), root.join("new file.ts")).unwrap();
        std::fs::remove_file(root.join("removed.ts")).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(root.join("z-symbolic")).unwrap();
        std::fs::write(root.join("added.ts"), "added\n").unwrap();
        run(&["status"]);

        let comparison = Comparison::load(&root, Some("main"), "@").unwrap();
        let unchanged = comparison.catalog.find("unchanged 漢\n.txt").unwrap();
        assert!(unchanged.kind().is_none());
        std::fs::write(root.join("unchanged 漢\n.txt"), "later edits").unwrap();
        assert_eq!(
            comparison.read_file(unchanged).unwrap().1,
            "pinned unchanged\n"
        );
        let same = Comparison::load(&root, Some("main"), "@").unwrap();
        assert!(Arc::ptr_eq(&comparison.catalog, &same.catalog));
        #[cfg(unix)]
        {
            let link = comparison.catalog.find("symbolic").unwrap();
            assert_eq!(link.content_kind(), &FileKind::Symlink);
            assert_eq!(comparison.read_file(link).unwrap().1, "not-a-real-target");
            let removed_link = comparison.catalog.find("z-symbolic").unwrap();
            assert_eq!(
                comparison.read_file(removed_link).unwrap(),
                ("deleted-target".into(), String::new())
            );
        }
        let operation_before = comparison.operation.clone();
        let history = comparison.history(&Default::default()).unwrap();
        let selected = history
            .revisions
            .iter()
            .find(|revision| revision.commit_id == comparison.tip)
            .unwrap();
        let revision_comparison = comparison.for_revision(selected).unwrap();
        assert_eq!(revision_comparison.base, comparison.base);
        assert_eq!(revision_comparison.operation, comparison.operation);
        std::fs::write(root.join("added.ts"), "unsnapshotted\n").unwrap();
        let added = comparison
            .files
            .iter()
            .find(|file| file.status() == "added")
            .unwrap();
        let renamed = comparison
            .files
            .iter()
            .find(|file| file.status() == "renamed")
            .unwrap();
        let removed = comparison
            .files
            .iter()
            .find(|file| file.status() == "removed")
            .unwrap();

        assert_eq!(
            comparison.contents(added).unwrap(),
            (String::new(), "added\n".into())
        );
        assert_eq!(
            comparison.contents(renamed).unwrap(),
            ("const value = 1;\n".into(), "const value = 1;\n".into())
        );
        assert_eq!(
            comparison.contents(removed).unwrap(),
            ("removed\n".into(), String::new())
        );
        assert_eq!(
            Comparison::load(&root, Some("main"), "@")
                .unwrap()
                .operation,
            operation_before
        );
        // A merge comparison follows first-parent review semantics, while
        // history still exposes both edges of the recorded DAG.
        run(&["new", &comparison.tip, "-m", "left branch"]);
        std::fs::write(root.join("left.rs"), "left\n").unwrap();
        run(&["bookmark", "create", "left"]);
        run(&["new", &comparison.tip, "-m", "right branch"]);
        std::fs::write(root.join("right.rs"), "right\n").unwrap();
        run(&["bookmark", "create", "right"]);
        run(&["new", "left", "right", "-m", "merge branches"]);
        let merge = Comparison::load(&root, Some(&comparison.tip), "@").unwrap();
        let history = merge.history(&Default::default()).unwrap();
        let revision = history
            .revisions
            .iter()
            .find(|revision| revision.commit_id == merge.tip)
            .unwrap();
        assert_eq!(revision.parents.len(), 2);
        let reviewed = merge.for_revision(revision).unwrap();
        assert_eq!(reviewed.base, revision.parents[0]);
        assert!(
            !reviewed
                .files
                .iter()
                .any(|file| file.after_path == "left.rs")
        );
        let added = reviewed
            .files
            .iter()
            .find(|file| file.after_path == "right.rs")
            .unwrap();
        assert_eq!(
            reviewed.contents(added).unwrap(),
            (String::new(), "right\n".into())
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
