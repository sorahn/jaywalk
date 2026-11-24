//! Sparse legacy overrides captured before shipped defaults are merged.
//! A missing appearance remains inherited, even when the other is explicit.
use crate::settings::{ColorSetting, DiffColorSettings, RgbColor};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum ColorOverride {
    Shared(RgbColor),
    Modes {
        light: Option<RgbColor>,
        dark: Option<RgbColor>,
    },
}
impl ColorOverride {
    pub fn resolve(self, dark_mode: bool) -> Option<RgbColor> {
        match self {
            Self::Shared(value) => Some(value),
            Self::Modes { light, dark } => {
                if dark_mode {
                    dark
                } else {
                    light
                }
            }
        }
    }
    fn apply(self, fallback: ColorSetting) -> ColorSetting {
        match self {
            Self::Shared(value) => ColorSetting::Shared(value),
            Self::Modes {
                light: None,
                dark: None,
            } => fallback,
            Self::Modes { light, dark } => ColorSetting::Modes {
                light: light.unwrap_or_else(|| fallback.resolve(false)),
                dark: dark.unwrap_or_else(|| fallback.resolve(true)),
            },
        }
    }
}

/// Only values actually present in [diff.colors], not resolved defaults.
/// Explicitly setting today's default is still an override of a later theme.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DiffColorOverrides {
    pub added: Option<ColorOverride>,
    pub removed: Option<ColorOverride>,
    pub modified: Option<ColorOverride>,
    pub added_background: Option<ColorOverride>,
    pub removed_background: Option<ColorOverride>,
    pub modified_background: Option<ColorOverride>,
    pub word_added: Option<ColorOverride>,
    pub word_removed: Option<ColorOverride>,
    pub conflict_base: Option<ColorOverride>,
    pub whitespace_error: Option<ColorOverride>,
}
impl DiffColorOverrides {
    pub fn apply(self, fallback: DiffColorSettings) -> DiffColorSettings {
        DiffColorSettings {
            added: self
                .added
                .map_or(fallback.added, |value| value.apply(fallback.added)),
            removed: self
                .removed
                .map_or(fallback.removed, |value| value.apply(fallback.removed)),
            modified: self
                .modified
                .map_or(fallback.modified, |value| value.apply(fallback.modified)),
            added_background: self
                .added_background
                .map_or(fallback.added_background, |value| {
                    value.apply(fallback.added_background)
                }),
            removed_background: self
                .removed_background
                .map_or(fallback.removed_background, |value| {
                    value.apply(fallback.removed_background)
                }),
            modified_background: self
                .modified_background
                .map_or(fallback.modified_background, |value| {
                    value.apply(fallback.modified_background)
                }),
            word_added: self.word_added.map_or(fallback.word_added, |value| {
                value.apply(fallback.word_added)
            }),
            word_removed: self.word_removed.map_or(fallback.word_removed, |value| {
                value.apply(fallback.word_removed)
            }),
            conflict_base: self.conflict_base.map_or(fallback.conflict_base, |value| {
                value.apply(fallback.conflict_base)
            }),
            whitespace_error: self
                .whitespace_error
                .map_or(fallback.whitespace_error, |value| {
                    value.apply(fallback.whitespace_error)
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Config;
    fn color(hex: &str) -> RgbColor {
        RgbColor::try_from(hex.to_owned()).unwrap()
    }

    #[test]
    fn omitted_and_empty_legacy_colors_leave_a_different_palette_untouched() {
        let mut blue_orange = Config::default().diff.colors;
        blue_orange.added = ColorSetting::Shared(color("#0088ff"));
        blue_orange.removed = ColorSetting::Shared(color("#ff8800"));
        for input in ["", "[diff.colors]", "[diff.colors.added]"] {
            let config = Config::parse(input).unwrap();
            assert_eq!(
                config.diff_color_overrides().apply(blue_orange),
                blue_orange
            );
        }
    }
    #[test]
    fn partial_appearance_overrides_do_not_freeze_the_other_appearance() {
        let config =
            Config::parse("[diff.colors]\nadded={light='#abcdef'}\nword_added='#123456'").unwrap();
        let mut base = Config::default().diff.colors;
        base.added = ColorSetting::Shared(color("#0088ff"));
        let overrides = config.diff_color_overrides();
        assert_eq!(overrides.added.unwrap().resolve(true), None);
        assert_eq!(
            overrides.added.unwrap().resolve(false),
            Some(color("#abcdef"))
        );
        let resolved = overrides.apply(base);
        assert_eq!(resolved.added.resolve(true), color("#0088ff"));
        assert_eq!(resolved.added.resolve(false), color("#abcdef"));
        for dark in [true, false] {
            assert_eq!(resolved.word_added.resolve(dark), color("#123456"));
        }
        assert_eq!(resolved.removed, base.removed);
    }
    #[test]
    fn explicitly_setting_the_current_default_remains_an_override() {
        let default = Config::default().diff.colors.added.resolve(true);
        let config =
            Config::parse(&format!("[diff.colors]\nadded='#{:06x}'", default.value())).unwrap();
        assert_eq!(
            config.diff_color_overrides().added,
            Some(ColorOverride::Shared(default))
        );
        let mut base = Config::default().diff.colors;
        base.added = ColorSetting::Shared(color("#0088ff"));
        assert_eq!(
            config.diff_color_overrides().apply(base).added,
            ColorSetting::Shared(default)
        );
        assert!(Config::parse("diff_color_overrides={}").is_err());
    }
}
