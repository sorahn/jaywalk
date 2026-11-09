use super::*;
use gpui::component::Sizable;
use gpui::component::tooltip::Tooltip;
pub(super) use jaywalk::evolution::ReviewMode;
use jaywalk::evolution::grouping::{GroupedEvolutionView, TimelineRow};
use jaywalk::evolution::{Evolution, EvolutionView};
use jaywalk::history::Revision;
use std::collections::HashSet;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct HeaderPresentation {
    label: String,
    controls: Option<HeaderControls>,
}

#[derive(Clone, PartialEq, Eq)]
struct HeaderControls {
    kind: EvolutionKind,
    index: usize,
    version: String,
    mode: ReviewMode,
    has_predecessors: bool,
}

impl HeaderPresentation {
    fn label(label: String) -> Self {
        Self {
            label,
            controls: None,
        }
    }
    fn pending(previous: Option<&Self>, show_loading: bool) -> Self {
        if show_loading {
            Self::label("Loading evolution…".into())
        } else {
            previous
                .cloned()
                .unwrap_or_else(|| Self::label(String::new()))
        }
    }
}

#[derive(Clone)]
pub(super) struct EvolutionSession {
    operation: String,
    anchor: Revision,
    timeline: Rc<Evolution>,
    view: Rc<EvolutionView>,
    presentation: Rc<GroupedEvolutionView>,
    selected: usize,
    mode: ReviewMode,
}

#[derive(Clone)]
struct TimelinePresentation {
    session: EvolutionSession,
    file_path: Option<String>,
    scroll: UniformListScrollHandle,
}

enum TimelineState {
    Unselected,
    Loading {
        anchor: String,
        previous: Option<TimelinePresentation>,
    },
    Failed(String),
    Loaded(EvolutionSession),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EvolutionKind {
    Change,
    File,
}

impl EvolutionKind {
    fn id(self) -> usize {
        match self {
            Self::Change => 0,
            Self::File => 1,
        }
    }
}

pub(super) struct EvolutionPanel {
    pub(super) visible: bool,
    pub(super) show_unchanged_rebases: bool,
    file_path: Option<String>,
    pub(super) collapse_repeats: bool,
    expanded_groups: HashSet<String>,
    state: TimelineState,
    generation: u64,
    task: Option<Task<()>>,
    scroll: UniformListScrollHandle,
}

impl EvolutionPanel {
    pub(super) fn new(visible: bool, show_unchanged_rebases: bool, collapse_repeats: bool) -> Self {
        Self {
            visible,
            show_unchanged_rebases,
            file_path: None,
            collapse_repeats,
            expanded_groups: HashSet::new(),
            state: TimelineState::Unselected,
            generation: 0,
            task: None,
            scroll: UniformListScrollHandle::new(),
        }
    }

    fn presentation(&self) -> Option<TimelinePresentation> {
        match &self.state {
            TimelineState::Loaded(session) => Some(TimelinePresentation {
                session: session.clone(),
                file_path: self.file_path.clone(),
                scroll: self.scroll.clone(),
            }),
            TimelineState::Loading { previous, .. } => previous.clone(),
            _ => None,
        }
    }

    pub(super) fn rebuild(&mut self) {
        if let TimelineState::Loaded(session) = &mut self.state {
            session.view = Rc::new(
                session
                    .timeline
                    .view(self.show_unchanged_rebases, session.selected),
            );
            session.presentation = Rc::new(session.view.grouped(
                &session.timeline,
                self.file_path.as_deref(),
                session.mode,
                self.collapse_repeats,
                &self.expanded_groups,
            ));
        }
    }

    pub(super) fn clear(&mut self) {
        self.generation += 1;
        self.task = None;
        self.state = TimelineState::Unselected;
        self.file_path = None;
        self.expanded_groups.clear();
        self.scroll = UniformListScrollHandle::new();
    }
}

impl Viewer {
    fn evolution_panel(&self, kind: EvolutionKind) -> &EvolutionPanel {
        match kind {
            EvolutionKind::Change => &self.evolution,
            EvolutionKind::File => &self.file_evolution,
        }
    }

    fn evolution_panel_mut(&mut self, kind: EvolutionKind) -> &mut EvolutionPanel {
        match kind {
            EvolutionKind::Change => &mut self.evolution,
            EvolutionKind::File => &mut self.file_evolution,
        }
    }

