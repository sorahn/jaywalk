//! viewer-contrast-v1, ported from the approved viewer-anchored shell gallery.
use super::*;

/// Ungenerated code-theme inputs. Backgrounds are opaque, composited colors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CodeColors {
    pub background: Color,
    pub foreground: Option<Color>,
}
impl CodeColors {
    pub(super) fn with_overrides(mut self, settings: &ThemeSettings, dark: bool) -> Self {
        if let Some(color) = settings
            .overrides
            .get(&Role::Surface(Surface::Viewer, Part::Background))
        {
            self.background = Color::rgb(color.resolve(dark).value());
        }
        if let Some(color) = settings
            .overrides
            .get(&Role::Surface(Surface::Viewer, Part::Foreground))
        {
            self.foreground = Some(Color::rgb(color.resolve(dark).value()));
        }
        self
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CodeThemes {
    pub light: CodeColors,
    pub dark: CodeColors,
}
impl Default for CodeThemes {
    fn default() -> Self {
        Self {
            light: CodeColors {
                background: Color::rgb(0xf7f7f7),
                foreground: None,
            },
            dark: CodeColors {
                background: Color::rgb(0x121212),
                foreground: None,
            },
        }
    }
}
fn at_luminance(target: f64, chroma: f64, hue: f64) -> Color {
    let (mut low, mut high) = (0.015, 0.995);
    for _ in 0..30 {
        let mid = (low + high) / 2.;
        if Color::from_lch(mid, chroma, hue).luminance() < target {
            low = mid;
        } else {
            high = mid;
        }
    }
    Color::from_lch((low + high) / 2., chroma, hue)
}
pub(super) struct Derived {
    pub color: Color,
    pub limited: bool,
}
pub(super) fn background(
    surface: Surface,
    recipe: &ShellAuto,
    dark: bool,
    anchor: Color,
) -> Derived {
    // A fixed code base cannot depend back on the generated shell. Window and
    // file-header defaults also start on that base; exact overrides remain final.
    if matches!(
        surface,
        Surface::Viewer | Surface::Window | Surface::ViewerHeader
    ) {
        return Derived {
            color: anchor,
            limited: false,
        };
    }
    let reference = super::built_in(Surface::Viewer, dark);
    let step = reference.contrast(super::built_in(surface, dark));
    let exponent = recipe.layer_strength.resolve(dark) / 1.4 * recipe.contrast_spread.resolve(dark);
    let ratio = step.powf(exponent);
    let target = if dark {
        (anchor.luminance() + 0.05) * ratio - 0.05
    } else {
        (anchor.luminance() + 0.05) / ratio - 0.05
    };
    let [_, c, h] = anchor.lch();
    let adjustment = recipe
        .roles
        .get(&Role::Surface(surface, Part::Background))
        .copied()
        .unwrap_or_default();
    let color = if exponent == 0. {
        anchor
    } else {
        at_luminance(
            target.clamp(0., 1.),
            c * recipe.chroma_scale.resolve(dark)
                * recipe.tint_strength.resolve(dark)
                * adjustment.chroma_scale.resolve(dark),
            h + recipe.hue_shift.resolve(dark) + adjustment.hue_shift.resolve(dark),
        )
    };
    let shift = recipe.lightness_shift.resolve(dark) + adjustment.lightness_shift.resolve(dark);
    let limited = (color.contrast(anchor) - ratio).abs() > 0.04;
    let color = if shift == 0. {
        color
    } else {
        let [l, c, h] = color.lch();
        Color::from_lch(l + shift, c, h)
    };
    Derived { color, limited }
}
pub(super) fn foreground(preferred: Color, background: Color) -> Color {
    if preferred.contrast(background) >= 4.5 {
        return preferred;
    }
    let [l, c, h] = preferred.lch();
    for step in 1..=500 {
        for direction in [-1., 1.] {
            let color = Color::from_lch(l + direction * f64::from(step) / 500., c, h);
            if color.contrast(background) >= 4.5 {
                return color;
            }
        }
    }
    if Color::rgb(0xffffff).contrast(background) > Color::rgb(0).contrast(background) {
        Color::rgb(0xffffff)
    } else {
        Color::rgb(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ColorSetting, Config, Settings};
    fn color(hex: &str) -> Color {
        Color::rgb(u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap())
    }
    fn settings() -> Settings {
        Settings::parse("[theme.shell_auto]\nsource='code_theme'").unwrap()
    }
    #[test]
    fn approved_gallery_colors_match_for_gruvbox_and_neutral_modes() {
        // Independent output from gallery commit 243f429, rather than computing
        // expected colors with the Rust implementation under test.
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("viewer-fixtures.json")).unwrap();
        for f in fixtures.as_array().unwrap() {
            let colors = CodeColors {
                background: color(f["background"].as_str().unwrap()),
                foreground: Some(color(f["foreground"].as_str().unwrap())),
            };
            let code = CodeThemes {
                light: colors,
                dark: colors,
            };
            let resolved = resolve_with_code(&settings().theme, code);
            let shell = resolved.shell(f["mode"] == "dark");
            for (key, surface) in [
                ("window", Surface::Window),
                ("history", Surface::History),
                ("files", Surface::Files),
                ("header", Surface::Header),
                ("footer", Surface::Footer),
                ("historyHeader", Surface::HistoryHeader),
                ("filesHeader", Surface::FilesHeader),
                ("historyFooter", Surface::HistoryFooter),
                ("filesFooter", Surface::FilesFooter),
                ("viewerHeader", Surface::ViewerHeader),
                ("viewerToolbar", Surface::ViewerToolbar),
                ("floating", Surface::Floating),
            ] {
                for (key_part, part) in [("bg", Part::Background), ("fg", Part::Foreground)] {
                    let expected = color(f["roles"][key][key_part].as_str().unwrap());
                    let actual = shell.surface(surface, part);
                    assert!(
                        actual
                            .channels()
                            .into_iter()
                            .zip(expected.channels())
                            .all(|(a, b)| (a - b).abs() <= 1.01 / 255.),
                        "{} {key} {key_part}: {actual:?} vs {expected:?}",
                        f["name"]
                    );
                }
            }
        }
    }
    #[test]
    fn code_source_ignores_custom_seeds_and_preserves_the_viewer_under_knob_changes() {
        let mut config = Config::parse(
            r##"[theme.shell_auto]
source="code_theme"
seed="#ff00ff"
[theme.overrides]
"editor.background"={light="#f9f5d7",dark="#1d2021"}
"##,
        )
        .unwrap();
        let original = resolve(&config.theme);
        config.theme.shell_auto.seed = ColorSetting::Shared(
            crate::settings::RgbColor::try_from("#00ff00".to_owned()).unwrap(),
        );
        assert_eq!(resolve(&config.theme), original);
        config.theme.shell_auto.hue_shift = Number::Shared(60.);
        config.theme.shell_auto.chroma_scale = Number::Shared(2.);
        config.theme.shell_auto.lightness_shift = Number::Shared(0.05);
        let changed = resolve(&config.theme);
        for dark in [false, true] {
            assert_eq!(
                original
                    .shell(dark)
                    .surface(Surface::Viewer, Part::Background),
                changed
                    .shell(dark)
                    .surface(Surface::Viewer, Part::Background)
            );
            assert_ne!(
                original
                    .shell(dark)
                    .surface(Surface::Header, Part::Background),
                changed
                    .shell(dark)
                    .surface(Surface::Header, Part::Background)
            );
            assert_eq!(
                original.shell(dark).get(Role::Success),
                changed.shell(dark).get(Role::Success)
            );
        }
    }
    #[test]
    fn separation_is_ordered_flat_at_zero_and_range_limits_are_reported() {
        let mut config = Config::parse("[theme.shell_auto]\nsource='code_theme'").unwrap();
        for dark in [true, false] {
            let mut last = 1.;
            for strength in [0., 0.7, 1.4, 2.3] {
                config.theme.shell_auto.layer_strength = Number::Shared(strength);
                let themes = resolve(&config.theme);
                let shell = themes.shell(dark);
                let base = shell.surface(Surface::Viewer, Part::Background);
                let ratio = base.contrast(shell.surface(Surface::Header, Part::Background));
                assert!(ratio >= last - 0.01);
                last = ratio;
                if strength == 0. {
                    for surface in Surface::ALL {
                        assert_eq!(shell.surface(surface, Part::Background), base);
                    }
                }
            }
        }
        let white = CodeColors {
            background: Color::rgb(0xffffff),
            foreground: None,
        };
        let themes = resolve_with_code(
            &config.theme,
            CodeThemes {
                light: white,
                dark: white,
            },
        );
        assert!(
            themes
                .dark
                .warnings
                .iter()
                .any(|s| s.contains("target contrast"))
        );
    }
    #[test]
    fn exact_overrides_and_disabled_generation_remain_authoritative() {
        let mut config=Config::parse("[theme.shell_auto]\nsource='code_theme'\n[theme.overrides]\n'shell.viewer.toolbar.background'='#123456'").unwrap();
        assert_eq!(
            resolve(&config.theme)
                .dark
                .surface(Surface::ViewerToolbar, Part::Background),
            color("#123456")
        );
        config.theme.shell_auto.enabled = false;
        let resolved = resolve(&config.theme);
        assert_eq!(
            resolved.dark.surface(Surface::Files, Part::Background),
            color("#181818")
        );
        assert_eq!(
            resolved
                .dark
                .surface(Surface::ViewerToolbar, Part::Background),
            color("#123456")
        );
        assert!(Config::parse("[theme.shell_auto]\nsource='missing'").is_err());
    }
}
