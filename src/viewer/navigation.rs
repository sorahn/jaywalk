//! Visited comparisons and files, independent of graph/file ordering.
use super::*;
use jaywalk::navigation::History;
use jaywalk::repository_file::FileEntry;

#[derive(Clone, PartialEq, Eq)]
enum ScopeKey {
    WholeBranch,
    Revision(String),
    WorkingCopy,
    Evolution(String, String, evolution::ReviewMode),
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct LocationKey {
    scope: ScopeKey,
    path: Option<String>,
}
#[derive(Clone)]
struct FileVisit {
    file: FileEntry,
    source: Comparison,
    reading: Option<live::ReadingSnapshot>,
}
#[derive(Clone)]
pub(super) struct ViewVisit {
    scope: ComparisonScope,
    comparison: Rc<Comparison>,
    file: Option<FileVisit>,
    overview: Option<overview::OverviewCheckpoint>,
    evolution: evolution::EvolutionCheckpoint,
    anchor: Option<jaywalk::history::Revision>,
}
pub(super) struct NavigationState {
    pub(super) history: History<LocationKey, ViewVisit>,
    suppress: bool,
    restoring: bool,
    pub(super) pinned: bool,
}
impl NavigationState {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            history: History::new(limit),
            suppress: false,
            restoring: false,
            pinned: false,
        }
    }
}
impl Viewer {
    fn navigation_key(&self) -> Option<LocationKey> {
        let session = self.repository.ready()?;
        let comparison = session.comparison.ready()?;
        if let Some(file) = &self.selected_file {
            if matches!(file.content, LoadState::Loaded(_))
                && (self.live.restore.is_some()
                    || self.live.pending_position.is_some()
                    || self.config.pending_standard.is_some()
                    || self.config.pending_horizontal.is_some())
            {
                // The restored editors have loaded, but their viewport has not
                // settled yet. Rapid Back/Forward must keep the saved anchors.
                return None;
            }
            if matches!(file.content, LoadState::Loading)
                || (file.source.identity() != comparison.identity() && !self.is_retained_preview())
            {
                return None;
            }
        } else if self.capture_overview_identity() != Some(comparison.identity()) {
            return None;
        }
        let scope = match &session.scope {
            ComparisonScope::WholeBranch => ScopeKey::WholeBranch,
            ComparisonScope::Revision {
                working_copy: true, ..
            } => ScopeKey::WorkingCopy,
            ComparisonScope::Revision { change_id, .. } => ScopeKey::Revision(change_id.clone()),
            ComparisonScope::Evolution {
                anchor,
                version,
                mode,
                ..
            } => ScopeKey::Evolution(anchor.clone(), version.clone(), *mode),
        };
        Some(LocationKey {
            scope,
            path: self.selected_path(),
        })
    }
    fn capture_visit(&self, cx: &App) -> Option<(LocationKey, ViewVisit)> {
        let key = self.navigation_key()?;
        let session = self.repository.ready()?;
        let comparison = session.comparison.ready()?.clone();
        let anchor = if let ComparisonScope::Revision { commit_id, .. } = &session.scope {
            session
                .history
                .as_ref()
                .ok()
                .and_then(|history| {
                    history
                        .revisions
                        .iter()
                        .find(|revision| &revision.commit_id == commit_id)
                        .cloned()
                })
                .or_else(|| {
                    self.overview_revision(commit_id)
                        .map(|(revision, _)| revision)
                })
        } else {
            None
        };
        Some((
            key,
            ViewVisit {
                scope: session.scope.clone(),
                comparison,
                file: self.selected_file.as_ref().map(|selected| FileVisit {
                    file: selected.file.clone(),
                    source: selected.source.clone(),
                    reading: self.capture_reading(cx),
                }),
                overview: self
                    .selected_file
                    .is_none()
                    .then(|| self.capture_overview(cx))
                    .flatten(),
                evolution: self.capture_evolution(),
                anchor,
            },
        ))
    }
    /// Refresh the leaving view's position before an explicit navigation.
    pub(super) fn checkpoint_navigation(&mut self, cx: &App) {
        if self.navigation.suppress {
            return;
        }
        // Explicit navigation supersedes any unfinished history restoration.
        self.navigation.restoring = false;
        if let Some((key, visit)) = self.capture_visit(cx) {
            self.navigation.history.observe(key, visit);
        }
    }
    /// Observe only complete destinations. Scroll/refresh updates at the same
    /// logical location do not create visits or copy file anchors each frame.
    pub(super) fn sync_navigation(&mut self, cx: &App) {
        let Some(key) = self.navigation_key() else {
            return;
        };
        if self
            .navigation
            .history
            .current()
            .is_some_and(|entry| entry.key == key)
        {
            self.navigation.restoring = false;
            return;
        }
        self.checkpoint_navigation(cx);
    }
    pub(super) fn show_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.checkpoint_navigation(cx);
        window.focus(&self.focus, cx);
        self.remember_reading(cx);
        self.clear_file();
        self.rebuild_browser(cx);
        self.focus_evolution_file(None);
        cx.notify();
    }
    pub(super) fn show_whole_branch_overview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint_navigation(cx);
        self.navigation.suppress = true;
        self.remember_reading(cx);
        self.clear_file();
        self.show_whole_branch(window, cx);
        self.navigation.suppress = false;
        window.focus(&self.focus, cx);
    }
    pub(super) fn navigate_view(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ready = self.navigation_key().is_some();
        if ready {
            self.checkpoint_navigation(cx);
        }
        // Back during a new pending request cancels it and restores the last
        // displayed visit. A pending history restoration can be stepped again.
        let state = if ready {
            jaywalk::navigation::VisitState::Ready
        } else if self.navigation.restoring {
            jaywalk::navigation::VisitState::Restoring
        } else {
            jaywalk::navigation::VisitState::Pending
        };
        let target = self.navigation.history.navigate(forward, state);
        let Some(target) = target else {
            return;
        };
        self.navigation.restoring = true;
        self.navigation.suppress = true;
        self.invalidate_history_reload();
        self.cancel_live_refresh();
        self.repository_generation += 1;
        self.repository_task = None;
        self.pending_file = None;
        self.finish_loading(LoadingKind::Comparison);
        self.finish_loading(LoadingKind::Evolution);
        self.clear_file();
        let visit = target.value;
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = visit.scope;
            session.revision_comparison = Some(visit.comparison.clone());
            session.comparison = LoadState::Loaded(visit.comparison);
        }
        let has_evolution = self.restore_evolution(visit.evolution);
        self.restore_overview(visit.overview, cx);
        self.rebuild_browser(cx);
        if let Some(file) = visit.file {
            let path = file.file.path().to_owned();
            self.load_file(
                file.source,
                file.file,
                loading::FileLoad::Select,
                window,
                cx,
            );
            self.live.restore = file.reading.map(|reading| (path, reading));
        }
        if !has_evolution && let Some(anchor) = visit.anchor {
            self.load_evolution(anchor, window, cx);
        }
        self.navigation.pinned = true;
        self.live.message =
            Some("Visited view · Refresh or select a comparison to follow recorded changes".into());
        self.navigation.suppress = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

impl Viewer {
    pub(super) fn render_navigation_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut buttons = div().h_flex().items_center();
        for (id, forward, icon, command, enabled) in [
            (
                "view-back",
                false,
                IconName::ArrowLeft,
                jaywalk::commands::CommandId::GoBack,
                self.navigation.history.can_back()
                    || (self.navigation_key().is_none()
                        && !self.navigation.restoring
                        && self.navigation.history.current().is_some()),
            ),
            (
                "view-forward",
                true,
                IconName::ArrowRight,
                jaywalk::commands::CommandId::GoForward,
                self.navigation.history.can_forward()
                    && (self.navigation_key().is_some() || self.navigation.restoring),
            ),
        ] {
            buttons = buttons.child(
                Button::new(id)
                    .ghost()
                    .compact()
                    .icon(icon)
                    .disabled(!enabled)
                    .tooltip_with_action(
                        command.label(),
                        &commands::ExecuteCommand { command },
                        None,
                    )
                    .on_click(cx.listener(move |viewer, _, window, cx| {
                        viewer.navigate_view(forward, window, cx)
                    })),
            );
        }
        buttons.into_any_element()
    }
}
