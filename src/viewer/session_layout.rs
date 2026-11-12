//! Persist only local layout choices, leaving user TOML settings untouched.
use super::*;
use jaywalk::layout_state::{LayoutState, RepositoryLayoutStore};
use std::path::PathBuf;
pub(crate) struct SessionLayout {
    pub(super) saved: LayoutState,
    path: Option<PathBuf>,
    repository: PathBuf,
    pub(super) error: Option<String>,
}
impl SessionLayout {
    pub(crate) fn load(arguments: &Arguments, settings: &Settings) -> Self {
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
            && !arguments.welcome
            && arguments.diff.is_none()
            && let Some(path) = &path
        {
            match RepositoryLayoutStore::new(path.clone(), arguments.directory())
                .and_then(|store| store.read())
            {
                Ok(Some(state)) => saved = state,
                Ok(None) => {}
                Err(message) => error = Some(message),
            }
        }
        Self {
            path,
            repository: arguments.directory().to_owned(),
            saved,
            error,
        }
    }
}
impl SessionLayout {
    pub(crate) fn use_latest(&mut self, settings: &Settings, cx: &App) {
        if settings.layout.remember_per_repository
            && let Some(path) = &self.path
            && let Some(saved) = crate::interface_state::latest(path, &self.repository, cx)
        {
            self.saved = saved;
        }
    }
}
impl Viewer {
    pub(super) fn apply_saved_layout(&mut self) {
        if let Some(labels) = self.layout.saved.history_labels {
            self.references.labels = labels;
        }
        if let Some(remotes) = &self.layout.saved.history_remotes {
            self.references.visibility = remotes.clone();
        }
        self.history_query =
            jaywalk::history::HistoryQuery::sources(self.references.visibility.clone(), None)
                .expect("saved visibility has no commit expression");
        if let Some(visible) = self.layout.saved.change_evolution {
            self.evolution.visible = visible;
        }
        if let Some(visible) = self.layout.saved.file_evolution {
            self.file_evolution.visible = visible;
        }
        if let Some(view) = self.layout.saved.view {
            self.view.select(view);
        }
        if let Some(view) = self.layout.saved.file_view {
            self.browser.mode = view;
        }
        if let Some(scope) = self.layout.saved.file_scope {
            self.browser.scope = scope;
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
        if self.is_welcome() {
            return;
        }
        if !self.settings.layout.remember_per_repository || self.arguments.diff.is_some() {
            return;
        }
        self.layout.saved.history_labels = Some(self.references.labels);
        self.layout.saved.history_remotes = Some(self.references.visibility.clone());
        self.layout.saved.view = Some(self.view.preferred());
        self.layout.saved.file_view = Some(self.browser.mode);
        self.layout.saved.file_scope = Some(self.browser.scope);
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
        if let Some(path) = &self.layout.path {
            let key = crate::interface_state::StoreKey {
                path: path.clone(),
                repository: self.layout.repository.clone(),
            };
            if let Err(error) = crate::interface_state::save(key, self.layout.saved.clone(), cx) {
                self.layout.error = Some(error);
                cx.notify();
            }
        }
    }
    pub(crate) fn layout_saved(
        &mut self,
        key: &crate::interface_state::StoreKey,
        error: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.layout.path.as_ref() == Some(&key.path) && self.layout.repository == key.repository
        {
            self.layout.error = error;
            cx.notify();
        }
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
            let active = if history {
                !self.layout.saved.history_hidden
            } else {
                !self.layout.saved.files_hidden
            };
            controls =
                controls.child(
                    Button::new(if history {
                        "toggle-history-panel"
                    } else {
                        "toggle-files-panel"
                    })
                    .ghost()
                    .compact()
                    .icon(crate::appearance::active_icon(
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
                        active,
                        cx,
                    ))
                    .tooltip(label)
                    .toggled(active)
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
                    .icon(crate::appearance::active_icon(
                        IconName::PanelBottom,
                        self.evolution.visible || self.file_evolution.visible,
                        cx,
                    ))
                    .tooltip("Toggle evolution panels")
                    .toggled(self.evolution.visible || self.file_evolution.visible)
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
