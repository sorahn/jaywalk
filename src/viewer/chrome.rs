//! Shared panel surfaces and geometry, resolved from the active theme/settings.
use super::*;
use gpui::component::Theme;

impl Viewer {
    pub(super) fn panel_header(&self, font_size: f32, cx: &App) -> gpui::Div {
        div()
            .h_flex()
            .items_center()
            .justify_between()
            .px_3()
            .h(px(self
                .settings
                .ui
                .panel_header_height
                .max(font_size * 1.5)))
            .flex_shrink_0()
            .border_b_1()
            .border_color(Theme::global(cx).border)
            .bg(Theme::global(cx).sidebar)
            .text_size(px(font_size))
    }
}
