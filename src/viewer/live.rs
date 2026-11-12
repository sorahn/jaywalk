//! Watch plain Git worktrees and poll recorded jj metadata; never mutate either repository.
use super::*;
use jaywalk::git_watch::{GitStamp, GitWatch};
use jaywalk::live_refresh::{FollowedChange, GitRefresh, RecordedRefresh, RefreshGate};
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
    git_applied: Option<GitStamp>,
    git_retry_after: Option<std::time::Instant>,
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
            git_applied: None,
            git_retry_after: None,
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
        if let Some(position) = self.reflow_position(cx) {
            self.live.pending_position = Some(position);
        }
    }

    pub(super) fn reflow_position(&self, cx: &App) -> Option<(usize, usize, f32)> {
        let snapshot = self.capture_reading(cx)?;
        Some((
            snapshot.before.line,
            snapshot.after.line,
            snapshot.horizontal,
        ))
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
        if !recorded {
            self.start_git_live(directory, window, cx);
            return;
        }
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

    fn git_follows_worktree(&self) -> bool {
        self.repository.ready().is_some_and(|session| {
            matches!(session.review.source, review_context::ReviewSource::Home(_))
                && (matches!(
                    session.scope,
                    ComparisonScope::Revision {
                        working_copy: true,
                        ..
                    }
                ) || (matches!(session.scope, ComparisonScope::Aggregate)
                    && self.home_arguments().tip == "@"))
        })
    }

    fn start_git_live(
        &mut self,
        root: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fallback = Duration::from_millis(self.settings.live.poll_interval_ms);
        self.live.git_applied = None;
        self.live.poll = Some(cx.spawn_in(window, async move |this, cx| {
            let mut watcher = None;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if watcher.is_none() {
                    let directory = root.clone();
                    match cx
                        .background_spawn(async move { GitWatch::subscribe(&directory) })
                        .await
                    {
                        Ok(subscription) => watcher = Some(subscription),
                        Err(error) => {
                            if this
                                .update_in(cx, |viewer, _, cx| {
                                    viewer.live.message = Some(format!("Live refresh: {error}"));
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                            cx.background_executor().timer(fallback).await;
                            continue;
                        }
                    }
                }
                let subscription = watcher
                    .as_ref()
                    .expect("subscription was installed above")
                    .clone();
                let result = cx
                    .background_spawn(async move { subscription.poll(fallback) })
                    .await;
                let failed = result.is_err();
                if this
                    .update_in(cx, |viewer, window, cx| match result {
                        Err(error) => {
                            let message = format!("Live refresh: {error}");
                            if viewer.live.message.as_ref() != Some(&message) {
                                viewer.live.message = Some(message);
                                cx.notify();
                            }
                        }
                        Ok(stamp) => viewer.on_git_change(stamp, window, cx),
                    })
                    .is_err()
                {
                    break;
                }
                if failed {
                    cx.background_executor().timer(fallback).await;
                }
            }
        }));
    }

    fn on_git_change(&mut self, stamp: GitStamp, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .live
            .message
            .as_deref()
            .is_some_and(|message| message.starts_with("Live refresh:"))
        {
            self.live.message = None;
            cx.notify();
        }
        if self.live.git_applied == Some(stamp)
            || self
                .live
                .git_retry_after
                .is_some_and(|time| std::time::Instant::now() < time)
            || self.live.busy
            || self.repository.ready().is_none()
        {
            return;
        }
        if !self.navigation.pinned && self.git_follows_worktree() {
            self.refresh_git(Some(stamp), window, cx);
        } else {
            let refs_changed = self
                .live
                .git_applied
                .is_some_and(|previous| previous.references != stamp.references);
            self.live.git_applied = Some(stamp);
            if refs_changed
                && !self.navigation.pinned
                && self.repository.ready().is_some_and(|session| {
                    session.review.source.follows_branch()
                        && matches!(session.scope, ComparisonScope::Aggregate)
                })
            {
                self.refresh_recorded(window, cx);
            } else {
                self.reload_history(None, window, cx);
            }
        }
    }

    fn refresh_git(
        &mut self,
        stamp: Option<GitStamp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.live.busy {
            return;
        }
        let Some(session) = self.repository.ready() else {
            return;
        };
        let working_copy = matches!(
            session.scope,
            ComparisonScope::Revision {
                working_copy: true,
                ..
            }
        );
        let arguments = self.home_arguments();
        let history = self.history_query.clone();
        let query = history.clone();
        let limit =
            jaywalk::text_content::TextLimit::from_mib(self.settings.diff.max_text_file_size_mb)
                .expect("viewer settings are validated before publication");
        let generation = self.requests.comparison();
        let file_generation = self.requests.file();
        self.live.busy = true;
        self.begin_loading(LoadingKind::Live, window, cx);
        self.live.refresh = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    GitRefresh::load(
                        arguments.directory(),
                        arguments.base.as_deref(),
                        &arguments.tip,
                        working_copy,
                        &history,
                        limit,
                    )
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                viewer.live.busy = false;
                viewer.finish_loading(LoadingKind::Live);
                if viewer.requests.comparison() != generation
                    || viewer.requests.file() != file_generation
                    || viewer.history_query != query
                {
                    return;
                }
                match result {
                    Err(error) => {
                        viewer.live.git_retry_after = Some(
                            std::time::Instant::now()
                                + Duration::from_millis(viewer.settings.live.poll_interval_ms),
                        );
                        viewer.live.message =
                            Some(format!("Refresh paused; previous view retained: {error}"));
                    }
                    Ok(GitRefresh {
                        home,
                        history,
                        revision,
                        comparison,
                    }) => {
                        viewer.preserve_overview_reading(cx);
                        viewer.invalidate_history_reload();
                        viewer.invalidate_comparison_request();
                        let scope =
                            revision
                                .as_ref()
                                .map_or(ComparisonScope::Aggregate, |revision| {
                                    ComparisonScope::Revision {
                                        commit_id: revision.commit_id.clone(),
                                        change_id: revision.change_id.clone(),
                                        working_copy: true,
                                        merge: false,
                                    }
                                });
                        let comparison = Rc::new(comparison);
                        let home = Rc::new(home);
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
                        viewer.live.git_applied = stamp.or(viewer.live.git_applied);
                        viewer.live.git_retry_after = None;
                        viewer.live.message = None;
                        viewer.rebuild_browser(cx);
                        viewer.follow_open_file(window, cx);
                    }
                }
                cx.notify();
            });
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
        self.live.git_retry_after = None;
        if !self.live.busy {
            self.live.message = None;
        }
    }

    pub(super) fn refresh_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigation.pinned = false;
        self.live.retry.clear();
        self.live.git_retry_after = None;
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
            if self.arguments.diff.is_some() {
                self.load_repository(None, window, cx);
            } else if self.git_follows_worktree() {
                self.refresh_git(None, window, cx);
            } else {
                self.reload_history(None, window, cx);
            }
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