    pub(super) fn focus_evolution_file(&mut self, path: Option<&str>) {
        if self.file_evolution.file_path.as_deref() != path {
            self.file_evolution.expanded_groups.clear();
            self.file_evolution.file_path = path.map(str::to_owned);
        }
        self.active_evolution = if path.is_some() {
            EvolutionKind::File
        } else {
            EvolutionKind::Change
        };
        self.sync_file_evolution();
        // File clicks review the comparison currently on screen. The change
        // panel keeps its own selection while the file projection follows it.
        let displayed = self
            .repository
            .ready()
            .and_then(|session| match &session.scope {
                ComparisonScope::Revision { commit_id, .. } => {
                    Some((commit_id.clone(), ReviewMode::Patch))
                }
                ComparisonScope::Evolution { version, mode, .. } => Some((version.clone(), *mode)),
                ComparisonScope::WholeBranch => None,
            });
        if let Some((commit, mode)) = displayed
            && let TimelineState::Loaded(file) = &mut self.file_evolution.state
            && let Some(selected) = file
                .timeline
                .versions
                .iter()
                .position(|version| version.commit_id == commit)
        {
            file.selected = selected;
            file.mode = mode;
            self.file_evolution.rebuild();
        }
    }

    fn sync_file_evolution(&mut self) {
        self.file_evolution.state = if self.file_evolution.file_path.is_none() {
            TimelineState::Unselected
        } else {
            match &self.evolution.state {
                TimelineState::Loaded(shared) => {
                    // Preserve the file timeline's selection when only its subject changes.
                    let mut session = shared.clone();
                    if let TimelineState::Loaded(previous) = &self.file_evolution.state
                        && Rc::ptr_eq(&previous.timeline, &shared.timeline)
                    {
                        session.selected = previous.selected;
                        session.mode = previous.mode;
                    }
                    TimelineState::Loaded(session)
                }
                TimelineState::Loading { anchor, .. } => TimelineState::Loading {
                    anchor: anchor.clone(),
                    previous: self.file_evolution.presentation(),
                },
                TimelineState::Failed(error) => TimelineState::Failed(error.clone()),
                TimelineState::Unselected => TimelineState::Unselected,
            }
        };
        self.file_evolution.rebuild();
    }

    fn toggle_repeat_grouping(&mut self, kind: EvolutionKind, cx: &mut Context<Self>) {
        let panel = self.evolution_panel_mut(kind);
        panel.collapse_repeats = !panel.collapse_repeats;
        panel.expanded_groups.clear();
        panel.rebuild();
        panel.scroll = UniformListScrollHandle::new();
        cx.notify();
        self.save_layout(cx);
    }

    fn toggle_repeated_group(&mut self, kind: EvolutionKind, head: &str, cx: &mut Context<Self>) {
        let panel = self.evolution_panel_mut(kind);
        if !matches!(panel.state, TimelineState::Loaded(_)) {
            return;
        }
        if !panel.expanded_groups.insert(head.to_owned()) {
            panel.expanded_groups.remove(head);
        }
        panel.rebuild();
        cx.notify();
    }

    fn toggle_unchanged_rebases(&mut self, kind: EvolutionKind, cx: &mut Context<Self>) {
        let panel = self.evolution_panel_mut(kind);
        panel.show_unchanged_rebases = !panel.show_unchanged_rebases;
        panel.expanded_groups.clear();
        panel.rebuild();
        panel.scroll = UniformListScrollHandle::new();
        cx.notify();
        self.save_layout(cx);
    }

    pub(super) fn toggle_evolution(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_evolution_panel(EvolutionKind::Change, window, cx);
    }

    pub(super) fn toggle_evolution_panel(
        &mut self,
        kind: EvolutionKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.supports_evolution() {
            return;
        }
        self.remember_evolution_header();
        let panel = self.evolution_panel_mut(kind);
        panel.visible = !panel.visible;
        if panel.visible && matches!(self.evolution.state, TimelineState::Unselected) {
            let revision = self.repository.ready().and_then(|session| {
                let commit = match &session.scope {
                    ComparisonScope::Revision { commit_id, .. } => commit_id,
                    ComparisonScope::Evolution { anchor, .. } => anchor,
                    ComparisonScope::WholeBranch => return None,
                };
                session
                    .history
                    .as_ref()
                    .ok()?
                    .revisions
                    .iter()
                    .find(|revision| &revision.commit_id == commit)
                    .cloned()
            });
            if let Some(revision) = revision {
                self.load_evolution(revision, window, cx);
            }
        }
        self.sync_file_evolution();
        self.save_layout(cx);
        cx.notify();
    }

    pub(super) fn apply_evolution_settings(
        &mut self,
        previous: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for panel in [&mut self.evolution, &mut self.file_evolution] {
            if previous.evolution.show_unchanged_rebases
                != self.settings.evolution.show_unchanged_rebases
            {
                panel.show_unchanged_rebases = self.settings.evolution.show_unchanged_rebases;
            }
            if previous.evolution.collapse_repeats != self.settings.evolution.collapse_repeats {
                panel.collapse_repeats = self.settings.evolution.collapse_repeats;
                panel.expanded_groups.clear();
            }
            panel.rebuild();
        }
        for (kind, old, new) in [
            (
                EvolutionKind::Change,
                previous.evolution.visible,
                self.settings.evolution.visible,
            ),
            (
                EvolutionKind::File,
                previous.file_evolution.visible,
                self.settings.file_evolution.visible,
            ),
        ] {
            if old != new && self.evolution_panel(kind).visible != new {
                self.toggle_evolution_panel(kind, window, cx);
            }
        }
        if previous.evolution.limit != self.settings.evolution.limit {
            self.reload_evolution_limit(window, cx);
        }
    }

