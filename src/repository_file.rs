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
impl From<String> for ChangeKind {
    fn from(value: String) -> Self {
        match value.as_str() {
            "added" => Self::Added,
            "removed" => Self::Removed,
            "renamed" => Self::Renamed,
            "copied" => Self::Copied,
            "modified" => Self::Modified,
            _ => Self::Other(value),
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
        self.change.as_ref().map(|change| {
            ChangedFile::new(
                change.kind.clone(),
                change.before_path.clone(),
                self.path.clone(),
            )
        })
    }
}
impl From<&ChangedFile> for FileEntry {
    fn from(file: &ChangedFile) -> Self {
        Self {
            path: file.after_path().to_owned(),
            content_kind: FileKind::Regular,
            type_change: None,
            change: Some(FileChange {
                kind: file.kind().clone(),
                before_path: file.before_path().to_owned(),
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
        let copy = ChangedFile::new("copied".into(), plain.path().into(), "copy.rs".into());
        let mut entries = vec![FileEntry::from(&copy)];
        assert_eq!(find_entry(&entries, plain.path()), None);
        entries.push(plain);
        assert_eq!(find_entry(&entries, "src/lib.rs"), Some(1));
        entries[0] = FileEntry::from(&ChangedFile::new(
            ChangeKind::Renamed,
            copy.before_path().to_owned(),
            copy.after_path().to_owned(),
        ));
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
        before: std::collections::BTreeMap<String, FileKind>,
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
            let before_kind = before.get(file.before_path());
            let after_kind = entries
                .get(file.after_path())
                .map(|entry| &entry.content_kind);
            // Presence comes from source trees, not whether displayed text is empty.
            let kind = after_kind
                .or(before_kind)
                .cloned()
                .unwrap_or(FileKind::Regular);
            let type_change = before_kind
                .zip(after_kind)
                .filter(|(before, after)| before != after)
                .map(|(before, after)| (before.clone(), after.clone()));
            let mut entry = FileEntry::from(file).with_content_kind(kind);
            entry.type_change = type_change;
            entries.insert(file.after_path().to_owned(), entry);
            if file.kind() == &ChangeKind::Renamed {
                renames.insert(file.before_path().to_owned(), file.after_path().to_owned());
            }
        }
        Self { entries, renames }
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
    fn renamed_and_copied_entries_use_source_kinds_without_hiding_unchanged_sources() {
        let before = [("source", FileKind::Symlink), ("old", FileKind::Symlink)]
            .into_iter()
            .map(|(path, kind)| (path.into(), kind))
            .collect();
        let after = [
            ("source", FileKind::Symlink),
            ("copy", FileKind::Regular),
            ("renamed", FileKind::Symlink),
        ]
        .into_iter()
        .map(|(path, kind)| (path.into(), kind))
        .collect();
        let changes = [
            ChangedFile::new("copied".into(), "source".into(), "copy".into()),
            ChangedFile::new("renamed".into(), "old".into(), "renamed".into()),
        ];
        let catalog = FileCatalog::new(before, after, &changes);
        assert!(catalog.find("source").unwrap().kind().is_none());
        assert_eq!(
            catalog
                .find("copy")
                .unwrap()
                .type_change_notice()
                .as_deref(),
            Some("File type changed: symlink → regular file")
        );
        assert_eq!(catalog.find("old").unwrap().path(), "renamed");
        assert_eq!(
            catalog.find("renamed").unwrap().content_kind(),
            &FileKind::Symlink
        );
        assert!(
            catalog
                .find("renamed")
                .unwrap()
                .type_change_notice()
                .is_none()
        );
    }

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
            ChangedFile::new("renamed".into(), "old.rs".into(), "renamed.rs".into()),
            ChangedFile::new("removed".into(), "gone.rs".into(), "gone.rs".into()),
            ChangedFile::new("copied".into(), "original.rs".into(), "copy.rs".into()),
        ];
        let catalog = FileCatalog::new(Default::default(), paths, &changes);
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

#[cfg(test)]
mod unknown_kind_tests {
    use super::{ChangeKind, FileEntry};
    use crate::repository::parse_changed_files;

    #[test]
    fn unknown_adapter_status_survives_catalog_conversion_and_selection() {
        // Defensive forward compatibility for a future jj status value.
        let files =
            parse_changed_files("[\"future-status\",\"old λ.txt\",\"new λ.txt\"]\n").unwrap();
        let selected = FileEntry::from(&files[0]);
        assert_eq!(
            selected.kind(),
            Some(&ChangeKind::Other("future-status".into()))
        );
        let changed = selected.changed().unwrap();
        assert_eq!(changed.status(), "future-status");
        assert_eq!(changed.before_path(), "old λ.txt");
        assert_eq!(changed.after_path(), "new λ.txt");
    }
}
