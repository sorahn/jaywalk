//! One action path for shortcuts, the palette, and native menus.
use super::*;
use jaywalk::commands::CommandId;

#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = jaywalk, no_json)]
pub(crate) struct ExecuteCommand {
    pub(crate) command: CommandId,
}

pub(crate) fn init(arguments: &Arguments, settings: &Settings, cx: &mut App) -> Result<(), String> {
    install_bindings(settings, cx)?;
    let arguments = arguments.clone();
    cx.on_action(move |action: &ExecuteCommand, cx| match action.command {
        CommandId::About => cx.defer(|cx| {
            // A menu action may run inside the active window's update. Reusing
            // that window must wait until it is back in the app's window table.
            if let Err(error) = crate::about::show(cx) {
                eprintln!("Could not open About Jaywalk: {error}");
            }
        }),
        CommandId::OpenRepository => crate::choose_repository(arguments.clone(), None, cx),
        CommandId::OpenRecent => crate::show_recent_repository_picker(arguments.clone(), cx),
        CommandId::ComparePaths => crate::choose_comparison(arguments.clone(), None, cx),
        CommandId::ShowWelcome => crate::open_arguments(arguments.for_welcome(), None, false, cx),
        CommandId::ClearRecent => crate::recents::clear(cx),
        CommandId::Quit => cx.quit(),
        CommandId::Hide => cx.hide(),
        CommandId::HideOthers => cx.hide_other_apps(),
        CommandId::ShowAll => cx.unhide_other_apps(),
        _ => cx.propagate(),
    });
    Ok(())
}

#[derive(Default)]
struct InstalledKeymap {
    defaults: Vec<KeyBinding>,
    settings: Option<Settings>,
    version: Option<KeymapVersion>,
}
impl Global for InstalledKeymap {}

fn collect_defaults(
    previous: &[KeyBinding],
    current: impl Iterator<Item = KeyBinding>,
) -> Vec<KeyBinding> {
    let mut defaults = previous.to_vec();
    for binding in current {
        if binding.action().as_any().is::<ExecuteCommand>()
            || binding.meta() == Some(jaywalk::keymap::OVERRIDE_META)
        {
            continue;
        }
        if !defaults
            .iter()
            .any(|old| jaywalk::keymap::same_binding(old, &binding))
        {
            defaults.push(binding);
        }
    }
    defaults
}

pub(super) fn validate_bindings(settings: &Settings, cx: &App) -> Result<(), String> {
    let previous = cx
        .try_global::<InstalledKeymap>()
        .map_or(&[][..], |state| state.defaults.as_slice());
    let defaults = collect_defaults(previous, cx.key_bindings().borrow().bindings().cloned());
    jaywalk::keymap::remap(&defaults, &settings.keymap).map(|_| ())
}

pub(crate) fn install_bindings(settings: &Settings, cx: &mut App) -> Result<(), String> {
    let previous = cx
        .try_global::<InstalledKeymap>()
        .map_or(&[][..], |state| state.defaults.as_slice());
    let defaults = collect_defaults(previous, cx.key_bindings().borrow().bindings().cloned());
    // Prepare everything before mutating the active map. Failed overrides must
    // leave both bindings and the previous settings intact.
    let toolkit = jaywalk::keymap::remap(&defaults, &settings.keymap)?;
    cx.clear_key_bindings();
    cx.bind_keys(toolkit);
    cx.bind_keys(configured_bindings(settings));
    let version = cx.key_bindings().borrow().version();
    cx.set_global(InstalledKeymap {
        defaults,
        settings: Some(settings.clone()),
        version: Some(version),
    });
    crate::platform::update_menus(cx);
    Ok(())
}

/// Components can register bindings after startup. Reapply only when the
/// framework map's version changes, retaining the last applied app-wide config.
pub(super) fn refresh_bindings(cx: &mut App) -> Result<(), String> {
    let version = cx.key_bindings().borrow().version();
    let settings = cx.try_global::<InstalledKeymap>().and_then(|state| {
        (state.version != Some(version))
            .then(|| state.settings.clone())
            .flatten()
    });
    if let Some(settings) = settings
        && let Err(error) = install_bindings(&settings, cx)
    {
        cx.global_mut::<InstalledKeymap>().version = Some(version);
        return Err(error);
    }
    Ok(())
}