    fn reload_evolution_limit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let TimelineState::Loaded(session) = &self.evolution.state else {
            // An initial load checks the current limit on completion below.
            return;
        };
        let commit = session.anchor.commit_id.clone();
        let operation = session.operation.clone();
        let Some(repository) = self.repository.ready() else {
            return;
        };
        let branch = repository.branch.as_ref().clone();
        let limit = self.settings.evolution.limit;
        self.evolution.generation += 1;
        let generation = self.evolution.generation;
        self.begin_loading(LoadingKind::Evolution, window, cx);
        self.evolution.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { branch.evolution_at(&commit, limit, &operation) })
                .await;
            let _ = this.update_in(cx, |viewer, _, cx| {
                if viewer.evolution.generation != generation {
                    return;
                }
                viewer.finish_loading(LoadingKind::Evolution);
                match result {
                    Ok(mut timeline) => {
                        // A smaller limit must not discard either dock's selected
                        // revision or replace the pinned comparison being reviewed.
                        for panel in [&viewer.evolution, &viewer.file_evolution] {
                            if let TimelineState::Loaded(session) = &panel.state {
                                let selected = &session.timeline.versions[session.selected];
                                timeline.retain_selected(selected);
                            }
                        }
                        let timeline = Rc::new(timeline);
                        for panel in [&mut viewer.evolution, &mut viewer.file_evolution] {
                            if let TimelineState::Loaded(session) = &mut panel.state {
                                let selected =
                                    &session.timeline.versions[session.selected].commit_id;
                                session.selected = timeline
                                    .versions
                                    .iter()
                                    .position(|version| &version.commit_id == selected)
                                    .expect("the reloaded timeline retains the selected version");
                                session.timeline = timeline.clone();
                                panel.rebuild();
                            }
                        }
                    }
                    Err(error) => {
                        viewer.live.message = Some(format!(
                            "Evolution reload failed; keeping the previous timeline: {error}"
                        ))
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn load_evolution(
        &mut self,
        anchor: Revision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.supports_evolution() {
            return;
        }
        // File focus and evolution-version selection are projections of this
        // same recorded timeline. Returning to its graph row needs no jj read.
        if let TimelineState::Loaded(session) = &mut self.evolution.state
            && session.anchor.commit_id == anchor.commit_id
        {
            session.selected = session
                .timeline
                .versions
                .iter()
                .position(|version| version.commit_id == anchor.commit_id)
                .expect("a loaded timeline contains its selected anchor");
            session.mode = ReviewMode::Patch;
            self.evolution.expanded_groups.clear();
            self.evolution.rebuild();
            cx.notify();
            return;
        }
        if matches!(&self.evolution.state, TimelineState::Loading { anchor: loading, .. } if loading == &anchor.commit_id)
        {
            cx.notify();
            return;
        }
        let previous = self.evolution.presentation();
        self.evolution.clear();
        self.finish_loading(LoadingKind::Evolution);
        if !self.evolution.visible && !self.file_evolution.visible {
            return;
        }
        let Some(session) = self.repository.ready() else {
            return;
        };
        let branch = session.branch.as_ref().clone();
        let operation = session
            .history
            .as_ref()
            .ok()
            .and_then(|history| history.references.operation.clone())
            .unwrap_or_else(|| branch.operation.clone());
        let generation = self.evolution.generation;
        let limit = self.settings.evolution.limit;
        self.evolution.state = TimelineState::Loading {
            anchor: anchor.commit_id.clone(),
            previous,
        };
        self.begin_loading(LoadingKind::Evolution, window, cx);
        self.evolution.task = Some(cx.spawn_in(window, async move |this, cx| {
            let commit = anchor.commit_id.clone();
            let read_operation = operation.clone();
            let result = cx
                .background_spawn(async move { branch.evolution_at(&commit, limit, &read_operation) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.evolution.generation != generation {
                    return;
                }
                viewer.finish_loading(LoadingKind::Evolution);
                viewer.evolution.state = match result {
                    Ok(timeline) if !timeline.versions.is_empty() => {
                        match timeline.versions.iter().position(|version| version.commit_id == anchor.commit_id) {
                            Some(selected) => {
                                let view = Rc::new(timeline.view(viewer.evolution.show_unchanged_rebases, selected));
                                let presentation = Rc::new(view.grouped(&timeline, viewer.evolution.file_path.as_deref(), ReviewMode::Patch, viewer.evolution.collapse_repeats, &viewer.evolution.expanded_groups));
                                TimelineState::Loaded(EvolutionSession { operation, anchor, timeline: Rc::new(timeline), view, presentation, selected, mode: ReviewMode::Patch })
                            },
                            None => TimelineState::Failed("Selected version is outside the evolution limit; increase evolution.limit.".into()),
                        }
                    }
                    Ok(_) => TimelineState::Failed(
                        "No recorded evolution is available for this revision.".into(),
                    ),
                    Err(error) => TimelineState::Failed(error),
                };
                viewer.sync_file_evolution();
                if limit != viewer.settings.evolution.limit {
                    viewer.reload_evolution_limit(window, cx);
                }
                cx.notify();
            });
        }));
        self.sync_file_evolution();
        cx.notify();
    }

    pub(super) fn select_evolution_mode(
        &mut self,
        mode: ReviewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = self.active_evolution;
        if let TimelineState::Loaded(session) = &self.evolution_panel(kind).state {
            self.select_evolution_version(kind, session.selected, mode, window, cx);
        }
    }

    fn select_evolution_version(
        &mut self,
        kind: EvolutionKind,
        index: usize,
        mode: ReviewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.remember_evolution_header();
        self.active_evolution = kind;
        let panel = self.evolution_panel_mut(kind);
        let TimelineState::Loaded(evolution) = &mut panel.state else {
            return;
        };
        let Some(version) = evolution.timeline.versions.get(index).cloned() else {
            return;
        };
        let mode = if version.predecessors.is_empty() {
            ReviewMode::Patch
        } else {
            mode
        };
        evolution.selected = index;
        if evolution.mode != mode {
            panel.expanded_groups.clear();
        }
        evolution.mode = mode;
        let label = match mode {
            ReviewMode::Patch => format!(
                "Change · {} · version patch{}",
                version.timestamp,
                if version.parents.len() > 1 {
                    " · first parent"
                } else {
                    ""
                }
            ),
            ReviewMode::SincePrevious => format!(
                "Evolution · {} · {} → selected",
                version.timestamp,
                if version.predecessors.len() > 1 {
                    "combined predecessors"
                } else {
                    "previous"
                }
            ),
        };
        let operation = evolution.operation.clone();
        let scope = ComparisonScope::Evolution {
            anchor: evolution.anchor.commit_id.clone(),
            version: version.commit_id.clone(),
            label,
            mode,
        };
        panel.rebuild();
        let Some(session) = self.repository.ready() else {
            return;
        };
        let branch = session.branch.as_ref().clone();
        let preferred_path = self
            .selected_path()
            .or_else(|| self.file_evolution.file_path.clone());
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.remember_reading(cx);
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = scope;
            session.comparison = LoadState::Loading;
        }
        self.begin_loading(LoadingKind::Comparison, window, cx);
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    match mode {
                        ReviewMode::Patch => {
                            branch.for_history_revision(&version.revision(), Some(&operation))
                        }
                        ReviewMode::SincePrevious => branch.for_evolution_at(&version, &operation),
                    }
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
                viewer.finish_loading(LoadingKind::Comparison);
                if let LoadState::Loaded(session) = &mut viewer.repository {
                    session.comparison = match result {
                        Ok(comparison) => LoadState::Loaded(Rc::new(comparison)),
                        Err(error) => LoadState::Failed(error),
                    };
                }
                viewer.rebuild_browser(cx);
                let file_selection = match &viewer.file_evolution.state {
                    TimelineState::Loaded(session) => Some((session.selected, session.mode)),
                    _ => None,
                };
                if viewer.selected_file.is_some() {
                    viewer.follow_open_file(window, cx);
                } else if let Some(path) = preferred_path {
                    viewer.select_path(&path, window, cx);
                }
                if kind == EvolutionKind::Change
                    && let Some((selected, mode)) = file_selection
                    && let TimelineState::Loaded(session) = &mut viewer.file_evolution.state
                {
                    session.selected = selected;
                    session.mode = mode;
                    viewer.file_evolution.rebuild();
                }
                viewer.active_evolution = kind;
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn remember_evolution_header(&mut self) {
        // Keep the last coherent presentation through rapid replacement requests;
        // transient loading feedback must never become the next placeholder.
        if !matches!(
            self.evolution_panel(self.active_evolution).state,
            TimelineState::Loading { .. }
        ) {
            self.evolution_header = Some(self.evolution_header_presentation());
        }
    }

    fn evolution_header_presentation(&self) -> HeaderPresentation {
        if !matches!(
            self.repository.ready().map(|session| &session.scope),
            Some(ComparisonScope::Revision { .. } | ComparisonScope::Evolution { .. })
        ) {
            return HeaderPresentation::label(self.scope_label());
        }
        let kind = self.active_evolution;
        match &self.evolution_panel(kind).state {
            TimelineState::Loading { .. } => HeaderPresentation::pending(
                self.evolution_header.as_ref(),
                self.loading_visible(LoadingKind::Evolution),
            ),
            TimelineState::Loaded(evolution) => {
                let version = &evolution.timeline.versions[evolution.selected];
                HeaderPresentation {
                    label: format!(
                        "{} · {} · {}",
                        match kind {
                            EvolutionKind::Change => "Change evolution",
                            EvolutionKind::File => "File evolution",
                        },
                        version.timestamp,
                        if version.hidden {
                            "historical version"
                        } else if version.divergent {
                            "current version"
                        } else {
                            "latest"
                        }
                    ),
                    controls: Some(HeaderControls {
                        kind,
                        index: evolution.selected,
                        version: version.commit_id.clone(),
                        mode: evolution.mode,
                        has_predecessors: !version.predecessors.is_empty(),
                    }),
                }
            }
            _ => HeaderPresentation::label(self.scope_label()),
        }
    }

    pub(super) fn render_evolution_comparison(&self, cx: &mut Context<Self>) -> AnyElement {
        let presentation = self.evolution_header_presentation();
        let mut bar = div()
            .h_flex()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .h(px(self
                .settings
                .ui
                .comparison_bar_height
                .max(self.settings.ui.font_size * 1.75 + 8.)))
            .flex_shrink_0()
            .overflow_hidden()
            .bg(gpui::component::Theme::global(cx).sidebar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .truncate()
                    .child(self.overview_comparison_label(presentation.label)),
            );
        if let Some(controls) = presentation.controls {
            let kind = controls.kind;
            let index = controls.index;
            let mut modes = div().h_flex().items_center().gap_1();
            for (id, mode, label, tooltip) in [
                (
                    "evolution-patch",
                    ReviewMode::Patch,
                    "Version patch",
                    "The complete patch at this version (first parent for merges)",
                ),
                (
                    "evolution-interdiff",
                    ReviewMode::SincePrevious,
                    "Since previous",
                    "Compare to predecessor(s), accounting for rebases",
                ),
            ] {
                let version = controls.version.clone();
                modes = modes.child(
                    Button::new(id)
                        .ghost().small()
                        .compact()
                        .label(label)
                        .tooltip(tooltip)
                        .selected(controls.mode == mode)
                        .disabled(mode == ReviewMode::SincePrevious && !controls.has_predecessors)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            // Frozen controls belong to the previous timeline. Keep
                            // their appearance stable without dispatching old indices.
                            if matches!(&this.evolution_panel(kind).state,
                                TimelineState::Loaded(session) if session.timeline.versions.get(index).is_some_and(|current| current.commit_id == version))
                            {
                                this.select_evolution_version(kind, index, mode, window, cx);
                            }
                        })),
                );
            }
            bar = bar.child(modes);
        }
        bar.into_any_element()
    }

    pub(super) fn render_evolution(
        &self,
        kind: EvolutionKind,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let font_size = match kind {
            EvolutionKind::Change => self.settings.panels.history.font.size,
            EvolutionKind::File => self.settings.panels.files.font.size,
        };
        let state = self.evolution_panel(kind);
        let theme = gpui::component::Theme::global(cx);
        let surface = theme.sidebar;
        let selection = theme.list_active;
        let hover = theme.secondary_hover;
        let previous_surface = theme.secondary;
        let mut panel = div()
            .id(("evolution-panel", kind.id()))
            .v_flex().size_full().bg(surface)
            .child(div().v_flex()
                .child(self.panel_header(font_size, cx)
                    .child(div().flex_1().min_w_0().truncate().text_size(px(font_size * 0.85)).child(match kind {
                        EvolutionKind::Change => "CHANGE EVOLUTION",
                        EvolutionKind::File => "FILE EVOLUTION",
                    }))
                    .child(self.panel_menu(match kind {
                        EvolutionKind::Change => jaywalk::panel_layout::PanelId::ChangeEvolution,
                        EvolutionKind::File => jaywalk::panel_layout::PanelId::FileEvolution,
                    }))
                    .child(Button::new("close-evolution").ghost().small().compact().icon(IconName::X)
                        .tooltip("Hide evolution")
                        .on_click(cx.listener(move |this, _, window, cx| this.toggle_evolution_panel(kind, window, cx)))))
                .child(div().h_flex().gap_1().px_3().py_1()
                    .child(Button::new("group-repeats").ghost().small().compact().label("Group")
                        .selected(state.collapse_repeats)
                        .tooltip("Group consecutive matching entries")
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_repeat_grouping(kind, cx))))
                    .child(Button::new("unchanged-rebases").ghost().small().compact().label("Rebases")
                        .selected(state.show_unchanged_rebases)
                        .tooltip("Show unchanged rebases (current and selected versions stay visible)")
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_unchanged_rebases(kind, cx))))));
        let (evolution, file_path, scroll) = match &state.state {
            TimelineState::Unselected => {
                return panel
                    .child(div().p_3().text_size(px(font_size)).child(match kind {
                        EvolutionKind::Change => {
                            "Select a change in history to explore its versions."
                        }
                        EvolutionKind::File => {
                            "Select a change in history, then a file, to explore its versions."
                        }
                    }))
                    .into_any_element();
            }
            TimelineState::Loading {
                previous: Some(previous),
                ..
            } => (
                &previous.session,
                previous.file_path.clone(),
                &previous.scroll,
            ),
            TimelineState::Loading { previous: None, .. } => {
                return panel
                    .child(div().p_3().text_size(px(font_size)).child(
                        if self.loading_visible(LoadingKind::Evolution) {
                            "Loading recorded versions…"
                        } else {
                            ""
                        },
                    ))
                    .into_any_element();
            }
            TimelineState::Failed(error) => {
                return panel
                    .child(
                        div()
                            .p_3()
                            .text_size(px(font_size))
                            .child(feedback::error_message(
                                if kind == evolution::EvolutionKind::Change {
                                    "change-evolution-error"
                                } else {
                                    "file-evolution-error"
                                },
                                error,
                            )),
                    )
                    .into_any_element();
            }
            TimelineState::Loaded(evolution) => (evolution, state.file_path.clone(), &state.scroll),
        };
        let selected = evolution.selected;
        let mode = evolution.mode;
        if let Some(path) = &file_path {
            let name = path
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            let context = format!(
                "File · {name} · {}",
                if mode == ReviewMode::Patch {
                    "version patch"
                } else {
                    "since previous"
                }
            );
            let tooltip = path.clone();
            panel = panel.child(
                div()
                    .id("evolution-file-context")
                    .px_3()
                    .pb_1()
                    .text_size(px(font_size * 0.85))
                    .text_color(rgb(0xb8c6d9))
                    .truncate()
                    .child(context)
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)),
            );
        } else {
            panel = panel.child(
                div()
                    .px_3()
                    .pb_1()
                    .text_size(px(font_size * 0.85))
                    .text_color(rgb(0xb8c6d9))
                    .child("Timeline · changes since previous"),
            );
        }
        let version = &evolution.timeline.versions[selected];
        let timeline = evolution.timeline.clone();
        let view = evolution.view.clone();
        let presentation = evolution.presentation.clone();
        let count = view.indices.len();
        let group_count = presentation
            .rows
            .iter()
            .filter(|row| {
                matches!(
                    row,
                    TimelineRow::Collapsed(_) | TimelineRow::ExpandedHead(_)
                )
            })
            .count();
        let hidden_count = timeline.versions.len() - count;
        let compared_predecessors = if mode == ReviewMode::SincePrevious {
            view.graph
                .revisions
                .iter()
                .find(|revision| revision.commit_id == version.commit_id)
                .expect("the selected version is retained by the evolution filter")
                .parents
                .clone()
        } else {
            Vec::new()
        };
        let actual_predecessors = version.predecessors.clone();
        let viewer = cx.entity().downgrade();
        let height = self.settings.evolution.row_height.max(font_size * 2.5);
        let spacing = self.settings.history.lane_spacing;
        let grouping_enabled = state.collapse_repeats;
        let rows = uniform_list(("evolution-rows", kind.id()), presentation.rows.len(), move |visible, _, _| {
            let width = 20. + (presentation.graph.visible_lane_count(visible.clone()) - 1) as f32 * spacing;
            visible
                .map(|row_index| {
                    let row = &presentation.rows[row_index];
                    let index = row.representative();
                    let row_selected = row.contains_version(selected);
                    let collapsed = matches!(row, TimelineRow::Collapsed(_));
                    let group_control = matches!(row, TimelineRow::Collapsed(_) | TimelineRow::ExpandedHead(_));
                    let member_count = row.members().len();
                    let group_head = timeline.versions[index].commit_id.clone();
                    let version = &timeline.versions[index];
                    let event = timeline.event(index);
                    let file_effect = file_path
                        .as_deref()
                        .map(|path| version.file_effect(path, mode));
                    let patch_changed = file_effect
                        .map_or(version.patch_changed, |effect| effect.changes_contents());
                    let previous = !row_selected && compared_predecessors.iter().any(|id| timeline.versions.iter().position(|version| &version.commit_id == id).is_some_and(|index| row.contains_version(index)));
                    let mut tooltip = format!(
                        "{}\n{}\n{}\noperation {}\n{}",
                        version.description.trim(),
                        version.timestamp,
                        version.commit_id,
                        version.operation_id.as_deref().map(|id| &id[..id.len().min(12)]).unwrap_or("unavailable"),
                        version.operation
                    );
                    tooltip.push_str(&format!("\nChange event: {}", event.label()));
                    if let (Some(path), Some(effect)) = (&file_path, file_effect) {
                        tooltip.push_str(&format!("\nFile: {path}\n{}", effect.label()));
                    }
                    let stamp = if version.operation_id.is_none() {
                        version.timestamp.as_str()
                    } else {
                        version.timestamp.split_once(' ').map_or(version.timestamp.as_str(), |(_, time)| time)
                    };
                    let summary = file_effect.map_or_else(
                        || version.rewrite_summary(event),
                        |effect| effect.label().to_owned(),
                    );
                    if collapsed {
                        let oldest = &timeline.versions[*row.members().last().expect("a group has at least two versions")];
                        tooltip.push_str(&format!("\n{member_count} versions: {} to {}\nClick to view the newest version; use the chevron to expand.", oldest.timestamp, version.timestamp));
                    }
                    let title = format!(
                        "{}{}",
                        if collapsed { "" } else if previous && actual_predecessors.contains(&version.commit_id) {
                            "previous · "
                        } else if previous {
                            "previous shown · "
                        } else if !version.hidden && version.divergent {
                            "current · "
                        } else if !version.hidden
                            && version.change_id == timeline.versions[selected].change_id
                        {
                            "latest · "
                        } else {
                            ""
                        },
                        summary
                    );
                    let detail = if collapsed {
                        format!("{member_count} versions{}", if row_selected { " · includes selection" } else { "" })
                    } else { format!(
                        "{} · {}",
                        stamp,
                        if file_effect.is_some() {
                            event.label()
                        } else {
                            version.operation.as_str()
                        }
                    ) };
                    let graph = presentation.clone();
                    let viewer = viewer.clone();
                    div()
                        .id(("evolution-version", index))
                        .h(px(height))
                        .w_full()
                        .h_flex()
                        .items_center()
                        .cursor_pointer()
                        .bg(if row_selected { selection } else if previous { previous_surface } else { surface })
                        .hover(move |style| style.bg(hover))
                        .child(
                            canvas(
                                |_, _, _| (),
                                move |bounds, _, window, _| {
                                    let row = &graph.graph.graph[row_index];
                                    let x = |lane| bounds.left() + px(10. + lane as f32 * spacing);
                                    let middle = bounds.center().y;
                                    let color = rgb(if row_selected || previous {
                                        0x9dcaff
                                    } else {
                                        0x7797bd
                                    });
                                    for stroke in &row.strokes {
                                        super::history::paint_graph_stroke(stroke,bounds,spacing,color.into(),1.,window);
                                    }
                                    let mut marker = PathBuilder::stroke(px(1.));
                                    let center = x(row.lane);
                                    if !patch_changed {
                                        marker.move_to(point(center, middle - px(4.)));
                                        marker.line_to(point(center + px(4.), middle));
                                        marker.line_to(point(center, middle + px(4.)));
                                        marker.line_to(point(center - px(4.), middle));
                                        marker.close();
                                        if let Ok(path) = marker.build() {
                                            window.paint_path(path, color);
                                        }
                                    } else {
                                        window.paint_quad(
                                            fill(
                                                Bounds::new(
                                                    point(center - px(3.5), middle - px(3.5)),
                                                    size(px(7.), px(7.)),
                                                ),
                                                color,
                                            )
                                            .corner_radii(px(3.5)),
                                        );
                                    }
                                },
                            )
                            .w(px(width))
                            .h_full()
                            .flex_shrink_0(),
                        )
                        .child({
                            let mut slot = if grouping_enabled { div().w_6() } else { div().w_0() }.flex_shrink_0();
                            if group_control {
                                let head = group_head.clone();
                                let control_viewer = viewer.clone();
                                slot = slot.child(Button::new(("toggle-repeat-group", index)).ghost().small().compact()
                                    .icon(if collapsed { IconName::ChevronRight } else { IconName::ChevronDown })
                                    .tooltip(if collapsed { "Expand versions" } else { "Collapse versions" })
                                    .on_click(move |_, _, cx| {
                                        let _ = control_viewer.update(cx, |viewer, cx| viewer.toggle_repeated_group(kind, &head, cx));
                                        cx.stop_propagation();
                                    }));
                            }
                            slot
                        })
                        .child(
                            div()
                                .v_flex()
                                .justify_center()
                                .flex_1()
                                .min_w_0()
                                .pr_2()
                                .child(div().text_size(px(font_size)).truncate().child(title))
                                .child(
                                    div()
                                        .text_size(px(font_size * 0.85))
                                        .text_color(rgb(0xa6b5ca))
                                        .truncate()
                                        .child(detail),
                                ),
                        )
                        .tooltip(move |window, cx| {
                            Tooltip::new(tooltip.clone())
                                .max_w(rems(32.))
                                .build(window, cx)
                        })
                        .on_click(move |_, window, cx| {
                            let _ = viewer.update(cx, |viewer, cx| {
                                viewer.select_evolution_version(kind, index, mode, window, cx);
                            });
                        })
                        .into_any_element()
                })
                .collect()
        })
        .size_full()
        .track_scroll(scroll);
        let mut details = div()
            .px_3()
            .py_2()
            .text_size(px(font_size * 0.85))
            .text_color(rgb(0xb8c6d9))
            .child(format!(
                "{} versions · {} {} · {} hidden{}",
                count,
                group_count,
                if group_count == 1 { "group" } else { "groups" },
                hidden_count,
                if evolution.timeline.truncated {
                    " (more available)"
                } else {
                    ""
                }
            ))
            .child(
                div().truncate().child(
                    version
                        .description
                        .lines()
                        .next()
                        .unwrap_or("(no description)")
                        .to_owned(),
                ),
            )
            .child(div().child(format!(
                "{} · {}",
                version.timestamp,
                &version.commit_id[..8.min(version.commit_id.len())]
            )));
        if version.divergent {
            details = details.child(
                div()
                    .text_color(rgb(0xe5c07b))
                    .child("Other current versions exist"),
            );
        }
        if version.conflict {
            details = details.child(div().text_color(rgb(0xe58a83)).child("Contains conflicts"));
        }
        if version.hidden {
            details = details.child(div().child("Historical / hidden revision"));
        }
        panel
            .child(div().flex_1().min_h_0().child(rows))
            .child(details)
            .into_any_element()
    }
}

