//! File glyphs reuse the bundled icon catalog and the shared syntax associations.
use gpui::assets::IconName;
use jaywalk::repository_file::{FileEntry, FileKind};
use std::path::Path;

pub(super) fn file_icon(file: &FileEntry) -> IconName {
    match file.content_kind() {
        FileKind::Symlink => return IconName::FileSymlink,
        FileKind::Submodule => return IconName::GitBranch,
        _ => {}
    }
    let path = Path::new(file.path());
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".lock") || name.contains("-lock.") {
        return IconName::FileLock;
    }
    if matches!(
        name.as_str(),
        ".gitignore" | ".gitattributes" | ".gitmodules"
    ) {
        return IconName::GitBranch;
    }
    if matches!(name.as_str(), "justfile" | "dockerfile") {
        return IconName::FileTerminal;
    }
    if name.starts_with("readme") || name.starts_with("license") {
        return IconName::FileText;
    }
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "avif" => IconName::FileImage,
        "zip" | "gz" | "tar" | "xz" | "7z" | "bz2" => IconName::FileArchive,
        _ => match jaywalk::syntax::file_language(file.path()) {
            "json" => IconName::Braces,
            "toml" | "yaml" | "ini" | "xml" => IconName::FileCog,
            "bash" | "fish" | "make" => IconName::FileTerminal,
            "markdown" => IconName::FileText,
            "text" => IconName::File,
            _ => IconName::FileCode,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_associations_cover_code_configs_documents_assets_and_unknowns() {
        for (path, icon) in [
            ("src/lib.rs", IconName::FileCode),
            ("types/example.d.ts", IconName::FileCode),
            ("Cargo.toml", IconName::FileCog),
            ("Cargo.lock", IconName::FileLock),
            ("package.json", IconName::Braces),
            ("README.md", IconName::FileText),
            ("Makefile", IconName::FileTerminal),
            ("justfile", IconName::FileTerminal),
            (".gitignore", IconName::GitBranch),
            ("assets/LOGO.SVG", IconName::FileImage),
            ("file.unknown", IconName::File),
        ] {
            assert_eq!(file_icon(&FileEntry::unchanged(path)), icon, "{path}");
        }
        assert_eq!(
            file_icon(&FileEntry::unchanged("link.rs").with_content_kind(FileKind::Symlink)),
            IconName::FileSymlink
        );
    }
}
