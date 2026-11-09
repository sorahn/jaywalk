//! Persist only local layout choices, leaving user TOML settings untouched.
use super::*;
use jaywalk::layout_state::{LayoutState, RepositoryLayoutStore};
use std::path::PathBuf;
use std::time::Duration;
pub(super) struct SessionLayout {
    pub(super) saved: LayoutState,
    path: Option<PathBuf>,
    repository: PathBuf,
    pub(super) error: Option<String>,
    saving: bool,
    revision: u64,
    task: Option<Task<()>>,
}
impl SessionLayout {
    pub(super) fn load(arguments: &Arguments, settings: &Settings) -> Self {
        let path = arguments
            .config
            .as_ref()
            .map(|path| Ok(path.with_extension("interface.toml")))
            .unwrap_or_else(|| {
                jaywalk::settings::default_path().map(|p| p.with_file_name("interface.toml"))
            });
        let mut saved = LayoutState::from_settings(settings);
        let mut error = None;
        let path = match path {
            Ok(path) => Some(path),
            Err(message) => {
                error = Some(message);
                None
            }
        };
        if settings.layout.remember_per_repository
            && arguments.diff.is_none()
            && let Some(path) = &path
        {
            match RepositoryLayoutStore::new(path.clone(), &arguments.directory)
                .and_then(|store| store.read())
            {
                Ok(Some(state)) => saved = state,
                Ok(None) => {}
                Err(message) => error = Some(message),
            }
        }
        Self {
            path,
            repository: arguments.directory.clone(),
            saved,
            error,
            saving: false,
            revision: 0,
            task: None,
        }
    }
}
impl Viewer {
    pub(super) fn apply_saved_layout(&mut self) {
        if let Some(visible) = self.layout.saved.change_evolution {
            self.evolution.visible = visible;
        }
        if let Some(visible) = self.layout.saved.file_evolution {
            self.file_evolution.visible = visible;
        }
        if let Some(view) = self.layout.saved.view {
            self.view_mode = view;
        }
        if let Some(view) = self.layout.saved.file_view {
            self.browser.mode = view;
        }
        if let Some(wrap) = self.layout.saved.wrap_before {
            self.wrapping.before = wrap;
        }
        if let Some(wrap) = self.layout.saved.wrap_after {
            self.wrapping.after = wrap;
        }
        if let Some(wrap) = self.layout.saved.wrap_inline {
            self.wrapping.inline = wrap;
        }
        if let Some(numbers) = self.layout.saved.line_numbers {
            self.line_numbers = numbers;
        }
        for (panel, grouping, rebases) in [
            (
                &mut self.evolution,
                self.layout.saved.change_grouping,
                self.layout.saved.change_rebases,
            ),
            (
                &mut self.file_evolution,
                self.layout.saved.file_grouping,
                self.layout.saved.file_rebases,
            ),
        ] {
            if let Some(grouping) = grouping {
                panel.collapse_repeats = grouping;
            }
            if let Some(rebases) = rebases {
                panel.show_unchanged_rebases = rebases;
            }
        }
    }
    pub(super) fn save_layout(&mut self, cx: &mut Context<Self>) {
        if !self.settings.layout.remember_per_repository || self.arguments.diff.is_some() {
            self.layout.task = None;
            self.layout.saving = false;
            return;
        }
        self.layout.saved.view = Some(self.view_mode);
        self.layout.saved.file_view = Some(self.browser.mode);
        self.layout.saved.wrap_before = Some(self.wrapping.before);
        self.layout.saved.wrap_after = Some(self.wrapping.after);
        self.layout.saved.wrap_inline = Some(self.wrapping.inline);
        self.layout.saved.line_numbers = Some(self.line_numbers);
        self.layout.saved.change_grouping = Some(self.evolution.collapse_repeats);
        self.layout.saved.file_grouping = Some(self.file_evolution.collapse_repeats);
        self.layout.saved.change_rebases = Some(self.evolution.show_unchanged_rebases);
        self.layout.saved.file_rebases = Some(self.file_evolution.show_unchanged_rebases);
        self.layout.saved.change_evolution = Some(self.evolution.visible);
        self.layout.saved.file_evolution = Some(self.file_evolution.visible);
        self.layout.revision += 1;
        if self.layout.saving {
            return;
        }
        let Some(path) = self.layout.path.clone() else {
            return;
        };
        let repository = self.layout.repository.clone();
        self.layout.saving = true;
        self.layout.task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let Ok((saved, revision)) = this.update(cx, |viewer, _| {
                    (viewer.layout.saved.clone(), viewer.layout.revision)
                }) else {
                    break;
                };
                let path = path.clone();
                let repository = repository.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        RepositoryLayoutStore::new(path, &repository)
                            .and_then(|store| store.save(&saved))
                    })
                    .await;
                let Ok(again) = this.update(cx, |viewer, cx| {
                    match result {
                        Err(error) => {
                            viewer.layout.error = Some(error);
                            cx.notify();
                        }
                        Ok(()) => {
                            if viewer.layout.error.take().is_some() {
                                cx.notify();
                            }
                        }
                    }
                    let again = viewer.layout.revision != revision;
                    if !again {
                        viewer.layout.saving = false;
                    }
                    again
                }) else {
                    break;
                };
                if !again {
                    break;
                }
            }
        }));
    }
    pub(super) fn toggle_sidebar(
        &mut self,
        history: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preserve_reflow_position(cx);
        self.capture_sidebar_widths(cx);
        if history {
            self.layout.saved.history_hidden = !self.layout.saved.history_hidden;
        } else {
            self.layout.saved.files_hidden = !self.layout.saved.files_hidden;
        }
        self.rebuild_sidebar_layout(cx);
        self.save_layout(cx);
        cx.notify();
    }
    pub(super) fn sidebar_width(&self, history: bool) -> f32 {
        let (saved, settings) = if history {
            (
                self.layout.saved.history_width,
                &self.settings.panels.history,
            )
        } else {
            (self.layout.saved.files_width, &self.settings.panels.files)
        };
        saved
            .unwrap_or(settings.default_width)
            .clamp(settings.min_width, settings.max_width)
    }
    pub(super) fn evolution_height(&self, file: bool) -> f32 {
        if file {
            let settings = &self.settings.file_evolution;
            self.layout
                .saved
                .file_height
                .unwrap_or(settings.default_height)
                .clamp(settings.min_height, settings.max_height)
        } else {
            let settings = &self.settings.evolution;
            self.layout
                .saved
                .change_height
                .unwrap_or(settings.default_height)
                .clamp(settings.min_height, settings.max_height)
        }
    }
    pub(super) fn render_panel_toggles(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut controls = div().h_flex().items_center().gap_1();
        for (history, label) in [
            (true, "Toggle history panel"),
            (false, "Toggle changed files panel"),
        ] {
            controls =
                controls.child(
                    Button::new(if history {
                        "toggle-history-panel"
                    } else {
                        "toggle-files-panel"
                    })
                    .ghost()
                    .compact()
                    .icon(
                        if (if history {
                            self.placement().history_side
                        } else {
                            self.placement().files_side
                        }) == jaywalk::panel_layout::Side::Left
                        {
                            IconName::PanelLeft
                        } else {
                            IconName::PanelRight
                        },
                    )
                    .tooltip(label)
                    .selected(if history {
                        !self.layout.saved.history_hidden
                    } else {
                        !self.layout.saved.files_hidden
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_sidebar(history, window, cx)
                    })),
                );
        }
        controls
            .child(
                Button::new("toggle-timeline-panels")
                    .ghost()
                    .compact()
                    .icon(IconName::PanelBottom)
                    .tooltip("Toggle evolution panels")
                    .selected(self.evolution.visible || self.file_evolution.visible)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let visible = !(this.evolution.visible || this.file_evolution.visible);
                        if this.evolution.visible != visible {
                            this.toggle_evolution(window, cx);
                        }
                        if this.file_evolution.visible != visible {
                            this.toggle_evolution_panel(evolution::EvolutionKind::File, window, cx);
                        }
                    })),
            )
            .into_any_element()
    }
}
