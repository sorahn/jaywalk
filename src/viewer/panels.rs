use super::*;
use gpui::base::{ResizableState, ResizeHandleRenderer};
use gpui::component::resizable::{ResizablePanelGroup, resize_handle_appearance, v_resizable};
use gpui::prelude::FluentBuilder;

// Match the native handle's four-pixel padding around its one-pixel line.
const HANDLE_PADDING: Pixels = px(4.);

#[derive(Clone, Copy)]
enum PanelGroup {
    Sidebars,
    Diff,
    Evolution,
    FileEvolution,
}

pub(super) struct PanelLayout {
    sidebars: Entity<ResizableState>,
    diff: Entity<ResizableState>,
    evolution: Entity<ResizableState>,
    file_evolution: Entity<ResizableState>,
    sidebar_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl PanelLayout {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            sidebars: cx.new(|_| ResizableState::default()),
            diff: cx.new(|_| ResizableState::default()),
            evolution: cx.new(|_| ResizableState::default()),
            file_evolution: cx.new(|_| ResizableState::default()),
            sidebar_bounds: Rc::new(Cell::new(None)),
        }
    }
}

impl Viewer {
    pub(super) fn capture_sidebar_widths(&mut self, cx: &App) {
        let sizes = self.panels.sidebars.read(cx).sizes();
        if !self.layout.saved.history_hidden {
            self.layout.saved.history_width = sizes
                .first()
                .map(|size| f32::from(*size))
                .filter(|size| *size > 0.);
        }
        if !self.layout.saved.files_hidden {
            self.layout.saved.files_width = sizes
                .get(1)
                .map(|size| f32::from(*size))
                .filter(|size| *size > 0.);
        }
    }
    pub(super) fn rebuild_sidebar_layout(&mut self, cx: &mut App) {
        // Reinitialize native distribution from the saved widths after a pane's
        // visibility changes; otherwise showing it can shrink both sidebars.
        self.panels.sidebars = cx.new(|_| ResizableState::default());
    }

