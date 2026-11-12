use super::*;
use gpui::base::{ResizableState, ResizeHandleRenderer};
use gpui::component::Sizable;
use gpui::component::menu::DropdownMenu;
use gpui::component::resizable::{ResizablePanelGroup, resize_handle_appearance, v_resizable};
use gpui::prelude::FluentBuilder;
use jaywalk::commands::CommandId;
use jaywalk::panel_layout::{EvolutionDock, PanelId, Placement, Side, SidebarOrder};

// Match the native handle's four-pixel padding around its one-pixel line.
const HANDLE_PADDING: Pixels = px(4.);

#[derive(Clone, Copy)]
enum PanelGroup {
    Sidebars,
    Diff,
    Evolution,
    FileEvolution,
    Bottom,
    BottomSplit,
}

pub(super) struct PanelLayout {
    sidebars: Entity<ResizableState>,
    diff: Entity<ResizableState>,
    evolution: Entity<ResizableState>,
    file_evolution: Entity<ResizableState>,
    bottom: Entity<ResizableState>,
    bottom_split: Entity<ResizableState>,
    sidebar_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl PanelLayout {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            sidebars: cx.new(|_| ResizableState::default()),
            diff: cx.new(|_| ResizableState::default()),
            evolution: cx.new(|_| ResizableState::default()),
            file_evolution: cx.new(|_| ResizableState::default()),
            bottom: cx.new(|_| ResizableState::default()),
            bottom_split: cx.new(|_| ResizableState::default()),
            sidebar_bounds: Rc::new(Cell::new(None)),
        }
    }
}

impl Viewer {
    pub(super) fn placement(&self) -> &Placement {
        self.layout
            .saved
            .placement
            .as_ref()
            .unwrap_or(&self.settings.layout.placement)
    }

    fn columns(&self) -> Vec<Option<PanelId>> {
        use jaywalk::panel_layout::Column;
        self.placement()
            .columns(
                !self.layout.saved.history_hidden,
                !self.layout.saved.files_hidden,
            )
            .into_iter()
            .map(|column| match column {
                Column::Workspace => None,
                Column::History => Some(PanelId::History),
                Column::Files => Some(PanelId::Files),
            })
            .collect()
    }

    pub(super) fn capture_sidebar_widths(&mut self, cx: &App) {
        let sizes = self.panels.sidebars.read(cx).sizes();
        for (index, column) in self.columns().iter().enumerate() {
            let value = sizes
                .get(index)
                .map(|value| f32::from(*value))
                .filter(|value| *value > 0.);
            match column {
                Some(PanelId::History) if value.is_some() && !self.layout.saved.history_hidden => {
                    self.layout.saved.history_width = value
                }
                Some(PanelId::Files) if value.is_some() && !self.layout.saved.files_hidden => {
                    self.layout.saved.files_width = value
                }
                _ => {}
            }
        }
    }

    pub(super) fn render_workbench(&self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let mut group = self.sidebar_group(cx);
        let workspace_min =
            self.settings.panels.diff.min_code_width * 2. + self.settings.panels.diff.gutter_width;
        let visible = [
            (true, !self.layout.saved.history_hidden),
            (false, !self.layout.saved.files_hidden),
        ];
        let count = visible.iter().filter(|(_, show)| *show).count().max(1) as f32;
        let requested: f32 = visible
            .iter()
            .filter(|(_, show)| *show)
            .map(|(history, _)| self.sidebar_width(*history))
            .sum();
        let available = (width - workspace_min).max(0.);
        let constrained = requested > available;
        for column in self.columns() {
            group = group.child(match column {
                None => resizable_panel()
                    .size_range(px(workspace_min)..Pixels::MAX)
                    .child(self.render_workspace(cx)),
                Some(panel) => {
                    let history = panel == PanelId::History;
                    let settings = if history {
                        &self.settings.panels.history
                    } else {
                        &self.settings.panels.files
                    };
                    resizable_panel()
                        .fixed()
                        .visible(if history {
                            !self.layout.saved.history_hidden
                        } else {
                            !self.layout.saved.files_hidden
                        })
                        .size(px(self.sidebar_width(history)))
                        .size_range({
                            let max = if constrained {
                                settings.max_width.min((available / count).max(1.))
                            } else {
                                settings.max_width
                            };
                            px(settings.min_width.min(max))..px(max)
                        })
                        .child(
                            div()
                                .size_full()
                                .font(typography::panel_font(&settings.font, cx))
                                .child(if history {
                                    self.render_history_sidebar(cx)
                                } else {
                                    self.render_file_sidebar(cx)
                                }),
                        )
                }
            });
        }
        group.into_any_element()
    }

