use crate::repository_file::FileEntry;
use std::collections::{BTreeMap, BTreeSet};

/// Resolve a followed file by its current path, then an explicit rename.
/// A copy does not move the source file's identity to the new destination.
pub fn find_file(files: &[crate::repository::ChangedFile], path: &str) -> Option<usize> {
    files
        .iter()
        .position(|file| file.after_path == path)
        .or_else(|| {
            files
                .iter()
                .position(|file| file.status == "renamed" && file.before_path == path)
        })
}

/// UI-only projection: retain one preview without changing the actual diff.
pub fn with_preview(
    files: &[crate::repository::ChangedFile],
    open: Option<&FileEntry>,
) -> (Vec<FileEntry>, Option<usize>) {
    let mut displayed: Vec<_> = files.iter().map(FileEntry::from).collect();
    let retained = open
        .filter(|file| find_file(files, file.path()).is_none())
        .map(|file| {
            let index = displayed
                .iter()
                .position(|entry| entry.path() > file.path())
                .unwrap_or(displayed.len());
            displayed.insert(index, file.clone());
            index
        });
    (displayed, retained)
}

#[derive(Debug)]
pub struct CompactPath {
    variants: Vec<String>,
}

impl CompactPath {
    pub fn label(&self, columns: usize) -> &str {
        self.variants
            .iter()
            .find(|label| label.chars().count() <= columns)
            .or_else(|| self.variants.last())
            .map(String::as_str)
            .expect("every compact path retains its original display path")
    }
}

pub enum FileNode {
    Directory {
        path: String,
        name: String,
        children: Vec<FileNode>,
    },
    File {
        index: usize,
        name: String,
    },
}

#[derive(Default)]
struct Directory {
    directories: BTreeMap<String, Self>,
    files: Vec<(usize, String)>,
}

impl Directory {
    fn insert(&mut self, parts: &[&str], index: usize) {
        if let Some((name, parents)) = parts.split_last() {
            let mut directory = self;
            for parent in parents {
                directory = directory
                    .directories
                    .entry((*parent).to_owned())
                    .or_default();
            }
            directory.files.push((index, display_component(name)));
        }
    }

    fn into_nodes(self, parent: &str) -> Vec<FileNode> {
        let mut nodes: Vec<_> = self
            .directories
            .into_iter()
            .map(|(name, directory)| {
                let path = if parent.is_empty() {
                    name.clone()
                } else {
                    format!("{parent}/{name}")
                };
                FileNode::Directory {
                    children: directory.into_nodes(&path),
                    name: display_component(&name),
                    path,
                }
            })
            .collect();
        let mut files = self.files;
        files.sort_by(|a, b| a.1.cmp(&b.1));
        nodes.extend(
            files
                .into_iter()
                .map(|(index, name)| FileNode::File { index, name }),
        );
        nodes
    }
}

pub(crate) fn display_component(value: &str) -> String {
    value
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn shortest_prefix<'a>(name: &'a str, siblings: &BTreeSet<&str>) -> &'a str {
    // Keep a dot plus at least one character in hidden directory names.
    let minimum = if name.starts_with('.') { 2 } else { 1 };
    for (count, (offset, character)) in name.char_indices().enumerate() {
        if count + 1 < minimum {
            continue;
        }
        let prefix = &name[..offset + character.len_utf8()];
        if siblings
            .iter()
            .filter(|sibling| sibling.starts_with(prefix))
            .count()
            == 1
        {
            return prefix;
        }
    }
    name
}

pub fn present_paths(paths: &[&str]) -> (Vec<CompactPath>, Vec<FileNode>) {
    let parts: Vec<Vec<&str>> = paths.iter().map(|path| path.split('/').collect()).collect();
    let mut siblings: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    let mut tree = Directory::default();
    for (index, path) in parts.iter().enumerate() {
        tree.insert(path, index);
        for (depth, directory) in path.iter().take(path.len().saturating_sub(1)).enumerate() {
            siblings
                .entry(path[..depth].join("/"))
                .or_default()
                .insert(directory);
        }
    }
    let labels = parts
        .iter()
        .map(|parts| {
            let mut shortened: Vec<_> = parts.iter().map(|part| display_component(part)).collect();
            let mut variants = vec![shortened.join("/")];
            for depth in 0..parts.len().saturating_sub(1) {
                if let Some(peers) = siblings.get(&parts[..depth].join("/")) {
                    shortened[depth] = display_component(shortest_prefix(parts[depth], peers));
                    let candidate = shortened.join("/");
                    if variants.last() != Some(&candidate) {
                        variants.push(candidate);
                    }
                }
            }
            CompactPath { variants }
        })
        .collect();
    (labels, tree.into_nodes(""))
}

#[cfg(test)]
mod tests {
    use super::{FileNode, present_paths};