#[cfg(test)]
mod header_tests {
    use super::{EvolutionKind, HeaderControls, HeaderPresentation, ReviewMode};
    #[test]
    fn pending_metadata_keeps_the_entire_previous_toolbar_until_feedback_is_due() {
        let previous = HeaderPresentation {
            label: "File evolution · previous timestamp · latest".into(),
            controls: Some(HeaderControls {
                kind: EvolutionKind::File,
                index: 3,
                version: "previous-version".into(),
                mode: ReviewMode::SincePrevious,
                has_predecessors: true,
            }),
        };
        let fast = HeaderPresentation::pending(Some(&previous), false);
        assert!(fast == previous);
        let slow = HeaderPresentation::pending(Some(&previous), true);
        assert_eq!(slow.label, "Loading evolution…");
        assert!(slow.controls.is_none());
        let first = HeaderPresentation::pending(
            Some(&HeaderPresentation::label("Whole branch".into())),
            false,
        );
        assert_eq!(first.label, "Whole branch");
        assert!(first.controls.is_none());
    }
}

#[cfg(test)]
mod timeline_tests {
    use super::{EvolutionPanel, EvolutionSession, ReviewMode, TimelineState};
    use jaywalk::{evolution::Evolution, history::Revision};
    use std::{collections::HashSet, rc::Rc};