    pub(super) fn sidebar_group(&self, cx: &Context<Self>) -> ResizablePanelGroup {
        h_resizable("viewer-layout")
            .with_state(&self.panels.sidebars)
            .on_resize(self.record_panel_resize(PanelGroup::Sidebars, cx))
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::Sidebars, cx))
    }

    pub(super) fn diff_group(&self, cx: &Context<Self>) -> ResizablePanelGroup {
        h_resizable("code-panes")
            .with_state(&self.panels.diff)
            .on_resize(self.record_panel_resize(PanelGroup::Diff, cx))
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::Diff, cx))
    }

    pub(super) fn render_history_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.evolution.visible {
            return self.render_history(cx);
        }
        let settings = &self.settings.evolution;
        v_resizable("history-evolution")
            .with_state(&self.panels.evolution)
            .on_resize(self.record_panel_resize(PanelGroup::Evolution, cx))
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::Evolution, cx))
            .child(
                resizable_panel()
                    .size_range(px(settings.min_history_height)..Pixels::MAX)
                    .child(self.render_history(cx)),
            )
            .child(
                resizable_panel()
                    .size(px(self.evolution_height(false)))
                    .size_range(px(settings.min_height)..px(settings.max_height))
                    .child(self.render_evolution(evolution::EvolutionKind::Change, cx)),
            )
            .into_any_element()
    }

    pub(super) fn render_file_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.file_evolution.visible {
            return self.render_files(cx);
        }
        let settings = &self.settings.file_evolution;
        v_resizable("files-evolution")
            .with_state(&self.panels.file_evolution)
            .on_resize(self.record_panel_resize(PanelGroup::FileEvolution, cx))
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::FileEvolution, cx))
            .child(
                resizable_panel()
                    .size_range(px(settings.min_files_height)..Pixels::MAX)
                    .child(self.render_files(cx)),
            )
            .child(
                resizable_panel()
                    .size(px(self.evolution_height(true)))
                    .size_range(px(settings.min_height)..px(settings.max_height))
                    .child(self.render_evolution(evolution::EvolutionKind::File, cx)),
            )
            .into_any_element()
    }

    pub(super) fn initial_code_width(&self, before: bool) -> Pixels {
        let width = self
            .panels
            .sidebar_bounds
            .get()
            .map_or(self.settings.window.width, |bounds| {
                f32::from(bounds.size.width)
            });
        let history = if self.layout.saved.history_hidden {
            0.
        } else {
            self.sidebar_width(true)
        };
        let files = if self.layout.saved.files_hidden {
            0.
        } else {
            self.sidebar_width(false)
        };
        let available = (width - history - files - self.settings.panels.diff.gutter_width)
            .max(self.settings.panels.diff.min_code_width * 2.);
        let fraction = self.layout.saved.before_fraction.unwrap_or(0.5);
        px((available * if before { fraction } else { 1. - fraction })
            .max(self.settings.panels.diff.min_code_width))
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

    fn record_panel_resize(
        &self,
        group: PanelGroup,
        cx: &Context<Self>,
    ) -> impl Fn(&Entity<ResizableState>, &mut Window, &mut App) + 'static {
        let viewer = cx.entity().downgrade();
        move |state, window, cx| {
            let sizes = state
                .read(cx)
                .sizes()
                .iter()
                .map(|size| f32::from(*size))
                .collect::<Vec<_>>();
            let _ = viewer.update(cx, |viewer, cx| {
                match group {
                    PanelGroup::Sidebars => {
                        viewer.layout.saved.history_width =
                            sizes.first().copied().filter(|size| *size > 0.);
                        viewer.layout.saved.files_width =
                            sizes.get(1).copied().filter(|size| *size > 0.);
                    }
                    PanelGroup::Evolution => {
                        viewer.layout.saved.change_height = sizes.get(1).copied()
                    }
                    PanelGroup::FileEvolution => {
                        viewer.layout.saved.file_height = sizes.get(1).copied()
                    }
                    PanelGroup::Diff => {
                        if sizes.len() == 3 && sizes[0] + sizes[2] > 0. {
                            viewer.layout.saved.before_fraction =
                                Some((sizes[0] / (sizes[0] + sizes[2])).clamp(0.05, 0.95));
                        }
                    }
                }
                viewer.preserve_reflow_position(cx);
                viewer.save_layout(window, cx);
            });
        }
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
            let vertical = handle.axis() == Axis::Vertical;
            Some(
                div()
                    .relative()
                    .flex_none()
                    .map(|element| {
                        if vertical {
                            element.h(px(1.)).w_full()
                        } else {
                            element.w(px(1.)).h_full()
                        }
                    })
                    .child(line)
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let hit_bounds = if vertical {
                                    Bounds::from_corners(
                                        point(bounds.left(), bounds.top() - HANDLE_PADDING),
                                        point(bounds.right(), bounds.bottom() + HANDLE_PADDING),
                                    )
                                } else {
                                    Bounds::from_corners(
                                        point(bounds.left() - HANDLE_PADDING, bounds.top()),
                                        point(bounds.right() + HANDLE_PADDING, bounds.bottom()),
                                    )
                                };
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
            PanelGroup::FileEvolution => {
                let height = px(self.settings.file_evolution.default_height);
                self.panels
                    .file_evolution
                    .update(cx, |state, cx| state.resize_panel(1, height, window, cx));
            }
            PanelGroup::Evolution => {
                let height = px(self.settings.evolution.default_height);
                self.panels
                    .evolution
                    .update(cx, |state, cx| state.resize_panel(1, height, window, cx));
            }
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
        match group {
            PanelGroup::Sidebars => {
                let sizes = self.panels.sidebars.read(cx).sizes();
                self.layout.saved.history_width = sizes.first().map(|value| f32::from(*value));
                self.layout.saved.files_width = sizes.get(1).map(|value| f32::from(*value));
            }
            PanelGroup::Evolution => {
                self.layout.saved.change_height = Some(self.settings.evolution.default_height)
            }
            PanelGroup::FileEvolution => {
                self.layout.saved.file_height = Some(self.settings.file_evolution.default_height)
            }
            PanelGroup::Diff => self.layout.saved.before_fraction = None,
        }
        self.save_layout(window, cx);
        cx.notify();
    }
}
