//! Apply validated configuration without reloading the selected comparison.
use super::*;
use gpui::component::Theme;
use jaywalk::settings_reload::{SettingsSource, SettingsWatcher};
use std::time::Duration;

pub(super) struct ConfigState {
    task: Option<Task<()>>,
    pub(super) error: Option<String>,
    pub(super) pending_horizontal: Option<f32>,
    pub(super) pending_standard: Option<gpui::component::diff::DiffLinePosition>,
}

impl ConfigState {
    pub(super) fn new() -> Self {
        Self {
            task: None,
            error: None,
            pending_standard: None,
            pending_horizontal: None,
        }
    }
}

impl Viewer {
    pub(super) fn start_settings_reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let source = match SettingsSource::resolve(self.arguments.config.as_deref()) {
            Ok(source) => source,
            Err(error) => {
                self.config.error = Some(error);
                return;
            }
        };
        self.config.task = Some(cx.spawn_in(window, async move |this, cx| {
            let mut watcher = SettingsWatcher::new(source);
            loop {
                let Ok(interval) = this.update(cx, |viewer, _| viewer.settings.settings.poll_interval_ms) else {
                    break;
                };
                cx.background_executor().timer(Duration::from_millis(interval)).await;
                let result = cx.background_spawn(async move {
                    let result = watcher.poll();
                    (watcher, result)
                }).await;
                watcher = result.0;
                if let Some(result) = result.1
                    && this.update_in(cx, |viewer, window, cx| {
                        match result {
                            Ok(settings) => {
                                viewer.config.error = None;
                                viewer.apply_settings(settings, window, cx);
                            }
                            Err(error) => {
                                eprintln!("Settings reload: {error}");
                                let mut lines = error.lines().filter(|line| !line.trim().is_empty());
                                let first = lines.next().unwrap_or(&error);
                                let detail = lines.next_back().map_or(String::new(), |last| format!(" · {last}"));
                                viewer.config.error = Some(format!("Settings not applied; keeping the last valid configuration. {first}{detail}"));
                            }
                        }
                        cx.notify();
                    }).is_err()
                {
                    break;
                }
            }
        }));
    }

    pub(super) fn prepare_display_reflow(
        &mut self,
        cx: &App,
    ) -> Option<gpui::component::diff::DiffLinePosition> {
        self.preserve_reflow_position(cx);
        self.code().and_then(|code| {
            if self.view.effective() != ViewMode::Fluid
                && let Ok(Some(standard)) = &code.standard
            {
                return standard.read(cx).viewport_position();
            }
            let state = if self.view.is_single_file() {
                code.before.read(cx)
            } else {
                code.after.read(cx)
            };
            let line = state.visible_row_range().map_or(0, |range| range.start);
            let path = self.selected_path()?;
            Some(gpui::component::diff::DiffLinePosition::new(
                path,
                gpui::component::diff::DiffSide::Modified,
                line + 1,
            ))
        })
    }

    fn apply_settings(&mut self, settings: Settings, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings == settings {
            return;
        }
        let previous_wrap = self.wrapping;
        let previous = std::mem::replace(&mut self.settings, settings);
        let position = self.prepare_display_reflow(cx);
        self.preserve_row_positions(&previous, cx);
        // A changed default is an explicit config edit. Unchanged fields leave
        // toolbar overrides and independently resized/hidden panels untouched.
        if previous.diff.default_view != self.settings.diff.default_view {
            self.set_view_mode(self.settings.diff.default_view, cx);
        }
        for (current, old, new) in [
            (
                &mut self.wrapping.before,
                previous.diff.before,
                self.settings.diff.before,
            ),
            (
                &mut self.wrapping.after,
                previous.diff.after,
                self.settings.diff.after,
            ),
            (
                &mut self.wrapping.inline,
                previous.diff.inline,
                self.settings.diff.inline,
            ),
        ] {
            if old != new {
                *current = new.word_wrap;
            }
        }
        if previous.editor.font_size != self.settings.editor.font_size {
            self.code_font_size = self.settings.editor.font_size;
        }
        if previous.editor.line_numbers != self.settings.editor.line_numbers {
            self.line_numbers = self.settings.editor.line_numbers;
        }
        if previous.keybindings != self.settings.keybindings {
            commands::install_bindings(&self.settings, cx);
        }
        if previous.files.default_view != self.settings.files.default_view {
            self.browser.mode = self.settings.files.default_view;
        }
        if previous.files.tree_scope != self.settings.files.tree_scope {
            self.browser.scope = self.settings.files.tree_scope;
        }
        if previous.files.tree_scope != self.settings.files.tree_scope
            || previous.files.default_view != self.settings.files.default_view
        {
            self.rebuild_browser(cx);
        }
        if previous.ui != self.settings.ui || previous.editor != self.settings.editor {
            Theme::update(cx, |theme| {
                theme.font_size = px(self.settings.ui.font_size);
                theme.mono_font_size = px(self.code_font_size);
                theme.font_family = self
                    .settings
                    .ui
                    .font_family
                    .clone()
                    .map(SharedString::from)
                    .unwrap_or_else(|| Theme::default().font_family);
                theme.mono_font_family = self
                    .settings
                    .editor
                    .font_family
                    .clone()
                    .map(SharedString::from)
                    .unwrap_or_else(|| Theme::default().mono_font_family);
            });
        }
        if let Some(code) = self.code() {
            for (state, old, wrap) in [
                (
                    &code.before,
                    if self.view.is_single_file() {
                        previous_wrap.inline
                    } else {
                        previous_wrap.before
                    },
                    self.editor_wraps()[0],
                ),
                (
                    &code.after,
                    if self.view.is_single_file() {
                        previous_wrap.inline
                    } else {
                        previous_wrap.after
                    },
                    self.editor_wraps()[1],
                ),
            ] {
                state.update(cx, |state, cx| {
                    state.set_line_number(self.line_numbers, window, cx)
                });
                if old != wrap || previous.editor != self.settings.editor {
                    state.update(cx, |state, cx| state.set_soft_wrap(wrap, window, cx));
                }
            }
        }
        if self.view.effective() != ViewMode::Fluid {
            self.config.pending_standard = position;
        }
        if previous.ui.loading_delay_ms != self.settings.ui.loading_delay_ms {
            self.reschedule_feedback(window, cx);
        }
        if previous.layout.history_visible != self.settings.layout.history_visible {
            self.layout.saved.history_hidden = !self.settings.layout.history_visible;
            self.rebuild_sidebar_layout(cx);
        }
        if previous.layout.files_visible != self.settings.layout.files_visible {
            self.layout.saved.files_hidden = !self.settings.layout.files_visible;
            self.rebuild_sidebar_layout(cx);
        }
        self.apply_panel_settings(&previous, cx);
        self.apply_evolution_settings(&previous, window, cx);
        if previous.panels.files.font != self.settings.panels.files.font
            || previous.ui != self.settings.ui
            || previous.files.indent_width != self.settings.files.indent_width
        {
            self.refresh_browser_metrics();
        }
        if previous.window != self.settings.window {
            let current = window.bounds().size;
            window.resize(size(
                if previous.window.width != self.settings.window.width {
                    px(self.settings.window.width)
                } else {
                    current.width
                },
                if previous.window.height != self.settings.window.height {
                    px(self.settings.window.height)
                } else {
                    current.height
                },
            ));
        }
        if previous.live != self.settings.live {
            self.live.poll = None;
            self.start_live(window, cx);
        }
        if previous.panels != self.settings.panels
            || previous.evolution != self.settings.evolution
            || previous.file_evolution != self.settings.file_evolution
        {
            self.save_layout(cx);
        }
        if previous.editor.tab_size != self.settings.editor.tab_size && self.selected_file.is_some()
        {
            self.reload_open_file(window, cx);
        }
        self.save_layout(cx);
        // Fonts and wrapping invalidate text measurements as well as paint.
        window.refresh();
        cx.notify();
    }
}
