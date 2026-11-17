//! Read-only comparisons supplied by command-line diff tools.
use crate::repository::{
    ChangedFile, Comparison, ComparisonMetadata, ContentSource, FileSide, next_capture,
};
mod capture;
use capture::{Builder, Capture, Entry as ExternalEntry, Source};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ExternalContents {
    pub(crate) git_context: bool,
    pub(crate) recorded_working: Option<String>,
    left: BTreeMap<String, ExternalEntry>,
    right: BTreeMap<String, ExternalEntry>,
    limit: crate::text_content::TextLimit,
    capture: Arc<Capture>,
}

fn collect(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, Source>,
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
            files.insert(key.to_owned(), Source::new(path)?);
        } else {
            return Err(format!("Unsupported file type: {}", path.display()));
        }
    }
    Ok(())
}

/// Resolve parents without dereferencing a comparison's final entry.
pub fn normalize_input(path: &Path) -> Result<PathBuf, String> {
    let normalized = if let Some(name) = path.file_name() {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::canonicalize(parent).map(|parent| parent.join(name))
    } else {
        std::fs::canonicalize(path)
    }
    .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    std::fs::symlink_metadata(&normalized)
        .map_err(|error| format!("Could not inspect {}: {error}", normalized.display()))?;
    Ok(normalized)
}

impl ExternalContents {
    pub(crate) fn swapped(&self) -> Self {
        Self {
            git_context: self.git_context,
            recorded_working: self.recorded_working.clone(),
            left: self.right.clone(),
            right: self.left.clone(),
            limit: self.limit,
            capture: self.capture.clone(),
        }
    }

    pub fn comparison(left: &Path, right: &Path) -> Result<Comparison, String> {
        Self::comparison_with_limit(left, right, crate::text_content::TextLimit::default())
    }
    pub fn comparison_with_limit(
        left: &Path,
        right: &Path,
        limit: crate::text_content::TextLimit,
    ) -> Result<Comparison, String> {
        let left = normalize_input(left)?;
        let right = normalize_input(right)?;
        let left_kind = std::fs::symlink_metadata(&left)
            .map_err(|error| error.to_string())?
            .file_type();
        let right_kind = std::fs::symlink_metadata(&right)
            .map_err(|error| error.to_string())?
            .file_type();
        let mut builder = Builder::new(limit)?;
        let mut before = BTreeMap::new();
        let mut after = BTreeMap::new();
        if left_kind.is_dir() && right_kind.is_dir() {
            collect(&left, &left, &mut before)?;
            collect(&right, &right, &mut after)?;
        } else if (left_kind.is_file() || left_kind.is_symlink() || left == Path::new("/dev/null"))
            && (right_kind.is_file() || right_kind.is_symlink() || right == Path::new("/dev/null"))
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
            // The null device denotes an absent entry, not an empty file.
            if left != Path::new("/dev/null") {
                before.insert(name.clone(), Source::new(left.clone())?);
            }
            if right != Path::new("/dev/null") {
                after.insert(name, Source::new(right.clone())?);
            }
        } else {
            return Err("Diff inputs must both be files or both be directories.".into());
        }
        let before_sources = before;
        let after_sources = after;
        let names: BTreeSet<_> = before_sources
            .keys()
            .chain(after_sources.keys())
            .cloned()
            .collect();
        let mut captured_left = BTreeMap::new();
        let mut captured_right = BTreeMap::new();
        let mut files = Vec::new();
        for name in names {
            let left = before_sources.get(&name);
            let right = after_sources.get(&name);
            let left = left.map(|source| builder.entry(source)).transpose()?;
            let right = right.map(|source| builder.entry(source)).transpose()?;
            let status = match (&left, &right) {
                (None, Some(_)) => Some("added"),
                (Some(_), None) => Some("removed"),
                (Some(before), Some(after)) => {
                    let equal = match before.equal(after) {
                        Some(equal) => equal,
                        None => before_sources
                            .get(&name)
                            .expect("captured before entry has a source")
                            .equal_oversized(
                                after_sources
                                    .get(&name)
                                    .expect("captured after entry has a source"),
                            )?,
                    };
                    (!equal).then_some("modified")
                }
                (None, None) => unreachable!("name came from an input tree"),
            };
            if let Some(status) = status {
                files.push(ChangedFile::new(status.into(), name.clone(), name.clone()));
            }
            if let Some(entry) = left {
                captured_left.insert(name.clone(), entry);
            }
            if let Some(entry) = right {
                captured_right.insert(name, entry);
            }
        }
        let contents = Self {
            git_context: false,
            recorded_working: None,
            left: captured_left,
            right: captured_right,
            limit,
            capture: Arc::new(builder.finish()),
        };
        Comparison::assemble(
            ComparisonMetadata {
                root: right.clone(),
                operation: String::new(),
                base: left.display().to_string(),
                tip: right.display().to_string(),
                capture: next_capture(),
            },
            files,
            ContentSource::External(Arc::new(contents)),
            limit,
        )
    }

    pub(crate) fn paths(
        &self,
        side: FileSide,
    ) -> Result<BTreeMap<String, crate::repository_file::FileKind>, String> {
        match side {
            FileSide::Before => &self.left,
            FileSide::After => &self.right,
        }
        .iter()
        .map(|(path, source)| Ok((path.clone(), source.kind.clone())))
        .collect()
    }
    pub(crate) fn executable_changes(&self) -> impl Iterator<Item = (&str, bool, bool)> {
        self.left.iter().filter_map(|(path, before)| {
            let after = self.right.get(path)?;
            (before.kind == crate::repository_file::FileKind::Regular
                && after.kind == crate::repository_file::FileKind::Regular
                && before.executable != after.executable)
                .then_some((path.as_str(), before.executable, after.executable))
        })
    }

    pub(crate) fn read_path(&self, path: &str, side: FileSide) -> Result<String, String> {
        let read =
            |paths: &BTreeMap<String, ExternalEntry>, name: &str| -> Result<String, String> {
                let Some(path) = paths.get(name) else {
                    return Ok(String::new());
                };
                self.capture.read(path, self.limit)
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
