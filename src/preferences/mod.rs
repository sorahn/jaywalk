//! Validated settings documents, field-level transactions and searchable metadata.
pub mod catalogue;
use crate::{
    settings::{Config, DEFAULT_CONFIG, Settings},
    settings_reload::{self, SettingsSource},
};
use catalogue::{Kind, Setting};
use std::{
    io::Write,
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
};
static REVISION: AtomicU64 = AtomicU64::new(0);
static DEFAULTS: LazyLock<toml::Value> =
    LazyLock::new(|| toml::from_str(DEFAULT_CONFIG).expect("validated shipped defaults"));
pub(crate) fn record_commit() {
    REVISION.fetch_add(1, Ordering::AcqRel);
}
pub(crate) fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub settings: Settings,
    pub text: String,
    pub revision: u64,
    explicit: toml::Value,
    effective: toml::Value,
}
impl Snapshot {
    pub(crate) fn new(settings: Settings, text: String) -> Result<Self, String> {
        let _ = catalogue::SETTINGS.len();
        let mut explicit: toml::Value =
            toml::from_str(&text).map_err(|e| format!("Invalid settings document: {e}"))?;
        if let Some(table) = explicit.as_table_mut() {
            table.remove("setup");
        }
        let mut effective = DEFAULTS.clone();
        if explicit
            .get("ui")
            .and_then(|ui| ui.get("accent_color"))
            .is_some_and(toml::Value::is_table)
        {
            effective["ui"]["accent_color"] = effective["ui"]["accent_color_fallback"].clone();
        }
        crate::settings::merge_defaults(&mut effective, explicit.clone());
        Ok(Self {
            settings,
            text,
            revision: revision(),
            explicit,
            effective,
        })
    }
    pub fn value(&self, key: &str) -> Option<&toml::Value> {
        lookup(&self.effective, key)
    }
    pub fn explicit(&self, key: &str) -> Option<&toml::Value> {
        lookup(&self.explicit, key)
    }
    pub fn default_value(key: &str) -> Option<&'static toml::Value> {
        lookup(&DEFAULTS, key)
    }
    pub fn modified(&self, setting: &Setting) -> bool {
        if setting.key == "diff.colors" {
            return catalogue::SETTINGS.iter().any(|child| {
                child.advanced && child.key.starts_with("diff.colors.") && self.modified(child)
            });
        }
        if setting.key == "theme.overrides"
            && catalogue::setting("ui.accent_color_fallback")
                .is_some_and(|fallback| self.modified(fallback))
        {
            return true;
        }
        if setting.key == "accounts" {
            return false;
        }
        !equivalent(
            setting,
            self.value(&setting.key),
            Self::default_value(&setting.key),
        )
    }
    pub fn matching(
        &self,
        query: &str,
        category: Option<catalogue::Category>,
        changed: bool,
    ) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let mut found: Vec<_> = catalogue::SETTINGS
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !s.advanced
                    && s.key != "accounts"
                    && category.is_none_or(|category| category == s.category)
                    && (!changed || self.modified(s))
            })
            .filter_map(|(index, s)| s.rank(&query).map(|rank| (rank, index)))
            .collect();
        found.sort_by_key(|(rank, index)| (*rank, *index));
        found.into_iter().map(|(_, index)| index).collect()
    }
}
fn lookup<'a>(value: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let exact = |key: &str| {
        key.split('.')
            .try_fold(value, |value, part| value.get(part))
    };
    exact(key).or_else(|| {
        matches!(
            key,
            "theme.shell_auto.seed.light" | "theme.shell_auto.seed.dark"
        )
        .then(|| exact("theme.shell_auto.seed").filter(|value| value.is_str()))
        .flatten()
    })
}

