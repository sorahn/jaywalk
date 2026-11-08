use super::*;

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
        let layout = session_layout::SessionLayout::load(&arguments);
        let mut viewer = Self {
            code_font_size: settings.editor.font_size,
            line_numbers: settings.editor.line_numbers,
            config: settings::ConfigState::new(),
            live: live::LiveState::new(),
            palette,
            layout,
            arguments,
            settings,
            focus,
            panels: panels::PanelLayout::new(cx),
            history_scroll: UniformListScrollHandle::new(),
            evolution,
            file_evolution,
            active_evolution: evolution::EvolutionKind::Change,
            repository: LoadState::Loading,
            selected_file: None,
            browser: files::FileBrowser::new(file_width, file_mode, cx),
            repository_generation: 0,
            file_generation: 0,
            repository_task: None,
            file_task: None,
            active_change: None,
            navigation_target: None,
            view_mode,
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
        self.evolution.clear();
        self.file_evolution.clear();
        self.active_evolution = evolution::EvolutionKind::Change;
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.clear_file();
        self.repository = LoadState::Loading;
        let arguments = self.arguments.clone();
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let comparison = arguments.comparison()?;
                    if !comparison.supports_evolution() {
                        let history = if arguments.diff.is_some() {
                            jaywalk::history::History::parse("")
                        } else {
                            comparison.history()
                        };
                        return Ok((comparison, history, None));
                    }
                    let history = comparison.history();
                    let tip_expression = if arguments.tip == "@" {
                        let revision = comparison.revision(&comparison.tip)?;
                        Some(if revision.bookmarks.len() == 1 {
                            revision.bookmarks[0].clone()
                        } else {
                            revision.change_id
                        })
                    } else {
                        Some(arguments.tip.clone())
                    };
                    Ok::<_, String>((comparison, history, tip_expression))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
                viewer.repository = match result {
                    Ok((comparison, history, tip_expression)) => {
                        if viewer.live.tip_expression.is_none() {
                            viewer.live.tip_expression = tip_expression;
                        }
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
                viewer.start_live(window, cx);
                viewer.rebuild_browser(cx);
                let preferred_path = preferred_path.or_else(|| {
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
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn select_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.comparison().and_then(|comparison| {
            comparison
                .files
                .iter()
                .position(|file| file.after_path == path)
        }) {
            self.select_file(index, window, cx);
        }
    }

    pub(super) fn select_file(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(comparison) = self.comparison() else {
            return;
        };
        let Some(file) = comparison.files.get(index).cloned() else {
            return;
        };
        let comparison = comparison.clone();
        self.remember_reading(cx);
        self.recall_reading(&comparison, &file.after_path);
        self.clear_file();
        let generation = self.file_generation;
        self.selected_file = Some(SelectedFile {
            index,
            content: LoadState::Loading,
        });
        self.focus_evolution_file(Some(&file.after_path));
        self.select_browser_file(index, cx);
        self.fluid = FluidScroll::default();
        self.horizontal_position = 0.;
        self.reveal_after_layout = false;
        let tab_size = self.settings.editor.tab_size;
        self.file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let extension = std::path::Path::new(&file.after_path)
                .extension()
                .and_then(|value| value.to_str());
            let language = match extension {
                Some("ts") => "typescript",
                Some("tsx") => "tsx",
                Some("js" | "jsx") => "javascript",
                _ => "plain",
            };
            let path = file.after_path.clone();
            let result = cx
                .background_spawn(async move { PreparedFile::load(&comparison, &file, tab_size) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.file_generation != generation {
                    return;
                }
                let content = match result {
                    Err(error) => LoadState::Failed(error),
                    Ok(prepared) => {
                        viewer.restore_prepared_reading(&path, &prepared.before, &prepared.after);
                        let standard = prepared.standard.map(|files| {
                            if files.is_empty() {
                                return None;
                            }
                            let mode = standard_mode(viewer.view_mode);
                            Some(cx.new(|cx| {
                                DiffState::new(
                                    files.into_iter().map(|file| file.with_language(language)),
                                    cx,
                                )
                                .with_context_lines(None)
                                .with_mode(mode)
                            }))
                        });
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
                        let before = make_editor(prepared.before, viewer.wrapping.before, cx);
                        let after = make_editor(prepared.after, viewer.wrapping.after, cx);
                        for (state, ranges, color) in [
                            (&before, &prepared.model.before_words, 0xf8514955),
                            (&after, &prepared.model.after_words, 0x4ec96e55),
                        ] {
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
                                );
                            });
                        }
                        if viewer.view_mode != ViewMode::Fluid
                            && let Some((_, line, _)) = viewer.live.pending_position.take()
                            && let Ok(Some(state)) = &standard
                        {
                            state.update(cx, |state, cx| {
                                if let Some(path) =
                                    state.files().first().map(|file| file.path().clone())
                                {
                                    state.scroll_to_line(
                                        gpui::component::diff::DiffLinePosition::new(
                                            path,
                                            gpui::component::diff::DiffSide::Modified,
                                            line + 1,
                                        ),
                                        cx,
                                    );
                                }
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
                            standard,
                        })
                    }
                };
                viewer.selected_file = Some(SelectedFile { index, content });
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn show_whole_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // There is no recorded branch to restore until the initial snapshot
        // finishes. Cancelling that task here would leave a loading view.
        if self.repository.ready().is_none() {
            return;
        }
        self.remember_reading(cx);
        let preferred_path = self.live.branch_file.clone();
        self.evolution.clear();
        self.file_evolution.clear();
        self.active_evolution = evolution::EvolutionKind::Change;
        self.repository_generation += 1;
        self.repository_task = None;
        self.clear_file();
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::WholeBranch;
            session.comparison = LoadState::Loaded(session.branch.clone());
        }
        self.rebuild_browser(cx);
        if let Some(path) = preferred_path {
            self.select_path(&path, window, cx);
        }
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
        // A graph click selects the whole commit, not the last file inspected
        // in it. Cancel stale file completion before clearing the file focus.
        self.remember_reading(cx);
        self.clear_file();
        self.focus_evolution_file(None);
        if already_loading {
            self.load_evolution(revision, window, cx);
            cx.notify();
            return;
        }
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.repository_task = None;
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::Revision {
                commit_id: revision.commit_id.clone(),
                merge: revision.parents.len() > 1,
            };
            session.comparison = cached.as_ref().map_or(LoadState::Loading, |comparison| {
                LoadState::Loaded(comparison.clone())
            });
        }
        if cached.is_some() {
            self.rebuild_browser(cx);
            self.load_evolution(revision, window, cx);
            cx.notify();
            return;
        }
        let revision_for_evolution = revision.clone();
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { branch.for_revision(&revision) })
                .await;
            let _ = this.update_in(cx, |viewer, _, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
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
                cx.notify();
            });
        }));
        self.load_evolution(revision_for_evolution, window, cx);
        cx.notify();
    }

    pub(super) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_live(window, cx);
    }
}
