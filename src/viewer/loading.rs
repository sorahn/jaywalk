use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FileLoad {
    Select,
    Follow,
    Reflow,
}

/// Captures the selected subject and request together until preparation completes.
struct FileLoadRequest {
    id: jaywalk::review_requests::FileRequest,
    source: Comparison,
    file: jaywalk::repository_file::FileEntry,
    reason: FileLoad,
}

impl Viewer {
    pub(crate) fn new(
        arguments: Arguments,
        settings: Settings,
        layout: SessionLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let home_source = review_context::HomeSource::from_arguments(&arguments);
        let preferred_path = arguments.file.clone();
        let view_mode = settings.diff.default_view;
        let wrapping = Wrapping::from_settings(&settings.diff);
        let file_width = settings.panels.files.default_width;
        let file_mode = settings.files.default_view;
        let file_scope = settings.files.tree_scope;
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let evolution = evolution::EvolutionPanel::new(
            settings.evolution.visible,
            settings.evolution.show_unchanged_rebases,
            settings.evolution.collapse_repeats,
        );
        let file_evolution = evolution::EvolutionPanel::new(
            settings.file_evolution.visible,
            settings.evolution.show_unchanged_rebases,
            settings.evolution.collapse_repeats,
        );
        let palette = palette::PaletteState::new(window, cx);
        let references = references::ReferenceState::new(&settings.history);
        let navigation = navigation::NavigationState::new(settings.navigation.history_limit);
        let welcome = arguments.welcome.then(welcome::WelcomeState::default);
        let mut viewer = Self {
            welcome,
            recent_recorded: false,
            code_font_size: settings.editor.font_size,
            line_numbers: settings.editor.line_numbers,
            collapse_unchanged: settings.diff.collapse_unchanged,
            config: settings::ConfigState::new(),
            live: live::LiveState::new(),
            feedback: feedback::Feedback::default(),
            palette,
            layout,
            arguments,
            home_source,
            review_picker: review_context::ReviewPicker::default(),
            settings,
            focus,
            panels: panels::PanelLayout::new(cx),
            overview: overview::OverviewState::default(),
            navigation,
            history_scroll: UniformListScrollHandle::new(),
            history_query: jaywalk::history::HistoryQuery::sources(Default::default(), None)
                .expect("default source query has no unvalidated IDs"),
            references,
            evolution,
            evolution_header: None,
            file_evolution,
            active_evolution: evolution::EvolutionKind::Change,
            repository: LoadState::Loading,
            selected_file: None,
            pending_file: None,
            browser: files::FileBrowser::new(file_width, file_mode, file_scope, cx),
            requests: review::ReviewRequests::default(),
            active_change: None,
            navigation_target: None,
            view: jaywalk::view_choice::ViewChoice::new(view_mode),
            wrapping,
            horizontal_position: 0.,
            reveal_after_layout: false,
            fluid: FluidScroll::default(),
            fluid_drag: None,
            editor_observers: Vec::new(),
        };
        // Theme changes include OS appearance and shared diff-palette edits.
        // Recolor existing word ranges without replacing editors or their selection.
        cx.observe_global::<gpui::component::Theme>(|viewer, cx| {
            viewer.refresh_word_colors(cx);
        })
        .detach();
        viewer.apply_saved_layout();
        if viewer.arguments.diff.is_some() && viewer.arguments.difftool.is_none() {
            viewer.layout.saved.history_hidden = true;
            viewer.evolution.visible = false;
            viewer.file_evolution.visible = false;
        }
        viewer.observe_review(window, cx);
        viewer.start_settings_reload(window, cx);
        if !viewer.is_welcome() {
            viewer.initialize_browser(window, cx);
            viewer.load_repository(preferred_path, window, cx);
        }
        viewer
    }