fn equivalent(setting: &Setting, a: Option<&toml::Value>, b: Option<&toml::Value>) -> bool {
    if a == b {
        return true;
    }
    if let (Some(a), Some(b)) = (a, b) {
        if let (Some(a), Some(b)) = (
            a.as_float().or_else(|| a.as_integer().map(|n| n as f64)),
            b.as_float().or_else(|| b.as_integer().map(|n| n as f64)),
        ) {
            return a == b;
        }
        if setting.kind == Kind::Color {
            let parse = |v: &toml::Value| v.clone().try_into::<crate::settings::ColorSetting>();
            if let (Ok(a), Ok(b)) = (parse(a), parse(b)) {
                return a.resolve(false) == b.resolve(false) && a.resolve(true) == b.resolve(true);
            }
        }
    }
    false
}
pub fn literal(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => format!("{s:?}"),
        toml::Value::Array(v) => {
            format!("[{}]", v.iter().map(literal).collect::<Vec<_>>().join(", "))
        }
        toml::Value::Table(v) => format!(
            "{{ {} }}",
            v.iter()
                .map(|(k, v)| format!("{k:?} = {}", literal(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => value.to_string(),
    }
}
pub fn display(value: Option<&toml::Value>) -> String {
    match value {
        Some(toml::Value::String(s)) => s.clone(),
        Some(v) => literal(v),
        None => "Automatic / inherited".into(),
    }
}
#[derive(Clone)]
pub enum Expect {
    Any,
    Explicit(Option<toml::Value>),
}
#[derive(Clone)]
pub struct Patch {
    pub key: String,
    pub value: Option<toml::Value>,
    pub expected: Expect,
}
impl Patch {
    pub fn set(key: impl Into<String>, value: toml::Value) -> Self {
        Self {
            key: key.into(),
            value: Some(value),
            expected: Expect::Any,
        }
    }
}
fn table_at<'a>(doc: &'a mut toml_edit::DocumentMut, parts: &[&str]) -> &'a mut toml_edit::Table {
    let mut table = doc.as_table_mut();
    for part in parts {
        table = settings_reload::standard_table(table, part);
    }
    table
}
fn expand_shared_seed(doc: &mut toml_edit::DocumentMut, key: &str) {
    if !matches!(
        key,
        "theme.shell_auto.seed.light" | "theme.shell_auto.seed.dark"
    ) {
        return;
    }
    let table = table_at(doc, &["theme", "shell_auto"]);
    if let Some(previous) = table.get("seed").and_then(toml_edit::Item::as_value)
        && let Some(color) = previous.as_str()
    {
        let mut pair = toml_edit::InlineTable::new();
        pair.insert("light", color.into());
        pair.insert("dark", color.into());
        let mut replacement = toml_edit::Value::InlineTable(pair);
        *replacement.decor_mut() = previous.decor().clone();
        table["seed"] = toml_edit::Item::Value(replacement);
    }
}
fn set(doc: &mut toml_edit::DocumentMut, key: &str, value: &toml::Value) -> Result<(), String> {
    expand_shared_seed(doc, key);
    let mut parts: Vec<_> = key.split('.').collect();
    let leaf = parts.pop().ok_or("Empty setting key")?;
    let table = table_at(doc, &parts);
    let mut value = literal(value)
        .parse::<toml_edit::Value>()
        .map_err(|e| format!("Invalid setting value: {e}"))?;
    if let Some(previous) = table.get(leaf).and_then(toml_edit::Item::as_value) {
        *value.decor_mut() = previous.decor().clone();
    }
    table[leaf] = toml_edit::Item::Value(value);
    Ok(())
}
fn remove(text: &str, key: &str) -> Result<String, String> {
    let doc = toml_edit::Document::parse(text).map_err(|e| e.to_string())?;
    let parts: Vec<_> = key.split('.').collect();
    let mut item = doc.as_item();
    for part in &parts {
        let Some(next) = item.get(part) else {
            return Ok(text.into());
        };
        item = next;
    }
    if item.is_table() || item.is_array_of_tables() {
        // Table spans cover their headers, not necessarily their assignments or
        // nested tables. Comment every owned item, preserving intervening notes.
        fn spans(item: &toml_edit::Item, ranges: &mut Vec<std::ops::Range<usize>>) {
            if let Some(span) = item.span() {
                ranges.push(span);
            }
            if let Some(table) = item.as_table() {
                for (_, child) in table.iter() {
                    spans(child, ranges);
                }
            } else if let Some(tables) = item.as_array_of_tables() {
                for table in tables.iter() {
                    if let Some(span) = table.span() {
                        ranges.push(span);
                    }
                    for (_, child) in table.iter() {
                        spans(child, ranges);
                    }
                }
            }
        }
        let mut ranges = Vec::new();
        spans(item, &mut ranges);
        if ranges.is_empty() {
            return Err("Could not locate setting for reset".into());
        }
        let mut position = 0;
        let mut updated = String::new();
        for line in text.split_inclusive('\n') {
            let end = position + line.len();
            if ranges
                .iter()
                .any(|span| span.start < end && span.end > position)
            {
                updated.push_str("# ");
            }
            updated.push_str(line);
            position = end;
        }
        return Ok(updated);
    }
    let span = item.span().ok_or("Could not locate setting for reset")?;
    let start = text[..span.start].rfind('\n').map_or(0, |n| n + 1);
    let end = text[span.end..]
        .find('\n')
        .map_or(text.len(), |n| span.end + n);
    let mut block = text[start..end].to_owned();
    // Scalar examples become the current default; complex/multiline examples
    // stay intact as comments so none of their documentation is discarded.
    if !block.contains('\n')
        && let Some(default) = Snapshot::default_value(key)
    {
        let relative_start = span.start - start;
        let relative_end = span.end - start;
        block.replace_range(relative_start..relative_end, &literal(default));
    }
    let commented = block
        .lines()
        .map(|line| format!("# {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut updated = text.to_owned();
    updated.replace_range(start..end, &commented);
    Ok(updated)
}
fn edit(text: &str, patch: &Patch) -> Result<String, String> {
    let setting = catalogue::setting(&patch.key).ok_or("Unknown setting")?;
    if patch.key == "accounts" {
        return Err("Manage connected accounts from Accounts".into());
    }
    let explicit: toml::Value = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut text = text.to_owned();
    if let Some(value) = &patch.value {
        if setting.kind == Kind::Complex {
            return Err("Edit this setting in TOML".into());
        }
        if let Some((section, leaf)) = patch.key.rsplit_once('.')
            && lookup(&explicit, &patch.key).is_none()
        {
            text = settings_reload::activate_reference(&text, section, &[leaf]);
        }
        let mut doc: toml_edit::DocumentMut = text
            .parse()
            .map_err(|e| format!("Could not edit settings: {e}"))?;
        set(&mut doc, &patch.key, value)?;
        // Revoking a prerequisite revokes its dependants in the same transaction.
        if value.as_bool() == Some(false) {
            if patch.key == "updates.automatic_checks" {
                set(&mut doc, "updates.automatic_downloads", &false.into())?;
            }
            if matches!(
                patch.key.as_str(),
                "updates.automatic_checks" | "updates.automatic_downloads"
            ) {
                set(&mut doc, "updates.automatic_installation", &false.into())?;
            }
        }
        text = doc.to_string();
    } else {
        // Normalize inline sections first, so removing a single child cannot
        // comment out an unrelated sibling in an inline table.
        let mut doc: toml_edit::DocumentMut = text
            .parse()
            .map_err(|e| format!("Could not edit settings: {e}"))?;
        expand_shared_seed(&mut doc, &patch.key);
        let mut parts: Vec<_> = patch.key.split('.').collect();
        parts.pop();
        table_at(&mut doc, &parts);
        text = remove(&doc.to_string(), &patch.key)?;
        if patch.key == "updates.automatic_checks" || patch.key == "updates.automatic_downloads" {
            let mut doc: toml_edit::DocumentMut = text
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| e.to_string())?;
            if patch.key == "updates.automatic_checks" {
                set(&mut doc, "updates.automatic_downloads", &false.into())?;
            }
            set(&mut doc, "updates.automatic_installation", &false.into())?;
            text = doc.to_string();
        }
    }
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|error| format!("Could not edit settings: {error}"))?;
    if let Some((key, legacy)) = doc.remove_entry("setup") {
        let decoration = legacy
            .as_table()
            .map(toml_edit::Table::decor)
            .or_else(|| legacy.as_value().map(toml_edit::Value::decor));
        let mut comments = String::new();
        for raw in [
            key.leaf_decor().prefix(),
            decoration.and_then(|decor| decor.prefix()),
            decoration.and_then(|decor| decor.suffix()),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(text) = raw.as_str()
                && text.contains('#')
            {
                comments.push_str(text);
                comments.push('\n');
            }
        }
        if !comments.is_empty() {
            comments.push_str(doc.trailing().as_str().unwrap_or_default());
            doc.set_trailing(comments);
        }
    }
    text = doc.to_string();
    Ok(text)
}

/// Build a validated draft without touching the filesystem or publishing settings.
impl Snapshot {
    pub fn preview(&self, patches: &[Patch]) -> Result<Self, String> {
        let mut text = self.text.clone();
        for patch in patches {
            text = edit(&text, patch)?;
        }
        let settings = Settings::resolve(
            Config::parse(&text)?,
            crate::keybinding_profiles::Profile {
                keybindings: self.settings.keybindings.clone(),
                keymap: self.settings.keymap.clone(),
            },
        )?;
        let mut snapshot = Self::new(settings, text)?;
        snapshot.revision = self.revision;
        Ok(snapshot)
    }
}

/// Apply one intent to the latest valid file, never a window's cached settings.
pub fn patch(source: &SettingsSource, patch: Patch) -> Result<Snapshot, String> {
    patch_many(source, &[patch])
}

/// Validate all expectations and all edits before atomically replacing the file once.
pub fn patch_many(source: &SettingsSource, patches: &[Patch]) -> Result<Snapshot, String> {
    let _writer = settings_reload::CONFIG_WRITER
        .lock()
        .expect("config writer mutex poisoned");
    let before = source.read_snapshot_unlocked()?;
    for patch in patches {
        if let Expect::Explicit(expected) = &patch.expected
            && before.explicit(&patch.key) != expected.as_ref()
        {
            return Err(format!(
                "{} changed elsewhere. Discard or reload your draft before saving.",
                patch.key
            ));
        }
    }
    let mut text = before.text.clone();
    for patch in patches {
        if patch.value.is_some() || before.explicit(&patch.key).is_some() {
            text = edit(&text, patch)?;
        }
    }
    let config = Config::parse(&text)?;
    let profile_path = crate::keybinding_profiles::path(source.path(), &config.keyboard.profile)?;
    let profile = std::fs::read_to_string(&profile_path)
        .map_err(|e| format!("Could not read keybindings: {e}"))?;
    let settings = Settings::resolve(
        config,
        crate::keybinding_profiles::Profile::parse(&profile)?,
    )?;
    let target = std::fs::canonicalize(source.path()).map_err(|e| e.to_string())?;
    if text != before.text {
        if std::fs::metadata(&target)
            .map_err(|error| error.to_string())?
            .permissions()
            .readonly()
        {
            return Err(format!(
                "Settings file is read-only: {}",
                source.path().display()
            ));
        }
        let mut staged = tempfile::NamedTempFile::new_in(
            target.parent().ok_or("Settings have no parent directory")?,
        )
        .map_err(|e| e.to_string())?;
        staged
            .as_file()
            .set_permissions(
                std::fs::metadata(&target)
                    .map_err(|e| e.to_string())?
                    .permissions(),
            )
            .map_err(|e| e.to_string())?;
        staged
            .write_all(text.as_bytes())
            .and_then(|_| staged.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        if std::fs::read_to_string(&target).map_err(|e| e.to_string())? != before.text {
            return Err(
                "Settings changed while saving. Your external edit was preserved; retry.".into(),
            );
        }
        staged.persist(target).map_err(|e| e.to_string())?;
        record_commit();
    }
    Snapshot::new(settings, text)
}

#[cfg(test)]
mod tests;