fn configured_bindings(settings: &Settings) -> Vec<KeyBinding> {
    let mut result = Vec::new();
    for (command, bindings) in &settings.keybindings {
        let context = match command {
            CommandId::OpenInNewWindow => Some("JaywalkRecent"),
            CommandId::About
            | CommandId::OpenRepository
            | CommandId::OpenRecent
            | CommandId::ComparePaths
            | CommandId::ShowWelcome
            | CommandId::ClearRecent
            | CommandId::Quit
            | CommandId::Hide
            | CommandId::HideOthers
            | CommandId::ShowAll => None,
            _ => Some("Jaywalk"),
        };
        // Register the first configured shortcut last so native menus and
        // palette hints show it as the primary shortcut.
        for binding in bindings.iter().rev() {
            // Settings::validate has parsed every keystroke, and the context is static.
            result.push(KeyBinding::new(
                binding,
                ExecuteCommand { command: *command },
                context,
            ));
            if matches!(command, CommandId::NextChange | CommandId::PreviousChange) {
                for context in ["JaywalkDiff > ReadonlyEditor > Input", "JaywalkDiff > Diff"] {
                    result.push(KeyBinding::new(
                        binding,
                        ExecuteCommand { command: *command },
                        Some(context),
                    ));
                }
            }
        }
    }
    result
}

