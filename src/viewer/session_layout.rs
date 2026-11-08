//! Persist only local layout choices, leaving user TOML settings untouched.
use super::*;
use jaywalk::layout_state::LayoutState;
use std::path::PathBuf;
use std::time::Duration;
pub(super) struct SessionLayout {
    pub(super) saved: LayoutState,
    path: Option<PathBuf>,
    pub(super) error: Option<String>,
    saving: bool,
    revision: u64,
    task: Option<Task<()>>,
}
impl SessionLayout {
    pub(super) fn load(arguments: &Arguments) -> Self {
        let path = arguments
            .config
            .as_ref()
            .map(|path| Ok(path.with_extension("layout.toml")))
            .unwrap_or_else(LayoutState::path);
        let (path, saved, error) = match path {
            Ok(path) => match LayoutState::read(&path) {
                Ok(saved) => (Some(path), saved, None),
                Err(error) => (Some(path), LayoutState::default(), Some(error)),
            },
            Err(error) => (None, LayoutState::default(), Some(error)),
        };
        Self {
            path,
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
        self.live.message = self.layout.error.take();
    }
    pub(super) fn save_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.saved.change_evolution = Some(self.evolution.visible);
        self.layout.saved.file_evolution = Some(self.file_evolution.visible);
        self.layout.revision += 1;
        if self.layout.saving {
            return;
        }
        let Some(path) = self.layout.path.clone() else {
            return;
        };
        self.layout.saving = true;
        self.layout.task = Some(cx.spawn_in(window, async move |this, cx| {
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
                let result = cx.background_spawn(async move { saved.save(&path) }).await;
                let Ok(again) = this.update(cx, |viewer, cx| {
                    if let Err(error) = result {
                        viewer.live.message = Some(error);
                        cx.notify();
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
        window: &mut Window,
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
        self.save_layout(window, cx);
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
                    .icon(if history {
                        IconName::PanelLeft
                    } else {
                        IconName::PanelRight
                    })
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
