//! One action path for shortcuts, the palette, and native menus.
use super::*;
use jaywalk::commands::CommandId;

#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = jaywalk, no_json)]
pub(crate) struct ExecuteCommand {
    pub(crate) command: CommandId,
}

pub(crate) fn init(arguments: &Arguments, settings: &Settings, cx: &mut App) {
    install_bindings(settings, cx);
    let arguments = arguments.clone();
    cx.on_action(move |action: &ExecuteCommand, cx| match action.command {
        CommandId::About => cx.defer(|cx| {
            // A menu action may run inside the active window's update. Reusing
            // that window must wait until it is back in the app's window table.
            if let Err(error) = crate::about::show(cx) {
                eprintln!("Could not open About Jaywalk: {error}");
            }
        }),
        CommandId::OpenRepository => crate::choose_repository(arguments.clone(), cx),
        CommandId::Quit => cx.quit(),
        CommandId::Hide => cx.hide(),
        CommandId::HideOthers => cx.hide_other_apps(),
        CommandId::ShowAll => cx.unhide_other_apps(),
        _ => cx.propagate(),
    });
}

pub(super) fn install_bindings(settings: &Settings, cx: &mut App) {
    // Keep framework bindings registered after startup as well as initial ones.
    let toolkit: Vec<_> = cx
        .key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| !binding.action().as_any().is::<ExecuteCommand>())
        .cloned()
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(toolkit);
    cx.bind_keys(configured_bindings(settings));
    crate::platform::update_menus(cx);
}

fn configured_bindings(settings: &Settings) -> Vec<KeyBinding> {
    let mut result = Vec::new();
    for (command, bindings) in &settings.keybindings {
        let context = match command {
            CommandId::About
            | CommandId::OpenRepository
            | CommandId::Quit
            | CommandId::Hide
            | CommandId::HideOthers
            | CommandId::ShowAll => None,
            _ => Some("Jaywalk"),
        };
        for binding in bindings {
            // Settings::validate has parsed every keystroke, and the context is static.
            result.push(KeyBinding::new(
                binding,
                ExecuteCommand { command: *command },
                context,
            ));
        }
    }
    result
}