impl Viewer {
    pub(crate) fn execute_command(
        &mut self,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_welcome() && !command.available_without_repository() {
            return;
        }
        match command {
            CommandId::RevealComparisonLeft | CommandId::RevealComparisonRight => {
                self.reveal_pair_endpoint(command == CommandId::RevealComparisonRight, window, cx)
            }
            CommandId::SwapComparison => self.swap_comparison(window, cx),
            CommandId::CompareRevision | CommandId::DiffFromBranchPoint => {
                if let Some(target) = self.current_revision_target() {
                    self.execute_revision_action(command, &target, window, cx);
                }
            }
            CommandId::CollapseOverview
            | CommandId::ExpandOverview
            | CommandId::OverviewContext
            | CommandId::OverviewFull => self.overview_action(command, cx),
            CommandId::InspectRevision
            | CommandId::ShowChangeEvolution
            | CommandId::CopyCommitId
            | CommandId::CopyChangeId
            | CommandId::CopyCommitMessage => {
                if let Some(target) = self.current_revision_target() {
                    self.execute_revision_action(command, &target, window, cx);
                }
            }
            CommandId::CopyOperationId
            | CommandId::ToggleEvolutionGroups
            | CommandId::ToggleUnchangedVersions => {
                if let Some(target) = self.active_version_target() {
                    self.execute_version_action(command, &target, window, cx);
                }
            }
            CommandId::OpenFile
            | CommandId::CopyPath
            | CommandId::CopyRelativePath
            | CommandId::CopyFileName
            | CommandId::RevealInFileManager => {
                if let Some(target) = self.keyboard_file_target(window, cx) {
                    self.execute_file_action(command, &target, window, cx);
                }
            }
            CommandId::CollapseFiles | CommandId::ExpandFiles => {
                self.set_directory_expansion(None, command == CommandId::ExpandFiles, cx);
            }
            CommandId::RevealFile => self.reveal_active_file(cx),
            CommandId::FileList | CommandId::FileTree => {
                self.set_file_mode(
                    if command == CommandId::FileList {
                        jaywalk::settings::FileView::List
                    } else {
                        jaywalk::settings::FileView::Tree
                    },
                    cx,
                );
            }
            CommandId::TreeChanged | CommandId::TreeAll => {
                self.set_tree_scope(
                    if command == CommandId::TreeAll {
                        jaywalk::settings::TreeScope::All
                    } else {
                        jaywalk::settings::TreeScope::Changed
                    },
                    cx,
                );
            }
            CommandId::CurrentWork | CommandId::OpenedComparison => {
                self.load_home_review(
                    if command == CommandId::CurrentWork {
                        review_context::HomeSource::Workspace
                    } else {
                        review_context::HomeSource::Opened
                    },
                    window,
                    cx,
                );
            }
            CommandId::OpenRepository => {
                crate::choose_repository(self.arguments.clone(), Some(cx.entity().downgrade()), cx)
            }
            CommandId::OpenInNewWindow => self.open_selected_recent_in_new_window(window, cx),
            CommandId::OpenRecent => {
                self.open_palette(palette::PaletteKind::Repositories, window, cx)
            }
            CommandId::ComparePaths => {
                crate::choose_comparison(self.arguments.clone(), Some(cx.entity().downgrade()), cx)
            }
            CommandId::ShowWelcome => crate::open_arguments(
                self.arguments.for_welcome(),
                Some(cx.entity().downgrade()),
                false,
                cx,
            ),
            CommandId::ClearRecent => crate::recents::clear(cx),
            CommandId::GoBack => self.navigate_view(false, window, cx),
            CommandId::GoForward => self.navigate_view(true, window, cx),
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
            CommandId::OpenSettings => {
                self.edit_configuration_file(keyboard::ConfigurationFile::Settings, window, cx)
            }
            CommandId::OpenKeybindings => {
                self.edit_configuration_file(keyboard::ConfigurationFile::Keybindings, window, cx)
            }
            CommandId::SelectKeybindings => self.choose_keybindings_profile(window, cx),
            CommandId::AllChanges => self.all_review_changes(None, window, cx),
            CommandId::ChooseReview => self.choose_review(None, window, cx),
            CommandId::ChooseReviewBase => self.choose_review_base(window, cx),
            CommandId::BranchChanges | CommandId::MergedBranch | CommandId::MergeResult => {
                if let Some(target) = self.current_revision_target() {
                    self.execute_revision_action(command, &target, window, cx);
                }
            }
            CommandId::Overview => {
                self.show_overview(window, cx);
            }
            CommandId::Refresh => {
                self.invalidate_overview();
                self.refresh(window, cx);
            }
            CommandId::ToggleWrap => self.toggle_wrap(window, cx),
            CommandId::ShowReferences => self.show_references(cx),
            CommandId::ToggleReferenceLabels => self.toggle_reference_labels(cx),
            CommandId::ShowAllRemotes => self.set_remote_visibility(None, true, window, cx),
            CommandId::HideAllRemotes => self.set_remote_visibility(None, false, window, cx),
            CommandId::RevealSelectedRevision => self.reveal_history_selection(window, cx),
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
            CommandId::ToggleUnchangedLines => self.toggle_unchanged_lines(cx),
            CommandId::ToggleLineNumbers => {
                let position = self.prepare_display_reflow(cx);
                if self.view.effective() != ViewMode::Fluid {
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
                self.save_layout(cx);
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
                if self.view.effective() != ViewMode::Fluid {
                    self.config.pending_standard = position;
                }
                self.code_font_size = next;
                window.refresh();
                cx.notify();
            }
            CommandId::HistoryLeft
            | CommandId::HistoryRight
            | CommandId::FilesLeft
            | CommandId::FilesRight
            | CommandId::SwapSidebars
            | CommandId::ChangeEvolutionSidebar
            | CommandId::ChangeEvolutionBottom
            | CommandId::FileEvolutionSidebar
            | CommandId::FileEvolutionBottom => self.move_panel(command, cx),
            CommandId::ResetInterface => {
                self.references.labels = self.settings.history.show_reference_labels;
                self.references.visibility = self.settings.history.remotes.clone();
                self.history_query = jaywalk::history::HistoryQuery::sources(
                    self.references.visibility.clone(),
                    None,
                )
                .expect("default visibility has no commit expression");
                self.reload_history(None, window, cx);
                self.set_view_mode(self.settings.diff.default_view, cx);
                self.wrapping = Wrapping::from_settings(&self.settings.diff);
                self.line_numbers = self.settings.editor.line_numbers;
                self.collapse_unchanged = self.settings.diff.collapse_unchanged;
                self.apply_standard_context(cx);
                self.apply_file_view();
                self.rebuild_browser(cx);
                for panel in [&mut self.evolution, &mut self.file_evolution] {
                    panel.collapse_repeats = self.settings.evolution.collapse_repeats;
                    panel.show_unchanged_rebases = self.settings.evolution.show_unchanged_rebases;
                    panel.rebuild();
                }
                if let Some(code) = self.code() {
                    for (state, wrap) in [
                        (&code.before, self.editor_wraps()[0]),
                        (&code.after, self.editor_wraps()[1]),
                    ] {
                        state.update(cx, |state, cx| {
                            state.set_soft_wrap(wrap, window, cx);
                            state.set_line_number(self.line_numbers, window, cx);
                        });
                    }
                }
                self.reset_layout(window, cx);
            }
            CommandId::ResetLayout => self.reset_layout(window, cx),
            CommandId::CloseWindow => window.remove_window(),
            CommandId::Minimize => window.minimize_window(),
            CommandId::Fullscreen => window.toggle_fullscreen(),
            CommandId::About
            | CommandId::Quit
            | CommandId::Hide
            | CommandId::HideOthers
            | CommandId::ShowAll => {
                window.dispatch_action(Box::new(ExecuteCommand { command }), cx);
                cx.propagate();
            }
        }
    }

    fn reset_layout(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.preserve_reflow_position(cx);
        self.layout.saved = jaywalk::layout_state::LayoutState::from_settings(&self.settings);
        self.panels = panels::PanelLayout::new(cx);
        self.evolution.visible = self.settings.evolution.visible;
        self.file_evolution.visible = self.settings.file_evolution.visible;
        self.save_layout(cx);
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
    fn recent_shortcut_is_available_in_the_repository_free_welcome_context() {
        assert!(CommandId::OpenRecent.available_without_repository());
        let map = Keymap::new(configured_bindings(&Settings::default()));
        // Welcome uses the same root context, without any repository panels.
        let contexts = [KeyContext::parse("Jaywalk").unwrap()];
        let bindings = map.bindings_for_input(&[Keystroke::parse("ctrl-r").unwrap()], &contexts);
        assert!(bindings.0.iter().any(|binding| {
            binding
                .action()
                .as_any()
                .downcast_ref::<ExecuteCommand>()
                .is_some_and(|action| action.command == CommandId::OpenRecent)
        }));
    }

    #[test]
    fn new_window_shortcut_is_remappable_and_scoped_to_recents() {
        for (config, shortcut) in [
            ("", "secondary-enter"),
            (
                "[keybindings]\nopen_in_new_window=['alt-enter']",
                "alt-enter",
            ),
        ] {
            let settings = Settings::default()
                .with_keybindings(jaywalk::keybinding_profiles::Profile::parse(config).unwrap())
                .unwrap();
            let map = Keymap::new(configured_bindings(&settings));
            let matches = |key: &str, recent: bool| {
                let mut contexts = vec![KeyContext::parse("Jaywalk").unwrap()];
                if recent {
                    contexts.push(KeyContext::parse("JaywalkRecent").unwrap());
                }
                contexts.extend([
                    KeyContext::parse("Command").unwrap(),
                    KeyContext::parse("Input").unwrap(),
                ]);
                map.bindings_for_input(&[Keystroke::parse(key).unwrap()], &contexts)
                    .0
                    .iter()
                    .any(|binding| {
                        binding
                            .action()
                            .as_any()
                            .downcast_ref::<ExecuteCommand>()
                            .is_some_and(|action| action.command == CommandId::OpenInNewWindow)
                    })
            };
            assert!(matches(shortcut, true));
            assert!(!matches(shortcut, false));
            if !config.is_empty() {
                assert!(!matches("secondary-enter", true));
            }
        }
    }

    #[test]
    fn macos_shifted_bracket_events_navigate_history() {
        let map = Keymap::new(configured_bindings(&Settings::parse("").unwrap()));
        let contexts = [KeyContext::parse("Jaywalk").unwrap()];
        // GPUI's macOS adapter emits shifted punctuation with Shift consumed.
        for (event, expected) in [
            ("secondary-}", CommandId::NextRevision),
            ("secondary-{", CommandId::PreviousRevision),
        ] {
            let bindings = map
                .bindings_for_input(&[Keystroke::parse(event).unwrap()], &contexts)
                .0;
            assert_eq!(
                bindings[0]
                    .action()
                    .as_any()
                    .downcast_ref::<ExecuteCommand>()
                    .unwrap()
                    .command,
                expected
            );
        }
    }

    #[test]
    fn pristine_defaults_survive_remapping_removal_and_late_widget_registration() {
        use gpui::base::input::{Copy, SelectAll};
        let defaults = vec![KeyBinding::new("secondary-c", Copy, Some("Input"))];
        let rule = jaywalk::keymap::Override {
            action: "input::Copy".into(),
            keys: vec!["secondary-insert".into()],
            context: None,
            from: None,
        };
        let mut current = jaywalk::keymap::remap(&defaults, &[rule]).unwrap();
        current.push(KeyBinding::new("secondary-a", SelectAll, Some("Input")));
        let collected = super::collect_defaults(&defaults, current.into_iter());
        assert_eq!(collected.len(), 2);
        let restored = Keymap::new(jaywalk::keymap::remap(&collected, &[]).unwrap());
        let context = [KeyContext::parse("Input").unwrap()];
        assert!(
            !restored
                .bindings_for_input(&[Keystroke::parse("secondary-c").unwrap()], &context)
                .0
                .is_empty()
        );
        assert!(
            !restored
                .bindings_for_input(&[Keystroke::parse("secondary-a").unwrap()], &context)
                .0
                .is_empty()
        );
        assert!(
            restored
                .bindings_for_input(&[Keystroke::parse("secondary-insert").unwrap()], &context)
                .0
                .is_empty()
        );
    }

    #[test]
    fn change_keys_override_readonly_source_but_keep_editable_input_navigation() {
        let settings = Settings::parse("").unwrap();
        let mut map = Keymap::new(vec![KeyBinding::new(
            "secondary-down",
            gpui::base::input::MoveToEnd,
            Some("Input"),
        )]);
        map.add_bindings(configured_bindings(&settings));
        let readonly = ["Jaywalk", "JaywalkDiff", "ReadonlyEditor", "Input"]
            .map(|context| KeyContext::parse(context).unwrap());
        let search =
            ["Jaywalk", "JaywalkDiff", "Input"].map(|context| KeyContext::parse(context).unwrap());
        let input = [Keystroke::parse("secondary-down").unwrap()];
        assert!(
            map.bindings_for_input(&input, &readonly).0[0]
                .action()
                .as_any()
                .is::<ExecuteCommand>()
        );
        assert!(
            map.bindings_for_input(&input, &search).0[0]
                .action()
                .as_any()
                .is::<gpui::base::input::MoveToEnd>()
        );
    }

    #[test]
    fn live_keymap_replacement_keeps_late_toolkit_bindings_and_removes_old_app_shortcuts() {
        let initial = Settings::parse("").unwrap();
        let mut map = Keymap::new(configured_bindings(&initial));
        // These can arrive after the application's initial startup snapshot.
        map.add_bindings([
            KeyBinding::new("secondary-a", gpui::base::input::SelectAll, Some("Input")),
            KeyBinding::new("secondary-c", gpui::base::input::Copy, Some("Input")),
        ]);
        let updated = Settings::default()
            .with_keybindings(
                jaywalk::keybinding_profiles::Profile::parse(
                    "[keybindings]\ntoggle_history=['secondary-j']",
                )
                .unwrap(),
            )
            .unwrap();
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
