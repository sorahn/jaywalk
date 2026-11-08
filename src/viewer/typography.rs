use super::*;
use gpui::component::Theme;
use jaywalk::settings::FontSettings;
use std::sync::Arc;

pub(super) fn features(ligatures: bool) -> FontFeatures {
    let enabled = u32::from(ligatures);
    FontFeatures(Arc::new(vec![
        ("liga".into(), enabled),
        ("clig".into(), enabled),
        ("calt".into(), enabled),
    ]))
}

pub(super) fn panel_font(settings: &FontSettings, cx: &App) -> Font {
    Font {
        family: settings
            .family
            .clone()
            .map(SharedString::from)
            .unwrap_or_else(|| Theme::global(cx).font_family.clone()),
        features: features(settings.ligatures),
        ..Default::default()
    }
}
