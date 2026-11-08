//! One file association path for every viewer. Grammar names and their normal
//! aliases come from GPUI Kit; only extra filesystem conventions live here.
use gpui_kit::component::highlighter::Language;
use std::path::Path;

pub fn file_language(path: &str) -> &'static str {
    let path = Path::new(path);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let named = match filename.as_str() {
        "makefile" | "gnumakefile" => Some("make"),
        "cmakelists.txt" => Some("cmake"),
        "cargo.lock" | "pipfile" => Some("toml"),
        "pipfile.lock" => Some("json"),
        "gemfile" | "rakefile" | "guardfile" | "vagrantfile" | "brewfile" => Some("ruby"),
        ".bashrc" | ".bash_profile" | ".bash_login" | ".profile" | ".zshrc" | ".zprofile" => {
            Some("bash")
        }
        _ if filename.starts_with("makefile.") || filename.starts_with("gnumakefile.") => {
            Some("make")
        }
        _ => None,
    };
    if let Some(language) = named {
        return Language::from_str(language).name();
    }
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    // Uppercase .C/.H conventionally denote C++ on case-sensitive systems.
    if matches!(extension, "C" | "H") {
        return "cpp";
    }
    let extension = extension.to_ascii_lowercase();
    let language = match extension.as_str() {
        "h" => "c",
        "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" => "cpp",
        "jsx" | "mjs" | "cjs" => "javascript",
        "mts" | "cts" => "typescript",
        "htm" | "xhtml" => "html",
        "gql" => "graphql",
        "pyw" | "pyi" => "python",
        "rake" | "gemspec" => "ruby",
        "exs" => "elixir",
        "sc" => "scala",
        "patch" => "diff",
        "mk" | "mak" => "make",
        "mdown" | "mkd" => "markdown",
        "zsh" | "ksh" | "bats" => "bash",
        other => other,
    };
    Language::from_str(language).name()
}

#[cfg(test)]
mod tests {
    use super::file_language;
    use gpui_kit::component::highlighter::{Language, LanguageRegistry, PreparedInputHighlighter};

    #[test]
    fn conventional_names_and_extension_aliases_share_provider_names() {
        for (path, language) in [
            ("README.md", "markdown"),
            ("docs/guide.MARKDOWN", "markdown"),
            ("notes.mdx", "markdown"),
            ("Cargo.toml", "toml"),
            ("Cargo.lock", "toml"),
            ("Makefile", "make"),
            ("GNUmakefile", "make"),
            ("makefile", "make"),
            ("build/Makefile.in", "make"),
            ("rules.mk", "make"),
            ("CMakeLists.txt", "cmake"),
            ("src/lib.rs", "rust"),
            ("lib/entry.mjs", "javascript"),
            ("view.jsx", "javascript"),
            ("api.mts", "typescript"),
            ("header.hpp", "cpp"),
            ("source.C", "cpp"),
            ("types.pyi", "python"),
            ("Gemfile", "ruby"),
            ("script.exs", "elixir"),
            (".bashrc", "bash"),
            ("style.scss", "css"),
            ("config.yml", "yaml"),
            ("sample.cs", "csharp"),
            ("main.go", "go"),
            ("unknown.xyz", "text"),
            ("README", "text"),
        ] {
            assert_eq!(file_language(path), language, "{path}");
        }
    }

    #[test]
    fn full_bundle_registers_and_initializes_all_its_grammars() {
        let registry = LanguageRegistry::singleton();
        let mut count = 0;
        for language in Language::all().filter(|language| *language != Language::Plain) {
            let name = language.name();
            let config = registry
                .language(name)
                .expect("a bundled language is registered");
            assert!(config.language.is_some(), "{name} has a grammar");
            let factory = PreparedInputHighlighter::new(name, "fixture\n").into_factory();
            let highlighter = factory(name).expect("a bundled grammar initializes");
            assert_eq!(
                highlighter.language().as_ref(),
                name,
                "{name} must not silently fall back to text"
            );
            count += 1;
        }
        assert!(count >= 30, "the complete provider bundle is enabled");
    }
}
