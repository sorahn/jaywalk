//! Live application appearance and diff palettes, using the toolkit’s theme roles.
use gpui::component::highlighter::{HighlightTheme, ThemeStyle};
use gpui::component::{Icon, Theme, ThemeMode};
use gpui::prelude::FluentBuilder;
use gpui::{App, Global, Hsla, SharedString, Styled, Window, WindowAppearance, px, rgb};
use jaywalk::settings::{
    Appearance, DiffColorSettings, EditorSettings, RgbColor, Settings, UiSettings,
};
use jaywalk::theme::{Part, ResolvedThemes, Role, Surface};
use std::sync::Arc;

#[derive(Clone)]
struct AppAppearance {
    ui: UiSettings,
    editor: EditorSettings,
    diff: DiffColorSettings,
    mode: Option<ThemeMode>,
    themes: Arc<ResolvedThemes>,
}
impl Global for AppAppearance {}

pub(crate) fn init(settings: &Settings, cx: &mut App) {
    cx.set_global(AppAppearance {
        ui: settings.ui.clone(),
        editor: settings.editor.clone(),
        diff: settings.diff.colors,
        mode: None,
        themes: settings.resolved_theme().clone(),
    });
    apply(settings, cx);
    crate::platform::accent::observe(cx);
}

pub(crate) fn apply(settings: &Settings, cx: &mut App) {
    let state = cx.global_mut::<AppAppearance>();
    state.themes = settings.resolved_theme().clone();
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

/// The native macOS title bar needs an explicit divider at the content edge.
pub(crate) fn window_content(cx: &App) -> gpui::Div {
    gpui::div().when(cfg!(target_os = "macos"), |root| {
        root.border_t_1()
            .border_color(Theme::global(cx).title_bar_border)
    })
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

#[cfg(all(target_os = "macos", not(test)))]
pub(crate) fn system_colors_changed(cx: &mut App) {
    if cx.try_global::<AppAppearance>().is_some_and(|state| {
        matches!(
            state.ui.accent_color,
            jaywalk::settings::AccentColorSetting::System(_)
        )
    }) {
        refresh(None, true, cx);
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
            theme.highlight_theme = syntax_theme(dark);
        }
        project_shell(theme, state.themes.shell(dark));
        let system = matches!(
            state.ui.accent_color,
            jaywalk::settings::AccentColorSetting::System(_)
        )
        .then(|| crate::platform::accent::read(dark))
        .flatten();
        apply_accent(
            theme,
            state
                .ui
                .accent_color
                .resolve(dark, system, state.ui.accent_color_fallback),
        );
        theme.popup_outline = Some(if state.ui.accent_floating_outlines {
            theme.primary
        } else {
            gpui::rgba(
                state
                    .themes
                    .shell(dark)
                    .surface(Surface::Floating, Part::Border)
                    .0,
            )
            .into()
        });
        theme.host_spacing = Some(gpui::component::HostSpacing {
            unit: px(state.ui.spacing.unit),
            edge: px(state.ui.spacing.edge_px()),
            interactive: px(state.ui.spacing.interactive_px()),
        });
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

/// Lookup is allocation-free; settings resolution owns color generation.
pub(crate) fn surface(surface: Surface, part: Part, cx: &App) -> Hsla {
    color(Role::Surface(surface, part), cx)
}

pub(crate) fn color(role: Role, cx: &App) -> Hsla {
    if let Some(state) = cx.try_global::<AppAppearance>() {
        return gpui::rgba(
            state
                .themes
                .shell(Theme::global(cx).is_dark())
                .get(role)
                .color
                .0,
        )
        .into();
    }
    // Standalone component/headless hosts can use the toolkit without app globals.
    let theme = Theme::global(cx);
    match role {
        Role::Surface(_, Part::Foreground) | Role::ControlForeground => theme.foreground,
        Role::Surface(_, Part::MutedForeground) => theme.muted_foreground,
        Role::Surface(_, Part::Border) => theme.border,
        Role::Surface(_, Part::Background) => theme.background,
        _ => {
            static DEFAULTS: std::sync::LazyLock<ResolvedThemes> = std::sync::LazyLock::new(|| {
                jaywalk::theme::resolve(&jaywalk::settings::Config::default().theme)
            });
            gpui::rgba(DEFAULTS.shell(theme.is_dark()).get(role).color.0).into()
        }
    }
}

fn project_shell(theme: &mut Theme, shell: &jaywalk::theme::Shell) {
    let c = |s, p| -> Hsla { gpui::rgba(shell.surface(s, p).0).into() };
    theme.background = c(Surface::Viewer, Part::Background);
    theme.foreground = c(Surface::Window, Part::Foreground);
    theme.muted_foreground = c(Surface::Window, Part::MutedForeground);
    theme.border = c(Surface::Window, Part::Border);
    theme.sidebar = c(Surface::History, Part::Background);
    theme.sidebar_foreground = c(Surface::History, Part::Foreground);
    theme.sidebar_border = c(Surface::History, Part::Border);
    theme.popover = c(Surface::Floating, Part::Background);
    theme.popover_foreground = c(Surface::Floating, Part::Foreground);
    theme.title_bar = c(Surface::Header, Part::Background);
    theme.title_bar_border = c(Surface::Header, Part::Border);
    theme.muted = c(Surface::Footer, Part::Background);
    let role = |r| -> Hsla { gpui::rgba(shell.get(r).color.0).into() };
    theme.secondary = role(Role::ControlBackground);
    theme.secondary_foreground = role(Role::ControlForeground);
    theme.secondary_hover = role(Role::ControlHover);
    theme.button = theme.secondary;
    theme.button_foreground = theme.secondary_foreground;
    theme.button_hover = theme.secondary_hover;
    theme.colors.list = theme.popover;
    theme.list_hover = theme.secondary_hover;
    theme.status_bar = c(Surface::Footer, Part::Background);
    theme.status_bar_border = c(Surface::Footer, Part::Border);
    theme.success = role(Role::Success);
    theme.warning = role(Role::Warning);
    theme.danger = role(Role::Error);
    theme.info = role(Role::Info);
    theme.input = theme.popover;
    let mut highlight = (*theme.highlight_theme).clone();
    highlight.style.editor_background = Some(c(Surface::Viewer, Part::Background));
    highlight.style.editor_foreground = Some(c(Surface::Viewer, Part::Foreground));
    highlight.style.editor_line_number = Some(c(Surface::Viewer, Part::MutedForeground));
    theme.highlight_theme = Arc::new(highlight);
}

fn syntax_theme(dark: bool) -> std::sync::Arc<HighlightTheme> {
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
        whitespace_error: resolve(colors.whitespace_error),
    }
}

pub(crate) fn floating_outline(cx: &App) -> Hsla {
    let theme = Theme::global(cx);
    theme.popup_outline.unwrap_or(theme.border)
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
    use super::{project_shell, syntax_theme};
    use gpui::component::Theme;
    use gpui::{Hsla, rgb};
    use jaywalk::settings::Settings;
    #[test]
    fn mode_switches_supply_matching_editor_and_syntax_palettes() {
        let light = syntax_theme(false);
        let dark = syntax_theme(true);
        let settings = Settings::default();
        for (is_dark, background) in [(false, 0xf7f7f7), (true, 0x121212)] {
            let mut theme = Theme::default();
            theme.highlight_theme = syntax_theme(is_dark);
            let string = theme.highlight_theme.style.syntax.string;
            project_shell(&mut theme, settings.resolved_theme().shell(is_dark));
            assert_eq!(
                theme.highlight_theme.style.editor_background,
                Some(Hsla::from(rgb(background)))
            );
            assert_eq!(theme.highlight_theme.style.syntax.string, string);
        }
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

#[cfg(test)]
mod outline_tests {
    use super::*;

    #[gpui_test::test]
    fn floating_outlines_follow_live_accent_and_can_be_disabled(
        cx: &mut gpui_test::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            let mut settings = Settings::parse("[ui]\naccent_color='#aabbcc'").unwrap();
            assert!(settings.ui.accent_floating_outlines);
            init(&settings, cx);
            let neutral = Theme::global(cx).border;
            let accent: Hsla = rgb(0xaabbcc).into();
            assert_eq!(Theme::global(cx).popup_outline, Some(accent));
            assert_eq!(floating_outline(cx), accent);

            settings.ui.accent_floating_outlines = false;
            apply(&settings, cx);
            assert_eq!(Theme::global(cx).popup_outline, Some(neutral));
            assert_eq!(floating_outline(cx), neutral);

            let custom = Settings::parse(
                r##"
[ui]
accent_floating_outlines=false
[theme.overrides]
"shell.floating.border"="#abc123"
"##,
            )
            .unwrap();
            apply(&custom, cx);
            assert_eq!(floating_outline(cx), Hsla::from(rgb(0xabc123)));
            assert_eq!(
                Theme::global(cx).popup_outline,
                Some(Hsla::from(rgb(0xabc123)))
            );

            settings.ui.accent_floating_outlines = true;
            settings.ui.accent_color = jaywalk::settings::AccentColorSetting::Custom(
                jaywalk::settings::ColorSetting::Shared(
                    RgbColor::try_from("#123456".to_owned()).unwrap(),
                ),
            );
            apply(&settings, cx);
            assert_eq!(floating_outline(cx), Hsla::from(rgb(0x123456)));
            assert_eq!(Theme::global(cx).border, neutral);
        });
    }
}
