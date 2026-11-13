//! Typed reloadable settings. The shipped TOML is the single source of defaults.
use crate::commands::CommandId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

/// A validated RGB setting, written as #RRGGBB in TOML.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(try_from = "String")]
pub struct RgbColor(u32);
impl RgbColor {
    pub fn value(self) -> u32 {
        self.0
    }
}
impl TryFrom<String> for RgbColor {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let hex = value
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6)
            .filter(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| format!("invalid RGB color {value:?}; expected #RRGGBB"))?;
        u32::from_str_radix(hex, 16)
            .map(Self)
            .map_err(|error| error.to_string())
    }
}

/// Every user-configurable color accepts one value or a light/dark pair.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum ColorSetting {
    Shared(RgbColor),
    Modes { light: RgbColor, dark: RgbColor },
}
impl ColorSetting {
    pub fn resolve(self, dark_mode: bool) -> RgbColor {
        match self {
            Self::Shared(color) => color,
            Self::Modes { light, dark } => {
                if dark_mode {
                    dark
                } else {
                    light
                }
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Light,
    Dark,
    System,
}
impl Appearance {
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::Light => false,
            Self::Dark => true,
            Self::System => system_dark,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReloadSettings {
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub editor: Option<SettingsEditor>,
}

/// A native application name, or an executable with literal arguments.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SettingsEditor {
    App(String),
    Command(Vec<String>),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LiveSettings {
    pub enabled: bool,
    pub poll_interval_ms: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub application: ApplicationSettings,
    pub navigation: NavigationSettings,
    pub welcome: WelcomeSettings,
    pub keyboard: KeyboardSettings,
    pub settings: ReloadSettings,
    pub live: LiveSettings,
    pub window: WindowSettings,
    pub ui: UiSettings,
    pub editor: EditorSettings,
    pub panels: PanelSettings,
    pub layout: LayoutSettings,
    pub history: HistorySettings,
    pub files: FileSettings,
    pub diff: DiffSettings,
    pub overview: OverviewSettings,
    pub evolution: EvolutionSettings,
    pub file_evolution: FileEvolutionSettings,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeyboardSettings {
    pub profile: String,
    pub additional_directories: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WelcomeSettings {
    pub recent_count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NavigationSettings {
    pub history_limit: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApplicationSettings {
    pub quit_on_last_window: bool,
    pub single_window_mode: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutSettings {
    pub remember_per_repository: bool,
    pub history_visible: bool,
    pub files_visible: bool,
    pub bottom_height: f32,
    pub min_bottom_height: f32,
    pub max_bottom_height: f32,
    pub placement: crate::panel_layout::Placement,
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
    pub appearance: Appearance,
    pub accent_color: ColorSetting,
    pub loading_delay_ms: u64,
    pub row_height: f32,
    pub panel_header_height: f32,
    pub toolbar_height: f32,
    pub footer_height: f32,
    pub footer_font_size: f32,
    pub footer_control_height: f32,
    pub comparison_bar_height: f32,
    pub font_size: f32,
    #[serde(default)]
    pub font_family: Option<String>,
    pub ligatures: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EditorSettings {
    pub font_size: f32,
    pub line_numbers: bool,
    pub tab_size: usize,
    pub ligatures: bool,
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
    pub font: FontSettings,
    pub default_width: f32,
    pub min_width: f32,
    pub max_width: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollbarClick {
    System,
    Jump,
    Page,
}
impl ScrollbarClick {
    pub fn resolve(self, system: Option<Self>) -> Self {
        match self {
            Self::System => system
                .filter(|mode| *mode != Self::System)
                .unwrap_or(Self::Jump),
            explicit => explicit,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiffPanelSettings {
    pub gutter_width: f32,
    pub shared_scrollbar_height: f32,
    pub shared_scrollbar_opacity: f32,
    pub shared_scrollbar_click: ScrollbarClick,
    pub change_map_opacity: f32,
    pub min_code_width: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OverviewSettings {
    pub context_lines: usize,
    pub expansion_lines: usize,
    pub message_max_height: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HistorySettings {
    pub stripe_opacity: f32,
    pub show_reference_labels: bool,
    pub reference_badge_width: f32,
    pub remotes: crate::history::RemoteVisibility,
    pub row_height: Option<f32>,
    pub lane_spacing: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileSettings {
    pub default_view: FileView,
    pub tree_scope: TreeScope,
    pub tree_row_height: Option<f32>,
    pub list_row_height: Option<f32>,
    pub indent_width: f32,
    pub indent_guides: bool,
    pub indent_guide_opacity: f32,
    pub indent_guide_offset: f32,
    pub file_icons: bool,
    pub compact_paths: bool,
    pub list_word_wrap: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FontSettings {
    pub size: f32,
    #[serde(default)]
    pub family: Option<String>,
    pub ligatures: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TreeScope {
    #[default]
    Changed,
    All,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileView {
    #[default]
    List,
    Tree,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffView {
    Inline,
    SideBySide,
    Fluid,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiffSettings {
    pub max_text_file_size_mb: u64,
    pub collapse_unchanged: bool,
    pub context_lines: usize,
    pub expansion_lines: usize,
    pub colors: DiffColorSettings,
    pub default_view: DiffView,
    pub before: PaneSettings,
    pub after: PaneSettings,
    pub inline: PaneSettings,
}

/// Shared diff palette, independent of application control/status colors.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DiffColorSettings {
    pub added: ColorSetting,
    pub removed: ColorSetting,
    pub modified: ColorSetting,
    pub added_background: ColorSetting,
    pub removed_background: ColorSetting,
    pub modified_background: ColorSetting,
    pub word_added: ColorSetting,
    pub word_removed: ColorSetting,
    pub conflict_base: ColorSetting,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PaneSettings {
    pub word_wrap: bool,
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).expect("the shipped settings schema is validated in tests")
    }
}

/// Validated application configuration plus resolved keyboard overrides.
/// Only Config is deserialized from config.toml.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    config: Config,
    pub keybindings: BTreeMap<CommandId, Vec<String>>,
    pub keymap: Vec<crate::keymap::Override>,
}
impl std::ops::Deref for Settings {
    type Target = Config;
    fn deref(&self) -> &Config {
        &self.config
    }
}
impl std::ops::DerefMut for Settings {
    fn deref_mut(&mut self) -> &mut Config {
        &mut self.config
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self::resolve(
            Config::default(),
            crate::keybinding_profiles::Profile::default(),
        )
        .expect("shipped configuration and bindings are valid")
    }
}
impl Settings {
    pub fn resolve(
        config: Config,
        profile: crate::keybinding_profiles::Profile,
    ) -> Result<Self, String> {
        config.validate()?;
        profile.validate()?;
        Ok(Self {
            config,
            keybindings: profile.keybindings,
            keymap: profile.keymap,
        })
    }
    pub fn parse(input: &str) -> Result<Self, String> {
        Self::resolve(
            Config::parse(input)?,
            crate::keybinding_profiles::Profile::default(),
        )
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        crate::settings_reload::SettingsSource::resolve(Some(path))?.read()
    }
    pub fn load_default() -> Result<Self, String> {
        crate::settings_reload::SettingsSource::resolve(None)?.read()
    }
    pub fn with_keybindings(
        self,
        profile: crate::keybinding_profiles::Profile,
    ) -> Result<Self, String> {
        Self::resolve(self.config, profile)
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

impl Config {
    pub fn footer_height(&self) -> f32 {
        self.ui
            .footer_height
            .max(self.ui.footer_control_height)
            .max(self.ui.footer_font_size * 1.5)
    }

    /// One row rhythm by default; larger panel fonts establish a readable minimum.
    pub fn row_height(&self, override_height: Option<f32>, font_size: f32) -> f32 {
        override_height
            .unwrap_or(self.ui.row_height)
            .max(font_size * 1.25)
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        let mut defaults: toml::Value = toml::from_str(DEFAULT_CONFIG)
            .expect("the shipped TOML defaults are validated in tests");
        let overrides: toml::Value =
            toml::from_str(input).map_err(|error| format!("Invalid TOML: {error}"))?;
        if overrides.get("keybindings").is_some() || overrides.get("keymap").is_some() {
            return Err("Put [keybindings] and [[keymap]] in the selected keybindings profile, not config.toml".into());
        }
        merge_defaults(&mut defaults, overrides);
        let settings: Self = defaults
            .try_into()
            .map_err(|error| format!("Invalid setting: {error}"))?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<(), String> {
        crate::keybinding_profiles::validate_reference(&self.keyboard.profile)?;
        for directory in &self.keyboard.additional_directories {
            if directory.as_os_str().is_empty() || directory.to_string_lossy().contains('\0') {
                return Err("keyboard.additional_directories must contain nonempty directory paths without NUL characters".into());
            }
        }
        fn bounded(name: &str, value: f32, min: f32, max: f32) -> Result<(), String> {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("{name} must be between {min} and {max}"));
            }
            Ok(())
        }
        if !(100..=60000).contains(&self.settings.poll_interval_ms) {
            return Err("settings.poll_interval_ms must be between 100 and 60000".into());
        }
        if let Some(editor) = &self.settings.editor {
            let values = match editor {
                SettingsEditor::App(app) => std::slice::from_ref(app),
                SettingsEditor::Command(command) => command.as_slice(),
            };
            if values.first().is_none_or(|value| value.trim().is_empty())
                || values.iter().any(|value| value.contains('\0'))
            {
                return Err("settings.editor must name an app or a nonempty command array, without NUL characters".into());
            }
        }
        bounded(
            "layout.min_bottom_height",
            self.layout.min_bottom_height,
            40.,
            2000.,
        )?;
        bounded(
            "layout.max_bottom_height",
            self.layout.max_bottom_height,
            self.layout.min_bottom_height,
            4000.,
        )?;
        bounded(
            "layout.bottom_height",
            self.layout.bottom_height,
            self.layout.min_bottom_height,
            self.layout.max_bottom_height,
        )?;
        if !(1..=50).contains(&self.welcome.recent_count) {
            return Err("welcome.recent_count must be between 1 and 50".into());
        }
        if !(1..=1000).contains(&self.navigation.history_limit) {
            return Err("navigation.history_limit must be between 1 and 1000".into());
        }
        crate::text_content::TextLimit::from_mib(self.diff.max_text_file_size_mb)
            .map_err(|error| format!("diff.max_text_file_size_mb: {error}"))?;
        bounded("window.width", self.window.width, 320., 10000.)?;
        bounded("window.height", self.window.height, 240., 10000.)?;
        if self.ui.loading_delay_ms > 5000 {
            return Err("ui.loading_delay_ms must be between 0 and 5000".into());
        }
        bounded(
            "ui.comparison_bar_height",
            self.ui.comparison_bar_height,
            24.,
            120.,
        )?;
        bounded("ui.font_size", self.ui.font_size, 8., 48.)?;
        bounded("editor.font_size", self.editor.font_size, 8., 48.)?;
        if !(1..=16).contains(&self.editor.tab_size) {
            return Err("editor.tab_size must be between 1 and 16".into());
        }
        for (name, family) in [
            ("ui.font_family", &self.ui.font_family),
            ("editor.font_family", &self.editor.font_family),
            (
                "panels.history.font.family",
                &self.panels.history.font.family,
            ),
            ("panels.files.font.family", &self.panels.files.font.family),
        ] {
            if family
                .as_ref()
                .is_some_and(|family| family.trim().is_empty())
            {
                return Err(format!("{name} must name a font, or be omitted"));
            }
        }
        for (name, panel) in [
            ("history", &self.panels.history),
            ("files", &self.panels.files),
        ] {
            bounded(
                &format!("panels.{name}.font.size"),
                panel.font.size,
                8.,
                48.,
            )?;
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
            "panels.diff.shared_scrollbar_height",
            self.panels.diff.shared_scrollbar_height,
            1.,
            1000.,
        )?;
        bounded(
            "panels.diff.shared_scrollbar_opacity",
            self.panels.diff.shared_scrollbar_opacity,
            0.,
            1.,
        )?;
        bounded(
            "panels.diff.change_map_opacity",
            self.panels.diff.change_map_opacity,
            0.,
            1.,
        )?;
        bounded(
            "panels.diff.min_code_width",
            self.panels.diff.min_code_width,
            32.,
            2000.,
        )?;
        bounded("ui.row_height", self.ui.row_height, 12., 160.)?;
        bounded(
            "ui.panel_header_height",
            self.ui.panel_header_height,
            24.,
            120.,
        )?;
        bounded("ui.toolbar_height", self.ui.toolbar_height, 28., 120.)?;
        bounded("ui.footer_height", self.ui.footer_height, 20., 120.)?;
        bounded("ui.footer_font_size", self.ui.footer_font_size, 8., 48.)?;
        bounded(
            "ui.footer_control_height",
            self.ui.footer_control_height,
            16.,
            80.,
        )?;
        for (name, value) in [
            ("history.row_height", self.history.row_height),
            ("files.tree_row_height", self.files.tree_row_height),
            ("files.list_row_height", self.files.list_row_height),
        ] {
            if let Some(value) = value {
                bounded(name, value, 12., 200.)?;
            }
        }
        bounded(
            "history.stripe_opacity",
            self.history.stripe_opacity,
            0.,
            0.2,
        )?;
        bounded(
            "history.reference_badge_width",
            self.history.reference_badge_width,
            40.,
            300.,
        )?;
        if self.diff.context_lines > 100 || !(1..=1000).contains(&self.diff.expansion_lines) {
            return Err(
                "diff context must be 0–100 lines and expansion must be 1–1000 lines".into(),
            );
        }
        if self.overview.context_lines > 100 || !(1..=1000).contains(&self.overview.expansion_lines)
        {
            return Err(
                "overview context must be 0–100 lines and expansion must be 1–1000 lines".into(),
            );
        }
        bounded(
            "overview.message_max_height",
            self.overview.message_max_height,
            80.,
            1200.,
        )?;
        bounded("history.lane_spacing", self.history.lane_spacing, 4., 48.)?;
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
        bounded(
            "files.indent_guide_opacity",
            self.files.indent_guide_opacity,
            0.,
            1.,
        )?;
        bounded(
            "files.indent_guide_offset",
            self.files.indent_guide_offset,
            -48.,
            48.,
        )?;
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
    fn standard_context_limits_and_defaults() {
        let defaults = Settings::parse("").unwrap();
        assert!(!defaults.diff.collapse_unchanged);
        assert_eq!(defaults.diff.context_lines, 3);
        assert_eq!(defaults.diff.expansion_lines, 20);
        assert!(
            Settings::parse(
                "[diff]\ncollapse_unchanged=true\ncontext_lines=0\nexpansion_lines=1000"
            )
            .is_ok()
        );
        for setting in [
            "context_lines=101",
            "context_lines=-1",
            "expansion_lines=0",
            "expansion_lines=1001",
        ] {
            assert!(
                Settings::parse(&format!("[diff]\n{setting}")).is_err(),
                "{setting}"
            );
        }
    }

    #[test]
    fn single_window_mode_is_opt_in_and_requires_a_boolean() {
        assert!(!Settings::parse("").unwrap().application.single_window_mode);
        assert!(
            Settings::parse("[application]\nsingle_window_mode=true")
                .unwrap()
                .application
                .single_window_mode
        );
        assert!(Settings::parse("[application]\nsingle_window_mode='reuse'").is_err());
    }

    #[test]
    fn text_limit_defaults_to_fifty_mib_and_requires_representable_integer_bytes() {
        assert_eq!(Settings::parse("").unwrap().diff.max_text_file_size_mb, 50);
        assert_eq!(
            Settings::parse("[diff]\nmax_text_file_size_mb=1")
                .unwrap()
                .diff
                .max_text_file_size_mb,
            1
        );
        for value in ["0", "-1", "0.5", "inf", "18446744073709551615"] {
            assert!(
                Settings::parse(&format!("[diff]\nmax_text_file_size_mb={value}")).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn welcome_recent_count_is_live_configurable_and_bounded() {
        assert_eq!(Settings::parse("").unwrap().welcome.recent_count, 5);
        assert_eq!(
            Settings::parse("[welcome]\nrecent_count=12")
                .unwrap()
                .welcome
                .recent_count,
            12
        );
        assert!(Settings::parse("[welcome]\nrecent_count=0").is_err());
        assert!(Settings::parse("[welcome]\nrecent_count=51").is_err());
    }
    #[test]
    fn navigation_history_is_bounded_and_configurable() {
        assert_eq!(Settings::parse("").unwrap().navigation.history_limit, 100);
        assert_eq!(
            Settings::parse("[navigation]\nhistory_limit=1")
                .unwrap()
                .navigation
                .history_limit,
            1
        );
        assert!(Settings::parse("[navigation]\nhistory_limit=0").is_err());
        assert!(Settings::parse("[navigation]\nhistory_limit=1001").is_err());
    }

    #[test]
    fn references_have_independent_defaults_and_typed_remote_overrides() {
        let settings = Settings::parse("[history]\nshow_reference_labels=false\n[history.remotes]\ndefault_visible=false\n[history.remotes.overrides]\nupstream=true").unwrap();
        assert!(!settings.history.show_reference_labels);
        assert!(settings.history.remotes.includes("upstream"));
        assert!(!settings.history.remotes.includes("new-remote"));
        assert!(Settings::parse("[history]\nreference_badge_width=0").is_err());
        assert!(Settings::parse("[history.remotes.overrides]\norigin='hidden'").is_err());
    }

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
    fn tree_scope_defaults_and_overrides_are_independent_of_list_mode() {
        assert_eq!(
            Settings::parse("").unwrap().files.tree_scope,
            super::TreeScope::Changed
        );
        let settings = Settings::parse("[files]\ntree_scope='all'\n").unwrap();
        assert_eq!(settings.files.tree_scope, super::TreeScope::All);
        assert_eq!(settings.files.default_view, super::FileView::List);
        assert!(Settings::parse("[files]\ntree_scope='unknown'").is_err());
    }

    #[test]
    fn typography_and_keymap_overrides_inherit_without_losing_other_commands() {
        use crate::commands::CommandId;
        let settings = Settings::parse("[editor]\ntab_size=8\nligatures=false\nline_numbers=false\n[panels.history.font]\nfamily='Menlo'\nsize=18\n[files]\ndefault_view='tree'").unwrap().with_keybindings(crate::keybinding_profiles::Profile::parse("[keybindings]\nfind_file=['secondary-k secondary-p']\ntoggle_history=[]").unwrap()).unwrap();
        assert_eq!(settings.editor.tab_size, 8);
        assert!(!settings.editor.ligatures);
        assert_eq!(
            settings.panels.history.font.family.as_deref(),
            Some("Menlo")
        );
        assert_eq!(settings.panels.files.font.size, 14.);
        assert!(settings.keybindings[&CommandId::ToggleHistory].is_empty());
        assert_eq!(
            settings.keybindings[&CommandId::FindFile],
            ["secondary-k secondary-p"]
        );
        assert_eq!(
            settings.keybindings[&CommandId::OpenSettings],
            ["secondary-,"]
        );
        assert_eq!(settings.keybindings.len(), CommandId::ALL.len());
    }

    #[test]
    fn invalid_fonts_tabs_and_shortcuts_leave_actionable_errors() {
        for input in [
            "[editor]\ntab_size=0",
            "[editor]\ntab_size=17",
            "[panels.history.font]\nsize=nan",
            "[panels.files.font]\nfamily=' '",
            "[ui]\nfont_family=''",
            "[files]\ndefault_view='invalid'",
            "[keybindings]\nfind_file=['mystery-p']",
            "[keybindings]\nfind_file=['']",
            "[keybindings]\nfind_file=['secondary-b']",
            "[keybindings]\nunknown_command=['secondary-u']",
        ] {
            assert!(Settings::parse(input).is_err(), "accepted {input}");
        }
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
            "[files]\nindent_guide_opacity=1.1",
            "[files]\nindent_guide_offset=49",
            "[panels.diff]\nshared_scrollbar_height=0",
            "[panels.diff]\nshared_scrollbar_opacity=1.1",
            "[panels.diff]\nshared_scrollbar_click='invalid'",
            "[panels.diff]\nchange_map_opacity=-0.1",
            "[panels.diff]\nchange_map_opacity=1.1",
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

#[cfg(test)]
mod editor_tests {
    use super::{Settings, SettingsEditor};
    #[test]
    fn settings_editor_supports_app_names_and_literal_command_arguments() {
        assert_eq!(Settings::parse("").unwrap().settings.editor, None);
        assert_eq!(
            Settings::parse("[settings]\neditor='Example Editor'")
                .unwrap()
                .settings
                .editor,
            Some(SettingsEditor::App("Example Editor".into()))
        );
        assert_eq!(
            Settings::parse("[settings]\neditor=['code','--reuse-window']")
                .unwrap()
                .settings
                .editor,
            Some(SettingsEditor::Command(vec![
                "code".into(),
                "--reuse-window".into()
            ]))
        );
        for value in [
            "''",
            "'   '",
            "[]",
            "['']",
            "['   ', '--wait']",
            "42",
            "{app='Zed',typo=true}",
        ] {
            assert!(
                Settings::parse(&format!("[settings]\neditor={value}")).is_err(),
                "{value}"
            );
        }
    }
}

#[cfg(test)]
mod row_tests {
    use super::Settings;
    #[test]
    fn rows_inherit_common_density_but_allow_independent_overrides_and_large_fonts() {
        let settings = Settings::parse("[ui]\nrow_height=24\n[files]\nlist_row_height=32").unwrap();
        assert_eq!(settings.row_height(settings.history.row_height, 14.), 24.);
        assert_eq!(
            settings.row_height(settings.files.tree_row_height, 14.),
            24.
        );
        assert_eq!(
            settings.row_height(settings.files.list_row_height, 14.),
            32.
        );
        assert_eq!(settings.row_height(None, 32.), 40.);
        for input in [
            "[ui]\nrow_height=0",
            "[files]\nlist_row_height=nan",
            "[history]\nrow_height=-1",
        ] {
            assert!(Settings::parse(input).is_err());
        }
    }
}

#[cfg(test)]
mod application_tests {
    use super::Settings;
    #[test]
    fn closing_the_last_window_quits_by_default_and_can_be_disabled() {
        let defaults = Settings::parse("").unwrap();
        assert!(defaults.application.quit_on_last_window);
        let settings = Settings::parse("[application]\nquit_on_last_window=false").unwrap();
        assert!(!settings.application.quit_on_last_window);
        assert_eq!(settings.window, defaults.window);
        assert!(Settings::parse("[application]\nquit_on_last_window=1").is_err());
        assert!(Settings::parse("[application]\nquit_when_last_window_closes=false").is_err());
    }
}

#[cfg(test)]
mod advanced_keymap_tests {
    use super::Settings;
    #[test]
    fn widget_overrides_are_typed_replaceable_and_validate_before_installation() {
        assert!(Settings::parse("").unwrap().keymap.is_empty());
        let settings = crate::keybinding_profiles::Profile::parse("[[keymap]]\naction='input::Copy'\nkeys=['secondary-insert']\ncontext='Input'\nfrom='secondary-c'").unwrap();
        assert_eq!(settings.keymap.len(), 1);
        assert_eq!(settings.keymap[0].from.as_deref(), Some("secondary-c"));
        for invalid in [
            "[[keymap]]\naction='input::Copy'\nkeys=['']",
            "[[keymap]]\naction='input::Copy'\nkeys=[]\ncontext='Input &&'",
            "[[keymap]]\naction='input::Copy'\nkeys=[]\nunknown=true",
            "[[keymap]]\naction='jaywalk::ExecuteCommand'\nkeys=[]",
        ] {
            assert!(
                crate::keybinding_profiles::Profile::parse(invalid).is_err(),
                "{invalid}"
            );
        }
    }
}

#[cfg(test)]
mod resolved_config_tests {
    use super::{Config, Settings};
    use crate::keybinding_profiles::Profile;
    #[test]
    fn resolution_rejects_invalid_raw_config_and_preserves_independent_profile_overrides() {
        let mut config = Config::default();
        config.editor.tab_size = 0;
        assert!(Settings::resolve(config, Profile::default()).is_err());
        let config = Config::parse("[editor]\nfont_family='Example'").unwrap();
        let profile = Profile::parse("[keybindings]\nfind_file=[]").unwrap();
        let settings = Settings::resolve(config, profile).unwrap();
        assert_eq!(settings.editor.font_family.as_deref(), Some("Example"));
        assert!(settings.keybindings[&crate::commands::CommandId::FindFile].is_empty());
    }
}

#[cfg(test)]
mod color_tests {
    use super::Settings;
    #[test]
    fn application_accent_accepts_rgb_hex_and_rejects_invalid_config() {
        assert_eq!(
            Settings::parse("")
                .unwrap()
                .ui
                .accent_color
                .resolve(true)
                .value(),
            0x3794ff
        );
        for text in ["#A1b2C3", "#a1b2c3"] {
            let source = format!("[ui]\naccent_color = '{text}'");
            assert_eq!(
                Settings::parse(&source)
                    .unwrap()
                    .ui
                    .accent_color
                    .resolve(true)
                    .value(),
                0xa1b2c3
            );
        }
        for text in ["red", "3794ff", "#fff", "#3794ffff", "#zzzzzz"] {
            let source = format!("[ui]\naccent_color = '{text}'");
            assert!(Settings::parse(&source).is_err(), "accepted {text}");
        }
    }
}

#[cfg(test)]
mod appearance_tests {
    use super::{Appearance, Settings};
    #[test]
    fn appearance_resolves_system_or_forced_modes() {
        for system in [false, true] {
            assert_eq!(Appearance::System.is_dark(system), system);
            assert!(!Appearance::Light.is_dark(system));
            assert!(Appearance::Dark.is_dark(system));
        }
        assert_eq!(
            Settings::parse("").unwrap().ui.appearance,
            Appearance::System
        );
        assert!(Settings::parse("[ui]\nappearance='auto'").is_err());
    }
    #[test]
    fn colors_support_shared_values_pairs_and_partial_default_overrides() {
        let defaults = Settings::parse("").unwrap();
        assert_eq!(defaults.ui.accent_color.resolve(false).value(), 0x0066b8);
        assert_eq!(defaults.ui.accent_color.resolve(true).value(), 0x3794ff);
        let shared = Settings::parse("[ui]\naccent_color='#abcdef'").unwrap();
        assert_eq!(
            shared.ui.accent_color.resolve(false),
            shared.ui.accent_color.resolve(true)
        );
        let pair = Settings::parse("[ui]\naccent_color={light='#123456',dark='#654321'}").unwrap();
        assert_eq!(pair.ui.accent_color.resolve(false).value(), 0x123456);
        assert_eq!(pair.ui.accent_color.resolve(true).value(), 0x654321);
        let partial = Settings::parse("[ui.accent_color]\ndark='#ff9f43'").unwrap();
        assert_eq!(partial.ui.accent_color.resolve(false).value(), 0x0066b8);
        assert_eq!(partial.ui.accent_color.resolve(true).value(), 0xff9f43);
        for source in [
            "[ui.accent_color]\nlight='#zzzzzz'",
            "[ui.accent_color]\nligth='#123456'",
        ] {
            assert!(Settings::parse(source).is_err());
        }
    }
}