    fn loaded_panel(path: &str) -> EvolutionPanel {
        let version = serde_json::from_value(serde_json::json!({
            "commit_id": "aaaaaaaa", "change_id": "bbbbbbbb", "description": "edit file",
            "parents": [], "predecessors": [], "timestamp": "2026-01-01 12:00:00",
            "operation_id": "cccccccc", "operation": "snapshot", "hidden": false,
            "divergent": false, "conflict": false, "patch_changed": true,
            "patch_files": [], "evolution_files": []
        }))
        .unwrap();
        let anchor = Revision {
            commit_id: "aaaaaaaa".into(),
            change_id: "bbbbbbbb".into(),
            description: "edit file".into(),
            author: String::new(),
            references: vec![],
            parents: vec![],
            working_copy: false,
            empty: false,
        };
        let timeline = Rc::new(Evolution {
            versions: vec![version],
            graph: jaywalk::history::History::parse(
                r#"["aaaaaaaa","bbbbbbbb","edit file","",[],[],false,false]"#,
            )
            .unwrap(),
            truncated: false,
        });
        let view = Rc::new(timeline.view(false, 0));
        let presentation = Rc::new(view.grouped(
            &timeline,
            Some(path),
            ReviewMode::SincePrevious,
            true,
            &HashSet::new(),
        ));
        let mut panel = EvolutionPanel::new(true, false, true);
        panel.file_path = Some(path.into());
        panel.state = TimelineState::Loaded(EvolutionSession {
            operation: "test-operation".into(),
            anchor,
            timeline,
            view,
            presentation,
            selected: 0,
            mode: ReviewMode::SincePrevious,
        });
        panel
    }

    #[test]
    fn interrupted_timeline_loads_preserve_one_coherent_previous_subject() {
        let mut panel = loaded_panel("old.ts");
        let original = panel.presentation().unwrap();
        panel.state = TimelineState::Loading {
            anchor: "new-version".into(),
            previous: panel.presentation(),
        };
        panel.file_path = Some("new.ts".into());
        panel.state = TimelineState::Loading {
            anchor: "newer-version".into(),
            previous: panel.presentation(),
        };
        let retained = panel.presentation().unwrap();
        assert_eq!(retained.file_path.as_deref(), Some("old.ts"));
        assert!(Rc::ptr_eq(
            &retained.session.timeline,
            &original.session.timeline
        ));
        assert!(Rc::ptr_eq(
            &retained.session.presentation,
            &original.session.presentation
        ));
        assert!(retained.session.mode == ReviewMode::SincePrevious);
        panel.clear();
        assert!(panel.presentation().is_none());
        panel.state = TimelineState::Failed("unavailable".into());
        assert!(panel.presentation().is_none());
    }
}
