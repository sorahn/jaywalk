//! Poll local recorded metadata; never fetch, import or snapshot the observed repository.
use super::*;
use jaywalk::live_refresh::{FollowedChange, RecordedRefresh, RefreshGate};
use jaywalk::reading::ReadingAnchor;
use std::time::Duration;

#[derive(Clone)]
pub(super) struct ReadingSnapshot {
    before: ReadingAnchor,
    after: ReadingAnchor,
    horizontal: f32,
    side: gpui::component::diff::DiffSide,
}

pub(super) struct LiveState {
    pub(super) poll: Option<Task<()>>,
    refresh: Option<Task<()>>,
    busy: bool,
    pub(super) message: Option<String>,
    retry: RefreshGate,
    stamp: Option<jaywalk::references::ReferenceStamp>,
    pub(super) restore: Option<(String, ReadingSnapshot)>,
    pub(super) pending_position: Option<(usize, usize, f32)>,
    positions: std::collections::HashMap<String, ReadingSnapshot>,
    pub(super) standard_side: gpui::component::diff::DiffSide,
}
impl LiveState {
    pub(super) fn new() -> Self {
        Self {
            poll: None,
            refresh: None,
            busy: false,
            message: None,
            retry: RefreshGate::default(),
            stamp: None,
            restore: None,
            pending_position: None,
            positions: std::collections::HashMap::new(),
            standard_side: gpui::component::diff::DiffSide::Modified,
        }
    }
}
impl Viewer {
    pub(super) fn remember_reading(&mut self, cx: &App) {
        let Some(snapshot) = self.capture_reading(cx) else {
            return;
        };
        let Some(path) = self.selected_path() else {
            return;
        };
        if self.live.positions.len() > 128 {
            self.live.positions.clear();
        }
        self.live.positions.insert(path, snapshot);
    }
    pub(super) fn recall_reading(&mut self, path: &str) {
        if self.live.restore.is_some() {
            return;
        }
        if let Some(snapshot) = self.live.positions.get(path) {
            self.live.restore = Some((path.to_owned(), snapshot.clone()));
        }
    }

    pub(super) fn preserve_reflow_position(&mut self, cx: &App) {
        if self.view.effective() != ViewMode::Fluid && !self.view.is_single_file() {
            return;
        }
        if let Some(snapshot) = self.capture_reading(cx) {
            self.live.pending_position = Some((
                snapshot.before.line,
                snapshot.after.line,
                snapshot.horizontal,
            ));
        }
    }

    pub(super) fn capture_reading(&self, cx: &App) -> Option<ReadingSnapshot> {
        let code = self.code()?;
        // GPUI's visible range already uses buffer lines, even with wrapping.
        let line = |state: &EditorState| state.visible_row_range().map_or(0, |range| range.start);
        let before = code.before.read(cx);
        let after = code.after.read(cx);
        let mut horizontal = self.horizontal_position;
        let mut side = gpui::component::diff::DiffSide::Modified;
        let mut before_line = line(before);
        let mut after_line = line(after);
        if self.view.is_single_file() {
            after_line = before_line;
            horizontal = -f32::from(before.scroll_offset().x);
        }
        if self.view.effective() != ViewMode::Fluid
            && let Ok(Some(standard)) = &code.standard
            && let Some(position) = standard.read(cx).viewport_position()
        {
            horizontal = f32::from(standard.read(cx).horizontal_offset());
            side = position.side();
            match position.side() {
                gpui::component::diff::DiffSide::Original => {
                    before_line = position.line().saturating_sub(1)
                }
                _ => after_line = position.line().saturating_sub(1),
            }
        }
        Some(ReadingSnapshot {
            before: ReadingAnchor::capture(&before.value(), before_line),
            after: ReadingAnchor::capture(&after.value(), after_line),
            side,
            horizontal,
        })
    }

