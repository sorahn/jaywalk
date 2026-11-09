//! Read-only Git comparisons for repositories without Jujutsu metadata.
use crate::history::{History, HistoryQuery, HistoryScope, Revision};
use crate::repository::{ChangedFile, Comparison, ContentSource, FileSide, next_capture};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

type TextPair = Result<(String, String), String>;
#[derive(Clone, Debug)]
pub(crate) struct GitContents {
    files: BTreeMap<String, TextPair>,
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
fn output(root: &Path, args: &[&str]) -> Result<String, String> {
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
    Ok(Comparison {
        root,
        operation: String::new(),
        base,
        tip,
        files: files.into(),
        content: ContentSource::Git(Arc::new(GitContents { files: contents })),
        capture: if working { next_capture() } else { 0 },
    })
}
impl GitContents {
    pub(crate) fn read_path(&self, path: &str, side: FileSide) -> Result<String, String> {
        let pair = self
            .files
            .get(path)
            .ok_or_else(|| format!("File is absent from captured comparison: {path}"))?
            .as_ref()
            .map_err(Clone::clone)?;
        Ok(match side {
            FileSide::Before => pair.0.clone(),
            FileSide::After => pair.1.clone(),
        })
    }
}
pub(crate) fn history(root: &Path, query: &HistoryQuery) -> Result<History, String> {
    let limit = query.limit().to_string();
    let mut args = vec![
        "log",
        "--topo-order",
        "-n",
        &limit,
        "-z",
        "--format=%H%x00%P%x00%s%x00%an",
    ];
    match query.scope() {
        HistoryScope::Repository => args.extend(["--all", "HEAD"]),
        HistoryScope::Ancestors(commits) => args.extend(commits.iter().map(String::as_str)),
    }
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
    Ok(History::new(revisions))
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
