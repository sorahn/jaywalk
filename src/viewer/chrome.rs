//! Shared panel surfaces and geometry, resolved from the active theme/settings.
use super::*;
use gpui::component::Theme;
use gpui::component::menu::{ContextMenu, ContextMenuExt as _};
use jaywalk::panel_layout::PanelId;

pub(super) const PANEL_INSET: f32 = 8.;
// Small icon buttons center a 14 px glyph in a 24 px hit target.
const HEADER_ICON_INSET: f32 = 5.;

impl Viewer {
    pub(super) fn panel_header(
        &self,
        panel: PanelId,
        font_size: f32,
        cx: &App,
    ) -> ContextMenu<gpui::Div> {
        let entries = self.panel_menu_entries(panel);
        let focus = self.focus.clone();
        div()
            .h_flex()
            .items_center()
            .justify_between()
            .pl(px(PANEL_INSET))
            .pr(px((PANEL_INSET - HEADER_ICON_INSET).max(0.)))
            .h(px(self
                .settings
                .ui
                .panel_header_height
                .max(font_size * 1.5)))
            .flex_shrink_0()
            .border_b_1()
            .border_color(Theme::global(cx).border)
            .bg(Theme::global(cx).sidebar)
            .text_size(px(font_size * 0.85))
            .context_menu(move |menu, _, _| {
                context_menu::command_entries(menu.action_context(focus.clone()), &entries)
            })
    }
}
