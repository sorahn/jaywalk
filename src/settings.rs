//! Typed reloadable settings. The shipped TOML is the single source of defaults.
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReloadSettings {
    pub poll_interval_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LiveSettings {
    pub enabled: bool,
    pub poll_interval_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub settings: ReloadSettings,
    pub live: LiveSettings,
    pub window: WindowSettings,
    pub ui: UiSettings,
    pub editor: EditorSettings,
    pub panels: PanelSettings,
    pub history: HistorySettings,
    pub files: FileSettings,
    pub diff: DiffSettings,
    pub evolution: EvolutionSettings,
    pub file_evolution: FileEvolutionSettings,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvolutionSettings {
    pub visible: bool,
    pub show_unchanged_rebases: bool,
    pub collapse_repeats: bool,
    pub default_height: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub min_history_height: f32,
    pub row_height: f32,
    pub limit: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileEvolutionSettings {
    pub visible: bool,
    pub default_height: f32,
    pub min_height: f32,
    pub max_height: f32,
    pub min_files_height: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WindowSettings {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UiSettings {
    pub font_size: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EditorSettings {
    pub font_size: f32,
    #[serde(default)]
    pub font_family: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PanelSettings {
    pub history: SidebarSettings,
    pub files: SidebarSettings,
    pub diff: DiffPanelSettings,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SidebarSettings {
    pub default_width: f32,
    pub min_width: f32,
    pub max_width: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiffPanelSettings {
    pub gutter_width: f32,
    pub min_code_width: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HistorySettings {
    pub row_height: f32,
    pub lane_spacing: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileSettings {
    pub tree_row_height: f32,
    pub indent_width: f32,
    pub compact_paths: bool,
    pub list_word_wrap: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffView {
    Inline,
    SideBySide,
    Fluid,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiffSettings {
    pub default_view: DiffView,
    pub before: PaneSettings,
    pub after: PaneSettings,
    pub inline: PaneSettings,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaneSettings {
    pub word_wrap: bool,
}

impl Default for Settings {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).expect("the shipped settings schema is validated in tests")
    }
}

fn merge_defaults(base: &mut toml::Value, overrides: toml::Value) {
    match (base, overrides) {
        (toml::Value::Table(base), toml::Value::Table(overrides)) => {
            for (key, value) in overrides {
                match base.get_mut(&key) {
                    Some(base) => merge_defaults(base, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, value) => *base = value,
    }
}

impl Settings {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut defaults: toml::Value = toml::from_str(DEFAULT_CONFIG)
            .expect("the shipped TOML defaults are validated in tests");
        let overrides = toml::from_str(input).map_err(|error| format!("Invalid TOML: {error}"))?;
        merge_defaults(&mut defaults, overrides);
        let settings: Self = defaults
            .try_into()
            .map_err(|error| format!("Invalid setting: {error}"))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        let input = std::fs::read_to_string(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        Self::parse(&input).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn load_default() -> Result<Self, String> {
        let path = default_path()?;
        match std::fs::read_to_string(&path) {
            Ok(input) => {
                Self::parse(&input).map_err(|error| format!("{}: {error}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::parse(""),
            Err(error) => Err(format!("Could not read {}: {error}", path.display())),
        }
    }

    fn validate(&self) -> Result<(), String> {
        fn bounded(name: &str, value: f32, min: f32, max: f32) -> Result<(), String> {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("{name} must be between {min} and {max}"));
            }
            Ok(())
        }
        if !(100..=60000).contains(&self.settings.poll_interval_ms) {
            return Err("settings.poll_interval_ms must be between 100 and 60000".into());
        }
        bounded("window.width", self.window.width, 320., 10000.)?;
        bounded("window.height", self.window.height, 240., 10000.)?;
        bounded("ui.font_size", self.ui.font_size, 8., 48.)?;
        bounded("editor.font_size", self.editor.font_size, 8., 48.)?;
        if self
            .editor
            .font_family
            .as_ref()
            .is_some_and(|family| family.trim().is_empty())
        {
            return Err("editor.font_family must name a font, or be omitted".into());
        }
        for (name, panel) in [
            ("history", &self.panels.history),
            ("files", &self.panels.files),
        ] {
            bounded(
                &format!("panels.{name}.min_width"),
                panel.min_width,
                32.,
                4000.,
            )?;
            bounded(
                &format!("panels.{name}.max_width"),
                panel.max_width,
                panel.min_width,
                8000.,
            )?;
            bounded(
                &format!("panels.{name}.default_width"),
                panel.default_width,
                panel.min_width,
                panel.max_width,
            )?;
        }
        bounded(
            "panels.diff.gutter_width",
            self.panels.diff.gutter_width,
            8.,
            240.,
        )?;
        bounded(
            "panels.diff.min_code_width",
            self.panels.diff.min_code_width,
            32.,
            2000.,
        )?;
        bounded("history.row_height", self.history.row_height, 12., 160.)?;
        bounded("history.lane_spacing", self.history.lane_spacing, 8., 48.)?;
        bounded(
            "files.tree_row_height",
            self.files.tree_row_height,
            12.,
            200.,
        )?;
        if !(250..=60000).contains(&self.live.poll_interval_ms) {
            return Err("live.poll_interval_ms must be between 250 and 60000".into());
        }
        let file = &self.file_evolution;
        bounded("file_evolution.min_height", file.min_height, 100., 2000.)?;
        bounded(
            "file_evolution.max_height",
            file.max_height,
            file.min_height,
            4000.,
        )?;
        bounded(
            "file_evolution.default_height",
            file.default_height,
            file.min_height,
            file.max_height,
        )?;
        bounded(
            "file_evolution.min_files_height",
            file.min_files_height,
            80.,
            2000.,
        )?;
        let evolution = &self.evolution;
        bounded("evolution.min_height", evolution.min_height, 100., 2000.)?;
        bounded(
            "evolution.max_height",
            evolution.max_height,
            evolution.min_height,
            4000.,
        )?;
        bounded(
            "evolution.default_height",
            evolution.default_height,
            evolution.min_height,
            evolution.max_height,
        )?;
        bounded(
            "evolution.min_history_height",
            evolution.min_history_height,
            80.,
            2000.,
        )?;
        bounded("evolution.row_height", evolution.row_height, 24., 160.)?;
        if !(1..=1000).contains(&evolution.limit) {
            return Err("evolution.limit must be between 1 and 1000".into());
        }
        bounded("files.indent_width", self.files.indent_width, 8., 48.)
    }
}

pub fn default_path() -> Result<PathBuf, String> {
    if let Some(directory) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(directory).join("jaywalk/config.toml"));
    }
    let home =
        std::env::var_os("HOME").ok_or("Set HOME, XDG_CONFIG_HOME, or supply --config PATH")?;
    Ok(PathBuf::from(home).join(".config/jaywalk/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::{DiffView, Settings};

    #[test]
    fn partial_nested_settings_inherit_shipped_defaults() {
        let settings =
            Settings::parse("[panels.history]\ndefault_width=320\n[diff.before]\nword_wrap=true")
                .unwrap();
        assert_eq!(settings.panels.history.default_width, 320.);
        assert_eq!(settings.panels.files.default_width, 260.);
        assert_eq!(settings.diff.default_view, DiffView::Fluid);
        assert!(settings.diff.before.word_wrap);
        assert!(!settings.diff.after.word_wrap);
    }

    #[test]
    fn typos_and_invalid_geometry_are_actionable_errors() {
        for input in [
            "[editor]\nfont_szie=14",
            "[live]\npoll_interval_ms=0",
            "[settings]\npoll_interval_ms=0",
            "[settings]\npoll_interval_ms=60001",
            "[editor]\nfont_size=nan",
            "[panels.files]\nmin_width=500\ndefault_width=200",
            "[diff]\ndefault_view='invalid'",
            "[panels.diff]\ngutter_width=-1",
            "[evolution]\nlimit=0",
            "[evolution]\nmin_height=500\ndefault_height=200",
            "[file_evolution]\nmin_height=500\ndefault_height=200",
            "[file_evolution]\nmin_files_height=0",
        ] {
            assert!(
                Settings::parse(input).is_err(),
                "accepted invalid settings: {input}"
            );
        }
        Settings::parse("").unwrap();
    }
}
