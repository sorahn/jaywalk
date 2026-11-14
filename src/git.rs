//! Read-only Git comparisons for repositories without Jujutsu metadata.
use crate::history::{History, HistoryQuery, Revision};
use crate::repository::{
    ChangedFile, Comparison, ComparisonMetadata, ContentSource, FileSide, next_capture,
};
use crate::repository_file::ChangeKind;
use std::collections::{BTreeMap, BTreeSet};
pub(crate) mod blob_batch;
use std::path::{Path, PathBuf};
use std::process::Command;

use std::sync::Arc;

use crate::repository_file::FileKind;

#[derive(Clone, Debug)]
struct GitValue {
    object: String,
    kind: FileKind,
}

const WORKING_TREE: &str = "working tree";

type TextPair = Result<(String, String), String>;
#[derive(Clone, Debug)]
pub(crate) struct GitContents {
    pub(crate) recorded_tip: String,
    pub(crate) working_changes: bool,
    files: Arc<BTreeMap<String, TextPair>>,
    reversed: bool,
    root: PathBuf,
    trees: [Arc<BTreeMap<String, GitValue>>; 2],
    limit: crate::text_content::TextLimit,
}

// Shared by Git-backed jj and evolution; byte policy remains backend-independent.
pub(crate) fn read_blob(
    root: &Path,
    git_dir: Option<&Path>,
    object: &str,
    limit: crate::text_content::TextLimit,
) -> Result<String, String> {
    let command = || -> Result<Command, String> {
        let mut command = crate::tools::command("git")?;
        command
            .current_dir(root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["--no-pager", "--literal-pathspecs"]);
        if let Some(directory) = git_dir {
            command.arg("--git-dir").arg(directory);
        }
        Ok(command)
    };
    let size = limit.command(command()?.args(["cat-file", "-s", object]))?;
    let size = std::str::from_utf8(&size)
        .map_err(|error| error.to_string())?
        .trim()
        .parse::<u64>()
        .map_err(|error| format!("Invalid Git object size: {error}"))?;
    limit.check(size)?;
    limit.text(limit.command(command()?.args(["cat-file", "blob", object]))?)
}

