//! Display-only paths and changed-file hierarchy. Original paths stay untouched.
use std::collections::{BTreeMap, BTreeSet};

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

fn display_component(value: &str) -> String {
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
