//! Live application appearance and diff palettes, using the toolkit’s theme roles.
use gpui::component::highlighter::{HighlightTheme, ThemeStyle};
use gpui::component::{Icon, Theme, ThemeMode};
use gpui::prelude::FluentBuilder;
use gpui::{App, Global, Hsla, SharedString, Styled, Window, WindowAppearance, px, rgb};
use jaywalk::settings::{
    Appearance, DiffColorSettings, EditorSettings, RgbColor, Settings, UiSettings,
};

#[derive(Clone)]
struct AppAppearance {
    ui: UiSettings,
    editor: EditorSettings,
    diff: DiffColorSettings,
    mode: Option<ThemeMode>,
}
impl Global for AppAppearance {}

pub(crate) fn init(settings: &Settings, cx: &mut App) {
    cx.set_global(AppAppearance {
        ui: settings.ui.clone(),
        editor: settings.editor.clone(),
        diff: settings.diff.colors,
        mode: None,
    });
    apply(settings, cx);
}

pub(crate) fn apply(settings: &Settings, cx: &mut App) {
    let state = cx.global_mut::<AppAppearance>();
    state.ui = settings.ui.clone();
    state.editor = settings.editor.clone();
    state.diff = settings.diff.colors;
    cx.set_window_appearance(match settings.ui.appearance {
        Appearance::Light => Some(WindowAppearance::Light),
        Appearance::Dark => Some(WindowAppearance::Dark),
        Appearance::System => None,
    });
    refresh(None, true, cx);
}

pub(crate) fn watch_window(window: &mut Window, cx: &mut App) {
    window
        .observe_window_appearance(|window, cx| {
            if cx.global::<AppAppearance>().ui.appearance == Appearance::System {
                refresh(Some(window.appearance()), false, cx);
            }
        })
        .detach();
    if cx.global::<AppAppearance>().ui.appearance == Appearance::System {
        refresh(Some(window.appearance()), false, cx);
    }
}

fn refresh(system: Option<WindowAppearance>, force: bool, cx: &mut App) {
    let state = cx.global::<AppAppearance>().clone();
    let system_mode = ThemeMode::from(system.unwrap_or_else(|| cx.window_appearance()));
    let dark = state.ui.appearance.is_dark(system_mode == ThemeMode::Dark);
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let changed = state.mode != Some(mode);
    if !changed && !force {
        return;
    }
    cx.global_mut::<AppAppearance>().mode = Some(mode);
    if Theme::global(cx).mode != mode {
        Theme::change(mode, None, cx);
    }
    Theme::update(cx, |theme| {
        if changed {
            theme.highlight_theme = code_theme(dark);
        }
        apply_accent(theme, state.ui.accent_color.resolve(dark));
        theme.font_size = px(state.ui.font_size);
        theme.mono_font_size = px(state.editor.font_size);
        theme.font_family = state
            .ui
            .font_family
            .clone()
            .map(SharedString::from)
            .unwrap_or_else(|| Theme::default().font_family);
        theme.mono_font_family = state
            .editor
            .font_family
            .clone()
            .map(SharedString::from)
            .unwrap_or_else(|| Theme::default().mono_font_family);
    });
}

fn code_theme(dark: bool) -> std::sync::Arc<HighlightTheme> {
    let mut highlight = if dark {
        (*HighlightTheme::default_dark()).clone()
    } else {
        (*HighlightTheme::default_light()).clone()
    };
    highlight.name = if dark {
        "Jaywalk VS Code dark"
    } else {
        "Jaywalk VS Code light"
    }
    .into();
    highlight.style.editor_background = Some(rgb(if dark { 0x1f1f1f } else { 0xffffff }).into());
    highlight.style.editor_foreground = Some(rgb(if dark { 0xcccccc } else { 0x3b3b3b }).into());
    highlight.style.editor_line_number = Some(rgb(if dark { 0x6e7681 } else { 0x6e6e6e }).into());
    let token = |hex: &str| -> Option<ThemeStyle> {
        Some(
            serde_json::from_value(serde_json::json!({"color":hex}))
                .expect("built-in syntax color is valid"),
        )
    };
    highlight.style.syntax.comment = token(if dark { "#6a9955" } else { "#008000" });
    highlight.style.syntax.comment_doc = highlight.style.syntax.comment;
    highlight.style.syntax.keyword = token(if dark { "#569cd6" } else { "#0000ff" });
    highlight.style.syntax.string = token(if dark { "#ce9178" } else { "#a31515" });
    highlight.style.syntax.number = token(if dark { "#b5cea8" } else { "#098658" });
    highlight.style.syntax.function = token(if dark { "#dcdcaa" } else { "#795e26" });
    highlight.style.syntax.type_ = token(if dark { "#4ec9b0" } else { "#267f99" });
    std::sync::Arc::new(highlight)
}

pub(crate) fn diff_colors(cx: &App) -> gpui::component::diff::DiffColors {
    resolve_diff_colors(
        cx.global::<AppAppearance>().diff,
        Theme::global(cx).is_dark(),
    )
}

fn resolve_diff_colors(colors: DiffColorSettings, dark: bool) -> gpui::component::diff::DiffColors {
    let resolve = |color: jaywalk::settings::ColorSetting| rgb(color.resolve(dark).value()).into();
    gpui::component::diff::DiffColors {
        added: resolve(colors.added),
        removed: resolve(colors.removed),
        modified: resolve(colors.modified),
        added_background: resolve(colors.added_background),
        removed_background: resolve(colors.removed_background),
        modified_background: resolve(colors.modified_background),
        word_added: resolve(colors.word_added),
        word_removed: resolve(colors.word_removed),
        conflict_base: resolve(colors.conflict_base),
    }
}

