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
    change: Option<FileChange>,
}
impl FileEntry {
    pub fn unchanged(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            change: None,
        }
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