impl Viewer {
    pub(super) fn execute_command(
        &mut self,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            CommandId::FindFile => self.open_palette(palette::PaletteKind::Files, window, cx),
            CommandId::ShowCommands => {
                self.open_palette(palette::PaletteKind::Commands, window, cx)
            }
            CommandId::VersionPatch => {
                self.select_evolution_mode(evolution::ReviewMode::Patch, window, cx)
            }
            CommandId::SincePrevious => {
                self.select_evolution_mode(evolution::ReviewMode::SincePrevious, window, cx)
            }
            CommandId::OpenSettings => self.open_settings_file(window, cx),
            CommandId::WholeBranch => self.show_whole_branch(window, cx),
            CommandId::Overview => {
                self.clear_file();
                self.focus_evolution_file(None);
                cx.notify();
            }
            CommandId::Refresh => self.refresh(window, cx),
            CommandId::ToggleWrap => self.toggle_wrap(window, cx),
            CommandId::ToggleHistory => self.toggle_sidebar(true, window, cx),
            CommandId::ToggleFiles => self.toggle_sidebar(false, window, cx),
            CommandId::ToggleChangeEvolution => self.toggle_evolution(window, cx),
            CommandId::ToggleFileEvolution => {
                self.toggle_evolution_panel(evolution::EvolutionKind::File, window, cx)
            }
            CommandId::ViewInline => self.set_view_mode(ViewMode::Inline, cx),
            CommandId::ViewSideBySide => self.set_view_mode(ViewMode::SideBySide, cx),
            CommandId::ViewFluid => self.set_view_mode(ViewMode::Fluid, cx),
            CommandId::NextChange => self.navigate_change(1, cx),
            CommandId::PreviousChange => self.navigate_change(-1, cx),
            CommandId::NextFile => self.navigate_file(1, window, cx),
            CommandId::PreviousFile => self.navigate_file(-1, window, cx),
            CommandId::NextRevision => self.navigate_revision(1, window, cx),
            CommandId::PreviousRevision => self.navigate_revision(-1, window, cx),
            CommandId::ToggleLineNumbers => {
                let position = self.prepare_display_reflow(cx);
                if self.view_mode != ViewMode::Fluid {
                    self.config.pending_standard = position;
                }
                self.line_numbers = !self.line_numbers;
                if let Some(code) = self.code() {
                    for state in [&code.before, &code.after] {
                        state.update(cx, |state, cx| {
                            state.set_line_number(self.line_numbers, window, cx)
                        });
                    }
                }
                self.preserve_reflow_position(cx);
                cx.notify();
            }
            CommandId::ZoomIn | CommandId::ZoomOut | CommandId::ResetZoom => {
                let next = match command {
                    CommandId::ZoomIn => self.code_font_size + 1.,
                    CommandId::ZoomOut => self.code_font_size - 1.,
                    _ => self.settings.editor.font_size,
                }
                .clamp(8., 48.);
                let position = self.prepare_display_reflow(cx);
                if self.view_mode != ViewMode::Fluid {
                    self.config.pending_standard = position;
                }
                self.code_font_size = next;
                window.refresh();
                cx.notify();
            }
            CommandId::ResetLayout => self.reset_layout(window, cx),
            CommandId::CloseWindow => window.remove_window(),
            CommandId::Minimize => window.minimize_window(),
            CommandId::Fullscreen => window.toggle_fullscreen(),
            CommandId::About
            | CommandId::OpenRepository
            | CommandId::Quit
            | CommandId::Hide
            | CommandId::HideOthers
            | CommandId::ShowAll => {
                window.dispatch_action(Box::new(ExecuteCommand { command }), cx);
                cx.propagate();
            }
        }
    }

    fn open_settings_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self
            .arguments
            .config
            .clone()
            .map(Ok)
            .unwrap_or_else(jaywalk::settings::default_path);
        let editor = self.settings.settings.editor.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = path?;
                    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty())
                    {
                        std::fs::create_dir_all(parent).map_err(|error| {
                            format!("Could not create {}: {error}", parent.display())
                        })?;
                    }
                    match std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                    {
                        Ok(mut file) => {
                            use std::io::Write;
                            file.write_all(jaywalk::settings::DEFAULT_CONFIG.as_bytes())
                                .map_err(|error| {
                                    format!("Could not write {}: {error}", path.display())
                                })?;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => {
                            return Err(format!("Could not create {}: {error}", path.display()));
                        }
                    }
                    crate::platform::open_text_file(&path, editor.as_ref())
                })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |viewer, cx| {
                    viewer.live.message = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn reset_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preserve_reflow_position(cx);
        self.layout.saved = jaywalk::layout_state::LayoutState::default();
        self.panels = panels::PanelLayout::new(cx);
        self.evolution.visible = self.settings.evolution.visible;
        self.file_evolution.visible = self.settings.file_evolution.visible;
        self.save_layout(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecuteCommand, configured_bindings};
    use gpui::{KeyBinding, KeyContext, Keymap, Keystroke};
    use jaywalk::commands::CommandId;
    use jaywalk::settings::Settings;

    #[test]
    fn live_keymap_replacement_keeps_late_toolkit_bindings_and_removes_old_app_shortcuts() {
        let initial = Settings::parse("").unwrap();
        let mut map = Keymap::new(configured_bindings(&initial));
        // These can arrive after the application's initial startup snapshot.
        map.add_bindings([
            KeyBinding::new("secondary-a", gpui::base::input::SelectAll, Some("Input")),
            KeyBinding::new("secondary-c", gpui::base::input::Copy, Some("Input")),
        ]);
        let updated = Settings::parse("[keybindings]\ntoggle_history=['secondary-j']").unwrap();
        let retained: Vec<_> = map
            .bindings()
            .filter(|binding| !binding.action().as_any().is::<ExecuteCommand>())
            .cloned()
            .collect();
        map.clear();
        map.add_bindings(retained);
        map.add_bindings(configured_bindings(&updated));
        let contexts = [
            KeyContext::parse("Jaywalk").unwrap(),
            KeyContext::parse("Input").unwrap(),
        ];
        let matches = |key: &str| {
            map.bindings_for_input(&[Keystroke::parse(key).unwrap()], &contexts)
                .0
        };
        assert!(matches("secondary-a").iter().any(|binding| {
            binding
                .action()
                .as_any()
                .is::<gpui::base::input::SelectAll>()
        }));
        assert!(
            matches("secondary-c")
                .iter()
                .any(|binding| binding.action().as_any().is::<gpui::base::input::Copy>())
        );
        assert!(matches("secondary-b").is_empty());
        assert!(matches("secondary-j").iter().any(|binding| {
            binding.action().as_any().downcast_ref::<ExecuteCommand>()
                == Some(&ExecuteCommand {
                    command: CommandId::ToggleHistory,
                })
        }));
    }
}
