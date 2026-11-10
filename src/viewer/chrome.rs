//! Shared panel surfaces and geometry, resolved from the active theme/settings.
use super::*;
use gpui::component::Theme;
use gpui::component::menu::{ContextMenu, ContextMenuExt as _};
use jaywalk::panel_layout::PanelId;

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
            .context_menu(move |menu, _, _| {
                context_menu::command_entries(menu.action_context(focus.clone()), &entries)
            })
    }
}
