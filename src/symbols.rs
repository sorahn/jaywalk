//! Syntax declarations in immutable comparison sources; no workspace reads or LSP.
use crate::{
    repository::{Comparison, FileSide},
    repository_file::{ChangeKind, FileEntry, FileKind},
    syntax::file_language,
};
use gpui_kit::component::highlighter::LanguageRegistry;
use std::{
    ops::ControlFlow,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use tree_sitter::{Node, ParseOptions, Parser};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub container: String,
    pub kind: &'static str,
    /// Zero-based original source line, never a diff or wrapped row.
    pub line: usize,
}
impl Symbol {
    pub fn label(&self) -> String {
        if self.container.is_empty() {
            self.name.clone()
        } else {
            format!("{} · {}", self.name, self.container)
        }
    }
}
#[derive(Clone, Debug)]
pub struct Entry {
    /// Catalog path used to select the file, including renamed/deleted entries.
    pub path: String,
    pub side: FileSide,
    pub symbol: Symbol,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    File { path: String, before: bool },
    Project,
}
#[derive(Default)]
pub struct Index {
    pub entries: Vec<Entry>,
    pub indexed_files: usize,
    pub unsupported_files: usize,
    pub skipped_files: usize,
    pub limited: bool,
    pub first_error: Option<String>,
}
impl Index {
    pub fn status(&self) -> String {
        let mut status = format!(
            "{} symbols · {} files",
            self.entries.len(),
            self.indexed_files
        );
        if self.unsupported_files > 0 {
            status.push_str(&format!(" · {} unsupported files", self.unsupported_files));
        }
        if self.skipped_files > 0 {
            status.push_str(&format!(" · {} unavailable files", self.skipped_files));
        }
        if self.limited {
            status.push_str(" · Index limit reached; results are partial");
        }
        if let Some(error) = &self.first_error {
            status.push_str(&format!(" · {error}"));
        }
        status
    }
}

/// Explicit grammar coverage: adding a language requires declaration fixtures.
pub fn supported(language: &str) -> bool {
    matches!(
        language,
        "rust"
            | "javascript"
            | "typescript"
            | "tsx"
            | "python"
            | "go"
            | "c"
            | "cpp"
            | "java"
            | "csharp"
            | "ruby"
            | "php"
    )
}
fn declaration<'tree>(language: &str, node: Node<'tree>) -> Option<(&'static str, Node<'tree>)> {
    let kind = match (language, node.kind()) {
        ("rust", "function_item" | "function_signature_item") => "function",
        ("rust", "struct_item" | "enum_item" | "trait_item" | "type_item" | "mod_item") => "type",
        ("rust", "const_item" | "static_item") => "constant",
        ("rust", "impl_item") => {
            return node.child_by_field_name("type").map(|name| ("impl", name));
        }
        (
            "javascript" | "typescript" | "tsx",
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "method_signature"
            | "abstract_method_signature"
            | "function_signature",
        ) => "function",
        (
            "javascript" | "typescript" | "tsx",
            "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "internal_module",
        ) => "type",
        ("javascript" | "typescript" | "tsx", "variable_declarator") => "variable",
        ("python", "function_definition") => "function",
        ("python", "class_definition") => "class",
        ("go", "function_declaration" | "method_declaration") => "function",
        ("go", "type_spec") => "type",
        ("go", "const_spec") => "constant",
        ("c" | "cpp", "function_definition") => "function",
        (
            "c" | "cpp",
            "struct_specifier"
            | "union_specifier"
            | "enum_specifier"
            | "class_specifier"
            | "type_definition"
            | "alias_declaration"
            | "namespace_definition",
        ) => "type",
        (
            "java" | "csharp",
            "method_declaration" | "constructor_declaration" | "local_function_statement",
        ) => "function",
        (
            "java" | "csharp",
            "class_declaration"
            | "interface_declaration"
            | "enum_declaration"
            | "record_declaration"
            | "struct_declaration"
            | "namespace_declaration",
        ) => "type",
        ("csharp", "property_declaration") => "property",
        ("ruby", "method" | "singleton_method") => "function",
        ("ruby", "class" | "module") => "type",
        ("php", "function_definition" | "method_declaration") => "function",
        (
            "php",
            "class_declaration"
            | "interface_declaration"
            | "trait_declaration"
            | "enum_declaration",
        ) => "type",
        _ => return None,
    };
    let mut name = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("declarator"))?;
    // C/C++ function/pointer declarators nest the actual identifier.
    while let Some(inner) = name.child_by_field_name("declarator") {
        name = inner;
    }
    Some((kind, name))
}

