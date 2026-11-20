//! Shared panel surfaces and geometry, resolved from the active theme/settings.
use super::*;
use gpui::component::Theme;
use gpui::component::menu::{ContextMenu, ContextMenuExt as _};
use gpui::prelude::FluentBuilder;
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

/// Presentation only: each body keeps its existing scroll/virtualization owner.
#[derive(IntoElement)]
pub(super) struct Panel {
    id: ElementId,
    focus: Option<FocusHandle>,
    fallback_focus: Option<FocusHandle>,
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
        let id = id.into();
        Self {
            id: id.clone(),
            focus: None,
            fallback_focus: None,
            frame: frame.id(id),
            header: header.into_any_element(),
            children: Vec::new(),
            footer: None,
        }
    }
    pub(super) fn focus_handle(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
    }
    pub(super) fn fallback_focus(mut self, focus: &FocusHandle) -> Self {
        self.fallback_focus = Some(focus.clone());
        self
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = self.focus.unwrap_or_else(|| {
            window
                .use_keyed_state(self.id, cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        });
        let pointer_focus = focus.clone();
        let fallback = self.fallback_focus;
        let accented = crate::appearance::panel_outline(cx).is_some();
        self.frame
            .relative()
            .track_focus(&focus)
            .when(accented, |frame| {
                frame.capture_any_mouse_down(move |event, window, cx| {
                    // Capture before editors handle the press so their own focus
                    // can take over. Never steal focus back from an editor.
                    if event.button == MouseButton::Left {
                        window.focus(&pointer_focus, cx);
                    }
                })
            })
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
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        if (focus.contains_focused(window, cx)
                            || fallback
                                .as_ref()
                                .is_some_and(|focus| focus.is_focused(window)))
                            && let Some(color) = crate::appearance::panel_outline(cx)
                        {
                            window.paint_quad(outline(bounds, color, BorderStyle::Solid));
                        }
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
    }
}
