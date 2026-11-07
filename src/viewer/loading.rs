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
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut viewer = Self {
            arguments,
            settings,
            focus,
            repository: LoadState::Loading,
            selected_file: None,
            browser: files::FileBrowser::new(file_width, cx),
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
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.clear_file();
        self.repository = LoadState::Loading;
        let arguments = self.arguments.clone();
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let comparison = Comparison::load(
                        &arguments.directory,
                        arguments.base.as_deref(),
                        &arguments.tip,
                    )?;
                    let history = comparison.history();
                    Ok::<_, String>((comparison, history))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
                viewer.repository = match result {
                    Ok((comparison, history)) => {
                        let branch = Rc::new(comparison);
                        LoadState::Loaded(RepositorySession {
                            branch: branch.clone(),
                            history: history.map(Rc::new),
                            scope: ComparisonScope::WholeBranch,
                            comparison: LoadState::Loaded(branch),
                        })
                    }
                    Err(error) => LoadState::Failed(error),
                };
                viewer.rebuild_browser(cx);
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
        self.clear_file();
        let generation = self.file_generation;
        self.selected_file = Some(SelectedFile {
            index,
            content: LoadState::Loading,
        });
        self.select_browser_file(index, cx);
        self.fluid = FluidScroll::default();
        self.horizontal_position = 0.;
        self.reveal_after_layout = false;
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
            let result = cx
                .background_spawn(async move { PreparedFile::load(&comparison, &file) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.file_generation != generation {
                    return;
                }
                let content = match result {
                    Err(error) => LoadState::Failed(error),
                    Ok(prepared) => {
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
                                    .soft_wrap(wrap)
                                    .folding(false)
                                    .default_value(text)
                            })
                        };
                        let before = make_editor(prepared.before, viewer.wrapping.before, cx);
                        let after = make_editor(prepared.after, viewer.wrapping.after, cx);
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
        let preferred_path = self
            .selected_index()
            .and_then(|index| self.comparison()?.files.get(index))
            .map(|file| file.after_path.clone());
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
        let branch = session.branch.as_ref().clone();
        let preferred_path = self
            .selected_index()
            .and_then(|index| self.comparison()?.files.get(index))
            .map(|file| file.after_path.clone());
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.clear_file();
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::Revision {
                commit_id: revision.commit_id.clone(),
                merge: revision.parents.len() > 1,
            };
            session.comparison = LoadState::Loading;
        }
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { branch.for_revision(&revision) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
                if let LoadState::Loaded(session) = &mut viewer.repository {
                    session.comparison = match result {
                        Ok(comparison) => LoadState::Loaded(Rc::new(comparison)),
                        Err(error) => LoadState::Failed(error),
                    };
                }
                viewer.rebuild_browser(cx);
                if let Some(path) = preferred_path {
                    viewer.select_path(&path, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferred_path = self
            .selected_index()
            .and_then(|index| self.comparison()?.files.get(index))
            .map(|file| file.after_path.clone());
        self.load_repository(preferred_path, window, cx);
    }
}
