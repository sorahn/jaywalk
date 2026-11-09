use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileLoad {
    Select,
    Follow,
    Reflow,
}

impl Viewer {
    pub(crate) fn new(
        arguments: Arguments,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
        let layout = session_layout::SessionLayout::load(&arguments, &settings);
        let references = references::ReferenceState::new(&settings.history);
        let mut viewer = Self {
            code_font_size: settings.editor.font_size,
            line_numbers: settings.editor.line_numbers,
            config: settings::ConfigState::new(),
            live: live::LiveState::new(),
            feedback: feedback::Feedback::default(),
            palette,
            layout,
            arguments,
            settings,
            focus,
            panels: panels::PanelLayout::new(cx),
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
            repository_generation: 0,
            file_generation: 0,
            repository_task: None,
            file_task: None,
            active_change: None,
            navigation_target: None,
            view: jaywalk::view_choice::ViewChoice::new(view_mode),
            wrapping,
            horizontal_position: 0.,
            reveal_after_layout: false,
            fluid: FluidScroll::default(),
            editor_observers: Vec::new(),
        };
        viewer.apply_saved_layout();
        if viewer.arguments.diff.is_some() {
            viewer.layout.saved.history_hidden = true;
            viewer.evolution.visible = false;
            viewer.file_evolution.visible = false;
        }
        viewer.start_settings_reload(window, cx);
        viewer.initialize_browser(window, cx);
        viewer.load_repository(preferred_path, window, cx);
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
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.remember_reading(cx);
        self.repository = LoadState::Loading;
        self.begin_loading(LoadingKind::Comparison, window, cx);
        self.rebuild_browser(cx);
        let arguments = self.arguments.clone();
        let history_query = self.history_query.clone();
        let requested_history_query = history_query.clone();
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let comparison = arguments.comparison()?;
                    let history = if arguments.diff.is_some() {
                        jaywalk::history::History::parse("")
                    } else {
                        comparison.history(&history_query)
                    };
                    Ok::<_, String>((comparison, history))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
                viewer.finish_loading(LoadingKind::Comparison);
                viewer.repository = match result {
                    Ok((comparison, history)) => {
                        if !comparison.supports_evolution() {
                            viewer.evolution.visible = false;
                            viewer.file_evolution.visible = false;
                        }
                        let branch = Rc::new(comparison);
                        LoadState::Loaded(RepositorySession {
                            branch: branch.clone(),
                            history: history.map(Rc::new),
                            scope: ComparisonScope::WholeBranch,
                            comparison: LoadState::Loaded(branch),
                            revision_comparison: None,
                        })
                    }
                    Err(error) => LoadState::Failed(error),
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
                                .filter(|comparison| comparison.files.len() == 1)
                                .map(|comparison| comparison.files[0].after_path.clone())
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
            && let Some(file) = comparison.catalog.find(path).cloned()
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
            .and_then(|comparison| comparison.files.get(index))
            .map(|file| file.after_path.clone())
        {
            self.select_path(&path, window, cx);
        }
    }

    pub(super) fn follow_open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.selected_path()
            && let Some(comparison) = self.comparison().cloned()
            && let Some(file) = comparison.catalog.find(&path).cloned()
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

    fn load_file(
        &mut self,
        comparison: Comparison,
        file: jaywalk::repository_file::FileEntry,
        reason: FileLoad,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preserve = reason != FileLoad::Select;
        self.remember_reading(cx);
        if preserve {
            self.file_generation += 1;
        } else {
            self.clear_file();
            self.selected_file = Some(SelectedFile {
                file: file.clone(),
                source: comparison.clone(),
                content: LoadState::Loading,
            });
            self.fluid = FluidScroll::default();
            self.horizontal_position = 0.;
        }
        self.begin_loading(LoadingKind::File, window, cx);
        self.recall_reading(file.path());
        let generation = self.file_generation;
        let repository_generation = self.repository_generation;
        self.focus_evolution_file(Some(file.path()));
        self.rebuild_browser(cx);
        if reason == FileLoad::Select {
            self.reveal_browser_selection(cx);
        }
        self.reveal_after_layout = false;
        let source = comparison.clone();
        let metadata = file.clone();
        let tab_size = self.settings.editor.tab_size;
        self.file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let language = file_language(file.path());
            let path = file.path().to_owned();
            let result = cx
                .background_spawn(async move { PreparedFile::load(&comparison, &file, tab_size) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.file_generation != generation
                    || viewer.repository_generation != repository_generation
                {
                    return;
                }
                viewer.finish_loading(LoadingKind::File);
                if preserve && let Some(snapshot) = viewer.capture_reading(cx) {
                    viewer.live.restore = Some((path.clone(), snapshot));
                }
                viewer.view.present(if metadata.kind().is_none() {
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
                            && viewer.selected_path().as_deref() == Some(&path)
                            && viewer.code().is_some()
                        {
                            viewer.update_followed_file(prepared, window, cx);
                            let selected = viewer
                                .selected_file
                                .as_mut()
                                .expect("the followed file still owns its editors");
                            selected.file = metadata;
                            selected.source = source;
                            viewer.rebuild_browser(cx);
                            cx.notify();
                            return;
                        }
                        viewer.restore_prepared_reading(&path, &prepared.before, &prepared.after);
                        let standard = prepared.standard.map(|files| {
                            if files.is_empty() {
                                return None;
                            }
                            let mode = standard_mode(viewer.view.effective());
                            Some(cx.new(|cx| {
                                DiffState::new(
                                    files.into_iter().map(|file| file.with_language(language)),
                                    cx,
                                )
                                .with_context_lines(None)
                                .with_mode(mode)
                            }))
                        });
                        let [before_wrap, after_wrap] = viewer.editor_wraps();
                        let mut make_editor = |text, wrap, cx: &mut Context<Self>| {
                            cx.new(|cx| {
                                EditorState::new(window, cx)
                                    .language(language)
                                    .line_number(viewer.line_numbers)
                                    .tab_size(gpui::base::input::TabSize {
                                        tab_size: viewer.settings.editor.tab_size,
                                        hard_tabs: false,
                                    })
                                    .soft_wrap(wrap)
                                    .folding(false)
                                    .default_value(text)
                            })
                        };
                        let before = make_editor(prepared.before, before_wrap, cx);
                        let after = make_editor(prepared.after, after_wrap, cx);
                        for (state, syntax) in [&before, &after].into_iter().zip(prepared.syntax) {
                            state.update(cx, |editor, cx| {
                                editor.set_highlighter_factory(syntax.into_factory(), cx)
                            });
                        }
                        let decorations = [
                            (&before, &prepared.model.before_words, 0xf8514955),
                            (&after, &prepared.model.after_words, 0x4ec96e55),
                        ]
                        .map(|(state, ranges, color)| {
                            state.update(cx, |editor, cx| {
                                editor.create_decorations_collection(
                                    ranges
                                        .iter()
                                        .cloned()
                                        .map(|range| {
                                            gpui::base::input::TextDecoration::new(
                                                range,
                                                HighlightStyle {
                                                    background_color: Some(rgba(color).into()),
                                                    ..Default::default()
                                                },
                                            )
                                        })
                                        .collect(),
                                    cx,
                                )
                            })
                        });
                        if viewer.view.effective() != ViewMode::Fluid
                            && let Some((before, after, horizontal)) =
                                viewer.live.pending_position.take()
                            && let Ok(Some(state)) = &standard
                            && let Some(path) = state
                                .read(cx)
                                .files()
                                .first()
                                .map(|file| file.path().clone())
                        {
                            let side = viewer.live.standard_side;
                            let line = if side == gpui::component::diff::DiffSide::Original {
                                before
                            } else {
                                after
                            };
                            viewer.config.pending_horizontal = Some(horizontal);
                            state.update(cx, |state, cx| {
                                state.scroll_to_line(
                                    gpui::component::diff::DiffLinePosition::new(
                                        path,
                                        side,
                                        line + 1,
                                    ),
                                    cx,
                                )
                            });
                        }
                        viewer.editor_observers = vec![
                            cx.observe(&before, |_, _, cx| cx.notify()),
                            cx.observe(&after, |_, _, cx| cx.notify()),
                        ];
                        LoadState::Loaded(CodeComparison {
                            before,
                            after,
                            model: Rc::new(prepared.model),
                            decorations,
                            standard,
                        })
                    }
                };
                viewer.active_change = None;
                viewer.navigation_target = None;
                viewer.selected_file = Some(SelectedFile {
                    file: metadata,
                    source,
                    content,
                });
                viewer.rebuild_browser(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn update_followed_file(
        &mut self,
        prepared: PreparedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.selected_path().expect("a followed file has a path");
        let code = self.code().expect("a followed file has mounted editors");
        let before = code.before.clone();
        let after = code.after.clone();
        let wraps = self.editor_wraps();
        for (state, wrap) in [(&before, wraps[0]), (&after, wraps[1])] {
            state.update(cx, |editor, cx| editor.set_soft_wrap(wrap, window, cx));
        }
        if before.read(cx).value().as_ref() == prepared.before
            && after.read(cx).value().as_ref() == prepared.after
        {
            self.restore_prepared_reading(&path, &prepared.before, &prepared.after);
            return;
        }
        self.restore_prepared_reading(&path, &prepared.before, &prepared.after);
        for ((state, text), syntax) in [(&before, prepared.before), (&after, prepared.after)]
            .into_iter()
            .zip(prepared.syntax)
        {
            if state.read(cx).value().as_ref() != text {
                state.update(cx, |editor, cx| {
                    editor.set_value(text, window, cx);
                    editor.set_highlighter_factory(syntax.into_factory(), cx);
                });
            }
        }
        let code = self
            .code()
            .expect("updating text keeps its editor entities");
        for (collection, ranges, color) in [
            (
                &code.decorations[0],
                &prepared.model.before_words,
                0xf8514955,
            ),
            (
                &code.decorations[1],
                &prepared.model.after_words,
                0x4ec96e55,
            ),
        ] {
            collection.set(
                ranges
                    .iter()
                    .cloned()
                    .map(|range| {
                        gpui::base::input::TextDecoration::new(
                            range,
                            HighlightStyle {
                                background_color: Some(rgba(color).into()),
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
                cx,
            );
        }
        let presentation_changed = self
            .selected_file
            .as_ref()
            .is_some_and(|selected| selected.file.kind().is_none())
            != self.view.is_single_file();
        // A different pane width needs the new frame's measured wrap map. Leave
        // its source anchors for the fluid/standard overlay to restore together.
        if !presentation_changed
            && (self.view.effective() == ViewMode::Fluid || self.view.is_single_file())
            && let Some((left, right, horizontal)) = self.live.pending_position.take()
        {
            // set_value updates the cached wrap map synchronously. Restore
            // against it before prepaint, rather than after a frame at line 1.
            let geometry = comparison_geometry(before.read(cx), after.read(cx), &prepared.model);
            self.fluid.position = geometry.position_for_line(&prepared.model, left, true);
            for (state, positions, line) in [
                (
                    &before,
                    &geometry.before,
                    if self.view.is_single_file() {
                        right
                    } else {
                        left
                    },
                ),
                (&after, &geometry.after, right),
            ] {
                state.update(cx, |editor, cx| {
                    editor.set_scroll_offset(
                        point(
                            px(-horizontal),
                            px(-positions[line.min(positions.len() - 1)]),
                        ),
                        cx,
                    )
                });
            }
            self.horizontal_position = horizontal;
        }
        let standard = match prepared.standard {
            Ok(files) if files.is_empty() => Ok(None),
            Ok(files) => {
                let language = before.read(cx).language_name();
                let mode = standard_mode(self.view.effective());
                let state = cx.new(|cx| {
                    DiffState::new(
                        files
                            .into_iter()
                            .map(|file| file.with_language(language.clone())),
                        cx,
                    )
                    .with_context_lines(None)
                    .with_mode(mode)
                });
                if self.view.effective() != ViewMode::Fluid
                    && let Some((left, right, horizontal)) = self.live.pending_position.take()
                    && let Some(path) = state
                        .read(cx)
                        .files()
                        .first()
                        .map(|file| file.path().clone())
                {
                    let side = self.live.standard_side;
                    let line = if side == gpui::component::diff::DiffSide::Original {
                        left
                    } else {
                        right
                    };
                    state.update(cx, |state, cx| {
                        state.scroll_to_line(
                            gpui::component::diff::DiffLinePosition::new(path, side, line + 1),
                            cx,
                        );
                    });
                    self.config.pending_horizontal = Some(horizontal);
                }
                Ok(Some(state))
            }
            Err(error) => Err(error),
        };
        if let Some(SelectedFile {
            content: LoadState::Loaded(code),
            ..
        }) = &mut self.selected_file
        {
            code.model = Rc::new(prepared.model);
            code.standard = standard;
        }
        self.active_change = None;
        self.navigation_target = None;
    }

    pub(super) fn show_whole_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // There is no recorded branch to restore until the initial snapshot
        // finishes. Cancelling that task here would leave a loading view.
        if self.repository.ready().is_none() {
            return;
        }
        self.remember_reading(cx);
        self.invalidate_history_reload();
        self.resume_live_refresh();
        self.remember_evolution_header();
        self.evolution.clear();
        self.file_evolution.clear();
        self.active_evolution = evolution::EvolutionKind::Change;
        self.repository_generation += 1;
        self.repository_task = None;
        self.finish_loading(LoadingKind::Comparison);
        self.finish_loading(LoadingKind::Evolution);
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::WholeBranch;
            session.comparison = LoadState::Loaded(session.branch.clone());
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
        let Ok(history) = &session.history else {
            return;
        };
        let Some(revision) = history
            .revisions
            .iter()
            .find(|revision| revision.commit_id == commit_id)
            .cloned()
        else {
            return;
        };
        let cached = session
            .revision_comparison
            .as_ref()
            .filter(|comparison| comparison.tip == commit_id)
            .cloned();
        let already_loading = matches!(&session.scope, ComparisonScope::Revision {commit_id: current, ..} if current == commit_id)
            && matches!(session.comparison, LoadState::Loading);
        let branch = session.branch.as_ref().clone();
        let history_operation = history.references.operation.clone();
        self.resume_live_refresh();
        self.remember_evolution_header();
        self.remember_reading(cx);
        if already_loading {
            self.load_evolution(revision, window, cx);
            cx.notify();
            return;
        }
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.repository_task = None;
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
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    branch.for_history_revision(&revision, history_operation.as_deref())
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
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
