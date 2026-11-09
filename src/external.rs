//! Read-only comparisons supplied by command-line diff tools.
use crate::repository::{ChangedFile, Comparison, ContentSource, FileSide, next_capture};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ExternalContents {
    left: BTreeMap<String, PathBuf>,
    right: BTreeMap<String, PathBuf>,
}

fn collect(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, PathBuf>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("Could not list {}: {error}", directory.display()))?
    {
        let entry =
            entry.map_err(|error| format!("Could not list {}: {error}", directory.display()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        if kind.is_dir() {
            collect(root, &path, files)?;
        } else if kind.is_file() || kind.is_symlink() {
            let relative = path
                .strip_prefix(root)
                .expect("walked path is beneath its root");
            let key = relative
                .to_str()
                .ok_or_else(|| format!("Non-UTF-8 file name: {}", path.display()))?;
            files.insert(key.to_owned(), path);
        } else {
            return Err(format!("Unsupported file type: {}", path.display()));
        }
    }
    Ok(())
}

fn bytes(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        // Compare link targets rather than following links out of a supplied tree.
        let target = std::fs::read_link(path)
            .map_err(|error| format!("Could not read link {}: {error}", path.display()))?;
        return target
            .to_str()
            .map(|text| text.as_bytes().to_vec())
            .ok_or_else(|| format!("Non-UTF-8 link target: {}", path.display()));
    }
    std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))
}

impl ExternalContents {
    pub fn comparison(left: &Path, right: &Path) -> Result<Comparison, String> {
        let left = std::fs::canonicalize(left)
            .map_err(|error| format!("Could not open {}: {error}", left.display()))?;
        let right = std::fs::canonicalize(right)
            .map_err(|error| format!("Could not open {}: {error}", right.display()))?;
        let mut contents = Self {
            left: BTreeMap::new(),
            right: BTreeMap::new(),
        };
        if left.is_dir() && right.is_dir() {
            collect(&left, &left, &mut contents.left)?;
            collect(&right, &right, &mut contents.right)?;
        } else if (left.is_file() || left == Path::new("/dev/null"))
            && (right.is_file() || right == Path::new("/dev/null"))
        {
            let label = if right == Path::new("/dev/null") {
                &left
            } else {
                &right
            };
            let name = label
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("Non-UTF-8 file name: {}", right.display()))?
                .to_owned();
            contents.left.insert(name.clone(), left.clone());
            contents.right.insert(name, right.clone());
        } else {
            return Err("Diff inputs must both be files or both be directories.".into());
        }
        let names: BTreeSet<_> = contents
            .left
            .keys()
            .chain(contents.right.keys())
            .cloned()
            .collect();
        let mut files = Vec::new();
        for name in names {
            let status = match (contents.left.get(&name), contents.right.get(&name)) {
                (None, Some(_)) => "added",
                (Some(_), None) => "removed",
                (Some(before), Some(after)) => {
                    if bytes(before)? == bytes(after)? {
                        continue;
                    }
                    "modified"
                }
                (None, None) => unreachable!("name came from an input tree"),
            };
            files.push(ChangedFile {
                status: status.into(),
                before_path: name.clone(),
                after_path: name,
            });
        }
        Comparison {
            root: right.clone(),
            operation: String::new(),
            base: left.display().to_string(),
            tip: right.display().to_string(),
            files: files.into(),
            content: ContentSource::External(Arc::new(contents)),
            capture: next_capture(),
            catalog: Arc::default(),
        }
        .with_catalog()
    }

    pub(crate) fn paths(
        &self,
    ) -> Result<BTreeMap<String, crate::repository_file::FileKind>, String> {
        self.right
            .iter()
            .map(|(path, source)| {
                let metadata = std::fs::symlink_metadata(source)
                    .map_err(|error| format!("Could not inspect {}: {error}", source.display()))?;
                Ok((
                    path.clone(),
                    if metadata.is_symlink() {
                        crate::repository_file::FileKind::Symlink
                    } else {
                        crate::repository_file::FileKind::Regular
                    },
                ))
            })
            .collect()
    }
    pub(crate) fn read_path(&self, path: &str, side: FileSide) -> Result<String, String> {
        let read = |paths: &BTreeMap<String, PathBuf>, name: &str| -> Result<String, String> {
            let Some(path) = paths.get(name) else {
                return Ok(String::new());
            };
            let data = bytes(path)?;
            if data.len() > 2_000_000 || data.contains(&0) {
                return Err("Binary or large file: text comparison is unavailable.".into());
            }
            String::from_utf8(data)
                .map_err(|error| format!("Non-UTF-8 text in {}: {error}", path.display()))
        };
        read(
            match side {
                FileSide::Before => &self.left,
                FileSide::After => &self.right,
            },
            path,
        )
    }
}