    #[test]
    fn shortens_only_directories_and_keeps_sibling_prefixes_distinct() {
        let paths = ["source/client/unchanged-file.rs", "source/config/other.rs"];
        let (labels, _) = present_paths(&paths);
        assert_eq!(labels[0].label(200), paths[0]);
        assert_eq!(labels[0].label(1), "s/cl/unchanged-file.rs");
        assert_eq!(labels[1].label(1), "s/co/other.rs");
    }

    #[test]
    fn unicode_hidden_directories_and_control_characters_are_display_safe() {
        let (labels, _) = present_paths(&[".github/éclair/file\nname.rs"]);
        assert_eq!(labels[0].label(1), ".g/é/file\\nname.rs");
    }

    #[test]
    fn removed_file_and_new_directory_can_share_a_name() {
        let (_, nodes) = present_paths(&["src", "src/lib.rs"]);
        assert!(
            matches!(&nodes[0], FileNode::Directory { children, .. } if matches!(children[0], FileNode::File { index: 1, .. }))
        );
        assert!(matches!(&nodes[1], FileNode::File { index: 0, .. }));
    }
}

#[cfg(test)]
mod preview_tests {
    use super::{find_file, with_preview};
    use crate::repository::ChangedFile;
    use crate::repository_file::{FileEntry, find_entry};
    fn file(status: &str, before: &str, after: &str) -> ChangedFile {
        ChangedFile {
            status: status.into(),
            before_path: before.into(),
            after_path: after.into(),
        }
    }
    #[test]
    fn preview_is_temporary_and_never_becomes_a_changed_file() {
        let open = file("modified", "src/open.rs", "src/open.rs");
        let changed = [file("modified", "other.rs", "other.rs")];
        let (rows, retained) = with_preview(&changed, Some(&FileEntry::from(&open)));
        assert_eq!(changed.len(), 1);
        assert_eq!(rows.len(), 2);
        assert_eq!(retained, Some(1));
        let (rows, retained) = with_preview(&changed, Some(&FileEntry::from(&changed[0])));
        assert_eq!(rows.len(), 1);
        assert_eq!(retained, None);
        assert_eq!(
            with_preview(std::slice::from_ref(&open), Some(&FileEntry::from(&open))).1,
            None
        );
    }
    #[test]
    fn preview_keeps_its_sorted_place_and_changed_rows_resolve_by_path() {
        let changed = [
            file("modified", "Cargo.toml", "Cargo.toml"),
            file("modified", "src/main.rs", "src/main.rs"),
        ];
        for (path, expected) in [("Cargo.lock", 0), ("README.md", 1), ("zz.txt", 2)] {
            let open = file("modified", path, path);
            let (rows, retained) = with_preview(&changed, Some(&FileEntry::from(&open)));
            assert_eq!(retained, Some(expected));
            assert_eq!(rows[expected].path(), path);
            assert!(rows.windows(2).all(|pair| pair[0].path() < pair[1].path()));
            // Inserting a preview shifts display indices, never comparison indices.
            let row = find_entry(&rows, "src/main.rs").unwrap();
            assert_eq!(find_file(&changed, rows[row].path()), Some(1));
            let (closed, retained) = with_preview(&changed, None);
            assert_eq!(retained, None);
            assert_eq!(
                closed.iter().map(|file| file.path()).collect::<Vec<_>>(),
                vec!["Cargo.toml", "src/main.rs"]
            );
        }
    }

    #[test]
    fn empty_comparison_can_retain_a_file_until_it_reappears_or_is_closed() {
        let open = file("modified", "old.rs", "old.rs");
        let (rows, retained) = with_preview(&[], Some(&FileEntry::from(&open)));
        assert_eq!(retained, Some(0));
        assert_eq!(rows[0].path(), "old.rs");
        assert!(with_preview(&[], None).0.is_empty());
        let renamed = [file("renamed", "old.rs", "new.rs")];
        let (rows, retained) = with_preview(&renamed, Some(&FileEntry::from(&open)));
        assert_eq!(rows.len(), 1);
        assert_eq!(retained, None);
        assert_eq!(find_entry(&rows, &open.after_path), Some(0));
    }

    #[test]
    fn follows_explicit_renames_but_not_copies_or_unrelated_names() {
        let rename = file("renamed", "old.rs", "new.rs");
        assert_eq!(find_file(std::slice::from_ref(&rename), "old.rs"), Some(0));
        assert_eq!(
            find_file(&[file("copied", "old.rs", "copy.rs")], "old.rs"),
            None
        );
        assert_eq!(
            find_file(std::slice::from_ref(&rename), "other/old.rs"),
            None
        );
        assert_eq!(
            find_file(&[rename, file("added", "old.rs", "old.rs")], "old.rs"),
            Some(1)
        );
    }
}