pub fn extract(
    language: &str,
    source: &str,
    cancel: &AtomicBool,
    limit: usize,
) -> Result<Vec<Symbol>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    if !supported(language) {
        return Err(format!("Symbol navigation is not supported for {language}"));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Symbol indexing cancelled".into());
    }
    let grammar = LanguageRegistry::singleton()
        .language(language)
        .and_then(|config| config.language)
        .ok_or_else(|| format!("No grammar for {language}"))?;
    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|error| format!("Could not initialize {language} symbols: {error}"))?;
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut progress = |_: &tree_sitter::ParseState| {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let tree = parser
        .parse_with_options(
            &mut |offset, _| &source.as_bytes()[offset..],
            None,
            Some(ParseOptions::new().progress_callback(&mut progress)),
        )
        .ok_or_else(|| "Symbol parsing cancelled or exceeded its time budget".to_string())?;
    let mut symbols = Vec::new();
    // Iterative walk bounds stack use even for malformed, deeply nested input.
    let mut cursor = tree.walk();
    let mut scopes: Vec<(usize, String)> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Symbol indexing cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("Symbol extraction exceeded its time budget".into());
        }
        let node = cursor.node();
        while scopes
            .last()
            .is_some_and(|(end, _)| *end <= node.start_byte())
        {
            scopes.pop();
        }
        if let Some((kind, name_node)) = declaration(language, node)
            && let Ok(name) = name_node.utf8_text(source.as_bytes())
            && !name.is_empty()
            && !name.contains(['\n', '\r'])
            && name.chars().count() <= 200
        {
            symbols.push(Symbol {
                name: name.into(),
                container: scopes[scopes.len().saturating_sub(4)..]
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join("::")
                    .chars()
                    .take(240)
                    .collect(),
                kind,
                line: name_node.start_position().row,
            });
            if symbols.len() >= limit {
                break;
            }
            scopes.push((node.end_byte(), name.into()));
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return Ok(symbols);
            }
        }
    }
    Ok(symbols)
}