pub(crate) fn active_icon(icon: impl Into<Icon>, active: bool, cx: &App) -> Icon {
    Icon::new(icon).when(active, |icon| icon.text_color(Theme::global(cx).primary))
}

/// Call inside Theme::update so component tokens, base controls and all windows
/// receive the same palette.
pub(crate) fn apply_accent(theme: &mut Theme, color: RgbColor) {
    let accent: Hsla = rgb(color.value()).into();
    let foreground = on_accent(color);
    let hover = accent.blend(theme.foreground.opacity(0.12));
    let pressed = accent.blend(theme.background.opacity(0.16));
    let subtle = accent.opacity(0.18);
    theme.primary = accent;
    theme.primary_foreground = foreground;
    theme.primary_hover = hover;
    theme.primary_active = pressed;
    theme.button_primary = accent;
    theme.button_primary_foreground = foreground;
    theme.button_primary_hover = hover;
    theme.button_primary_active = pressed;
    theme.accent = subtle;
    theme.accent_foreground = theme.foreground;
    theme.ring = accent;
    theme.caret = accent;
    theme.selection = accent.opacity(0.3);
    theme.list_active = subtle;
    theme.list_active_border = accent;
    theme.secondary_active = subtle;
    theme.button_active = subtle;
    theme.button_secondary_active = subtle;
    theme.sidebar_primary = accent;
    theme.sidebar_primary_foreground = foreground;
    theme.tab_active = subtle;
    theme.tab_active_foreground = theme.foreground;
    theme.slider_bar = accent;
    theme.slider_thumb = accent;
    theme.link = accent;
    theme.link_hover = hover;
    theme.link_active = pressed;
}

fn on_accent(color: RgbColor) -> Hsla {
    let value = color.value();
    let linear = |shift: u32| {
        let channel = ((value >> shift) & 0xff) as f32 / 255.;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0);
    rgb(if luminance > 0.179 {
        0x000000
    } else {
        0xffffff
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_accent_updates_control_roles_without_recoloring_diff_or_status_semantics() {
        let mut theme = Theme::default();
        let status = (theme.success, theme.danger, theme.warning, theme.info);
        let syntax = theme.highlight_theme.clone();
        for hex in ["#3794ff", "#ff9f43"] {
            let color = RgbColor::try_from(hex.to_owned()).unwrap();
            let accent: Hsla = rgb(color.value()).into();
            apply_accent(&mut theme, color);
            assert_eq!(theme.primary, accent);
            assert_eq!(theme.button_primary, accent);
            assert_eq!(theme.ring, accent);
            assert_eq!(theme.list_active, accent.opacity(0.18));
            assert_eq!(theme.color_tokens().primary, accent);
            assert_eq!(
                status,
                (theme.success, theme.danger, theme.warning, theme.info)
            );
            assert!(std::sync::Arc::ptr_eq(&theme.highlight_theme, &syntax));
        }
    }
    #[test]
    fn filled_controls_choose_readable_light_or_dark_marks() {
        for (hex, expected) in [
            ("#000000", 0xffffff),
            ("#ffffff", 0x000000),
            ("#ffff00", 0x000000),
        ] {
            let color = RgbColor::try_from(hex.to_owned()).unwrap();
            assert_eq!(on_accent(color), Hsla::from(rgb(expected)));
        }
    }
}

#[cfg(test)]
mod mode_tests {
    use super::code_theme;
    use gpui::{Hsla, rgb};
    #[test]
    fn mode_switches_supply_matching_editor_and_syntax_palettes() {
        let light = code_theme(false);
        let dark = code_theme(true);
        assert_eq!(
            light.style.editor_background,
            Some(Hsla::from(rgb(0xffffff)))
        );
        assert_eq!(
            dark.style.editor_background,
            Some(Hsla::from(rgb(0x1f1f1f)))
        );
        assert_ne!(light.style.syntax.string, dark.style.syntax.string);
        assert_ne!(light.style.syntax.keyword, dark.style.syntax.keyword);
    }
}

#[cfg(test)]
mod diff_color_tests {
    use super::resolve_diff_colors;
    use gpui::{Hsla, rgb};
    use jaywalk::settings::Settings;

    #[test]
    fn diff_overrides_resolve_independent_marks_backgrounds_and_words_for_each_mode() {
        let settings = Settings::parse(
            r##"
[diff.colors]
added = "#aa00cc"
modified = { light = "#123456", dark = "#654321" }
added_background = "#eeaa00"
word_added = "#ff00aa"
[diff.colors.word_removed]
light = "#990055"
"##,
        )
        .unwrap();
        for dark in [false, true] {
            let palette = resolve_diff_colors(settings.diff.colors, dark);
            assert_eq!(palette.added, Hsla::from(rgb(0xaa00cc)));
            assert_eq!(palette.added_background, Hsla::from(rgb(0xeeaa00)));
            assert_eq!(palette.word_added, Hsla::from(rgb(0xff00aa)));
            assert_eq!(
                palette.modified,
                Hsla::from(rgb(if dark { 0x654321 } else { 0x123456 }))
            );
            assert_eq!(
                palette.word_removed,
                Hsla::from(rgb(if dark { 0xf85149 } else { 0x990055 }))
            );
            assert_eq!(
                palette.removed,
                resolve_diff_colors(Settings::parse("").unwrap().diff.colors, dark).removed
            );
        }
        assert!(Settings::parse("[diff.colors]\nadded='red'").is_err());
        assert!(Settings::parse("[diff.colors]\naddded='#ffffff'").is_err());
    }
}
