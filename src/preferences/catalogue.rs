//! Searchable UI metadata. Defaults stay in the existing settings schema.
use serde::Deserialize;
use std::sync::LazyLock;
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    General,
    Appearance,
    Layout,
    Diff,
    History,
    Repositories,
    Keyboard,
    Updates,
}
impl Category {
    pub const ALL: [Self; 8] = [
        Self::General,
        Self::Appearance,
        Self::Layout,
        Self::Diff,
        Self::History,
        Self::Repositories,
        Self::Keyboard,
        Self::Updates,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Appearance => "Appearance",
            Self::Layout => "Window and layout",
            Self::Diff => "Diff and overview",
            Self::History => "History and files",
            Self::Repositories => "Repositories",
            Self::Keyboard => "Navigation",
            Self::Updates => "Updates",
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Boolean,
    Number,
    Choice,
    CodeTheme,
    Text,
    Font,
    Color,
    Complex,
    Structured,
}
#[derive(Debug, Deserialize)]
pub struct Setting {
    pub key: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub category: Category,
    pub section: String,
    pub section_order: usize,
    pub kind: Kind,
    /// Discoverable through search/Changed, with Edit TOML instead of a widget.
    #[serde(default)]
    pub toml_only: bool,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub choice_labels: std::collections::BTreeMap<String, String>,
    /// Valid TOML literals for inherited optional values; never active defaults.
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub advanced: bool,
    #[serde(default)]
    pub group: Option<super::groups::Group>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub minimum: Option<NumberBound>,
    #[serde(default)]
    pub maximum: Option<NumberBound>,
    #[serde(skip)]
    search: String,
    #[serde(skip)]
    title_search: String,
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum NumberBound {
    Value(f64),
    Setting(String),
}
impl NumberBound {
    fn resolve(&self, snapshot: &super::Snapshot) -> Option<f64> {
        match self {
            Self::Value(value) => Some(*value),
            Self::Setting(key) => snapshot.value(key).and_then(number),
        }
    }
}
fn number(value: &toml::Value) -> Option<f64> {
    value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))
}
#[derive(Deserialize)]
struct Catalogue {
    setting: Vec<Setting>,
}
pub static SETTINGS: LazyLock<Vec<Setting>> = LazyLock::new(|| {
    let mut rows: Catalogue = toml::from_str(include_str!("../../config/settings-ui.toml"))
        .expect("shipped settings catalogue is validated in tests");
    let aliases = rows
        .setting
        .iter()
        .filter(|row| row.advanced)
        .map(|row| {
            (
                if row.key.starts_with("diff.colors.") {
                    "diff.colors"
                } else {
                    "theme.overrides"
                },
                row.key.clone(),
            )
        })
        .collect::<Vec<_>>();
    for row in &mut rows.setting {
        row.aliases.extend(
            aliases
                .iter()
                .filter(|(group, _)| *group == row.key)
                .map(|(_, key)| key.clone()),
        );
        row.title_search = row.title.to_lowercase();
        row.search = format!(
            "{} {} {} {} {} {} {} {}",
            row.key.replace('_', " "),
            row.title,
            row.description,
            row.summary.as_deref().unwrap_or(""),
            row.category.label(),
            row.section,
            row.group.map_or("", |group| group.label()),
            row.aliases.join(" ")
        )
        .to_lowercase();
    }
    rows.setting
});
pub fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|setting| setting.key == key)
}
impl Setting {
    pub fn choice_label(&self, value: &str) -> String {
        self.choice_labels
            .get(value)
            .cloned()
            .unwrap_or_else(|| value.replace('_', " "))
    }

    pub fn stepped(
        &self,
        text: &str,
        increment: bool,
        snapshot: &super::Snapshot,
    ) -> Result<toml::Value, String> {
        let step = self.step.ok_or("This setting has no numeric step")?;
        let min = self
            .minimum
            .as_ref()
            .and_then(|bound| bound.resolve(snapshot));
        let max = self
            .maximum
            .as_ref()
            .and_then(|bound| bound.resolve(snapshot));
        let mut value = if text.trim().is_empty() {
            min.unwrap_or(0.)
        } else {
            let current = text
                .trim()
                .parse::<f64>()
                .map_err(|_| "Enter a number before stepping")?;
            current + if increment { step } else { -step }
        };
        if !value.is_finite() {
            return Err("Enter a finite number".into());
        }
        if let Some(min) = min {
            value = value.max(min);
        }
        if let Some(max) = max {
            value = value.min(max);
        }
        value = (value * 1e9).round() / 1e9;
        Ok(
            if value.fract() == 0. && value >= i64::MIN as f64 && value < i64::MAX as f64 {
                toml::Value::Integer(value as i64)
            } else {
                toml::Value::Float(value)
            },
        )
    }
    /// The caller normalizes the query once before ranking all candidates.
    pub fn rank(&self, query: &str) -> Option<u8> {
        if query.is_empty() {
            return Some(0);
        }
        if self.key == query || self.aliases.iter().any(|alias| alias == query) {
            return Some(0);
        }
        if query
            .split_whitespace()
            .all(|term| self.search.contains(term) || self.key.contains(term))
        {
            return Some(if self.title_search == query {
                1
            } else if self.key.starts_with(query) {
                2
            } else if self.title_search.contains(query) {
                3
            } else {
                4
            });
        }
        let subsequence = |text: &str, term: &str| {
            let mut chars = text.chars();
            term.chars().all(|wanted| chars.any(|ch| ch == wanted))
        };
        query
            .split_whitespace()
            .all(|term| subsequence(&self.key, term) || subsequence(&self.title_search, term))
            .then_some(5)
    }
    pub fn parse_input(&self, text: &str) -> Result<toml::Value, String> {
        match self.kind {
            Kind::Text | Kind::Font | Kind::CodeTheme => Ok(toml::Value::String(text.into())),
            Kind::Color if !text.trim().starts_with('{') => {
                Ok(toml::Value::String(text.trim().into()))
            }
            Kind::Complex => Err("Edit this setting in TOML".into()),
            _ => toml::from_str::<toml::Table>(&format!("value = {text}"))
                .map_err(|_| "Enter a valid value".into())
                .and_then(|mut table| table.remove("value").ok_or("Enter a value".into())),
        }
    }
}