fn source_side(file: &FileEntry, before: bool) -> FileSide {
    if file.kind() == Some(&ChangeKind::Removed)
        || (before && file.kind() != Some(&ChangeKind::Added))
    {
        FileSide::Before
    } else {
        FileSide::After
    }
}
/// Bounded, cancellable work for the background executor. Results contain no text.
pub fn build(comparison: &Comparison, scope: &Scope, cancel: &AtomicBool) -> Index {
    build_with_budget(comparison, scope, cancel, 32 * 1024 * 1024)
}
fn build_with_budget(
    comparison: &Comparison,
    scope: &Scope,
    cancel: &AtomicBool,
    byte_budget: usize,
) -> Index {
    const MAX_FILES: usize = 5000;
    const MAX_SYMBOLS: usize = 20000;
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut index = Index::default();
    let mut bytes = 0;
    let mut visited = 0;
    for file in comparison.catalog().entries() {
        if let Scope::File { path, .. } = scope
            && file.path() != path
        {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if visited == MAX_FILES || index.entries.len() >= MAX_SYMBOLS || Instant::now() >= deadline
        {
            index.limited = true;
            break;
        }
        visited += 1;
        let side = source_side(file, matches!(scope, Scope::File { before: true, .. }));
        let path = if side == FileSide::Before {
            file.before_path()
        } else {
            file.path()
        };
        let language = file_language(path);
        if !supported(language) {
            index.unsupported_files += 1;
            continue;
        }
        if file.content_kind() != &FileKind::Regular {
            index.skipped_files += 1;
            continue;
        }
        let result = comparison.read_path(path, side).and_then(|text| {
            if text.len() > byte_budget.saturating_sub(bytes) {
                index.limited = true;
                return Err("Symbol source budget exceeded".into());
            }
            bytes += text.len();
            extract(language, &text, cancel, MAX_SYMBOLS - index.entries.len())
        });
        match result {
            Ok(symbols) => {
                index.indexed_files += 1;
                index
                    .entries
                    .extend(symbols.into_iter().map(|symbol| Entry {
                        path: file.path().into(),
                        side,
                        symbol,
                    }));
                if index.entries.len() == MAX_SYMBOLS {
                    index.limited = true;
                }
            }
            Err(error) => {
                index.skipped_files += 1;
                if index.first_error.is_none() {
                    index.first_error = Some(format!("{}: {error}", file.path()));
                }
            }
        }
        if index.limited {
            break;
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn declarations_keep_original_lines_and_distinguish_containers() {
        let source = "// 😀\r\nstruct One {}\r\nimpl One {\r\n fn same(&self) {}\r\n}\r\nstruct Two {}\r\nimpl Two { fn same(&self) {} }";
        let symbols = extract("rust", source, &AtomicBool::new(false), 100).unwrap();
        let methods: Vec<_> = symbols.iter().filter(|s| s.name == "same").collect();
        assert_eq!(methods.len(), 2);
        assert_eq!((methods[0].line, methods[0].container.as_str()), (3, "One"));
        assert_eq!((methods[1].line, methods[1].container.as_str()), (6, "Two"));
        assert!(!symbols.iter().any(|s| s.name == "self"));
    }
    #[test]
    fn supported_grammars_find_declarations_not_calls_or_comments() {
        for (language, source, name) in [
            ("rust", "fn target() { other(); } // fn fake()", "target"),
            ("javascript", "const target = () => other();", "target"),
            ("typescript", "interface Target { run(): void; }", "Target"),
            ("tsx", "function Target() { return <div/>; }", "Target"),
            (
                "python",
                "class Target:\n def run(self):\n  other()\n",
                "Target",
            ),
            ("go", "package main\nfunc target() { other() }", "target"),
            ("c", "int target(void) { return other(); }", "target"),
            (
                "cpp",
                "namespace Target { int run() { return 0; } }",
                "Target",
            ),
            ("java", "class Target { void run() {} }", "Target"),
            ("csharp", "class Target { void Run() {} }", "Target"),
            (
                "ruby",
                "class Target\n def run\n other()\n end\nend",
                "Target",
            ),
            ("php", "<?php class Target { function run() {} }", "Target"),
        ] {
            let symbols = extract(language, source, &AtomicBool::new(false), 100).unwrap();
            assert!(
                symbols.iter().any(|s| s.name == name),
                "{language}: {symbols:?}"
            );
            assert!(
                !symbols
                    .iter()
                    .any(|s| matches!(s.name.as_str(), "other" | "fake")),
                "{language}"
            );
        }
    }
    #[test]
    fn cancellation_limits_and_unsupported_inputs_are_explicit() {
        assert!(extract("rust", "fn run() {}", &AtomicBool::new(true), 10).is_err());
        assert!(extract("text", "anything", &AtomicBool::new(false), 10).is_err());
        let symbols = extract("rust", "fn a() {}\nfn b() {}", &AtomicBool::new(false), 1).unwrap();
        assert_eq!(symbols.len(), 1);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    fn git(root: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    fn project_symbols_include_unchanged_and_deleted_files_from_the_pinned_source() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-b", "main"]);
        for (path, text) in [
            ("changed.rs", "fn before() {}"),
            ("stable.rs", "fn stable() {}"),
            ("deleted.rs", "fn deleted() {}"),
        ] {
            std::fs::write(root.path().join(path), text).unwrap();
        }
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-m", "fixture"]);
        std::fs::write(root.path().join("changed.rs"), "fn after() {}\n").unwrap();
        std::fs::remove_file(root.path().join("deleted.rs")).unwrap();
        let comparison = Comparison::load(root.path(), None, "@").unwrap();
        std::fs::write(root.path().join("changed.rs"), "fn later() {}\n").unwrap();
        std::fs::write(root.path().join("stable.rs"), "fn wrong() {}\n").unwrap();
        let index = build(&comparison, &Scope::Project, &AtomicBool::new(false));
        let names: Vec<_> = index
            .entries
            .iter()
            .map(|entry| entry.symbol.name.as_str())
            .collect();
        assert_eq!(names, ["after", "deleted", "stable"]);
        assert_eq!(index.entries[1].side, FileSide::Before);
        let before = build(
            &comparison,
            &Scope::File {
                path: "changed.rs".into(),
                before: true,
            },
            &AtomicBool::new(false),
        );
        assert_eq!(before.entries[0].symbol.name, "before");
        assert_eq!(before.entries[0].side, FileSide::Before);
        let limited = build_with_budget(&comparison, &Scope::Project, &AtomicBool::new(false), 0);
        assert!(limited.limited);
        assert!(limited.status().contains("partial"));
        assert!(limited.entries.is_empty());
    }
}
