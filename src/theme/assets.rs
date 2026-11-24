//! Explicitly linked, read-only local code themes. No extension code or network.
mod jsonc;
mod vscode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
pub use vscode::{CAPTURES, SCOPE_MAP};

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 32;
pub const MAX_DEPTH: usize = 8;
pub const MAX_CATALOG: usize = 128;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Light,
    Dark,
}
impl Mode {
    pub fn dark(self) -> bool {
        self == Self::Dark
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenStyle {
    pub color: Option<u32>,
    pub italic: bool,
    pub bold: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub mode: Option<Mode>,
    pub source: PathBuf,
    pub identity: String,
    pub editor: BTreeMap<String, u32>,
    pub syntax: BTreeMap<String, TokenStyle>,
    pub diagnostics: Vec<String>,
    pub dependencies: Vec<PathBuf>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub mode: Option<Mode>,
    pub source: PathBuf,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub diagnostics: Vec<String>,
}
pub fn directory(config: &Path) -> PathBuf {
    config
        .parent()
        .unwrap_or(Path::new("."))
        .join("themes/code")
}
pub(super) fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    if !fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .is_file()
    {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let f = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !f.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let mut bytes = Vec::new();
    f.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "{} exceeds the {limit}-byte theme limit",
            path.display()
        ));
    }
    Ok(bytes)
}
pub fn catalog(config: &Path) -> Catalog {
    let mut catalog = Catalog::default();
    let paths = match fs::read_dir(directory(config)) {
        Ok(paths) => paths,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return catalog,
        Err(e) => {
            catalog
                .diagnostics
                .push(format!("Could not read theme catalog: {e}"));
            return catalog;
        }
    };
    let mut paths: Vec<_> = paths
        .filter_map(Result::ok)
        .map(|p| p.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    if paths.len() > MAX_CATALOG {
        catalog
            .diagnostics
            .push(format!("Theme catalog limited to {MAX_CATALOG} entries"));
    }
    for path in paths.into_iter().take(MAX_CATALOG) {
        let result = read(&path, 8192)
            .and_then(|bytes| serde_json::from_slice::<Entry>(&bytes).map_err(|e| e.to_string()))
            .and_then(|entry| {
                if !valid_id(&entry.id) || !entry.source.is_absolute() {
                    return Err("Invalid theme ID or source path".into());
                }
                if catalog.entries.iter().any(|e| e.id == entry.id) {
                    return Err(format!("Duplicate theme ID {}", entry.id));
                }
                Ok(entry)
            });
        match result {
            Ok(entry) => catalog.entries.push(entry),
            Err(e) => catalog.diagnostics.push(format!("{}: {e}", path.display())),
        }
    }
    catalog.entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    catalog
}
pub fn valid_id(id: &str) -> bool {
    id.strip_prefix("vscode:")
        .is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
/// Registers a link only after parsing succeeds. Existing user files are never written.
pub fn import(config: &Path, path: &Path) -> Result<Entry, String> {
    let path = fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let asset = load(&path)?;
    let id = format!(
        "vscode:{:.32x}",
        Sha256::digest(path.to_string_lossy().as_bytes())
    );
    let entry = Entry {
        id,
        name: asset.name,
        mode: asset.mode,
        source: path,
    };
    let dir = directory(config);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let destination = dir.join(format!("{}.json", &entry.id[7..]));
    let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    use std::io::Write;
    temp.write_all(&serde_json::to_vec_pretty(&entry).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    temp.persist(destination).map_err(|e| e.to_string())?;
    Ok(entry)
}
pub fn load(path: &Path) -> Result<Asset, String> {
    vscode::load(path, true)
}
#[derive(Default)]
pub struct Library {
    good: BTreeMap<(String, bool), Arc<Asset>>,
}
impl Library {
    /// Bad/missing reloads retain the last-good asset for that exact identity.
    pub fn selected(
        &mut self,
        catalog: &Catalog,
        id: &str,
        dark: bool,
    ) -> (Option<Arc<Asset>>, Vec<String>) {
        if id
            == if dark {
                "builtin:dark"
            } else {
                "builtin:light"
            }
        {
            return (None, vec![]);
        }
        let key = (id.to_owned(), dark);
        let result = catalog
            .entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("Code theme {id} is not installed; import it again"))
            .and_then(|entry| {
                if entry.mode.is_some_and(|m| m.dark() != dark) {
                    return Err(format!(
                        "{} is not a {} theme",
                        entry.name,
                        if dark { "dark" } else { "light" }
                    ));
                }
                if let Some(old) = self.good.get(&key)
                    && old.source == entry.source
                    && unchanged(old)
                {
                    return Ok((**old).clone());
                }
                vscode::load(&entry.source, dark)
            })
            .and_then(|asset| {
                if asset.mode.is_some_and(|m| m.dark() != dark) {
                    return Err(format!(
                        "{} changed appearance; choose it for the other mode",
                        asset.name
                    ));
                }
                Ok(asset)
            });
        match result {
            Ok(asset) => {
                let asset = if let Some(old) = self
                    .good
                    .get(&key)
                    .filter(|old| old.identity == asset.identity)
                {
                    old.clone()
                } else {
                    let asset = Arc::new(asset);
                    self.good.insert(key.clone(), asset.clone());
                    asset
                };
                let diagnostics = asset.diagnostics.clone();
                (Some(asset), diagnostics)
            }
            Err(error) => (
                self.good.get(&key).cloned(),
                vec![format!(
                    "{error}. Keeping the last valid theme, or built-in colors if none loaded."
                )],
            ),
        }
    }
}

fn unchanged(asset: &Asset) -> bool {
    let mut digest = Sha256::new();
    for path in &asset.dependencies {
        let Ok(bytes) = read(path, MAX_FILE_BYTES) else {
            return false;
        };
        digest.update(path.as_os_str().as_encoded_bytes());
        digest.update([0]);
        digest.update(bytes);
        digest.update([0]);
    }
    format!("{:x}", digest.finalize()) == asset.identity
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        path
    }
    #[test]
    fn inherited_styles_specificity_reset_and_normal_fallback() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "base.json",
            r##"{
          "type":"dark", "colors":{"editor.background":"#102030","editor.foreground":"#aabbcc","editor.selectionBackground":"#ff000080"},
          "tokenColors":[
            {"scope":"comment", "settings":{"foreground":"#123456","fontStyle":"italic bold"}},
            {"scope":"constant.numeric", "settings":{"foreground":"#112233"}},
            {"scope":"constant", "settings":{"foreground":"#445566"}}
          ]} "##,
        );
        let path = write(
            d.path(),
            "child.json",
            r##"{ // child
          "include":"base.json", "name":"Test",
          "tokenColors":[
            {"scope":["comment"],"settings":{"fontStyle":""}},
            {"scope":"constant.numeric, entity.name.function","settings":{"foreground":"#abcdef"}},
            {"scope":"source.rust meta.function entity.name", "settings":{"foreground":"#f00"}}
          ], "semanticTokenColors":{},
        }"##,
        );
        let asset = load(&path).unwrap();
        assert_eq!(asset.editor["editor.background"], 0x102030ff);
        assert_eq!(asset.editor["editor.selection.background"], 0xff000080);
        assert_eq!(asset.syntax["number"].color, Some(0xabcdefff));
        assert_eq!(asset.syntax["function"].color, Some(0xabcdefff));
        assert_eq!(asset.syntax["variable"].color, Some(0xaabbccff));
        assert!(!asset.syntax["comment_doc"].italic);
        assert!(!asset.syntax["comment"].bold);
        assert_eq!(asset.diagnostics.len(), 2);
        assert_eq!(asset.dependencies.len(), 2);
    }
    #[test]
    fn rejects_cycles_escape_plist_bad_colors_and_limits() {
        let d = tempfile::tempdir().unwrap();
        for (text, error) in [
            (r#"{"include":"test.json"}"#, "cycle"),
            (r#"{"tokenColors":"x.tmTheme"}"#, ".tmTheme"),
            (
                r##"{"colors":{"editor.background":"red"}}"##,
                "Invalid theme color",
            ),
        ] {
            let path = write(d.path(), "test.json", text);
            assert!(load(&path).unwrap_err().contains(error));
        }
        fs::create_dir(d.path().join("nested")).unwrap();
        write(d.path(), "outside.json", "{}");
        let path = write(
            &d.path().join("nested"),
            "test.json",
            r#"{"include":"../outside.json"}"#,
        );
        assert!(load(&path).unwrap_err().contains("escapes"));
        let path = write(
            d.path(),
            "large.json",
            &" ".repeat(MAX_FILE_BYTES as usize + 1),
        );
        assert!(load(&path).unwrap_err().contains("byte"));
    }
    #[test]
    fn stable_catalog_ids_duplicates_and_last_good_dependency_reload() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("config.toml");
        let parent = write(
            d.path(),
            "parent.json",
            r##"{"colors":{"editor.background":"#123"}}"##,
        );
        let source = write(
            d.path(),
            "source.json",
            r#"{"name":"Same name","type":"dark","include":"parent.json"}"#,
        );
        let entry = import(&config, &source).unwrap();
        assert_eq!(entry, import(&config, &source).unwrap());
        let other = write(
            d.path(),
            "other.json",
            r#"{"name":"Same name","type":"dark"}"#,
        );
        let other = import(&config, &other).unwrap();
        assert_ne!(entry.id, other.id);
        assert_eq!(catalog(&config).entries.len(), 2);
        let mut library = Library::default();
        let (first, _) = library.selected(&catalog(&config), &entry.id, true);
        let first = first.unwrap();
        let (same, _) = library.selected(&catalog(&config), &entry.id, true);
        assert!(Arc::ptr_eq(&first, &same.unwrap()));
        fs::write(&parent, r##"{"colors":{"editor.background":"#345"}}"##).unwrap();
        let (next, _) = library.selected(&catalog(&config), &entry.id, true);
        let next = next.unwrap();
        assert_ne!(first.identity, next.identity);
        fs::write(parent, "{").unwrap();
        let (kept, warnings) = library.selected(&catalog(&config), &entry.id, true);
        assert!(Arc::ptr_eq(&next, &kept.unwrap()));
        assert!(!warnings.is_empty());
        let (missing, warnings) = library.selected(
            &catalog(&config),
            "vscode:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            true,
        );
        assert!(missing.is_none());
        assert!(!warnings.is_empty());
        let (builtin, _) = library.selected(&catalog(&config), "builtin:dark", true);
        assert!(builtin.is_none());
        // A reload changing mode must retain a valid dark palette, not apply light.
        fs::write(source, r#"{"type":"light"}"#).unwrap();
        assert!(
            library
                .selected(&catalog(&config), &entry.id, true)
                .0
                .is_some()
        );
    }
    #[test]
    fn untyped_themes_use_each_modes_fallback_without_leaking_cached_colors() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("config.toml");
        let source = write(d.path(), "source.json", r#"{"name":"Empty"}"#);
        let entry = import(&config, &source).unwrap();
        let mut library = Library::default();
        let catalog = catalog(&config);
        let (dark, _) = library.selected(&catalog, &entry.id, true);
        let (light, _) = library.selected(&catalog, &entry.id, false);
        assert_eq!(dark.unwrap().editor["editor.background"], 0x121212ff);
        assert_eq!(light.unwrap().editor["editor.background"], 0xf7f7f7ff);
    }
}