fn run(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let result = crate::tools::command("git")?
        .current_dir(root)
        .args(["--no-pager", "--literal-pathspecs"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| format!("Could not execute Git: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "Git: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(result.stdout)
}
pub(crate) fn output(root: &Path, args: &[&str]) -> Result<String, String> {
    String::from_utf8(run(root, args)?).map_err(|error| format!("Non-UTF-8 Git metadata: {error}"))
}
fn resolve(root: &Path, revision: &str) -> Result<String, String> {
    output(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{revision}^{{commit}}"),
        ],
    )
    .map(|value| value.trim().to_owned())
}

pub(crate) fn load(
    directory: &Path,
    base: Option<&str>,
    tip: &str,
    limit: crate::text_content::TextLimit,
) -> Result<Comparison, String> {
    let root = output(directory, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root.trim_end_matches('\n'));
    let base = resolve(&root, base.unwrap_or("HEAD"))?;
    let tip = if tip == "@" {
        WORKING_TREE.into()
    } else {
        resolve(&root, tip)?
    };
    between(root, base, tip, limit)
}

fn tree(root: &Path, revision: &str) -> Result<BTreeMap<String, GitValue>, String> {
    let output = output(root, &["ls-tree", "-r", "-z", revision])?;
    let mut tree = BTreeMap::new();
    for entry in output.split_terminator('\0') {
        let (metadata, path) = entry.split_once('\t').ok_or("Incomplete Git tree entry")?;
        let values: Vec<_> = metadata.split_whitespace().collect();
        if values.len() != 3 {
            return Err("Invalid Git tree entry".into());
        }
        tree.insert(
            path.into(),
            GitValue {
                object: values[2].into(),
                kind: FileKind::from(values[0]),
            },
        );
    }
    Ok(tree)
}

pub(crate) fn between(
    root: PathBuf,
    base: String,
    tip: String,
    limit: crate::text_content::TextLimit,
) -> Result<Comparison, String> {
    if base == WORKING_TREE {
        if tip == WORKING_TREE {
            return Err("Choose two distinct working/recorded endpoints".into());
        }
        return between(root, tip, base, limit)?.swapped();
    }
    let working = tip == WORKING_TREE;
    let recorded_tip = if working {
        resolve(&root, "HEAD")?
    } else {
        tip.clone()
    };
    let working_changes = working
        && !output(
            &root,
            &["status", "--porcelain", "--untracked-files=normal"],
        )?
        .is_empty();
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--name-status",
        "-z",
        &base,
    ];
    if !working {
        args.push(&tip);
    }
    args.push("--");
    let data = output(&root, &args)?;
    let mut fields = data.split_terminator('\0');
    let before_tree = tree(&root, &base)?;
    let mut files = BTreeMap::new();
    while let Some(status) = fields.next() {
        let path = fields.next().ok_or("Incomplete Git diff entry")?;
        let kind = match status {
            "A" => ChangeKind::Added,
            "D" => ChangeKind::Removed,
            "M" | "T" => ChangeKind::Modified,
            other => ChangeKind::Other(other.to_owned()),
        };
        files.insert(
            path.to_owned(),
            ChangedFile::new(kind, path.into(), path.into()),
        );
    }
    if working {
        for path in output(&root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split_terminator('\0')
        {
            files.insert(
                path.into(),
                // Untracked is index-relative, not base-tree-relative.
                ChangedFile::new(
                    if before_tree.contains_key(path) {
                        ChangeKind::Modified
                    } else {
                        ChangeKind::Added
                    },
                    path.into(),
                    path.into(),
                ),
            );
        }
    }
    let files: Vec<_> = files.into_values().collect();
    let mut after_tree = if working {
        // The diff classified remaining tracked paths as unchanged. Bind their
        // bytes to the base objects, never to future filesystem reads.
        let mut after = before_tree.clone();
        for file in &files {
            if file.kind() == &ChangeKind::Removed {
                after.remove(file.after_path());
            } else {
                after
                    .entry(file.after_path().to_owned())
                    .or_insert(GitValue {
                        object: String::new(),
                        kind: FileKind::Regular,
                    });
            }
        }
        after
    } else {
        tree(&root, &tip)?
    };

    // Capture a coherent comparison now; selecting a file must not mix later edits
    // into an already displayed worktree comparison.
    let mut contents = BTreeMap::new();
    let mut blobs = blob_batch::BlobBatch::new(&root, limit);
    for file in &files {
        // Capture kind independently of text acquisition: a before-side read
        // may be unavailable or oversized, without hiding the after-side kind.
        let metadata = (working && file.kind() != &ChangeKind::Removed).then(|| {
            let path = root.join(file.after_path());
            std::fs::symlink_metadata(&path)
                .map_err(|error| format!("Could not inspect {}: {error}", path.display()))
        });
        if let Some(Ok(metadata)) = &metadata
            && let Some(value) = after_tree.get_mut(file.after_path())
        {
            value.kind = if metadata.file_type().is_symlink() {
                FileKind::Symlink
            } else if metadata.is_dir() {
                FileKind::Submodule
            } else {
                FileKind::Regular
            };
        }
        let pair = (|| {
            let before = if file.kind() == &ChangeKind::Added {
                String::new()
            } else {
                let value = before_tree
                    .get(file.before_path())
                    .ok_or("File is absent from captured base tree")?;
                blobs.read(&value.object)?
            };
            let after = if file.kind() == &ChangeKind::Removed {
                String::new()
            } else if working {
                let path = root.join(file.after_path());
                let metadata = metadata
                    .as_ref()
                    .expect("changed working-tree paths have a captured metadata result")
                    .as_ref()
                    .map_err(Clone::clone)?;
                if metadata.file_type().is_symlink() {
                    let target = std::fs::read_link(&path).map_err(|error| {
                        format!("Could not read link {}: {error}", path.display())
                    })?;
                    target.to_str().ok_or("Non-UTF-8 link target")?.to_owned()
                } else {
                    limit.text(limit.file(&path)?)?
                }
            } else {
                let value = after_tree
                    .get(file.after_path())
                    .ok_or("File is absent from captured tip tree")?;
                blobs.read(&value.object)?
            };
            Ok((before, after))
        })();
        contents.insert(file.after_path().to_owned(), pair);
    }
    Comparison::assemble(
        ComparisonMetadata {
            root: root.clone(),
            operation: String::new(),
            base,
            tip,
            capture: if working { next_capture() } else { 0 },
        },
        files,
        ContentSource::Git(Arc::new(GitContents {
            recorded_tip,
            working_changes,
            files: Arc::new(contents),
            reversed: false,
            root,
            trees: [Arc::new(before_tree), Arc::new(after_tree)],
            limit,
        })),
        limit,
    )
}
impl GitContents {
    pub(crate) fn swapped(&self) -> Self {
        let mut result = self.clone();
        result.trees.swap(0, 1);
        result.reversed = !result.reversed;
        result
    }

    pub(crate) fn paths(&self, side: FileSide) -> BTreeMap<String, FileKind> {
        self.trees[match side {
            FileSide::Before => 0,
            FileSide::After => 1,
        }]
        .iter()
        .map(|(path, value)| (path.clone(), value.kind.clone()))
        .collect()
    }
    pub(crate) fn read_path(&self, path: &str, side: FileSide) -> Result<String, String> {
        if let Some(pair) = self.files.get(path) {
            let pair = pair.as_ref().map_err(Clone::clone)?;
            return Ok(if (side == FileSide::Before) != self.reversed {
                pair.0.clone()
            } else {
                pair.1.clone()
            });
        }
        let tree = &self.trees[match side {
            FileSide::Before => 0,
            FileSide::After => 1,
        }];
        let value = tree
            .get(path)
            .ok_or_else(|| format!("File is absent from captured comparison: {path}"))?;
        if matches!(value.kind, FileKind::Submodule | FileKind::Other(_)) {
            return Err("Text view unavailable for this Git entry".into());
        }
        read_blob(&self.root, None, &value.object, self.limit)
    }
}
pub(crate) fn references(root: &Path) -> Result<crate::references::ReferenceSnapshot, String> {
    use crate::history::{Reference, ReferenceKind, TrackingState};
    let remotes = crate::references::configured_remotes(root)?;
    let format = "--format=%(refname)%00%(objectname)%00%(*objectname)%00%(upstream)%00%(objecttype)%00%(*objecttype)%00%(symref)";
    let data = output(
        root,
        &[
            "for-each-ref",
            format,
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    let entries: Vec<Vec<&str>> = data
        .lines()
        .map(|line| line.split('\0').collect())
        .collect();
    if entries.iter().any(|entry| entry.len() != 7) {
        return Err("Incomplete Git reference entry".into());
    }
    let mut snapshot = crate::references::ReferenceSnapshot {
        remotes,
        ..Default::default()
    };
    snapshot.local_heads.push(resolve(root, "HEAD")?);
    let mut remote_names: BTreeSet<String> = snapshot.remotes.iter().cloned().collect();
    let mut upstreams: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for entry in &entries {
        if let Some(name) = entry[0].strip_prefix("refs/heads/")
            && !entry[3].is_empty()
        {
            upstreams.entry(entry[3]).or_default().push(name.to_owned());
        }
    }
    for entry in &entries {
        if !entry[6].is_empty() {
            continue;
        } // symbolic remote HEAD aliases add no reference of their own.
        let target = if entry[4] == "commit" {
            entry[1]
        } else if entry[5] == "commit" {
            entry[2]
        } else {
            continue;
        };
        if entry[0].starts_with("refs/tags/") {
            snapshot.local_heads.push(target.into());
            continue;
        }
        let (name, kind, tracked_by) = if let Some(name) = entry[0].strip_prefix("refs/heads/") {
            snapshot.local_heads.push(target.into());
            (name.to_owned(), ReferenceKind::Local, Vec::new())
        } else if let Some(path) = entry[0].strip_prefix("refs/remotes/") {
            let remote = remote_names
                .iter()
                .filter(|remote| {
                    path.strip_prefix(remote.as_str())
                        .is_some_and(|suffix| suffix.starts_with('/'))
                })
                .max_by_key(|remote| remote.len())
                .cloned()
                .or_else(|| path.split_once('/').map(|(remote, _)| remote.into()))
                .ok_or("Invalid remote reference name")?;
            let name = path
                .strip_prefix(&format!("{remote}/"))
                .ok_or("Invalid remote reference prefix")?
                .to_owned();
            let tracked_by = upstreams.get(entry[0]).cloned().unwrap_or_default();
            remote_names.insert(remote.clone());
            (
                name,
                ReferenceKind::Remote {
                    remote,
                    tracking: Some(if tracked_by.is_empty() {
                        TrackingState::Untracked
                    } else {
                        TrackingState::Tracked
                    }),
                },
                tracked_by,
            )
        } else {
            continue;
        };
        snapshot.references.push(Reference {
            name,
            target: target.into(),
            kind,
            tracked_by,
            conflicted: false,
        });
    }
    snapshot.remotes = remote_names.into_iter().collect();
    snapshot.normalize();
    Ok(snapshot)
}

pub(crate) fn history(root: &Path, query: &HistoryQuery) -> Result<History, String> {
    let references = references(root)?;
    let limit = query.limit().to_string();
    let mut args = vec![
        "log",
        "--topo-order",
        "-n",
        &limit,
        "-z",
        "--format=%H%x00%P%x00%B%x00%an%x00%aI",
    ];
    let roots = query.roots(&references);
    if roots.is_empty() {
        return Ok(History::new(vec![]).with_references(references));
    }
    args.extend(roots.iter().map(String::as_str));
    args.push("--");
    let data = output(root, &args)?;
    let fields: Vec<_> = data.split_terminator('\0').collect();
    if !fields.len().is_multiple_of(5) {
        return Err("Incomplete Git history entry".into());
    }
    let mut revisions: Vec<_> = fields
        .as_chunks::<5>()
        .0
        .iter()
        .map(|entry| Revision {
            commit_id: entry[0].into(),
            change_id: entry[0].into(),
            description: entry[2].into(),
            author: entry[3].into(),
            timestamp: entry[4].into(),
            parents: entry[1].split_whitespace().map(str::to_owned).collect(),
            references: vec![],
            working_copy: false,
            empty: false,
        })
        .collect();
    if query.includes_working_copy() {
        let head = resolve(root, "HEAD")?;
        let empty =
            output(root, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty();
        revisions.insert(
            0,
            Revision {
                commit_id: WORKING_TREE.into(),
                change_id: String::new(),
                description: String::new(),
                author: String::new(),
                timestamp: String::new(),
                parents: vec![head],
                references: vec![],
                working_copy: true,
                empty,
            },
        );
        revisions.truncate(query.limit());
    }
    Ok(History::new(revisions).with_references(references))
}
pub(crate) fn for_revision(
    comparison: &Comparison,
    revision: &Revision,
) -> Result<Comparison, String> {
    if revision.is_virtual_working_copy() {
        return between(
            comparison.root().to_path_buf(),
            resolve(comparison.root(), "HEAD")?,
            WORKING_TREE.into(),
            comparison.text_limit(),
        );
    }
    let base = match revision.parents.first() {
        Some(parent) => parent.clone(),
        None => output(comparison.root(), &["hash-object", "-t", "tree", "--stdin"])?
            .trim()
            .to_owned(),
    };
    between(
        comparison.root().to_path_buf(),
        base,
        revision.commit_id.clone(),
        comparison.text_limit(),
    )
}
