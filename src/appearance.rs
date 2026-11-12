//! One application accent, projected into the toolkit's existing control roles.
use gpui::component::{Icon, Theme};
use gpui::prelude::FluentBuilder;
use gpui::{App, Hsla, Styled, rgb};
use jaywalk::settings::RgbColor;

pub(crate) fn active_icon(name: gpui::assets::IconName, active: bool, cx: &App) -> Icon {
    Icon::new(name).when(active, |icon| icon.text_color(Theme::global(cx).primary))
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