    pub(super) fn load_repository(
        &mut self,
        preferred_path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_history_reload();
        self.resume_live_refresh();
        self.remember_evolution_header();
        self.evolution.clear();
        self.file_evolution.clear();
        self.active_evolution = evolution::EvolutionKind::Change;
        self.invalidate_comparison_request();
        let generation = self.requests.comparison();
        self.remember_reading(cx);
        self.repository = LoadState::Loading;
        self.begin_loading(LoadingKind::Comparison, window, cx);
        self.rebuild_browser(cx);
        let arguments = self.home_arguments();
        let max_text_mb = self.settings.diff.max_text_file_size_mb;
        let history_query = self.history_query.clone();
        let requested_history_query = history_query.clone();
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let comparison = arguments.comparison_with_limit(
                        jaywalk::text_content::TextLimit::from_mib(max_text_mb)?,
                    )?;
                    let history = if arguments.diff.is_some() && arguments.difftool.is_none() {
                        jaywalk::history::History::parse("")
                    } else {
                        comparison.history(&history_query)
                    };
                    Ok::<_, String>((comparison, history))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != generation {
                    return;
                }
                viewer.finish_loading(LoadingKind::Comparison);
                viewer.repository = match result {
                    Ok((comparison, history)) => {
                        if !viewer.recent_recorded && viewer.arguments.diff.is_none() {
                            crate::recents::record(comparison.root(), cx);
                            viewer.recent_recorded = true;
                        }
                        if !comparison.supports_evolution() {
                            viewer.evolution.visible = false;
                            viewer.file_evolution.visible = false;
                        }
                        let branch = Rc::new(comparison);
                        LoadState::Loaded(RepositorySession {
                            home: branch.clone(),
                            review: review_context::ReviewContext {
                                source: if (viewer.arguments.diff.is_none()
                                    || (viewer.arguments.difftool.is_some()
                                        && valid_pair_endpoints(&branch)))
                                    && (viewer.arguments.difftool.is_some()
                                        || viewer.arguments.base.is_some()
                                        || viewer.arguments.tip != "@")
                                {
                                    review_context::ReviewSource::Target(
                                        jaywalk::branch_review::Target::Pair {
                                            left: branch.base().into(),
                                            right: branch.tip().into(),
                                        },
                                    )
                                } else {
                                    review_context::ReviewSource::Home(viewer.home_source)
                                },
                                aggregate: branch.clone(),
                            },
                            history: history.map(Rc::new),
                            scope: ComparisonScope::Aggregate,
                            comparison: LoadState::Loaded(branch),
                            revision_comparison: None,
                        })
                    }
                    Err(error) => {
                        viewer.welcome = Some(welcome::WelcomeState::failed(error.clone()));
                        window.set_window_title("Welcome — Jaywalk");
                        LoadState::Failed(error)
                    }
                };
                if viewer.history_query != requested_history_query {
                    viewer.reload_history(None, window, cx);
                }
                viewer.start_live(window, cx);
                viewer.rebuild_browser(cx);
                let preferred_path = viewer.pending_file.take().or(preferred_path).or_else(|| {
                    (viewer.arguments.diff.is_some())
                        .then(|| {
                            viewer
                                .comparison()
                                .filter(|comparison| comparison.files().len() == 1)
                                .map(|comparison| comparison.files()[0].after_path().to_owned())
                        })
                        .flatten()
                });
                if let Some(path) = preferred_path {
                    viewer.select_path(&path, window, cx);
                } else if viewer.selected_file.is_some() {
                    viewer.follow_open_file(window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(crate) fn reveal_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.repository, LoadState::Loading) {
            self.pending_file = Some(path.to_owned());
        } else {
            self.select_path(path, window, cx);
        }
    }

    pub(super) fn select_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(comparison) = self.comparison().cloned()
            && let Some(file) = comparison.catalog().find(path).cloned()
        {
            self.load_file(comparison, file, FileLoad::Select, window, cx);
        }
    }

    pub(super) fn select_file(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self
            .comparison()
            .and_then(|comparison| comparison.files().get(index))
            .map(|file| file.after_path().to_owned())
        {
            self.select_path(&path, window, cx);
        }
    }

