//! Change evolution follows predecessor edges, independent of commit ancestry.
pub mod grouping;
use crate::history::{History, Revision};
use crate::repository::{
    ChangedFile, Comparison, ComparisonMetadata, ContentSource, FileSide, query,
};
use crate::repository_file::FileKind;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
#[cfg(test)]
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize)]
pub struct Version {
    pub commit_id: String,
    pub change_id: String,
    pub description: String,
    pub parents: Vec<String>,
    pub predecessors: Vec<String>,
    pub timestamp: String,
    pub operation_id: Option<String>,
    pub operation: String,
    pub hidden: bool,
    pub divergent: bool,
    pub conflict: bool,
    pub patch_changed: bool,
    patch_files: Vec<FileDelta>,
    evolution_files: Vec<FileDelta>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReviewMode {
    Patch,
    SincePrevious,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileEffect {
    Renamed { unchanged: bool },
    Copied { unchanged: bool },
    Added,
    Removed,
    Edited,
    Metadata,
    Conflict,
    Unchanged,
}

impl FileEffect {
    pub fn label(self) -> &'static str {
        match self {
            Self::Renamed { unchanged: true } => "renamed · contents unchanged",
            Self::Renamed { unchanged: false } => "renamed · contents changed",
            Self::Copied { unchanged: true } => "copied · contents unchanged",
            Self::Copied { unchanged: false } => "copied · contents changed",
            Self::Added => "file added",
            Self::Removed => "file removed",
            Self::Edited => "file contents changed",
            Self::Metadata => "file metadata changed",
            Self::Conflict => "file conflicted",
            Self::Unchanged => "no changes to this file",
        }
    }

    pub fn changes_contents(self) -> bool {
        !matches!(
            self,
            Self::Renamed { unchanged: true }
                | Self::Copied { unchanged: true }
                | Self::Metadata
                | Self::Unchanged
        )
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct FileDelta {
    status: String,
    before_path: String,
    after_path: String,
    unchanged: bool,
    conflict: bool,
}

impl FileDelta {
    fn effect(&self) -> FileEffect {
        if self.conflict {
            return FileEffect::Conflict;
        }
        match self.status.as_str() {
            "renamed" => FileEffect::Renamed {
                unchanged: self.unchanged,
            },
            "copied" => FileEffect::Copied {
                unchanged: self.unchanged,
            },
            "added" => FileEffect::Added,
            "removed" => FileEffect::Removed,
            _ if self.unchanged => FileEffect::Metadata,
            _ => FileEffect::Edited,
        }
    }
}

// Only paths, types, and immutable object identities are read here, never file
// contents. Equal resolved object IDs establish byte-for-byte unchanged content.
const FILE_DELTA_TEMPLATE: &str = r#""{" ++ '"status":' ++ json(self.status()) ++ ',"before_path":' ++ json(self.source().path()) ++ ',"after_path":' ++ json(self.target().path()) ++ ',"unchanged":' ++ json(self.source().normal_value() && self.target().normal_value() && self.source().normal_value().file_type() == self.target().normal_value().file_type() && self.source().normal_value().object_id() == self.target().normal_value().object_id()) ++ ',"conflict":' ++ json(self.source().conflict() || self.target().conflict()) ++ "}""#;

fn file_delta_array(entries: &str) -> String {
    format!(
        r#""[" ++ {entries}.map(|entry| {}).join(",") ++ "]""#,
        FILE_DELTA_TEMPLATE.replace("self.", "entry.")
    )
}

impl Version {
    /// Summarize the actual rewrite, rather than repeating a generic event name.
    pub fn rewrite_summary(&self, event: Event) -> String {
        if self.evolution_files.is_empty() {
            return event.label().to_owned();
        }
        if self.evolution_files.len() == 1 {
            let file = &self.evolution_files[0];
            let action = match file.effect() {
                FileEffect::Added => "added",
                FileEffect::Removed => "removed",
                FileEffect::Renamed { .. } => "renamed",
                FileEffect::Copied { .. } => "copied",
                FileEffect::Conflict => "conflict in",
                FileEffect::Metadata => "metadata in",
                _ => "updated",
            };
            let name = crate::file_browser::display_component(
                file.after_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&file.after_path),
            );
            return format!("{action} {name}");
        }
        let mut counts = [0; 6];
        for file in &self.evolution_files {
            let category = match file.effect() {
                FileEffect::Added => 0,
                FileEffect::Removed => 1,
                FileEffect::Renamed { .. } => 2,
                FileEffect::Copied { .. } => 3,
                FileEffect::Conflict => 4,
                _ => 5,
            };
            counts[category] += 1;
        }
        counts
            .into_iter()
            .zip([
                "added",
                "removed",
                "renamed",
                "copied",
                "conflicted",
                "updated",
            ])
            .filter(|(count, _)| *count > 0)
            .map(|(count, label)| format!("{count} {label}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn file_effect(&self, path: &str, mode: ReviewMode) -> FileEffect {
        let files = match mode {
            ReviewMode::Patch => &self.patch_files,
            ReviewMode::SincePrevious => &self.evolution_files,
        };
        // Prefer a destination match when an old path is reused in the same
        // patch. A source match still identifies a moved/deleted selected path.
        files
            .iter()
            .find(|file| file.after_path == path)
            .or_else(|| files.iter().find(|file| file.before_path == path))
            .map_or(FileEffect::Unchanged, FileDelta::effect)
    }

    pub fn revision(&self) -> Revision {
        Revision {
            commit_id: self.commit_id.clone(),
            change_id: self.change_id.clone(),
            description: self.description.clone(),
            author: String::new(),
            timestamp: String::new(),
            references: Vec::new(),
            parents: self.parents.clone(),
            working_copy: false,
            empty: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Created,
    Edited,
    Rebased,
    Described,
    Combined,
    Metadata,
    Recorded,
}

impl Event {
    pub fn label(self) -> &'static str {
        match self {
            Self::Created => "first recorded",
            Self::Edited => "patch edited",
            Self::Rebased => "rebased · patch unchanged",
            Self::Described => "description changed",
            Self::Combined => "combined predecessors",
            Self::Metadata => "metadata changed",
            Self::Recorded => "recorded version",
        }
    }
}

/// A presentation graph over stable indices into the complete timeline.
/// Filtering never changes the revisions used by a comparison.
pub struct EvolutionView {
    pub indices: Vec<usize>,
    pub graph: History,
}

pub struct Evolution {
    pub versions: Vec<Version>,
    pub graph: History,
    pub truncated: bool,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Evolution {
    /// Keep an actively reviewed version visible when a smaller fetch limit
    /// truncates it. The caller retains its existing pinned comparison.
    pub fn retain_selected(&mut self, selected: &Version) {
        if self
            .versions
            .iter()
            .any(|version| version.commit_id == selected.commit_id)
        {
            return;
        }
        self.versions.push(selected.clone());
        self.graph = History::new(
            self.versions
                .iter()
                .map(|version| {
                    let mut revision = version.revision();
                    revision.parents = version.predecessors.clone();
                    revision
                })
                .collect(),
        );
    }

    fn parse(output: &str, limit: usize) -> Result<Self, String> {
        let mut versions = Vec::<Version>::new();
        let mut seen = HashSet::new();
        for line in output.lines() {
            let version: Version = serde_json::from_str(line)
                .map_err(|error| format!("Invalid jj evolution metadata: {error}"))?;
            if !valid_id(&version.commit_id)
                || version
                    .operation_id
                    .as_deref()
                    .is_some_and(|id| !valid_id(id))
                || version
                    .parents
                    .iter()
                    .chain(&version.predecessors)
                    .any(|id| !valid_id(id))
                || !seen.insert(version.commit_id.clone())
            {
                return Err("Invalid or duplicate ID in jj evolution metadata".into());
            }
            versions.push(version);
        }
        let truncated = versions.len() > limit;
        versions.truncate(limit);
        let graph = History::new(
            versions
                .iter()
                .map(|version| {
                    let mut revision = version.revision();
                    // These are rewrite relationships, never commit-parent edges.
                    revision.parents = version.predecessors.clone();
                    revision
                })
                .collect(),
        );
        Ok(Self {
            versions,
            graph,
            truncated,
        })
    }

    pub fn view(&self, show_unchanged_rebases: bool, selected: usize) -> EvolutionView {
        let indices: Vec<_> = (0..self.versions.len())
            .filter(|&index| {
                show_unchanged_rebases
                    || index == selected
                    || !self.versions[index].hidden
                    || self.event(index) != Event::Rebased
            })
            .collect();
        let retained: HashSet<_> = indices.iter().copied().collect();
        let by_id: HashMap<_, _> = self
            .versions
            .iter()
            .enumerate()
            .map(|(index, version)| (version.commit_id.as_str(), index))
            .collect();
        let revisions = indices
            .iter()
            .map(|&index| {
                let mut revision = self.versions[index].revision();
                let mut parents = Vec::new();
                let mut pending: Vec<_> = self.versions[index].predecessors.iter().rev().collect();
                let mut visited = HashSet::new();
                while let Some(id) = pending.pop() {
                    if !visited.insert(id.as_str()) {
                        continue;
                    }
                    if let Some(&ancestor) = by_id.get(id.as_str())
                        && !retained.contains(&ancestor)
                    {
                        // Bridge hidden rewrite nodes, retaining their branching and
                        // shared predecessors instead of connecting adjacent rows.
                        pending.extend(self.versions[ancestor].predecessors.iter().rev());
                    } else {
                        parents.push(id.clone());
                    }
                }
                revision.parents = parents;
                revision
            })
            .collect();
        EvolutionView {
            indices,
            graph: History::new(revisions),
        }
    }

    pub fn event(&self, index: usize) -> Event {
        let version = &self.versions[index];
        if version.predecessors.is_empty() {
            return Event::Created;
        }
        if version.predecessors.len() > 1 {
            return Event::Combined;
        }
        if version.patch_changed {
            return Event::Edited;
        }
        if let Some(previous) = self
            .versions
            .iter()
            .find(|other| version.predecessors.contains(&other.commit_id))
        {
            if version.description != previous.description {
                return Event::Described;
            }
            if version.parents != previous.parents {
                return Event::Rebased;
            }
            Event::Metadata
        } else {
            Event::Recorded
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct FileValue {
    kind: String,
    object: Option<String>,
    conflict: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct FileEntry {
    status: String,
    before_path: String,
    after_path: String,
    before: FileValue,
    after: FileValue,
}

/// jj's rebased interdiff can reference synthesized blobs absent from either
/// original commit. Read those immutable objects from its Git backend instead
/// of comparing the original snapshots (which would include ancestor edits).
#[derive(Clone, Debug)]
pub(crate) struct EvolutionContents {
    git_dir: PathBuf,
    entries: Arc<HashMap<String, (FileValue, FileValue)>>,
    before_paths: Arc<HashMap<String, String>>,
}

impl EvolutionContents {
    pub(crate) fn paths(
        &self,
        side: FileSide,
        mut target: BTreeMap<String, FileKind>,
    ) -> BTreeMap<String, FileKind> {
        // Unchanged paths come from the target tree. Changed paths must use
        // jj's synthesized interdiff values rather than original commit trees.
        for path in self.entries.keys() {
            target.remove(path);
        }
        let insert = |target: &mut BTreeMap<String, FileKind>, path: &str, value: &FileValue| {
            // jj reports an absent side with an empty file_type string.
            if !value.kind.is_empty() {
                let kind = if value.conflict {
                    FileKind::Conflict
                } else {
                    FileKind::from(value.kind.as_str())
                };
                target.insert(path.to_owned(), kind);
            }
        };
        match side {
            FileSide::Before => {
                for (path, key) in self.before_paths.iter() {
                    insert(&mut target, path, &self.entries[key].0);
                }
            }
            FileSide::After => {
                for (path, (_, value)) in self.entries.iter() {
                    insert(&mut target, path, value);
                }
            }
        }
        target
    }

    fn blob(
        &self,
        value: &FileValue,
        limit: crate::text_content::TextLimit,
    ) -> Result<String, String> {
        if value.conflict {
            return Err("Interdiff contains a conflict after rebasing the earlier patch onto these parents.".into());
        }
        let Some(id) = &value.object else {
            return Ok(String::new());
        };
        if value.kind != "file" && value.kind != "symlink" {
            return Err(format!(
                "Text interdiff unavailable for {} entries.",
                value.kind
            ));
        }
        if !valid_id(id) {
            return Err("Invalid interdiff object ID".into());
        }
        crate::git::read_blob(&self.git_dir, Some(&self.git_dir), id, limit)
    }

    pub(crate) fn contains(&self, path: &str, side: FileSide) -> bool {
        match side {
            FileSide::Before => self.before_paths.contains_key(path),
            FileSide::After => self.entries.contains_key(path),
        }
    }
    pub(crate) fn read_path(
        &self,
        path: &str,
        side: FileSide,
        limit: crate::text_content::TextLimit,
    ) -> Result<String, String> {
        let key = match side {
            FileSide::Before => self.before_paths.get(path).map(String::as_str),
            FileSide::After => Some(path),
        }
        .ok_or("Evolution file is absent from the recorded comparison")?;
        let (before, after) = self
            .entries
            .get(key)
            .ok_or("Evolution file is absent from the recorded comparison")?;
        self.blob(
            match side {
                FileSide::Before => before,
                FileSide::After => after,
            },
            limit,
        )
    }
}

impl Comparison {
    pub fn evolution(&self, commit: &str, limit: usize) -> Result<Evolution, String> {
        self.evolution_at(commit, limit, self.operation())
    }
    pub fn evolution_at(
        &self,
        commit: &str,
        limit: usize,
        operation: &str,
    ) -> Result<Evolution, String> {
        // Explicit fields avoid depending on jj's complete JSON representation.
        let template = r#""{" ++ '"commit_id":' ++ json(commit.commit_id()) ++ ',"change_id":' ++ json(commit.change_id()) ++ ',"description":' ++ json(commit.description()) ++ ',"parents":' ++ json(commit.parents().map(|p| p.commit_id())) ++ ',"predecessors":' ++ json(predecessors.map(|p| p.commit_id())) ++ ',"timestamp":' ++ if(operation, json(operation.time().end().local().format("%Y-%m-%d %H:%M:%S")), "\"unknown time\"") ++ ',"operation_id":' ++ if(operation, json(operation.id()), "null") ++ ',"operation":' ++ if(operation, json(operation.description()), "\"operation unavailable\"") ++ ',"hidden":' ++ json(commit.hidden()) ++ ',"divergent":' ++ json(commit.divergent()) ++ ',"conflict":' ++ json(commit.conflict()) ++ ',"patch_changed":' ++ json(self.inter_diff().files().len() > 0)"#;
        let template = format!(
            r#"{template} ++ ',"patch_files":' ++ {} ++ ',"evolution_files":' ++ {} ++ "}}\n""#,
            file_delta_array("commit.diff().files()"),
            file_delta_array("self.inter_diff().files()")
        );
        let change_id = query(
            self.root(),
            operation,
            &["log", "-G", "-r", commit, "-T", "change_id"],
        )?;
        if change_id.is_empty() || !change_id.bytes().all(|byte| (b'k'..=b'z').contains(&byte)) {
            return Err("Invalid change ID in evolution metadata".into());
        }
        // Include all current divergent versions, plus the requested historical
        // endpoint if it is hidden. Keep selection anchored by immutable ID.
        let revisions = format!(
            "change_id({}) | {commit}",
            serde_json::to_string(&change_id).map_err(|error| error.to_string())?
        );
        let output = query(
            self.root(),
            operation,
            &[
                "evolog",
                "-G",
                "-r",
                &revisions,
                "-n",
                &(limit + 1).to_string(),
                "-T",
                &template,
            ],
        )?;
        let mut evolution = Evolution::parse(&output, limit)?;
        for version in &mut evolution.versions {
            if version.parents.len() > 1 {
                // Commit.diff() compares against merged parent trees. Match the
                // viewer's explicitly labelled first-parent merge review.
                let output = query(
                    self.root(),
                    operation,
                    &[
                        "diff",
                        "--from",
                        &version.parents[0],
                        "--to",
                        &version.commit_id,
                        "-T",
                        &format!(r#"{FILE_DELTA_TEMPLATE} ++ "\n""#),
                    ],
                )?;
                version.patch_files = output
                    .lines()
                    .map(|line| {
                        serde_json::from_str(line)
                            .map_err(|error| format!("Invalid first-parent file metadata: {error}"))
                    })
                    .collect::<Result<_, _>>()?;
            }
        }
        Ok(evolution)
    }

    /// Compare a version to its actual predecessor(s), using jj's interdiff.
    /// Multiple predecessors use jj's combined predecessor semantics.
    pub fn for_evolution(&self, version: &Version) -> Result<Self, String> {
        self.for_evolution_at(version, self.operation())
    }
    pub fn for_evolution_at(&self, version: &Version, operation: &str) -> Result<Self, String> {
        if version.predecessors.is_empty() {
            return self.for_history_revision(&version.revision(), Some(operation));
        }
        let template = r#"self.inter_diff().files().map(|f| "{" ++ '"status":' ++ json(f.status()) ++ ',"before_path":' ++ json(f.source().path()) ++ ',"after_path":' ++ json(f.target().path()) ++ ',"before":{"kind":' ++ json(f.source().file_type()) ++ ',"object":' ++ if(f.source().normal_value(), json(f.source().normal_value().object_id()), "null") ++ ',"conflict":' ++ json(f.source().conflict()) ++ '},"after":{"kind":' ++ json(f.target().file_type()) ++ ',"object":' ++ if(f.target().normal_value(), json(f.target().normal_value().object_id()), "null") ++ ',"conflict":' ++ json(f.target().conflict()) ++ "}}\n").join("")"#;
        let output = query(
            self.root(),
            operation,
            &[
                "evolog",
                "-G",
                "-r",
                &version.commit_id,
                "-n",
                "1",
                "-T",
                template,
            ],
        )?;
        let entries: Vec<FileEntry> = output
            .lines()
            .map(|line| {
                serde_json::from_str(line)
                    .map_err(|error| format!("Invalid evolution file: {error}"))
            })
            .collect::<Result<_, _>>()?;
        let git_dir = PathBuf::from(query(self.root(), operation, &["git", "root"])?.trim());
        let files = entries
            .iter()
            .map(|entry| ChangedFile {
                status: entry.status.clone(),
                before_path: entry.before_path.clone(),
                after_path: entry.after_path.clone(),
            })
            .collect::<Vec<_>>();
        let before_paths = Arc::new(
            entries
                .iter()
                .map(|entry| (entry.before_path.clone(), entry.after_path.clone()))
                .collect(),
        );
        let entries = entries
            .into_iter()
            .map(|entry| (entry.after_path, (entry.before, entry.after)))
            .collect();
        Self::assemble(
            ComparisonMetadata {
                root: self.root().to_path_buf(),
                operation: operation.to_owned(),
                base: version.predecessors.join(","),
                tip: version.commit_id.clone(),
                capture: 0,
            },
            files,
            ContentSource::Evolution(EvolutionContents {
                git_dir,
                entries: Arc::new(entries),
                before_paths,
            }),
            self.text_limit(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let fixture = Self(std::env::temp_dir().join(format!(
                "viewer-evolution-{}-{unique}-{sequence}",
                std::process::id()
            )));
            std::fs::create_dir(&fixture.0).unwrap();
            fixture.run(&["git", "init"]);
            fixture
        }
        fn run(&self, args: &[&str]) -> String {
            let output = Command::new("jj")
                .current_dir(&self.0)
                .args([
                    "--config",
                    "user.name=Viewer Test",
                    "--config",
                    "user.email=viewer@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        }
        fn write(&self, path: &str, content: &str) {
            std::fs::write(self.0.join(path), content).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn evolution_catalog_uses_synthesized_side_kinds_and_keeps_deleted_links() {
        use crate::repository_file::FileKind;
        let f = Fixture::new();
        f.write("type.txt", "../target");
        f.run(&["describe", "-m", "baseline"]);
        f.run(&["bookmark", "create", "main"]);
        f.run(&["new", "-m", "first version"]);
        std::fs::remove_file(f.0.join("type.txt")).unwrap();
        std::os::unix::fs::symlink("../target", f.0.join("type.txt")).unwrap();
        std::os::unix::fs::symlink("../gone", f.0.join("gone.txt")).unwrap();
        f.run(&["describe", "-m", "first version"]);
        std::fs::remove_file(f.0.join("type.txt")).unwrap();
        f.write("type.txt", "../target");
        std::fs::remove_file(f.0.join("gone.txt")).unwrap();
        f.run(&["status"]);
        let comparison = Comparison::load(&f.0, Some("main"), "@").unwrap();
        let evolution = comparison.evolution(comparison.tip(), 100).unwrap();
        let diff = comparison.for_evolution(&evolution.versions[0]).unwrap();
        let changed = diff.catalog().find("type.txt").unwrap();
        assert_eq!(
            changed.type_change_notice().as_deref(),
            Some("File type changed: symlink → regular file")
        );
        assert_eq!(
            diff.read_file(changed).unwrap(),
            ("../target".into(), "../target".into())
        );
        let removed = diff.catalog().find("gone.txt").unwrap();
        assert_eq!(removed.content_kind(), &FileKind::Symlink);
        assert_eq!(removed.status(), Some("removed"));
        assert_eq!(
            diff.read_file(removed).unwrap(),
            ("../gone".into(), "".into())
        );
    }

    #[test]
    fn recorded_and_evolution_blobs_obey_the_captured_text_limit() {
        use crate::{repository::FileSide, text_content::TextLimit};
        let f = Fixture::new();
        f.write("large.txt", "small");
        f.run(&["describe", "-m", "small version"]);
        f.run(&["new", "-m", "large version"]);
        f.write("large.txt", &"x".repeat(1_048_577));
        f.run(&["status"]);
        let comparison =
            Comparison::load_with_limit(&f.0, Some("@-"), "@", TextLimit::from_mib(1).unwrap())
                .unwrap();
        assert!(
            comparison
                .read_path("large.txt", FileSide::After)
                .unwrap_err()
                .contains("1048577 bytes")
        );
        f.write("large.txt", &"y".repeat(1_048_578));
        f.run(&["status"]);
        let comparison =
            Comparison::load_with_limit(&f.0, Some("@-"), "@", TextLimit::from_mib(1).unwrap())
                .unwrap();
        let evolution = comparison.evolution(comparison.tip(), 100).unwrap();
        let diff = comparison.for_evolution(&evolution.versions[0]).unwrap();
        assert!(
            diff.read_path("large.txt", FileSide::Before)
                .unwrap_err()
                .contains("text limit")
        );
        assert!(
            diff.read_path("large.txt", FileSide::After)
                .unwrap_err()
                .contains("text limit")
        );
    }

    #[test]
    fn rebased_interdiff_excludes_ancestor_edits_and_preserves_recorded_versions() {
        let f = Fixture::new();
        let code = |ancestor: &str, value: u8| {
            format!(
                "// {ancestor}\n{}export const value = {value};\n",
                "// context\n".repeat(8)
            )
        };
        f.write("feature.ts", &code("ancestor one", 0));
        f.write("upstream.ts", "ancestor one\n");
        f.write("héllo.ts", "unchanged rename\n");
        f.write("remove.ts", "remove me\n");
        f.run(&["describe", "-m", "baseline"]);
        f.run(&["bookmark", "create", "main"]);
        f.run(&["new", "main", "-m", "feature"]);
        f.write("feature.ts", &code("ancestor one", 1));
        f.run(&["bookmark", "create", "feature"]);
        f.run(&["describe", "-m", "improve feature"]);
        f.run(&["new", "main", "-m", "upstream"]);
        f.write("upstream.ts", "ancestor two\n");
        f.write("feature.ts", &code("ancestor two", 0));
        f.run(&["bookmark", "create", "upstream"]);
        f.run(&["rebase", "-r", "feature", "-d", "upstream"]);
        let rebased = Comparison::load(&f.0, Some("main"), "feature").unwrap();
        let evolution = rebased.evolution(rebased.tip(), 100).unwrap();
        assert_eq!(evolution.event(0), Event::Rebased);
        assert!(
            evolution
                .versions
                .iter()
                .enumerate()
                .any(|(i, _)| evolution.event(i) == Event::Described)
        );
        assert!(
            rebased
                .for_evolution(&evolution.versions[0])
                .unwrap()
                .files()
                .is_empty()
        );
        f.run(&["edit", "feature"]);
        f.write("feature.ts", &code("ancestor two", 2));
        std::fs::rename(f.0.join("héllo.ts"), f.0.join("renamed.ts")).unwrap();
        std::fs::remove_file(f.0.join("remove.ts")).unwrap();
        f.write("new.ts", "new file without newline");
        f.run(&["status"]);
        let current = Comparison::load(&f.0, Some("main"), "feature").unwrap();
        let operation = current.operation().to_owned();
        f.write("new.ts", "unsnapshotted");
        let evolution = current.evolution(current.tip(), 100).unwrap();
        assert_eq!(evolution.event(0), Event::Edited);
        let diff = current.for_evolution(&evolution.versions[0]).unwrap();
        let unchanged = diff.catalog().find("upstream.ts").unwrap();
        assert!(unchanged.kind().is_none());
        assert_eq!(
            diff.read_file(unchanged).unwrap(),
            ("ancestor two\n".into(), "ancestor two\n".into())
        );
        assert!(
            !diff
                .files()
                .iter()
                .any(|file| file.after_path == "upstream.ts")
        );
        for file in diff.files().iter() {
            let (before, after) = diff.contents(file).unwrap();
            match file.after_path.as_str() {
                "feature.ts" => assert_eq!(
                    (before, after),
                    (code("ancestor two", 1), code("ancestor two", 2))
                ),
                "new.ts" => assert_eq!(
                    (before, after),
                    (String::new(), "new file without newline".into())
                ),
                "remove.ts" => assert_eq!((before, after), ("remove me\n".into(), String::new())),
                "renamed.ts" => assert_eq!(
                    (before, after),
                    (String::new(), "unchanged rename\n".into())
                ),
                "héllo.ts" => assert_eq!(
                    (before, after),
                    ("unchanged rename\n".into(), String::new())
                ),
                path => panic!("unexpected file {path}"),
            }
        }
        assert_eq!(diff.files().len(), 5);
        let old = current.evolution(rebased.tip(), 100).unwrap();
        let old_index = old
            .versions
            .iter()
            .position(|version| version.commit_id == rebased.tip())
            .unwrap();
        assert_eq!(old.event(old_index), Event::Rebased);
        assert_eq!(
            Comparison::load(&f.0, Some("main"), "feature")
                .unwrap()
                .operation(),
            operation
        );
        let mut truncated = current.evolution(current.tip(), 1).unwrap();
        assert!(truncated.truncated);
        assert_eq!(truncated.versions.len(), 1);
        let selected = &old.versions[old_index];
        truncated.retain_selected(selected);
        truncated.retain_selected(selected);
        assert_eq!(truncated.versions.len(), 2);
        assert_eq!(truncated.versions[1].commit_id, selected.commit_id);
        assert!(truncated.view(false, 1).indices.contains(&1));
        assert_eq!(truncated.graph.revisions.len(), 2);
    }

    #[test]
    fn historical_path_only_edits_keep_the_move_and_original_interdiff() {
        let f = Fixture::new();
        let source = "export const enabled = true;\n";
        f.write("module.ts", source);
        f.run(&["describe", "-m", "baseline"]);
        f.run(&["bookmark", "create", "main"]);
        f.run(&["new", "main", "-m", "move module"]);
        std::fs::create_dir(f.0.join("startup")).unwrap();
        std::fs::rename(f.0.join("module.ts"), f.0.join("startup/module.ts")).unwrap();
        f.run(&["bookmark", "create", "feature"]);
        let moved = Comparison::load(&f.0, Some("main"), "feature").unwrap();
        f.write("options.ts", "export const retries = 3;\n");
        f.run(&["status"]);
        let current = Comparison::load(&f.0, Some("main"), "feature").unwrap();
        let evolution = current.evolution(current.tip(), 100).unwrap();
        let index = evolution
            .versions
            .iter()
            .position(|version| version.commit_id == moved.tip())
            .unwrap();
        assert_eq!(evolution.event(index), Event::Edited);
        let version = &evolution.versions[index];
        assert_eq!(
            version.file_effect("startup/module.ts", ReviewMode::Patch),
            FileEffect::Renamed { unchanged: true }
        );
        assert_eq!(
            version.file_effect("module.ts", ReviewMode::Patch),
            FileEffect::Renamed { unchanged: true }
        );
        assert_eq!(
            version.file_effect("startup/module.ts", ReviewMode::SincePrevious),
            FileEffect::Added
        );
        assert_eq!(
            version.file_effect("module.ts", ReviewMode::SincePrevious),
            FileEffect::Removed
        );
        assert!(
            !version
                .file_effect("startup/module.ts", ReviewMode::Patch)
                .changes_contents()
        );
        let latest = &evolution.versions[0];
        assert_eq!(
            latest.file_effect("startup/module.ts", ReviewMode::SincePrevious),
            FileEffect::Unchanged
        );
        assert_eq!(
            latest.file_effect("startup/module.ts", ReviewMode::Patch),
            FileEffect::Renamed { unchanged: true }
        );
        assert_eq!(
            latest.rewrite_summary(evolution.event(0)),
            "added options.ts"
        );
        assert_eq!(
            version.rewrite_summary(evolution.event(index)),
            "1 added · 1 removed"
        );
        let patch = current.for_revision(&version.revision()).unwrap();
        let file = patch
            .files()
            .iter()
            .find(|file| file.after_path == "startup/module.ts")
            .unwrap();
        assert_eq!(file.status, "renamed");
        assert_eq!(file.before_path, "module.ts");
        let (before, after) = patch.contents(file).unwrap();
        assert_eq!((before.as_str(), after.as_str()), (source, source));
        assert!(
            !crate::diff_model::FileDiff::new(&before, &after)
                .blocks
                .iter()
                .any(|block| block.changed)
        );
        // The original evolution edit still contains the move: the target is
        // added and the original path removed, even though its full patch has
        // no changed text inside the rename pair.
        let diff = current.for_evolution(version).unwrap();
        let added = diff
            .files()
            .iter()
            .find(|file| file.after_path == "startup/module.ts")
            .unwrap();
        assert_eq!(
            diff.contents(added).unwrap(),
            (String::new(), source.into())
        );
        let removed = diff
            .files()
            .iter()
            .find(|file| file.after_path == "module.ts")
            .unwrap();
        assert_eq!(
            diff.contents(removed).unwrap(),
            (source.into(), String::new())
        );
    }

    #[test]
    fn filtering_rebases_bridges_hidden_branches_and_keeps_selection_identity() {
        let entry = |id: &str, predecessors: Vec<&str>, parent: &str, patch: bool| {
            serde_json::json!({
            "commit_id":id,"change_id":"change","description":"message","parents":[parent],
            "predecessors":predecessors,"timestamp":"2026-11-07 12:00:00","operation_id":"ff",
            "operation":"recorded rewrite","hidden":id != "a","divergent":false,"conflict":false,"patch_changed":patch,"patch_files":[],"evolution_files":[]
        }).to_string()
        };
        let output = [
            entry("a", vec!["b", "c"], "f", true),
            entry("b", vec!["d"], "e", false),
            entry("c", vec!["d"], "f", false),
            entry("d", vec![], "0", true),
        ]
        .join("\n");
        let mut evolution = Evolution::parse(&output, 100).unwrap();
        let filtered = evolution.view(false, 0);
        assert_eq!(filtered.indices, vec![0, 3]);
        assert_eq!(filtered.graph.revisions[0].parents, vec!["d"]);
        assert_eq!(filtered.graph.lane_count, 1);
        let retained = evolution.view(false, 1);
        assert_eq!(retained.indices, vec![0, 1, 3]);
        assert_eq!(retained.graph.revisions[0].parents, vec!["b", "d"]);
        assert_eq!(retained.graph.revisions[1].parents, vec!["d"]);
        let full = evolution.view(true, 0);
        assert_eq!(full.indices, vec![0, 1, 2, 3]);
        assert_eq!(full.graph.lane_count, 2);
        evolution.versions[1].description = "important description update".into();
        assert_eq!(evolution.event(1), Event::Described);
        assert_eq!(evolution.view(false, 0).indices, vec![0, 1, 3]);
        evolution.versions[2].hidden = false;
        assert_eq!(evolution.view(false, 0).indices, vec![0, 1, 2, 3]);
    }

    #[test]
    fn file_annotations_distinguish_edited_renames() {
        let f = Fixture::new();
        let source = (0..30)
            .map(|n| format!("const item{n} = {n};\n"))
            .collect::<String>();
        f.write("original.ts", &source);
        f.run(&["describe", "-m", "baseline"]);
        f.run(&["bookmark", "create", "main"]);
        f.run(&["new", "main", "-m", "move and edit"]);
        std::fs::rename(f.0.join("original.ts"), f.0.join("moved.ts")).unwrap();
        f.write("moved.ts", &(source + "export const enabled = true;\n"));
        f.run(&["status"]);
        let comparison = Comparison::load(&f.0, Some("main"), "@").unwrap();
        let evolution = comparison.evolution(comparison.tip(), 100).unwrap();
        let effect = evolution.versions[0].file_effect("moved.ts", ReviewMode::Patch);
        assert_eq!(effect, FileEffect::Renamed { unchanged: false });
        assert!(effect.changes_contents());
        assert_eq!(
            evolution.versions[0].file_effect("missing.ts", ReviewMode::Patch),
            FileEffect::Unchanged
        );
    }

    #[test]
    fn merge_file_annotations_match_the_first_parent_view() {
        let f = Fixture::new();
        f.write("base.ts", "base\n");
        f.run(&["describe", "-m", "baseline"]);
        f.run(&["bookmark", "create", "main"]);
        f.run(&["new", "main", "-m", "left"]);
        f.write("left.ts", "left\n");
        f.run(&["bookmark", "create", "left"]);
        f.run(&["new", "main", "-m", "right"]);
        f.write("right.ts", "right\n");
        f.run(&["bookmark", "create", "right"]);
        f.run(&["new", "left", "right", "-m", "merge"]);
        let comparison = Comparison::load(&f.0, Some("main"), "@").unwrap();
        let evolution = comparison.evolution(comparison.tip(), 100).unwrap();
        let version = &evolution.versions[0];
        assert_eq!(version.parents.len(), 2);
        assert_eq!(
            version.file_effect("left.ts", ReviewMode::Patch),
            FileEffect::Unchanged
        );
        assert_eq!(
            version.file_effect("right.ts", ReviewMode::Patch),
            FileEffect::Added
        );
        let patch = comparison.for_revision(&version.revision()).unwrap();
        assert_eq!(patch.files().len(), 1);
        assert_eq!(patch.files()[0].after_path, "right.ts");
    }

    #[test]
    fn divergent_current_versions_share_an_evolution_graph() {
        let f = Fixture::new();
        f.write("sample.ts", "base\n");
        f.run(&["describe", "-m", "baseline"]);
        f.run(&["bookmark", "create", "main"]);
        f.run(&["new", "main", "-m", "feature"]);
        f.write("sample.ts", "feature\n");
        f.run(&["bookmark", "create", "feature"]);
        let original = Comparison::load(&f.0, Some("main"), "feature").unwrap();
        f.run(&["describe", "-m", "left version"]);
        let left = Comparison::load(&f.0, Some("main"), "feature").unwrap();
        f.run(&[
            "--at-op",
            original.operation(),
            "describe",
            "-r",
            original.tip(),
            "-m",
            "right version",
        ]);
        f.run(&["log", "-G", "-n", "1"]);
        let comparison = Comparison::load(&f.0, Some("main"), left.tip()).unwrap();
        let evolution = comparison.evolution(left.tip(), 100).unwrap();
        let current: Vec<_> = evolution
            .versions
            .iter()
            .filter(|version| !version.hidden && version.divergent)
            .collect();
        assert_eq!(current.len(), 2);
        assert!(
            current
                .iter()
                .any(|version| version.commit_id == left.tip())
        );
        assert!(
            current
                .iter()
                .any(|version| version.description.trim() == "right version")
        );
        assert_eq!(evolution.graph.lane_count, 2);
    }

    #[test]
    fn rewrite_graph_retains_combined_predecessors_and_rejects_invalid_ids() {
        let entry = |id: &str, predecessors: Vec<&str>| {
            serde_json::json!({"commit_id":id,"change_id":"change","description":"message","parents":["f"],"predecessors":predecessors,"timestamp":"2026-11-07 12:00:00","operation_id":"ff","operation":"squash","hidden":false,"divergent":false,"conflict":false,"patch_changed":true,"patch_files":[],"evolution_files":[]}).to_string()
        };
        let output = [
            entry("a", vec!["b", "c"]),
            entry("b", vec!["d"]),
            entry("c", vec!["d"]),
            entry("d", vec![]),
        ]
        .join("\n");
        let evolution = Evolution::parse(&output, 100).unwrap();
        assert_eq!(evolution.event(0), Event::Combined);
        assert_eq!(evolution.graph.lane_count, 2);
        assert_eq!(
            evolution.graph.graph[0]
                .strokes
                .iter()
                .map(|stroke| (stroke.from, stroke.to))
                .collect::<Vec<_>>(),
            vec![(0, 0), (0, 1)]
        );
        assert!(Evolution::parse(&entry("all()", vec![]), 100).is_err());
    }
}