#[cfg(test)]
mod git_tests {
    use super::{Arguments, ComparisonScope, GitStamp, SessionLayout, Settings, Viewer};
    use clap::Parser;
    use gpui::{AppContext, WindowOptions};
    use gpui_test as gpui;
    #[gpui_test::test]
    fn git_refresh_updates_the_open_working_file_and_keeps_commit_inspection_pinned(
        cx: &mut gpui_test::TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!("jaywalk-live-ui-{}", std::process::id()));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(&repo)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout)
                .unwrap()
                .trim_end()
                .to_owned()
        };
        git(&["init", "-b", "main"]);
        std::fs::write(repo.join("file.txt"), "original\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);
        let head = git(&["rev-parse", "HEAD"]);
        let config = root.join("config.toml");
        std::fs::write(
            &config,
            "[live]\nenabled=false\n[layout]\nremember_per_repository=false\n",
        )
        .unwrap();
        let mut arguments = Arguments::parse_from(["jaywalk", repo.to_str().unwrap()]);
        arguments.config = Some(config);
        arguments.normalize().unwrap();
        let settings =
            Settings::parse("[live]\nenabled=false\n[layout]\nremember_per_repository=false")
                .unwrap();
        let (window, viewer) = cx.update(|app| {
            gpui_kit::init(app);
            crate::windows::init(app);
            crate::settings_service::init(app);
            crate::recents::init_without_storage(app);
            crate::appearance::init(&settings, app);
            super::super::init(&arguments, &settings, app).unwrap();
            let layout = SessionLayout::load(&arguments, &settings);
            gpui_kit::open_window(WindowOptions::default(), app, |window, app| {
                app.new(|cx| Viewer::new(arguments, settings, layout, window, cx))
            })
            .unwrap()
        });
        cx.run_until_parked();
        cx.update(|app| {
            window
                .update(app, |_, window, app| {
                    viewer.update(app, |viewer, cx| {
                        viewer.select_revision("working tree", window, cx);
                    })
                })
                .unwrap()
        });
        cx.run_until_parked();
        cx.update(|app| {
            window
                .update(app, |_, window, app| {
                    viewer.update(app, |viewer, cx| viewer.select_path("file.txt", window, cx))
                })
                .unwrap()
        });
        cx.run_until_parked();
        std::fs::write(repo.join("file.txt"), "new working text\n").unwrap();
        cx.update(|app| {
            window
                .update(app, |_, window, app| {
                    viewer.update(app, |viewer, cx| {
                        viewer.on_git_change(
                            GitStamp {
                                generation: 2,
                                references: 0,
                            },
                            window,
                            cx,
                        );
                    })
                })
                .unwrap()
        });
        cx.run_until_parked();
        cx.update(|app| {
            let viewer = viewer.read(app);
            assert_eq!(viewer.selected_path().as_deref(), Some("file.txt"));
            assert!(matches!(
                viewer.repository.ready().unwrap().scope,
                ComparisonScope::Revision {
                    working_copy: true,
                    ..
                }
            ));
            assert_eq!(
                viewer.code().unwrap().after.read(app).value().as_ref(),
                "new working text\n"
            );
        });
        cx.update(|app| {
            window
                .update(app, |_, window, app| {
                    viewer.update(app, |viewer, cx| viewer.select_revision(&head, window, cx))
                })
                .unwrap()
        });
        cx.run_until_parked();
        let inspected = cx.update(|app| viewer.read(app).comparison().unwrap().identity());
        std::fs::write(repo.join("file.txt"), "another edit\n").unwrap();
        cx.update(|app| {
            window
                .update(app, |_, window, app| {
                    viewer.update(app, |viewer, cx| {
                        viewer.on_git_change(
                            GitStamp {
                                generation: 3,
                                references: 0,
                            },
                            window,
                            cx,
                        );
                    })
                })
                .unwrap()
        });
        cx.run_until_parked();
        cx.update(|app| {
            let viewer = viewer.read(app);
            assert_eq!(viewer.comparison().unwrap().identity(), inspected);
            assert_eq!(
                viewer.code().unwrap().after.read(app).value().as_ref(),
                "original\n"
            );
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
