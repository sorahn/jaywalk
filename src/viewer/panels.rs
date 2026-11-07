use super::*;
use gpui::base::{ResizableState, ResizeHandleRenderer};
use gpui::component::resizable::{ResizablePanelGroup, resize_handle_appearance};

// Match the native handle's four-pixel padding around its one-pixel line.
const HANDLE_PADDING: Pixels = px(4.);

#[derive(Clone, Copy)]
enum PanelGroup {
    Sidebars,
    Diff,
}

pub(super) struct PanelLayout {
    sidebars: Entity<ResizableState>,
    diff: Entity<ResizableState>,
    sidebar_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl PanelLayout {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            sidebars: cx.new(|_| ResizableState::default()),
            diff: cx.new(|_| ResizableState::default()),
            sidebar_bounds: Rc::new(Cell::new(None)),
        }
    }
}

impl Viewer {
    pub(super) fn sidebar_group(&self, cx: &Context<Self>) -> ResizablePanelGroup {
        h_resizable("viewer-layout")
            .with_state(&self.panels.sidebars)
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::Sidebars, cx))
    }

    pub(super) fn diff_group(&self, cx: &Context<Self>) -> ResizablePanelGroup {
        h_resizable("code-panes")
            .with_state(&self.panels.diff)
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::Diff, cx))
    }

    pub(super) fn sidebar_bounds_marker(&self) -> AnyElement {
        let bounds = self.panels.sidebar_bounds.clone();
        canvas(
            move |current, _, _| bounds.set(Some(current)),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }

    fn reset_handle_appearance(
        &self,
        group: PanelGroup,
        cx: &Context<Self>,
    ) -> ResizeHandleRenderer {
        let appearance = resize_handle_appearance();
        let viewer = cx.entity().downgrade();
        Rc::new(move |handle, window, cx| {
            let line = appearance(handle, window, cx)?;
            let viewer = viewer.clone();
            Some(
                div()
                    .relative()
                    .flex_none()
                    .w(px(1.))
                    .h_full()
                    .child(line)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let hit_bounds = Bounds::from_corners(
                                    point(bounds.left() - HANDLE_PADDING, bounds.top()),
                                    point(bounds.right() + HANDLE_PADDING, bounds.bottom()),
                                );
                                let viewer = viewer.clone();
                                window.on_mouse_event(
                                    move |event: &MouseDownEvent, phase, window, cx| {
                                        if phase != DispatchPhase::Capture
                                            || event.button != MouseButton::Left
                                            || event.click_count != 2
                                            || !hit_bounds.contains(&event.position)
                                        {
                                            return;
                                        }
                                        let _ = viewer.update(cx, |viewer, cx| {
                                            viewer.reset_panel(group, bounds.left(), window, cx)
                                        });
                                        cx.stop_propagation();
                                    },
                                );
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .into_any_element(),
            )
        })
    }

    fn reset_panel(
        &mut self,
        group: PanelGroup,
        divider_x: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match group {
            PanelGroup::Sidebars => {
                let Some(bounds) = self.panels.sidebar_bounds.get() else {
                    return;
                };
                let sizes = self.panels.sidebars.read(cx).sizes();
                let Some(history_width) = sizes.first() else {
                    return;
                };
                let is_history = (divider_x - bounds.left() - *history_width).abs() <= px(2.);
                let file_width = sizes.get(1).copied().unwrap_or(px(self
                    .settings
                    .panels
                    .files
                    .default_width));
                let history_default = px(self.settings.panels.history.default_width);
                let files_default = px(self.settings.panels.files.default_width);
                self.panels.sidebars.update(cx, |state, cx| {
                    if is_history {
                        state.resize_panel(0, history_default, window, cx);
                        // Preserve the other sidebar; let the code area absorb
                        // the reset instead of permanently changing both panes.
                        state.resize_panel(1, file_width, window, cx);
                    } else {
                        state.resize_panel(1, files_default, window, cx);
                    }
                });
            }
            PanelGroup::Diff => {
                // Recreate the native layout state so its unconstrained code
                // panes share the remainder evenly and the fixed gutter keeps
                // its configured width.
                self.panels.diff = cx.new(|_| ResizableState::default());
                self.reveal_after_layout =
                    self.code().is_some() && (self.wrapping.before || self.wrapping.after);
            }
        }
        cx.notify();
    }
}
