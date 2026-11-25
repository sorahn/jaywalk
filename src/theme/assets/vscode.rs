use super::*;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

/// Representative TextMate scopes for the toolkit's fixed Tree-sitter palette.
/// This is an approximation, never TextMate scope-stack or LSP execution.
pub const SCOPE_MAP: &[(&str, &[&str])] = &[
    ("attribute", &["entity.other.attribute-name"]),
    ("boolean", &["constant.language.boolean"]),
    ("comment", &["comment"]),
    ("comment_doc", &["comment.block.documentation"]),
    ("constant", &["constant"]),
    (
        "constructor",
        &["entity.name.function.constructor", "support.class"],
    ),
    ("embedded", &["meta.embedded"]),
    ("emphasis", &["markup.italic"]),
    ("emphasis.strong", &["markup.bold"]),
    ("enum", &["entity.name.type.enum"]),
    ("function", &["entity.name.function", "support.function"]),
    ("hint", &["markup.hint"]),
    (
        "keyword",
        &[
            "keyword",
            "keyword.control",
            "storage.type",
            "storage.modifier",
        ],
    ),
    ("label", &["entity.name.label"]),
    ("link_text", &["markup.underline.link"]),
    ("link_uri", &["markup.underline.link.image"]),
    ("number", &["constant.numeric"]),
    ("operator", &["keyword.operator"]),
    ("predictive", &["markup.inserted"]),
    (
        "preproc",
        &["meta.preprocessor", "keyword.control.directive"],
    ),
    ("primary", &["variable"]),
    (
        "property",
        &["variable.other.property", "support.type.property-name"],
    ),
    ("punctuation", &["punctuation"]),
    ("punctuation.bracket", &["punctuation.section"]),
    ("punctuation.delimiter", &["punctuation.separator"]),
    ("punctuation.list_marker", &["punctuation.definition.list"]),
    ("punctuation.special", &["punctuation.definition"]),
    ("string", &["string"]),
    ("string.escape", &["constant.character.escape"]),
    ("string.regex", &["string.regexp"]),
    ("string.special", &["string.other"]),
    ("string.special.symbol", &["constant.other.symbol"]),
    ("tag", &["entity.name.tag"]),
    ("tag.doctype", &["meta.tag.sgml.doctype"]),
    ("text.code.span", &["markup.inline.raw"]),
    ("text.literal", &["markup.raw"]),
    ("title", &["entity.name.section", "markup.heading"]),
    (
        "type",
        &[
            "entity.name.type",
            "entity.name.class",
            "support.type",
            "support.class",
        ],
    ),
    ("variable", &["variable"]),
    ("variable.special", &["variable.language"]),
    ("variant", &["variable.other.enummember"]),
];
pub const CAPTURES: &[&str] = &[
    "attribute",
    "boolean",
    "comment",
    "comment_doc",
    "constant",
    "constructor",
    "embedded",
    "emphasis",
    "emphasis.strong",
    "enum",
    "function",
    "hint",
    "keyword",
    "label",
    "link_text",
    "link_uri",
    "number",
    "operator",
    "predictive",
    "preproc",
    "primary",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.list_marker",
    "punctuation.special",
    "string",
    "string.escape",
    "string.regex",
    "string.special",
    "string.special.symbol",
    "tag",
    "tag.doctype",
    "text.code.span",
    "text.literal",
    "title",
    "type",
    "variable",
    "variable.special",
    "variant",
];
const EDITOR: &[(&str, &str)] = &[
    ("editor.background", "editor.background"),
    ("editor.foreground", "editor.foreground"),
    ("editorGutter.background", "editor.gutter.background"),
    ("editorLineNumber.foreground", "editor.line_number"),
    (
        "editorLineNumber.activeForeground",
        "editor.active_line_number",
    ),
    ("editor.selectionBackground", "editor.selection.background"),
    ("editorCursor.foreground", "editor.caret"),
    ("editorWhitespace.foreground", "editor.invisible"),
    (
        "editor.lineHighlightBackground",
        "editor.active_line.background",
    ),
];
#[derive(Default)]
struct Document {
    name: Option<String>,
    mode: Option<Mode>,
    colors: BTreeMap<String, String>,
    rules: Vec<Value>,
}
#[derive(Default)]
struct Reader {
    active: HashSet<PathBuf>,
    files: BTreeMap<PathBuf, Vec<u8>>,
    bytes: usize,
    diagnostics: BTreeSet<String>,
}
impl Reader {
    fn read(&mut self, path: &Path, root: &Path, depth: usize) -> Result<Document, String> {
        if depth > MAX_DEPTH {
            return Err(format!("Theme includes exceed depth {MAX_DEPTH}"));
        }
        let path = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !path.starts_with(root) {
            return Err(format!(
                "Theme include escapes selected file's directory: {}",
                path.display()
            ));
        }
        if !self.active.insert(path.clone()) {
            return Err(format!("Theme include cycle at {}", path.display()));
        }
        if self.files.len() >= MAX_FILES {
            return Err(format!("Theme includes exceed {MAX_FILES} files"));
        }
        let bytes = read(&path, MAX_FILE_BYTES)?;
        self.bytes += bytes.len();
        if self.bytes > MAX_TOTAL_BYTES {
            return Err("Theme dependencies exceed 8 MiB".into());
        }
        let text =
            std::str::from_utf8(&bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;
        let value = jsonc::parse(text).map_err(|e| format!("{}: {e}", path.display()))?;
        self.files.insert(path.clone(), bytes);
        let object = value
            .as_object()
            .ok_or("VS Code theme must be a JSON object")?;
        let mut doc = if let Some(include) = object.get("include") {
            let include = include
                .as_str()
                .ok_or("Theme include must be a local filename")?;
            self.read(
                &path
                    .parent()
                    .expect("canonical file has parent")
                    .join(include),
                root,
                depth + 1,
            )?
        } else {
            Document::default()
        };
        if let Some(name) = object.get("name").and_then(Value::as_str) {
            doc.name = Some(name.chars().take(160).collect());
        }
        if let Some(mode) = object.get("type") {
            doc.mode = Some(match mode.as_str() {
                Some("light" | "hcLight") => Mode::Light,
                Some("dark" | "hc") => Mode::Dark,
                _ => return Err("Unsupported VS Code theme type".into()),
            });
        }
        if let Some(colors) = object.get("colors") {
            for (key, value) in colors.as_object().ok_or("Theme colors must be an object")? {
                if value.is_null() {
                    doc.colors.remove(key);
                } else {
                    doc.colors.insert(
                        key.clone(),
                        value
                            .as_str()
                            .ok_or("Theme colors must be hex strings or null")?
                            .into(),
                    );
                }
            }
        }
        if let Some(rules) = object.get("tokenColors") {
            if rules.is_string() {
                return Err("External .tmTheme tokenColors files are not supported; choose a JSON tokenColors theme".into());
            }
            doc.rules.extend(
                rules
                    .as_array()
                    .ok_or("tokenColors must be an array")?
                    .iter()
                    .cloned(),
            );
        }
        if object.contains_key("semanticTokenColors") {
            self.diagnostics.insert("semanticTokenColors are not applied: Jaywalk uses Tree-sitter, without LSP semantic tokens.".into());
        }
        self.active.remove(&path);
        Ok(doc)
    }
}
fn hex(text: &str) -> Result<u32, String> {
    let s = text
        .strip_prefix('#')
        .ok_or_else(|| format!("Invalid theme color {text:?}"))?;
    let s = match s.len() {
        3 | 4 => s.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 | 8 => s.to_owned(),
        _ => return Err(format!("Invalid theme color {text:?}")),
    };
    let value = u32::from_str_radix(&s, 16).map_err(|_| format!("Invalid theme color {text:?}"))?;
    Ok(if s.len() == 6 {
        (value << 8) | 255
    } else {
        value
    })
}
fn opaque(color: u32, base: u32) -> u32 {
    let alpha = (color & 255) as f64 / 255.;
    let mut out = 255;
    for shift in [8, 16, 24] {
        let c = ((color >> shift) & 255) as f64;
        let b = ((base >> shift) & 255) as f64;
        out |= ((c * alpha + b * (1. - alpha)).round() as u32) << shift;
    }
    out
}
#[derive(Clone)]
struct Rule {
    scopes: Vec<String>,
    color: Option<u32>,
    font: Option<(bool, bool)>,
}
pub(super) fn load(path: &Path, fallback_dark: bool) -> Result<Asset, String> {
    let path = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = Reader::default();
    let doc = reader.read(&path, path.parent().expect("canonical file has parent"), 0)?;
    if doc.rules.len() > 16384 {
        return Err("Theme exceeds 16384 token rules".into());
    }
    let mut editor = BTreeMap::new();
    for (source, target) in EDITOR {
        if let Some(value) = doc.colors.get(*source) {
            editor.insert((*target).into(), hex(value)?);
        }
    }
    let mut rules = Vec::new();
    let mut unsupported = BTreeSet::new();
    for raw in &doc.rules {
        let settings = raw
            .get("settings")
            .and_then(Value::as_object)
            .ok_or("Each tokenColors entry needs a settings object")?;
        let scopes = match raw.get("scope") {
            None => vec![],
            Some(Value::String(s)) => s
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect(),
            Some(Value::Array(values)) => {
                let mut scopes = vec![];
                for value in values {
                    let s = value.as_str().ok_or("Token scopes must be strings")?;
                    scopes.extend(
                        s.split(',')
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty()),
                    );
                }
                scopes
            }
            _ => return Err("Token scope must be a string or string array".into()),
        };
        for scope in &scopes {
            if !SCOPE_MAP
                .iter()
                .any(|(_, targets)| targets.iter().any(|target| matches_scope(scope, target)))
            {
                unsupported.insert(scope.clone());
            }
        }
        let color = settings
            .get("foreground")
            .map(|v| hex(v.as_str().ok_or("Token foreground must be a hex color")?))
            .transpose()?;
        let font = settings
            .get("fontStyle")
            .map(|value| {
                let style = value.as_str().ok_or("fontStyle must be a string")?;
                for part in style.split_whitespace() {
                    if !matches!(part, "bold" | "italic") {
                        reader.diagnostics.insert(format!(
                            "Token style {part:?} is not rendered (supported: bold, italic)."
                        ));
                    }
                }
                Ok::<_, String>((
                    style.split_whitespace().any(|s| s == "italic"),
                    style.split_whitespace().any(|s| s == "bold"),
                ))
            })
            .transpose()?;
        rules.push(Rule {
            scopes,
            color,
            font,
        });
    }
    if !unsupported.is_empty() {
        reader.diagnostics.insert(format!("{} unsupported or context/language-specific scope selectors; examples: {}. See docs/themes.md for capture coverage.",unsupported.len(),unsupported.into_iter().take(5).collect::<Vec<_>>().join(", ")));
    }
    let dark = doc.mode.map_or(fallback_dark, Mode::dark);
    let base = if dark { 0x121212ff } else { 0xf7f7f7ff };
    let background = opaque(
        editor.get("editor.background").copied().unwrap_or(base),
        base,
    );
    editor.insert("editor.background".into(), background);
    let mut normal = TokenStyle {
        color: Some(editor.get("editor.foreground").copied().unwrap_or(if dark {
            0xd4d4d4ff
        } else {
            0x1f1f1fff
        })),
        italic: false,
        bold: false,
    };
    for rule in rules.iter().filter(|r| r.scopes.is_empty()) {
        merge(&mut normal, rule);
    }
    editor
        .entry("editor.foreground".into())
        .or_insert(normal.color.expect("normal foreground exists"));
    let mut syntax = BTreeMap::new();
    for (capture, targets) in SCOPE_MAP {
        let mut matching = rules
            .iter()
            .enumerate()
            .filter_map(|(order, rule)| {
                rule.scopes
                    .iter()
                    .filter(|s| targets.iter().any(|t| matches_scope(s, t)))
                    .map(|s| s.split('.').count())
                    .max()
                    .map(|specificity| (specificity, order, rule))
            })
            .collect::<Vec<_>>();
        matching.sort_by_key(|(specificity, order, _)| (*specificity, *order));
        let mut style = normal.clone();
        for (_, _, rule) in matching {
            merge(&mut style, rule);
        }
        syntax.insert((*capture).into(), style);
    }
    let mut digest = Sha256::new();
    for (path, bytes) in &reader.files {
        digest.update(path.as_os_str().as_encoded_bytes());
        digest.update([0]);
        digest.update(bytes);
        digest.update([0]);
    }
    Ok(Asset {
        source: path.clone(),
        name: doc.name.unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        }),
        mode: doc.mode,
        identity: format!("{:x}", digest.finalize()),
        editor,
        syntax,
        diagnostics: reader.diagnostics.into_iter().collect(),
        dependencies: reader.files.into_keys().collect(),
    })
}
fn merge(style: &mut TokenStyle, rule: &Rule) {
    if let Some(color) = rule.color {
        style.color = Some(color);
    }
    if let Some((italic, bold)) = rule.font {
        style.italic = italic;
        style.bold = bold;
    }
}
fn matches_scope(selector: &str, target: &str) -> bool {
    !selector
        .bytes()
        .any(|b| b.is_ascii_whitespace() || matches!(b, b'|' | b'&' | b'-' | b'(' | b')'))
        && (selector == target
            || target
                .strip_prefix(selector)
                .is_some_and(|suffix| suffix.starts_with('.')))
}
