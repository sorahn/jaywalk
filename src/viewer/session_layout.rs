//! Persist only local layout choices, leaving user TOML settings untouched.
use super::*;
use gpui::component::Sizable;
use jaywalk::layout_state::{LayoutState, RepositoryLayoutStore};
use std::path::PathBuf;
pub(crate) struct SessionLayout {
    pub(super) saved: LayoutState,
    path: Option<PathBuf>,
    repository: Option<PathBuf>,
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
        let repository =
            arguments
                .interface_repository()
                .and_then(|root| match std::fs::canonicalize(root) {
                    Ok(root) => Some(root),
                    Err(message) => {
                        error = Some(format!(
                            "Could not identify repository {} for interface choices: {message}",
                            root.display()
                        ));
                        None
                    }
                });
        if settings.layout.remember_per_repository
            && let (Some(path), Some(repository)) = (&path, &repository)
        {
            match RepositoryLayoutStore::new(path.clone(), repository)
                .and_then(|store| store.read())
            {
                Ok(Some(state)) => saved = state,
                Ok(None) => {}
                Err(message) => error = Some(message),
            }
        }
        Self {
            path,
            repository,
            saved,
            error,
        }
    }
}
impl SessionLayout {
    pub(crate) fn use_latest(&mut self, settings: &Settings, cx: &App) {
        if settings.layout.remember_per_repository
            && let (Some(path), Some(repository)) = (&self.path, &self.repository)
            && let Some(saved) = crate::interface_state::latest(path, repository, cx)
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
                .expect("saved visibility has no commit expression")
                .with_limit(self.settings.history.page_size)
                .expect("validated page size");
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
        if let Some(collapse) = self.layout.saved.collapse_unchanged {
            self.collapse_unchanged = collapse;
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
        if !self.settings.layout.remember_per_repository {
            return;
        }
        let Some(repository) = self.layout.repository.clone() else {
            return;
        };
        self.layout.saved.history_labels = Some(self.references.labels);
        self.layout.saved.history_remotes = Some(self.references.visibility.clone());
        self.layout.saved.view = Some(self.view.preferred());
        self.layout.saved.file_view = Some(self.browser.mode);
        self.layout.saved.file_scope = Some(self.browser.scope);
        self.layout.saved.wrap_before = Some(self.wrapping.before);
        self.layout.saved.wrap_after = Some(self.wrapping.after);
        self.layout.saved.wrap_inline = Some(self.wrapping.inline);
        self.layout.saved.line_numbers = Some(self.line_numbers);
        self.layout.saved.collapse_unchanged = Some(self.collapse_unchanged);
        self.layout.saved.change_grouping = Some(self.evolution.collapse_repeats);
        self.layout.saved.file_grouping = Some(self.file_evolution.collapse_repeats);
        self.layout.saved.change_rebases = Some(self.evolution.show_unchanged_rebases);
        self.layout.saved.file_rebases = Some(self.file_evolution.show_unchanged_rebases);
        self.layout.saved.change_evolution = Some(self.evolution.visible);
        self.layout.saved.file_evolution = Some(self.file_evolution.visible);
        if let Some(path) = &self.layout.path {
            let key = crate::interface_state::StoreKey {
                path: path.clone(),
                repository,
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
        if self.layout.path.as_ref() == Some(&key.path)
            && self.layout.repository.as_ref() == Some(&key.repository)
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
    pub(super) fn render_panel_toggle(
        &self,
        panel: jaywalk::panel_layout::PanelId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let history = panel == jaywalk::panel_layout::PanelId::History;
        let label = if history {
            "Toggle history panel"
        } else {
            "Toggle changed files panel"
        };
        let active = if history {
            !self.layout.saved.history_hidden
        } else {
            !self.layout.saved.files_hidden
        };
        let side = if history {
            self.placement().history_side
        } else {
            self.placement().files_side
        };
        Button::new(if history {
            "toggle-history-panel"
        } else {
            "toggle-files-panel"
        })
        .debug_selector(move || format!("drawer-toggle-{panel:?}"))
        .ghost()
        .with_size(px(self.settings.ui.footer_control_height))
        .w(px(self.settings.ui.footer_control_height))
        .flex_shrink_0()
        .compact()
        .icon(crate::appearance::active_icon(
            if side == jaywalk::panel_layout::Side::Left {
                IconName::PanelLeft
            } else {
                IconName::PanelRight
            },
            active,
            cx,
        ))
        .tooltip(label)
        .toggled(active)
        .on_click(cx.listener(move |this, _, window, cx| this.toggle_sidebar(history, window, cx)))
        .into_any_element()
    }

    pub(super) fn bottom_timeline_state(&self) -> Option<bool> {
        self.supports_evolution()
            .then(|| {
                self.placement()
                    .bottom_visibility(self.evolution.visible, self.file_evolution.visible)
            })
            .flatten()
    }

    pub(super) fn render_timeline_toggle(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.bottom_timeline_state().unwrap_or(false);
        Button::new("toggle-timeline-panels")
            .ghost()
            .with_size(px(self.settings.ui.footer_control_height))
            .w(px(self.settings.ui.footer_control_height))
            .flex_shrink_0()
            .compact()
            .icon(crate::appearance::active_icon(
                IconName::PanelBottom,
                active,
                cx,
            ))
            .tooltip("Toggle bottom panel")
            .toggled(active)
            .on_click(cx.listener(|this, _, window, cx| {
                let (mut change, mut file) = (this.evolution.visible, this.file_evolution.visible);
                this.placement().toggle_bottom(&mut change, &mut file);
                if this.evolution.visible != change {
                    this.toggle_evolution(window, cx);
                }
                if this.file_evolution.visible != file {
                    this.toggle_evolution_panel(evolution::EvolutionKind::File, window, cx);
                }
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::SessionLayout;
    use crate::viewer::interaction_tests::Fixture;
    use crate::{
        Arguments,
        viewer::{Settings, ViewMode},
    };
    use clap::Parser;
    use jaywalk::layout_state::{LayoutState, RepositoryLayoutStore};
    fn captured(fixture: &Fixture, inputs: &std::path::Path) -> Arguments {
        use jaywalk::difftool::{Capture, Identity};
        let left = inputs.join("left");
        let right = inputs.join("right");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        let mut args = Arguments::parse_from([
            "jaywalk",
            inputs.to_str().unwrap(),
            "--diff",
            left.to_str().unwrap(),
            right.to_str().unwrap(),
        ]);
        let config = inputs.join("custom.toml");
        std::fs::write(&config, "").unwrap();
        args.config = Some(config);
        args.difftool = Some(Capture {
            identity: Identity {
                pid: 999999,
                started: 1,
                boot: [0, 0],
            },
            grouped: true,
            directory: true,
            repository: fixture.0.join("."),
            working_head: None,
            received: 1,
            total: 1,
            files: Vec::new(),
            intent: None,
        });
        args.normalize().unwrap();
        args
    }
    #[test]
    fn normal_and_captured_diff_opens_restore_the_same_canonical_repository_choices() {
        let fixture = Fixture::new();
        let inputs = tempfile::tempdir().unwrap();
        let diff = captured(&fixture, inputs.path());
        let root = fixture.0.canonicalize().unwrap();
        let path = diff
            .config
            .as_ref()
            .unwrap()
            .with_extension("interface.toml");
        RepositoryLayoutStore::new(path, &root)
            .unwrap()
            .save(&LayoutState {
                view: Some(ViewMode::Inline),
                wrap_inline: Some(false),
                collapse_unchanged: Some(true),
                history_width: Some(311.),
                ..Default::default()
            })
            .unwrap();
        let settings = Settings::default();
        for args in [&diff, &diff.for_repository(root.clone())] {
            let layout = SessionLayout::load(args, &settings);
            assert_eq!(layout.repository.as_deref(), Some(root.as_path()));
            assert_eq!(layout.saved.view, Some(ViewMode::Inline));
            assert_eq!(layout.saved.wrap_inline, Some(false));
            assert_eq!(layout.saved.collapse_unchanged, Some(true));
            assert_eq!(layout.saved.history_width, Some(311.));
        }
        let mut disabled = settings;
        disabled.layout.remember_per_repository = false;
        assert!(SessionLayout::load(&diff, &disabled).saved.view.is_none());
    }
    #[test]
    fn anonymous_external_inputs_and_welcome_do_not_borrow_the_launch_directory_state() {
        let fixture = Fixture::new();
        let inputs = tempfile::tempdir().unwrap();
        let mut args = captured(&fixture, inputs.path());
        args.directory = Some(fixture.0.canonicalize().unwrap());
        args.difftool = None;
        let path = args
            .config
            .as_ref()
            .unwrap()
            .with_extension("interface.toml");
        RepositoryLayoutStore::new(path, &fixture.0)
            .unwrap()
            .save(&LayoutState {
                view: Some(ViewMode::Inline),
                ..Default::default()
            })
            .unwrap();
        for args in [&args, &args.for_welcome()] {
            let layout = SessionLayout::load(args, &Settings::default());
            assert!(layout.repository.is_none());
            assert!(layout.saved.view.is_none());
        }
    }
}
