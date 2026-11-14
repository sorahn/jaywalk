//! Resolve sticky placement after the list's prepaint, using this frame's row
//! bounds. Build and prepaint the actual header so its controls remain usable.
use super::{CodePresentation, FileHeaderPlacement, render_file_header_content};
use crate::ActiveTheme as _;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId,
    InspectorElementId, InteractiveElement as _, IntoElement, LayoutId, ParentElement as _, Pixels,
    Style, Styled as _, Window, div, point, px, relative, size,
};
use std::rc::Rc;

pub(super) struct StickyHeader {
    code: Rc<CodePresentation>,
    content: Option<AnyElement>,
}
impl StickyHeader {
    pub(super) fn new(code: Rc<CodePresentation>) -> Self {
        Self {
            code,
            content: None,
        }
    }
}

impl Element for StickyHeader {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        Some("sticky-file-header".into())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size = size(relative(1.).into(), relative(1.).into());
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.content = None;
        let state = self.code.state.read(cx);
        let list = state.list();
        let top = list.logical_scroll_top();
        let Some(row) = state.rows().get(top.item_ix) else {
            return;
        };
        let file = row.file();
        let natural_top = if file == 0 && self.code.preamble.is_some() {
            self.code
                .first_header_bounds
                .get()
                .map(|header| header.top() - bounds.top())
        } else {
            state
                .header_row(file)
                .and_then(|ix| list.bounds_for_item(ix))
                .map(|header| header.top() - bounds.top())
        };
        let next_top = state
            .header_row(file + 1)
            .and_then(|ix| list.bounds_for_item(ix))
            .map(|next| next.top() - bounds.top());
        let state = self.code.state.clone();
        let row_height = self.code.row_height;
        let mut header = div()
            .id(("sticky-header-content", file))
            .w(bounds.size.width)
            .bg(cx.theme().background)
            .occlude()
            .on_scroll_wheel(move |event, _, cx| {
                let delta = event.delta.pixel_delta(row_height);
                state.update(cx, |state, cx| {
                    if let Some(horizontal) = super::sticky_geometry::horizontal_delta(
                        f32::from(delta.x),
                        f32::from(delta.y),
                        event.modifiers.shift,
                    ) {
                        state.scroll_horizontal(px(-horizontal), cx);
                    } else {
                        state.list().scroll_by(-delta.y);
                        cx.notify();
                    }
                });
                cx.stop_propagation();
            })
            .child(render_file_header_content(
                file, &self.code, FileHeaderPlacement::Sticky, window, cx,
            ))
            .into_any_element();
        let measured = header.layout_as_root(
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::MinContent,
            ),
            window,
            cx,
        );
        let Some(top) = super::sticky_geometry::file_header_top(
            natural_top.map(f32::from),
            next_top.map(f32::from),
            f32::from(measured.height),
        ) else {
            return;
        };
        let origin = point(bounds.left(), bounds.top() + px(top));
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            header.prepaint_at(origin, window, cx)
        });
        self.content = Some(header);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(content) = &mut self.content {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                content.paint(window, cx)
            });
        }
    }
}
impl IntoElement for StickyHeader {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
