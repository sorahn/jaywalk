//! Shared panel surfaces and geometry, resolved from the active theme/settings.
use super::*;
use jaywalk::theme::{Part, Surface};

pub(super) fn panel_surface(panel: PanelId) -> Surface {
    match panel {
        PanelId::History | PanelId::ChangeEvolution => Surface::History,
        PanelId::Files | PanelId::FileEvolution => Surface::Files,
    }
}

fn header_surface(panel: PanelId) -> Surface {
    match panel_surface(panel) {
        Surface::History => Surface::HistoryHeader,
        _ => Surface::FilesHeader,
    }
}
use gpui::component::menu::{ContextMenu, ContextMenuExt as _};
use jaywalk::panel_layout::PanelId;

// Small icon buttons center a 14 px glyph in a 24 px hit target.

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
            .pl(crate::spacing::edge(cx))
            .pr((crate::spacing::edge(cx) - crate::spacing::interactive(cx)).max(px(0.)))
            .h(px(self.settings.panel_header_height().max(font_size * 1.5)))
            .flex_shrink_0()
            .border_b_1()
            .border_color(crate::appearance::surface(
                header_surface(panel),
                Part::Border,
                cx,
            ))
            .bg(crate::appearance::surface(
                header_surface(panel),
                Part::Background,
                cx,
            ))
            .text_color(crate::appearance::surface(
                header_surface(panel),
                Part::Foreground,
                cx,
            ))
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
