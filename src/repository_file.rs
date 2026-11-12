//! A file can be selected independently of whether it changed.
use crate::repository::ChangedFile;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Renamed,
    Copied,
    Modified,
    Other(String),
}
impl ChangeKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Renamed => "renamed",
            Self::Copied => "copied",
            Self::Modified => "modified",
            Self::Other(value) => value,
        }
    }
}
impl From<&str> for ChangeKind {
    fn from(value: &str) -> Self {
        match value {
            "added" => Self::Added,
            "removed" => Self::Removed,
            "renamed" => Self::Renamed,
            "copied" => Self::Copied,
            "modified" => Self::Modified,
            other => Self::Other(other.into()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileChange {
    kind: ChangeKind,
    before_path: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    path: String,
    content_kind: FileKind,
    type_change: Option<(FileKind, FileKind)>,
    change: Option<FileChange>,
}
impl FileEntry {
    pub fn unchanged(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content_kind: FileKind::Regular,
            type_change: None,
            change: None,
        }
    }
    pub fn with_content_kind(mut self, kind: FileKind) -> Self {
        self.content_kind = kind;
        self
    }
    pub fn content_kind(&self) -> &FileKind {
        &self.content_kind
    }
    pub fn type_change_notice(&self) -> Option<String> {
        let label = |kind: &FileKind| match kind {
            FileKind::Regular => "regular file",
            FileKind::Symlink => "symlink",
            FileKind::Submodule => "submodule",
            FileKind::Conflict => "conflicted file",
            FileKind::Other(_) => "unsupported entry",
        };
        self.type_change.as_ref().map(|(before, after)| {
            format!("File type changed: {} → {}", label(before), label(after))
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn before_path(&self) -> &str {
        self.change
            .as_ref()
            .map_or(&self.path, |change| &change.before_path)
    }
    pub fn kind(&self) -> Option<&ChangeKind> {
        self.change.as_ref().map(|change| &change.kind)
    }
    pub fn status(&self) -> Option<&str> {
        self.kind().map(ChangeKind::as_str)
    }
    pub fn changed(&self) -> Option<ChangedFile> {
        self.change.as_ref().map(|change| ChangedFile {
            status: change.kind.as_str().into(),
            before_path: change.before_path.clone(),
            after_path: self.path.clone(),
        })
    }
}
impl From<&ChangedFile> for FileEntry {
    fn from(file: &ChangedFile) -> Self {
        Self {
            path: file.after_path.clone(),
            content_kind: FileKind::Regular,
            type_change: None,
            change: Some(FileChange {
                kind: file.status.as_str().into(),
                before_path: file.before_path.clone(),
            }),
        }
    }
}
/// Resolve a current path first; only a rename transfers the old path's identity.
pub fn find_entry(files: &[FileEntry], path: &str) -> Option<usize> {
    files
        .iter()
        .position(|file| file.path() == path)
        .or_else(|| {
            files.iter().position(|file| {
                file.kind() == Some(&ChangeKind::Renamed) && file.before_path() == path
            })
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_files_need_no_fake_change_and_copies_keep_the_source_identity() {
        let plain = FileEntry::unchanged("src/lib.rs");
        assert!(plain.kind().is_none());
        assert!(plain.changed().is_none());
        let copy = ChangedFile {
            status: "copied".into(),
            before_path: plain.path().into(),
            after_path: "copy.rs".into(),
        };
        let mut entries = vec![FileEntry::from(&copy)];
        assert_eq!(find_entry(&entries, plain.path()), None);
        entries.push(plain);
        assert_eq!(find_entry(&entries, "src/lib.rs"), Some(1));
        entries[0] = FileEntry::from(&ChangedFile {
            status: "renamed".into(),
            ..copy
        });
        assert_eq!(find_entry(&entries, "copy.rs"), Some(0));
        entries.pop();
        assert_eq!(find_entry(&entries, "src/lib.rs"), Some(0));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileKind {
    Regular,
    Symlink,
    Submodule,
    Conflict,
    Other(String),
}
impl From<&str> for FileKind {
    fn from(value: &str) -> Self {
        match value {
            "file" | "100644" | "100755" => Self::Regular,
            "symlink" | "120000" => Self::Symlink,
            "git-submodule" | "160000" => Self::Submodule,
            "conflict" => Self::Conflict,
            other => Self::Other(other.into()),
        }
    }
}

/// Complete target-tree entries plus deleted paths, independent of diff counts.
#[derive(Clone, Debug, Default)]
pub struct FileCatalog {
    entries: std::collections::BTreeMap<String, FileEntry>,
    renames: std::collections::BTreeMap<String, String>,
}
impl FileCatalog {
    pub(crate) fn new(
        paths: std::collections::BTreeMap<String, FileKind>,
        changes: &[ChangedFile],
    ) -> Self {
        let mut entries: std::collections::BTreeMap<_, _> = paths
            .into_iter()
            .map(|(path, kind)| {
                (
                    path.clone(),
                    FileEntry::unchanged(path).with_content_kind(kind),
                )
            })
            .collect();
        let mut renames = std::collections::BTreeMap::new();
        for file in changes {
            let kind = entries
                .get(&file.after_path)
                .map_or(FileKind::Regular, |entry| entry.content_kind.clone());
            entries.insert(
                file.after_path.clone(),
                FileEntry::from(file).with_content_kind(kind),
            );
            if file.status == "renamed" {
                renames.insert(file.before_path.clone(), file.after_path.clone());
            }
        }
        Self { entries, renames }
    }
    pub(crate) fn set_kinds(
        &mut self,
        path: &str,
        before: Option<FileKind>,
        after: Option<FileKind>,
    ) {
        if let Some(entry) = self.entries.get_mut(path) {
            if let Some(kind) = after.as_ref().or(before.as_ref()) {
                entry.content_kind = kind.clone();
            }
            entry.type_change = before.zip(after).filter(|(before, after)| before != after);
        }
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = &FileEntry> {
        self.entries.values()
    }
    pub fn find(&self, path: &str) -> Option<&FileEntry> {
        self.entries.get(path).or_else(|| {
            self.renames
                .get(path)
                .and_then(|target| self.entries.get(target))
        })
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::{ChangeKind, FileCatalog, FileKind};
    use crate::repository::ChangedFile;
    #[test]
    fn full_tree_preserves_unchanged_sources_deleted_paths_and_rename_aliases() {
        let paths = ["original.rs", "renamed.rs", "never.rs", "link"]
            .into_iter()
            .map(|path| {
                (
                    path.into(),
                    if path == "link" {
                        FileKind::Symlink
                    } else {
                        FileKind::Regular
                    },
                )
            })
            .collect();
        let changes = [
            ChangedFile {
                status: "renamed".into(),
                before_path: "old.rs".into(),
                after_path: "renamed.rs".into(),
            },
            ChangedFile {
                status: "removed".into(),
                before_path: "gone.rs".into(),
                after_path: "gone.rs".into(),
            },
            ChangedFile {
                status: "copied".into(),
                before_path: "original.rs".into(),
                after_path: "copy.rs".into(),
            },
        ];
        let catalog = FileCatalog::new(paths, &changes);
        assert_eq!(catalog.entries().len(), 6);
        assert_eq!(catalog.find("old.rs").unwrap().path(), "renamed.rs");
        assert!(catalog.find("original.rs").unwrap().kind().is_none());
        assert_eq!(
            catalog.find("gone.rs").unwrap().kind(),
            Some(&ChangeKind::Removed)
        );
        assert_eq!(
            catalog.find("link").unwrap().content_kind(),
            &FileKind::Symlink
        );
    }
}