    fn bottom_visible(&self, file: bool) -> bool {
        self.supports_evolution()
            && if file {
                self.file_evolution.visible
                    && self.placement().file_evolution == EvolutionDock::Bottom
            } else {
                self.evolution.visible && self.placement().change_evolution == EvolutionDock::Bottom
            }
    }

    fn render_workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        let workspace = super::chrome::Panel::new(
            "comparison-panel",
            self.render_evolution_comparison(cx),
            // The caption reads scroll geometry published by the body this frame.
            div().with_dynamic_prepaint_order(|_, _| [1, 0].into_iter().collect()),
        )
        .child(self.render_comparison(cx));
        if !self.bottom_visible(false) && !self.bottom_visible(true) {
            return workspace.into_any_element();
        }
        let settings = &self.settings.layout;
        let width = self.initial_code_width(true)
            + self.initial_code_width(false)
            + px(self.settings.panels.diff.gutter_width);
        let fraction = self.layout.saved.bottom_fraction.unwrap_or(0.5);
        let mut timelines = h_resizable("bottom-timelines")
            .with_state(&self.panels.bottom_split)
            .on_resize(self.record_panel_resize(PanelGroup::BottomSplit, cx))
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::BottomSplit, cx));
        for (file, kind, fraction) in [
            (false, evolution::EvolutionKind::Change, fraction),
            (true, evolution::EvolutionKind::File, 1. - fraction),
        ] {
            if !self.bottom_visible(file) {
                continue;
            }
            let font = if file {
                &self.settings.panels.files.font
            } else {
                &self.settings.panels.history.font
            };
            timelines = timelines.child(
                resizable_panel()
                    .visible(self.bottom_visible(file))
                    .size(width * fraction)
                    .size_range(px(self.settings.panels.diff.min_code_width)..Pixels::MAX)
                    .child(
                        div()
                            .size_full()
                            .font(typography::panel_font(font, cx))
                            .child(self.render_evolution(kind, cx)),
                    ),
            );
        }
        v_resizable("workspace-bottom")
            .with_state(&self.panels.bottom)
            .on_resize(self.record_panel_resize(PanelGroup::Bottom, cx))
            .with_handle_appearance(self.reset_handle_appearance(PanelGroup::Bottom, cx))
            .child(
                resizable_panel()
                    .size_range(px(self.settings.panels.diff.min_code_width)..Pixels::MAX)
                    .child(workspace),
            )
            .child(
                resizable_panel()
                    .size(px(self
                        .layout
                        .saved
                        .bottom_height
                        .unwrap_or(settings.bottom_height)
                        .clamp(
                            settings.min_bottom_height,
                            settings.max_bottom_height,
                        )))
                    .size_range(px(settings.min_bottom_height)..px(settings.max_bottom_height))
                    .child(timelines),
            )
            .into_any_element()
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
        if !self.supports_evolution()
            || !self.evolution.visible
            || self.placement().change_evolution != EvolutionDock::Sidebar
        {
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
                    .fixed()
                    .size(px(self.evolution_height(false)))
                    .size_range(px(settings.min_height)..px(settings.max_height))
                    .child(self.render_evolution(evolution::EvolutionKind::Change, cx)),
            )
            .into_any_element()
    }

    pub(super) fn render_file_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.supports_evolution()
            || !self.file_evolution.visible
            || self.placement().file_evolution != EvolutionDock::Sidebar
        {
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
                    .fixed()
                    .size(px(self.evolution_height(true)))
                    .size_range(px(settings.min_height)..px(settings.max_height))
                    .child(self.render_evolution(evolution::EvolutionKind::File, cx)),
            )
            .into_any_element()
    }

    pub(super) fn right_sidebar_width(&self, cx: &App) -> f32 {
        let columns = self.columns();
        let sizes = self.panels.sidebars.read(cx).sizes();
        let workspace = columns
            .iter()
            .position(Option::is_none)
            .expect("every layout contains the workspace");
        columns
            .iter()
            .enumerate()
            .skip(workspace + 1)
            .map(|(index, panel)| {
                let history = *panel == Some(PanelId::History);
                if sizes.len() == columns.len() {
                    sizes
                        .get(index)
                        .map(|size| f32::from(*size))
                        .filter(|size| *size > 0.)
                } else {
                    None
                }
                .unwrap_or_else(|| self.sidebar_width(history))
            })
            .sum()
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
        move |state, _window, cx| {
            let sizes = state
                .read(cx)
                .sizes()
                .iter()
                .map(|size| f32::from(*size))
                .collect::<Vec<_>>();
            let _ = viewer.update(cx, |viewer, cx| {
                match group {
                    PanelGroup::Sidebars => viewer.capture_sidebar_widths(cx),
                    PanelGroup::Bottom => viewer.layout.saved.bottom_height = sizes.get(1).copied(),
                    PanelGroup::BottomSplit => {
                        if sizes.len() == 2 && sizes[0] > 0. && sizes[1] > 0. {
                            viewer.layout.saved.bottom_fraction =
                                Some((sizes[0] / (sizes[0] + sizes[1])).clamp(0.05, 0.95));
                        }
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
                viewer.save_layout(cx);
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preserve_reflow_position(cx);
        match group {
            PanelGroup::FileEvolution => {
                self.layout.saved.file_height = None;
                self.panels.file_evolution = cx.new(|_| ResizableState::default());
            }
            PanelGroup::Evolution => {
                self.layout.saved.change_height = None;
                self.panels.evolution = cx.new(|_| ResizableState::default());
            }
            PanelGroup::Sidebars => {
                let Some(bounds) = self.panels.sidebar_bounds.get() else {
                    return;
                };
                let sizes = self.panels.sidebars.read(cx).sizes();
                let columns = self.columns();
                let mut edge = bounds.left();
                let mut target = None;
                for index in 0..sizes.len().saturating_sub(1) {
                    edge += sizes[index];
                    if (divider_x - edge).abs() <= px(3.) {
                        let left = columns[index];
                        let right = columns[index + 1];
                        target = if right.is_some() && columns[..=index].contains(&None) {
                            right
                        } else {
                            left.or(right)
                        };
                        break;
                    }
                }
                self.capture_sidebar_widths(cx);
                match target {
                    Some(PanelId::History) => self.layout.saved.history_width = None,
                    Some(PanelId::Files) => self.layout.saved.files_width = None,
                    _ => return,
                }
                self.rebuild_sidebar_layout(cx);
            }
            PanelGroup::Bottom => {
                self.layout.saved.bottom_height = None;
                self.panels.bottom = cx.new(|_| ResizableState::default());
            }
            PanelGroup::BottomSplit => {
                self.layout.saved.bottom_fraction = None;
                self.panels.bottom_split = cx.new(|_| ResizableState::default());
            }
            PanelGroup::Diff => {
                self.layout.saved.before_fraction = None;
                self.panels.diff = cx.new(|_| ResizableState::default());
            }
        }
        self.save_layout(cx);
        cx.notify();
    }
}

impl Viewer {
    pub(super) fn apply_panel_settings(&mut self, previous: &Settings, cx: &mut Context<Self>) {
        // The settings value has already changed, but the measured columns
        // still belong to the old placement until the next render.
        if previous.layout.placement != self.settings.layout.placement
            && self.layout.saved.placement.is_none()
        {
            self.layout.saved.placement = Some(previous.layout.placement.clone());
        }
        if previous.panels.history != self.settings.panels.history
            || previous.panels.files != self.settings.panels.files
        {
            self.capture_sidebar_widths(cx);
            if previous.panels.history.default_width != self.settings.panels.history.default_width {
                self.layout.saved.history_width = None;
            }
            if previous.panels.files.default_width != self.settings.panels.files.default_width {
                self.layout.saved.files_width = None;
            }
            self.rebuild_sidebar_layout(cx);
        }
        if previous.layout.placement != self.settings.layout.placement {
            self.capture_sidebar_widths(cx);
            let mut placement = self.placement().clone();
            let old = &previous.layout.placement;
            let new = &self.settings.layout.placement;
            if old.history_side != new.history_side {
                placement.history_side = new.history_side;
            }
            if old.files_side != new.files_side {
                placement.files_side = new.files_side;
            }
            if old.sidebar_order != new.sidebar_order {
                placement.sidebar_order = new.sidebar_order;
            }
            if old.change_evolution != new.change_evolution {
                placement.change_evolution = new.change_evolution;
            }
            if old.file_evolution != new.file_evolution {
                placement.file_evolution = new.file_evolution;
            }
            self.layout.saved.placement = Some(placement);
            self.rebuild_sidebar_layout(cx);
        }
        if previous.layout.bottom_height != self.settings.layout.bottom_height {
            self.layout.saved.bottom_height = None;
        }
        if previous.layout != self.settings.layout {
            self.panels.bottom = cx.new(|_| ResizableState::default());
        }
        if previous.panels.diff != self.settings.panels.diff {
            self.panels.diff = cx.new(|_| ResizableState::default());
        }
        if previous.evolution.default_height != self.settings.evolution.default_height {
            self.layout.saved.change_height = None;
        }
        if previous.file_evolution.default_height != self.settings.file_evolution.default_height {
            self.layout.saved.file_height = None;
        }
        if previous.evolution != self.settings.evolution {
            self.panels.evolution = cx.new(|_| ResizableState::default());
        }
        if previous.file_evolution != self.settings.file_evolution {
            self.panels.file_evolution = cx.new(|_| ResizableState::default());
        }
    }
}

impl Viewer {
    pub(super) fn move_panel(&mut self, command: CommandId, cx: &mut Context<Self>) {
        self.preserve_reflow_position(cx);
        self.capture_sidebar_widths(cx);
        let mut placement = self.placement().clone();
        match command {
            CommandId::HistoryLeft => placement.history_side = Side::Left,
            CommandId::HistoryRight => placement.history_side = Side::Right,
            CommandId::FilesLeft => placement.files_side = Side::Left,
            CommandId::FilesRight => placement.files_side = Side::Right,
            CommandId::SwapSidebars => {
                placement.sidebar_order = match placement.sidebar_order {
                    SidebarOrder::HistoryFirst => SidebarOrder::FilesFirst,
                    SidebarOrder::FilesFirst => SidebarOrder::HistoryFirst,
                }
            }
            CommandId::ChangeEvolutionSidebar => {
                placement.change_evolution = EvolutionDock::Sidebar
            }
            CommandId::ChangeEvolutionBottom => placement.change_evolution = EvolutionDock::Bottom,
            CommandId::FileEvolutionSidebar => placement.file_evolution = EvolutionDock::Sidebar,
            CommandId::FileEvolutionBottom => placement.file_evolution = EvolutionDock::Bottom,
            _ => return,
        }
        self.layout.saved.placement = Some(placement);
        self.rebuild_sidebar_layout(cx);
        self.panels.bottom = cx.new(|_| ResizableState::default());
        self.panels.bottom_split = cx.new(|_| ResizableState::default());
        self.save_layout(cx);
        cx.notify();
    }

    pub(super) fn panel_menu_entries(&self, panel: PanelId) -> Vec<(CommandId, bool)> {
        let placement = self.placement();
        let entries: Vec<_> = match panel {
            PanelId::History => vec![
                (CommandId::HistoryLeft, placement.history_side == Side::Left),
                (
                    CommandId::HistoryRight,
                    placement.history_side == Side::Right,
                ),
                (CommandId::SwapSidebars, false),
                (CommandId::ChooseReview, false),
                (CommandId::AllChanges, false),
                (CommandId::CurrentWork, false),
                (CommandId::ShowReferences, self.references.open),
                (CommandId::ToggleReferenceLabels, self.references.labels),
                (CommandId::ShowAllRemotes, false),
                (CommandId::HideAllRemotes, false),
                (CommandId::RevealSelectedRevision, false),
                (CommandId::Refresh, false),
            ],
            PanelId::Files => vec![
                (CommandId::FilesLeft, placement.files_side == Side::Left),
                (CommandId::FilesRight, placement.files_side == Side::Right),
                (CommandId::SwapSidebars, false),
                (
                    CommandId::FileList,
                    self.browser.mode == jaywalk::settings::FileView::List,
                ),
                (
                    CommandId::FileTree,
                    self.browser.mode == jaywalk::settings::FileView::Tree,
                ),
                (
                    CommandId::TreeChanged,
                    self.browser.scope == jaywalk::settings::TreeScope::Changed,
                ),
                (
                    CommandId::TreeAll,
                    self.browser.scope == jaywalk::settings::TreeScope::All,
                ),
                (CommandId::RevealFile, false),
                (CommandId::CollapseFiles, false),
                (CommandId::ExpandFiles, false),
            ],
            PanelId::ChangeEvolution => vec![
                (
                    CommandId::ChangeEvolutionSidebar,
                    placement.change_evolution == EvolutionDock::Sidebar,
                ),
                (
                    CommandId::ChangeEvolutionBottom,
                    placement.change_evolution == EvolutionDock::Bottom,
                ),
            ],
            PanelId::FileEvolution => vec![
                (
                    CommandId::FileEvolutionSidebar,
                    placement.file_evolution == EvolutionDock::Sidebar,
                ),
                (
                    CommandId::FileEvolutionBottom,
                    placement.file_evolution == EvolutionDock::Bottom,
                ),
            ],
        };
        let hide = match panel {
            PanelId::History => CommandId::ToggleHistory,
            PanelId::Files => CommandId::ToggleFiles,
            PanelId::ChangeEvolution => CommandId::ToggleChangeEvolution,
            PanelId::FileEvolution => CommandId::ToggleFileEvolution,
        };
        entries.into_iter().chain([(hide, false)]).collect()
    }

    pub(super) fn panel_menu(&self, panel: PanelId) -> AnyElement {
        let entries = self.panel_menu_entries(panel);
        let focus = self.focus.clone();
        Button::new("panel-placement")
            .ghost()
            .small()
            .compact()
            .label("⋯")
            .tooltip("Panel actions")
            .dropdown_menu(move |menu, _, _| {
                context_menu::command_entries(menu.action_context(focus.clone()), &entries)
            })
            .into_any_element()
    }
}
