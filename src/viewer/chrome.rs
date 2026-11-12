//! Shared panel surfaces and geometry, resolved from the active theme/settings.
use super::*;
use gpui::component::Theme;
use gpui::component::menu::{ContextMenu, ContextMenuExt as _};
use jaywalk::panel_layout::PanelId;

pub(super) const PANEL_INSET: f32 = 8.;
// Small icon buttons center a 14 px glyph in a 24 px hit target.
const HEADER_ICON_INSET: f32 = 5.;

impl Viewer {
    pub(super) fn panel_footer(&self, label: String, font_size: f32, cx: &App) -> gpui::Div {
        // Match header edge alignment and keep room for a small 24 px control,
        // including its border, even when the application footer is compact.
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .pl(px(PANEL_INSET))
            .pr(px((PANEL_INSET - HEADER_ICON_INSET).max(0.)))
            .h(px(self
                .settings
                .footer_height()
                .max(font_size * 1.5)
                .max(25.)))
            .flex_shrink_0()
            .border_t_1()
            .border_color(Theme::global(cx).border)
            .bg(Theme::global(cx).sidebar)
            .text_color(Theme::global(cx).muted_foreground)
            .text_size(px(font_size * 0.85))
            .child(div().flex_1().min_w_0().truncate().child(label))
    }

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

/// Presentation only: each body keeps its existing scroll/virtualization owner.
#[derive(IntoElement)]
pub(super) struct Panel {
    frame: gpui::Stateful<gpui::Div>,
    header: AnyElement,
    children: Vec<AnyElement>,
    footer: Option<AnyElement>,
}
impl Panel {
    pub(super) fn new(
        id: impl Into<ElementId>,
        header: impl IntoElement,
        frame: gpui::Div,
    ) -> Self {
        Self {
            frame: frame.id(id),
            header: header.into_any_element(),
            children: Vec::new(),
            footer: None,
        }
    }
    pub(super) fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}
impl ParentElement for Panel {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}
impl RenderOnce for Panel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.frame
            .v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(div().flex_shrink_0().child(self.header))
            .child(
                div()
                    .relative()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .children(self.children),
            )
            .children(
                self.footer
                    .map(|footer| div().flex_shrink_0().child(footer)),
            )
    }
}
