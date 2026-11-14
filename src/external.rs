//! Read-only comparisons supplied by command-line diff tools.
use crate::repository::{
    ChangedFile, Comparison, ComparisonMetadata, ContentSource, FileSide, next_capture,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ExternalContents {
    pub(crate) git_context: bool,
    pub(crate) recorded_working: Option<String>,
    left: BTreeMap<String, PathBuf>,
    right: BTreeMap<String, PathBuf>,
    limit: crate::text_content::TextLimit,
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

fn entry_kind(path: &Path) -> Result<crate::repository_file::FileKind, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    Ok(if metadata.is_symlink() {
        crate::repository_file::FileKind::Symlink
    } else {
        crate::repository_file::FileKind::Regular
    })
}

fn bytes(path: &Path, limit: crate::text_content::TextLimit) -> Result<Vec<u8>, String> {
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
    limit.file(path)
}

fn equal_entries(
    before: &Path,
    after: &Path,
    limit: crate::text_content::TextLimit,
) -> Result<bool, String> {
    use std::io::BufRead;
    let kind = entry_kind(before)?;
    if kind != entry_kind(after)? {
        return Ok(false);
    }
    if kind == crate::repository_file::FileKind::Symlink {
        return Ok(bytes(before, limit)? == bytes(after, limit)?);
    }
    let open = |path: &Path| {
        std::fs::File::open(path)
            .map_err(|error| format!("Could not open {}: {error}", path.display()))
    };
    let left = open(before)?;
    let right = open(after)?;
    if left.metadata().map_err(|error| error.to_string())?.len()
        != right.metadata().map_err(|error| error.to_string())?.len()
    {
        return Ok(false);
    }
    let mut left = std::io::BufReader::new(left);
    let mut right = std::io::BufReader::new(right);
    loop {
        let a = left.fill_buf().map_err(|error| error.to_string())?;
        let b = right.fill_buf().map_err(|error| error.to_string())?;
        if a.is_empty() || b.is_empty() {
            return Ok(a.is_empty() && b.is_empty());
        }
        let count = a.len().min(b.len());
        if a[..count] != b[..count] {
            return Ok(false);
        }
        left.consume(count);
        right.consume(count);
    }
}

impl ExternalContents {
    pub(crate) fn swapped(&self) -> Self {
        Self {
            git_context: self.git_context,
            recorded_working: self.recorded_working.clone(),
            left: self.right.clone(),
            right: self.left.clone(),
            limit: self.limit,
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
        let mut contents = Self {
            git_context: false,
            recorded_working: None,
            left: BTreeMap::new(),
            right: BTreeMap::new(),
            limit,
        };
        if left_kind.is_dir() && right_kind.is_dir() {
            collect(&left, &left, &mut contents.left)?;
            collect(&right, &right, &mut contents.right)?;
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
                    if equal_entries(before, after, limit)? {
                        continue;
                    }
                    "modified"
                }
                (None, None) => unreachable!("name came from an input tree"),
            };
            files.push(ChangedFile::new(status.into(), name.clone(), name));
        }
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
            self.limit.text(bytes(path, self.limit)?)
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