    pub(super) fn follow_open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.selected_path()
            && let Some(comparison) = self.comparison().cloned()
            && let Some(file) = comparison.catalog().find(&path).cloned()
        {
            self.load_file(comparison, file, FileLoad::Follow, window, cx);
        }
    }

    pub(super) fn reload_open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected) = &self.selected_file {
            self.load_file(
                selected.source.clone(),
                selected.file.clone(),
                FileLoad::Reflow,
                window,
                cx,
            );
        }
    }

    pub(super) fn load_file(
        &mut self,
        comparison: Comparison,
        file: jaywalk::repository_file::FileEntry,
        reason: FileLoad,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preserve = reason != FileLoad::Select;
        if reason == FileLoad::Select {
            self.checkpoint_navigation(cx);
        }
        self.remember_reading(cx);
        if preserve {
            self.requests.invalidate_file();
        } else {
            self.clear_file();
            self.selected_file = Some(SelectedFile {
                file: file.clone(),
                source: comparison.clone(),
                content: LoadState::Loading,
            });
            self.fluid = FluidScroll::default();
            self.fluid_drag = None;
            self.horizontal_position = 0.;
        }
        self.begin_loading(LoadingKind::File, window, cx);
        self.recall_reading(file.path());
        let request = FileLoadRequest {
            id: self.requests.file(),
            source: comparison.clone(),
            file: file.clone(),
            reason,
        };
        self.focus_evolution_file(Some(file.path()));
        self.rebuild_browser(cx);
        if reason == FileLoad::Select {
            self.reveal_browser_selection(cx);
        }
        self.reveal_after_layout = false;
        let tab_size = self.settings.editor.tab_size;
        self.requests.file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { PreparedFile::load(&comparison, &file, tab_size) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                viewer.complete_file_load(request, result, window, cx);
            });
        }));
        cx.notify();
    }

    pub(super) fn restore_review_aggregate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // There is no recorded branch to restore until the initial snapshot
        // finishes. Cancelling that task here would leave a loading view.
        if self.repository.ready().is_none() {
            return;
        }
        self.checkpoint_navigation(cx);
        self.remember_reading(cx);
        self.invalidate_history_reload();
        self.resume_live_refresh();
        self.remember_evolution_header();
        self.evolution.clear();
        self.file_evolution.clear();
        self.active_evolution = evolution::EvolutionKind::Change;
        self.invalidate_comparison_request();
        self.requests.comparison_task = None;
        self.finish_loading(LoadingKind::Comparison);
        self.finish_loading(LoadingKind::Evolution);
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::Aggregate;
            session.comparison = LoadState::Loaded(session.review.aggregate.clone());
        }
        self.rebuild_browser(cx);
        self.follow_open_file(window, cx);
        cx.notify();
    }

    pub(super) fn select_revision(
        &mut self,
        commit_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let overview_revision = self.overview_revision(commit_id);
        let Some(revision) = session
            .history
            .as_ref()
            .ok()
            .and_then(|history| {
                history
                    .revisions
                    .iter()
                    .find(|revision| revision.commit_id == commit_id)
                    .cloned()
            })
            .or_else(|| {
                overview_revision
                    .as_ref()
                    .map(|(revision, _)| revision.clone())
            })
        else {
            return;
        };
        let cached = session
            .revision_comparison
            .as_ref()
            .filter(|comparison| comparison.tip() == commit_id)
            .cloned();
        let already_loading = matches!(&session.scope, ComparisonScope::Revision {commit_id: current, ..} if current == commit_id)
            && matches!(session.comparison, LoadState::Loading);
        let branch = session.review.aggregate.as_ref().clone();
        let history_operation = overview_revision
            .map(|(_, operation)| operation)
            .unwrap_or_else(|| {
                session
                    .history
                    .as_ref()
                    .ok()
                    .and_then(|history| history.references.operation.clone())
            });
        self.checkpoint_navigation(cx);
        self.resume_live_refresh();
        self.remember_evolution_header();
        self.remember_reading(cx);
        if already_loading {
            self.load_evolution(revision, window, cx);
            cx.notify();
            return;
        }
        self.invalidate_comparison_request();
        let generation = self.requests.comparison();
        self.requests.comparison_task = None;
        self.finish_loading(LoadingKind::Comparison);
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::Revision {
                commit_id: revision.commit_id.clone(),
                change_id: revision.change_id.clone(),
                working_copy: revision.working_copy,
                merge: revision.parents.len() > 1,
            };
            session.comparison = cached.as_ref().map_or(LoadState::Loading, |comparison| {
                LoadState::Loaded(comparison.clone())
            });
        }
        if cached.is_some() {
            self.rebuild_browser(cx);
            self.follow_open_file(window, cx);
            self.load_evolution(revision, window, cx);
            cx.notify();
            return;
        }
        self.begin_loading(LoadingKind::Comparison, window, cx);
        let revision_for_evolution = revision.clone();
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    branch.for_history_revision(&revision, history_operation.as_deref())
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != generation {
                    return;
                }
                viewer.finish_loading(LoadingKind::Comparison);
                if let LoadState::Loaded(session) = &mut viewer.repository {
                    session.comparison = match result {
                        Ok(comparison) => {
                            let comparison = Rc::new(comparison);
                            session.revision_comparison = Some(comparison.clone());
                            LoadState::Loaded(comparison)
                        }
                        Err(error) => LoadState::Failed(error),
                    };
                }
                viewer.rebuild_browser(cx);
                viewer.follow_open_file(window, cx);
                cx.notify();
            });
        }));
        self.rebuild_browser(cx);
        self.load_evolution(revision_for_evolution, window, cx);
        cx.notify();
    }

    pub(super) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_live(window, cx);
    }
    fn complete_file_load(
        &mut self,
        request: FileLoadRequest,
        result: Result<PreparedFile, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.requests.file() != request.id {
            return;
        }
        let FileLoadRequest {
            source,
            file: metadata,
            reason,
            ..
        } = request;
        let path = metadata.path().to_owned();
        let language = file_language(metadata.path());
        let preserve = reason != FileLoad::Select;
        self.finish_loading(LoadingKind::File);
        if preserve && let Some(snapshot) = self.capture_reading(cx) {
            self.live.restore = Some((path.clone(), snapshot));
        }
        self.view.present(if metadata.kind().is_none() {
            jaywalk::view_choice::FilePresentation::SingleFile
        } else {
            jaywalk::view_choice::FilePresentation::Comparison
        });
        let content = match result {
            Err(error) => LoadState::Failed(error),
            Ok(prepared) => {
                // Keeping the mounted editors preserves their measured wrap map,
                // focus and selection. Identical text needs no viewport reset.
                if reason == FileLoad::Follow
                    && self.selected_path().as_deref() == Some(&path)
                    && self.code().is_some()
                {
                    self.update_followed_file(prepared, window, cx);
                    let selected = self
                        .selected_file
                        .as_mut()
                        .expect("the followed file still owns its editors");
                    selected.file = metadata;
                    selected.source = source;
                    self.rebuild_browser(cx);
                    cx.notify();
                    return;
                }
                self.reveal_after_layout = reason == FileLoad::Select
                    && self.live.restore.is_none()
                    && self.live.pending_position.is_none()
                    && prepared.model.blocks.iter().any(|block| block.changed);
                self.restore_prepared_reading(&path, &prepared.before, &prepared.after);
                LoadState::Loaded(self.mount_file_editors(prepared, language, window, cx))
            }
        };
        self.active_change = None;
        self.navigation_target = None;
        self.selected_file = Some(SelectedFile {
            file: metadata,
            source,
            content,
        });
        self.rebuild_browser(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use gpui::base::input::HighlightStyleResolver;
    use gpui::component::highlighter::PreparedInputHighlighter;
    use gpui::{HighlightStyle, rgb};

    struct TestStyles;
    impl HighlightStyleResolver for TestStyles {
        fn style(&self, _: &str) -> Option<HighlightStyle> {
            Some(HighlightStyle {
                color: Some(rgb(0x3794ff).into()),
                ..Default::default()
            })
        }
    }

    #[test]
    fn rust_file_paths_use_a_real_prepared_parser() {
        let language = super::file_language("src/lib.rs");
        assert_eq!(language, "rust");
        let text = "// Rust syntax\npub fn greeting<'a>(name: &'a str) -> String {\n    format!(\"hello {name} 🦀\")\n}\n";
        let factory = PreparedInputHighlighter::new(language, text).into_factory();
        let highlighter = factory(language).expect("Rust source has its compiled parser");
        let spans = highlighter.styles(&(0..text.len()), &TestStyles);
        assert!(spans.iter().any(|(_, style)| style.color.is_some()));
        assert_eq!(spans.first().unwrap().0.start, 0);
        assert_eq!(spans.last().unwrap().0.end, text.len());
    }

    #[test]
    fn document_and_build_files_have_prepared_syntax_styles() {
        for (path, text) in [
            (
                "README.md",
                "# Heading\n\nA **bold** [link](https://example.invalid).\n\n```rust\npub fn main() {}\n```\n",
            ),
            (
                "Cargo.toml",
                "[package]\nname = \"example\"\nenabled = true\ncount = 42\n",
            ),
            (
                "Makefile",
                "# Build the project\nBIN := example\n.PHONY: build\nbuild:\n\tcargo build --release\n",
            ),
        ] {
            let language = super::file_language(path);
            let factory = PreparedInputHighlighter::new(language, text).into_factory();
            let highlighter = factory(language).expect("bundled file grammar initializes");
            let spans = highlighter.styles(&(0..text.len()), &TestStyles);
            assert!(
                spans.iter().any(|(_, style)| style.color.is_some()),
                "{path}"
            );
            assert_eq!(spans.first().unwrap().0.start, 0, "{path}");
            assert_eq!(spans.last().unwrap().0.end, text.len(), "{path}");
        }
    }

    #[test]
    fn large_comparisons_have_syntax_colors_before_any_editor_update() {
        // Above the live editor's synchronous parse budget. No window, timer,
        // or first paint is needed to obtain complete styles for either text.
        for value in ["before", "after"] {
            let text = format!("export const name = '{value} 🦀';\n").repeat(10_000);
            assert!(text.len() > 256 * 1024);
            let factory = PreparedInputHighlighter::new("typescript", &text).into_factory();
            let highlighter = factory("typescript").expect("typescript has a parser");
            let line_bytes = text.len() / 10_000;
            for range in [0..line_bytes, text.len() - line_bytes..text.len()] {
                let spans = highlighter.styles(&range, &TestStyles);
                assert!(spans.iter().any(|(_, style)| style.color.is_some()));
                assert_eq!(spans.first().unwrap().0.start, range.start);
                assert_eq!(spans.last().unwrap().0.end, range.end);
            }
        }
    }
}

#[cfg(test)]
mod completion_tests {
    use super::{FileLoad, FileLoadRequest, LoadingKind, PreparedFile, SessionLayout, Viewer};
    use crate::viewer::interaction_tests::{Fixture, setup};
    use gpui_test::TestAppContext;

    #[gpui_test::test]
    fn late_file_success_and_failure_leave_the_new_selection_and_loading_intact(
        cx: &mut TestAppContext,
    ) {
        let fixture = Fixture::new();
        std::fs::write(fixture.0.join("second.txt"), "second file\n").unwrap();
        let (viewer, cx) = setup(&fixture, cx);
        let source = cx.update(|_, app| viewer.read(app).comparison().unwrap().clone());
        let first = source.catalog().find("file.txt").unwrap().clone();
        let second = source.catalog().find("second.txt").unwrap().clone();
        for failure in [false, true] {
            let first_result = if failure {
                Err("late file failure".into())
            } else {
                PreparedFile::load(&source, &first, 4)
            };
            let second_result = PreparedFile::load(&source, &second, 4);
            cx.update(|window, app| {
                viewer.update(app, |viewer, cx| {
                    viewer.settings.ui.loading_delay_ms = 0;
                    viewer.load_file(source.clone(), first.clone(), FileLoad::Select, window, cx);
                    let old = FileLoadRequest {
                        id: viewer.requests.file(),
                        source: source.clone(),
                        file: first.clone(),
                        reason: FileLoad::Select,
                    };
                    viewer.load_file(source.clone(), second.clone(), FileLoad::Select, window, cx);
                    let current = FileLoadRequest {
                        id: viewer.requests.file(),
                        source: source.clone(),
                        file: second.clone(),
                        reason: FileLoad::Select,
                    };
                    // Own the completion order here; beginning and applying use the production paths.
                    viewer.requests.file_task = None;
                    viewer.complete_file_load(old, first_result, window, cx);
                    assert_eq!(viewer.selected_path().as_deref(), Some("second.txt"));
                    assert!(viewer.code().is_none());
                    assert!(viewer.loading_visible(LoadingKind::File));
                    viewer.complete_file_load(current, second_result, window, cx);
                    assert!(!viewer.loading_visible(LoadingKind::File));
                    assert_eq!(
                        viewer.code().unwrap().after.read(cx).value().as_ref(),
                        "second file\n"
                    );
                })
            });
        }
    }

    #[gpui_test::test]
    fn file_completion_from_replaced_repository_cannot_overwrite_its_successor(
        cx: &mut TestAppContext,
    ) {
        let first = Fixture::new();
        let second = Fixture::new();
        std::fs::write(second.0.join("file.txt"), "replacement contents\n").unwrap();
        let (viewer, cx) = setup(&first, cx);
        let old = cx.update(|_, app| {
            let viewer = viewer.read(app);
            let selected = viewer.selected_file.as_ref().unwrap();
            FileLoadRequest {
                id: viewer.requests.file(),
                source: selected.source.clone(),
                file: selected.file.clone(),
                reason: FileLoad::Select,
            }
        });
        let result = PreparedFile::load(&old.source, &old.file, 4);
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                let mut args = viewer.arguments.clone();
                args.directory = Some(second.0.clone());
                let settings = viewer.settings.clone();
                let layout = SessionLayout::load(&args, &settings);
                *viewer = Viewer::new(args, settings, layout, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, app| {
            viewer.update(app, |viewer, cx| {
                assert_eq!(
                    viewer.code().unwrap().after.read(cx).value().as_ref(),
                    "replacement contents\n"
                );
                viewer.complete_file_load(old, result, window, cx);
                assert_eq!(
                    viewer.selected_file.as_ref().unwrap().source.root(),
                    second.0.canonicalize().unwrap()
                );
                assert_eq!(
                    viewer.code().unwrap().after.read(cx).value().as_ref(),
                    "replacement contents\n"
                );
            })
        });
    }
}

fn valid_pair_endpoints(comparison: &Comparison) -> bool {
    [comparison.base(), comparison.tip()].iter().all(|id| {
        **id == *"working tree" || (id.len() >= 40 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    })
}
