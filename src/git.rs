//! Read-only Git comparisons for repositories without Jujutsu metadata.
use crate::history::{History, HistoryQuery, Revision};
use crate::repository::{ChangedFile, Comparison, ContentSource, FileSide, next_capture};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::repository_file::FileKind;

#[derive(Clone, Debug)]
struct GitValue {
    object: String,
    kind: FileKind,
}

type TextPair = Result<(String, String), String>;
#[derive(Clone, Debug)]
pub(crate) struct GitContents {
    files: BTreeMap<String, TextPair>,
    root: PathBuf,
    trees: [BTreeMap<String, GitValue>; 2],
}

fn run(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let result = Command::new("git")
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
fn text(data: Vec<u8>) -> Result<String, String> {
    if data.len() > 2_000_000 || data.contains(&0) {
        return Err("Binary or large file: text comparison is unavailable.".into());
    }
    String::from_utf8(data).map_err(|error| format!("Non-UTF-8 Git content: {error}"))
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

pub(crate) fn load(directory: &Path, base: Option<&str>, tip: &str) -> Result<Comparison, String> {
    let root = output(directory, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(root.trim_end_matches('\n'));
    let base = resolve(&root, base.unwrap_or("HEAD"))?;
    let tip = if tip == "@" {
        "working tree".into()
    } else {
        resolve(&root, tip)?
    };
    between(root, base, tip)
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

pub(crate) fn between(root: PathBuf, base: String, tip: String) -> Result<Comparison, String> {
    let working = tip == "working tree";
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
    let mut files = BTreeMap::new();
    while let Some(status) = fields.next() {
        let path = fields.next().ok_or("Incomplete Git diff entry")?;
        let status = match status {
            "A" => "added",
            "D" => "removed",
            _ => "modified",
        };
        files.insert(
            path.to_owned(),
            ChangedFile {
                status: status.into(),
                before_path: path.into(),
                after_path: path.into(),
            },
        );
    }
    if working {
        for path in output(&root, &["ls-files", "--others", "--exclude-standard", "-z"])?
            .split_terminator('\0')
        {
            files.insert(
                path.into(),
                ChangedFile {
                    status: "added".into(),
                    before_path: path.into(),
                    after_path: path.into(),
                },
            );
        }
    }
    let files: Vec<_> = files.into_values().collect();
    let before_tree = tree(&root, &base)?;
    let mut after_tree = if working {
        // The diff classified remaining tracked paths as unchanged. Bind their
        // bytes to the base objects, never to future filesystem reads.
        let mut after = before_tree.clone();
        for file in &files {
            if file.status == "removed" {
                after.remove(&file.after_path);
            } else {
                after.entry(file.after_path.clone()).or_insert(GitValue {
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
    for file in &files {
        let pair = (|| {
            let before = if file.status == "added" {
                String::new()
            } else {
                text(run(
                    &root,
                    &["show", &format!("{base}:{}", file.before_path)],
                )?)?
            };
            let after = if file.status == "removed" {
                String::new()
            } else if working {
                let path = root.join(&file.after_path);
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
                if let Some(value) = after_tree.get_mut(&file.after_path) {
                    value.kind = if metadata.file_type().is_symlink() {
                        FileKind::Symlink
                    } else if metadata.is_dir() {
                        FileKind::Submodule
                    } else {
                        FileKind::Regular
                    };
                }
                if metadata.file_type().is_symlink() {
                    let target = std::fs::read_link(&path).map_err(|error| {
                        format!("Could not read link {}: {error}", path.display())
                    })?;
                    target.to_str().ok_or("Non-UTF-8 link target")?.to_owned()
                } else {
                    text(
                        std::fs::read(&path).map_err(|error| {
                            format!("Could not read {}: {error}", path.display())
                        })?,
                    )?
                }
            } else {
                text(run(
                    &root,
                    &["show", &format!("{tip}:{}", file.after_path)],
                )?)?
            };
            Ok((before, after))
        })();
        contents.insert(file.after_path.clone(), pair);
    }
    Comparison {
        root: root.clone(),
        operation: String::new(),
        base,
        tip,
        files: files.into(),
        content: ContentSource::Git(Arc::new(GitContents {
            files: contents,
            root,
            trees: [before_tree, after_tree],
        })),
        capture: if working { next_capture() } else { 0 },
        catalog: Arc::default(),
    }
    .with_catalog()
}
impl GitContents {
    pub(crate) fn paths(&self) -> BTreeMap<String, FileKind> {
        self.trees[1]
            .iter()
            .map(|(path, value)| (path.clone(), value.kind.clone()))
            .collect()
    }
    pub(crate) fn read_path(&self, path: &str, side: FileSide) -> Result<String, String> {
        if let Some(pair) = self.files.get(path) {
            let pair = pair.as_ref().map_err(Clone::clone)?;
            return Ok(match side {
                FileSide::Before => pair.0.clone(),
                FileSide::After => pair.1.clone(),
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
        text(run(&self.root, &["cat-file", "blob", &value.object])?)
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
            let remote = snapshot
                .remotes
                .iter()
                .filter(|remote| path.starts_with(&format!("{remote}/")))
                .max_by_key(|remote| remote.len())
                .cloned()
                .or_else(|| path.split_once('/').map(|(remote, _)| remote.into()))
                .ok_or("Invalid remote reference name")?;
            let name = path
                .strip_prefix(&format!("{remote}/"))
                .ok_or("Invalid remote reference prefix")?
                .to_owned();
            let tracked_by: Vec<String> = entries
                .iter()
                .filter(|local| local[0].starts_with("refs/heads/") && local[3] == entry[0])
                .map(|local| local[0]["refs/heads/".len()..].to_owned())
                .collect();
            snapshot.remotes.push(remote.clone());
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
        "--format=%H%x00%P%x00%s%x00%an",
    ];
    let roots = query.roots(&references);
    if roots.is_empty() {
        return Ok(History::new(vec![]).with_references(references));
    }
    args.extend(roots.iter().map(String::as_str));
    args.push("--");
    let data = output(root, &args)?;
    let fields: Vec<_> = data.split_terminator('\0').collect();
    if !fields.len().is_multiple_of(4) {
        return Err("Incomplete Git history entry".into());
    }
    let revisions = fields
        .chunks_exact(4)
        .map(|entry| Revision {
            commit_id: entry[0].into(),
            change_id: entry[0].into(),
            description: entry[2].into(),
            author: entry[3].into(),
            parents: entry[1].split_whitespace().map(str::to_owned).collect(),
            references: vec![],
            working_copy: false,
            empty: false,
        })
        .collect();
    Ok(History::new(revisions).with_references(references))
}
pub(crate) fn for_revision(
    comparison: &Comparison,
    revision: &Revision,
) -> Result<Comparison, String> {
    let base = match revision.parents.first() {
        Some(parent) => parent.clone(),
        None => output(&comparison.root, &["hash-object", "-t", "tree", "--stdin"])?
            .trim()
            .to_owned(),
    };
    between(comparison.root.clone(), base, revision.commit_id.clone())
}