    pub(super) fn restore_prepared_reading(&mut self, path: &str, before: &str, after: &str) {
        if let Some((expected, snapshot)) = self.live.restore.take()
            && expected == path
        {
            self.live.standard_side = snapshot.side;
            self.live.pending_position = Some((
                snapshot.before.resolve(before),
                snapshot.after.resolve(after),
                snapshot.horizontal,
            ));
        }
    }

    pub(super) fn start_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.arguments.diff.is_some() || self.live.poll.is_some() || !self.settings.live.enabled
        {
            return;
        }
        let Some(session) = self.repository.ready() else {
            return;
        };
        self.live.stamp =
            session.history.as_ref().ok().map(|history| {
                jaywalk::references::ReferenceStamp::from_snapshot(&history.references)
            });
        let directory = session.home.root.clone();
        let recorded = session.home.supports_evolution();
        let interval = Duration::from_millis(self.settings.live.poll_interval_ms);
        self.live.poll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                let root = directory.clone();
                let stamp = cx
                    .background_spawn(async move {
                        jaywalk::references::ReferenceStamp::read(&root, recorded)
                    })
                    .await;
                if this
                    .update_in(cx, |viewer, window, cx| match stamp {
                        Ok(stamp) => {
                            if viewer
                                .live
                                .message
                                .as_deref()
                                .is_some_and(|message| message.starts_with("Live refresh:"))
                            {
                                viewer.live.message = None;
                                cx.notify();
                            }
                            let changed = viewer.live.stamp.as_ref() != Some(&stamp);
                            viewer.live.stamp = Some(stamp.clone());
                            let Some(session) = viewer.repository.ready() else {
                                return;
                            };
                            let branch_review = matches!(
                                session.review.source,
                                review_context::ReviewSource::Target(_)
                            );
                            let pinned = viewer.navigation.pinned
                                || matches!(session.scope, ComparisonScope::Evolution { .. })
                                || (branch_review
                                    && !matches!(session.scope, ComparisonScope::Aggregate));
                            if changed
                                && !pinned
                                && session.review.source.follows_branch()
                                && matches!(session.scope, ComparisonScope::Aggregate)
                            {
                                viewer.refresh_recorded(window, cx);
                                return;
                            }
                            if branch_review {
                                if changed {
                                    viewer.reload_history(None, window, cx);
                                }
                                return;
                            }
                            let operation_changed =
                                stamp.operation.as_ref().is_some_and(|operation| {
                                    viewer.live.retry.allows(operation, &session.home.operation)
                                });
                            if operation_changed && !pinned {
                                viewer.refresh_recorded(window, cx);
                            } else if changed {
                                viewer.reload_history(None, window, cx);
                            }
                        }
                        Err(error) => {
                            let message = format!("Live refresh: {error}");
                            if viewer.live.message.as_ref() != Some(&message) {
                                viewer.live.message = Some(message);
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(super) fn cancel_live_refresh(&mut self) {
        self.live.refresh = None;
        self.live.busy = false;
        self.finish_loading(LoadingKind::Live);
    }
    pub(super) fn resume_live_refresh(&mut self) {
        self.navigation.pinned = false;
        self.live.retry.clear();
        if !self.live.busy {
            self.live.message = None;
        }
    }

    pub(super) fn refresh_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation.pinned = false;
        self.live.retry.clear();
        self.refresh_recorded(window, cx);
    }

    fn refresh_recorded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.repository.ready()
            && let review_context::ReviewSource::Target(target) = &session.review.source
        {
            if matches!(session.scope, ComparisonScope::Aggregate) {
                self.open_review_target(target.clone(), window, cx);
            } else {
                self.live.message = Some(
                    "Historical branch review · All changes resumes the latest branch comparison"
                        .into(),
                );
                cx.notify();
            }
            return;
        }
        if !self.supports_evolution() {
            self.load_repository(None, window, cx);
            return;
        }
        if self.live.busy {
            return;
        }
        let Some(session) = self.repository.ready() else {
            self.load_repository(None, window, cx);
            return;
        };
        // A historical review is a pinned, internally coherent operation view.
        // Choosing its graph row resumes following the logical change.
        if matches!(session.scope, ComparisonScope::Evolution { .. }) {
            self.live.message = Some(
                "Pinned evolution revision · select its history row to follow the latest change"
                    .into(),
            );
            cx.notify();
            return;
        }
        let change = if let ComparisonScope::Revision {
            change_id,
            working_copy,
            ..
        } = &session.scope
        {
            Some(FollowedChange {
                change_id: change_id.clone(),
                working_copy: *working_copy,
            })
        } else {
            None
        };
        let limit =
            jaywalk::text_content::TextLimit::from_mib(self.settings.diff.max_text_file_size_mb)
                .expect("viewer settings are validated before publication");
        let directory = self.arguments.directory().to_owned();
        let home_arguments = self.home_arguments();
        let base = home_arguments.base.clone();
        // Preserve the requested reference. In particular, @ follows the recorded
        // workspace even when jj abandons and replaces its working-copy change.
        let tip = home_arguments.tip.clone();
        let history_query = self.history_query.clone();
        let requested_history_query = history_query.clone();
        let generation = self.requests.comparison();
        let file_generation = self.requests.file();
        self.begin_loading(LoadingKind::Live, window, cx);
        self.live.busy = true;
        self.live.message = Some("Updating recorded changes…".into());
        self.live.refresh = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    RecordedRefresh::load_with_limit(
                        &directory,
                        base.as_deref(),
                        &tip,
                        change.as_ref(),
                        &history_query,
                        limit,
                    )
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                viewer.live.busy = false;
                viewer.finish_loading(LoadingKind::Live);
                if viewer.requests.comparison() != generation
                    || viewer.requests.file() != file_generation
                {
                    if viewer.live.message.as_deref() == Some("Updating recorded changes…") {
                        viewer.live.message = None;
                        cx.notify();
                    }
                    return;
                }
                if viewer.history_query != requested_history_query {
                    viewer.live.message = None;
                    viewer.refresh_recorded(window, cx);
                    return;
                }
                match result {
                    Err(error) => {
                        viewer.live.retry.failed(error.operation);
                        viewer.live.message = Some(format!(
                            "Refresh paused; previous view retained: {}",
                            error.message
                        ));
                    }
                    Ok(RecordedRefresh {
                        branch,
                        history,
                        revision,
                        comparison,
                    }) => {
                        viewer.live.retry.clear();
                        viewer.invalidate_history_reload();
                        viewer.invalidate_comparison_request();
                        viewer.requests.comparison_task = None;
                        viewer.remember_evolution_header();
                        viewer.evolution.clear();
                        viewer.file_evolution.clear();
                        let scope =
                            revision
                                .as_ref()
                                .map_or(ComparisonScope::Aggregate, |revision| {
                                    ComparisonScope::Revision {
                                        commit_id: revision.commit_id.clone(),
                                        change_id: revision.change_id.clone(),
                                        working_copy: revision.working_copy,
                                        merge: revision.parents.len() > 1,
                                    }
                                });
                        let comparison = Rc::new(comparison);
                        let home = Rc::new(branch);
                        viewer.repository = LoadState::Loaded(RepositorySession {
                            home: home.clone(),
                            review: review_context::ReviewContext {
                                source: review_context::ReviewSource::Home(viewer.home_source),
                                aggregate: home,
                            },
                            history: Ok(Rc::new(history)),
                            scope,
                            revision_comparison: revision.as_ref().map(|_| comparison.clone()),
                            comparison: LoadState::Loaded(comparison),
                        });
                        viewer.rebuild_browser(cx);
                        let active_evolution = viewer.active_evolution;
                        let path = viewer.selected_path();
                        viewer.focus_evolution_file(path.as_deref());
                        viewer.active_evolution = active_evolution;
                        if let Some(revision) = revision {
                            viewer.load_evolution(revision, window, cx);
                        }
                        viewer.live.message = None;
                        viewer.follow_open_file(window, cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
