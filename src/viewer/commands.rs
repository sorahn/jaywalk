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
        CommandId::ShowSetup
        | CommandId::InstallCli
        | CommandId::CheckUpdates
        | CommandId::OpenPullRequest
        | CommandId::BrowsePullRequests
        | CommandId::Accounts
        | CommandId::ConnectAccount
        | CommandId::ReconnectAccount
        | CommandId::DisconnectAccount
        | CommandId::ChooseHostingWorkspace
        | CommandId::ImportHostingSetup
        | CommandId::CheckAccount => {
            crate::show_application_page(arguments.clone(), action.command, cx)
        }
        CommandId::CancelAccountSignIn => crate::account_ui::cancel_sign_in(cx),
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
        CommandId::ShowWelcome => {
            crate::show_application_page(arguments.clone(), action.command, cx)
        }
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
    jaywalk::keymap::remap(&defaults, &settings.keymap, cx).map(|_| ())
}

pub(crate) fn install_bindings(settings: &Settings, cx: &mut App) -> Result<(), String> {
    let previous = cx
        .try_global::<InstalledKeymap>()
        .map_or(&[][..], |state| state.defaults.as_slice());
    let defaults = collect_defaults(previous, cx.key_bindings().borrow().bindings().cloned());
    // Prepare everything before mutating the active map. Failed overrides must
    // leave both bindings and the previous settings intact.
    let toolkit = jaywalk::keymap::remap(&defaults, &settings.keymap, cx)?;
    cx.clear_key_bindings();
    cx.bind_keys(toolkit);
    cx.bind_keys(configured_bindings(settings, cx));
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

fn configured_bindings(settings: &Settings, cx: &App) -> Vec<KeyBinding> {
    let mut result = Vec::new();
    for (command, bindings) in &settings.keybindings {
        let context = match command {
            CommandId::OpenInNewWindow => Some("JaywalkRecent"),
            CommandId::ReviewSelectedReference => Some("JaywalkReferencePicker"),
            CommandId::ShowSetup
            | CommandId::InstallCli
            | CommandId::CheckUpdates
            | CommandId::OpenPullRequest
            | CommandId::BrowsePullRequests
            | CommandId::Accounts
            | CommandId::ConnectAccount
            | CommandId::ReconnectAccount
            | CommandId::DisconnectAccount
            | CommandId::ChooseHostingWorkspace
            | CommandId::ImportHostingSetup
            | CommandId::CheckAccount
            | CommandId::CancelAccountSignIn
            | CommandId::About
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
            result.push(command_binding(binding, *command, context, cx));
            if matches!(command, CommandId::NextChange | CommandId::PreviousChange) {
                for context in ["JaywalkDiff > ReadonlyEditor > Input", "JaywalkDiff > Diff"] {
                    result.push(command_binding(binding, *command, Some(context), cx));
                }
            }
        }
    }
    result
}

fn command_binding(
    binding: &str,
    command: CommandId,
    context: Option<&str>,
    cx: &App,
) -> KeyBinding {
    KeyBinding::load(
        binding,
        Box::new(ExecuteCommand { command }),
        context.map(|context| {
            gpui::KeyBindingContextPredicate::parse(context)
                .expect("command contexts are static valid predicates")
                .into()
        }),
        false,
        None,
        cx,
    )
    .expect("settings validation has parsed every configured shortcut")
}

impl Viewer {
    pub(crate) fn execute_command(
        &mut self,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if command == CommandId::Refresh && self.page == pages::Page::PullRequests {
            if let Some(page) = &self.pull_requests_page {
                page.update(cx, |page, cx| page.force_refresh(window, cx));
            }
            return;
        }
        if self.page != pages::Page::Comparison && !command.available_without_repository() {
            return;
        }
        match command {
            CommandId::OpenPullRequest => {
                self.open_palette(palette::PaletteKind::PullRequests, window, cx)
            }
            CommandId::BrowsePullRequests => self.show_pull_requests(window, cx),
            CommandId::RevealComparisonLeft | CommandId::RevealComparisonRight => {
                self.reveal_pair_endpoint(command == CommandId::RevealComparisonRight, window, cx)
            }
            CommandId::Accounts => self.show_accounts(command, window, cx),
            CommandId::ManageAccounts
            | CommandId::ConnectAccount
            | CommandId::ReconnectAccount
            | CommandId::DisconnectAccount
            | CommandId::ChooseHostingWorkspace
            | CommandId::ImportHostingSetup
            | CommandId::CheckAccount => self.open_account_palette(command, window, cx),
            CommandId::CancelAccountSignIn => crate::account_ui::cancel_sign_in(cx),
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
            CommandId::RefreshReviewStatus => self.refresh_pr_status(true, window, cx),
            CommandId::UpdatePublishedReview => self.update_hosted_review(window, cx),
            CommandId::OpenReviewInBrowser => {
                if let Some(contents) = self
                    .repository
                    .ready()
                    .and_then(|s| s.review.aggregate.hosted())
                {
                    cx.open_url(&contents.data.metadata.locator.url());
                }
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
            CommandId::ShowWelcome => self.show_page(pages::Page::Welcome, window, cx),
            CommandId::ClearRecent => crate::recents::clear(cx),
            CommandId::GoBack => self.navigate_view(false, window, cx),
            CommandId::GoForward => self.navigate_view(true, window, cx),
            CommandId::GoToFileSymbol => {
                self.open_palette(palette::PaletteKind::FileSymbols, window, cx)
            }
            CommandId::GoToProjectSymbol => {
                self.open_palette(palette::PaletteKind::ProjectSymbols, window, cx)
            }
            CommandId::ReviewPRLocally => self.choose_local_repository(window, cx),
            CommandId::RestoreReviewCheckout => self.choose_checkout_recovery(window, cx),
            CommandId::GoToLine => self.open_palette(palette::PaletteKind::Line, window, cx),
            CommandId::PickerHelp => self.open_palette(palette::PaletteKind::Help, window, cx),
            CommandId::ClearCommandUsage => crate::command_usage::clear(cx),
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
            CommandId::ShowSetup => self.show_setup(window, cx),
            CommandId::InstallCli => self.install_cli(window, cx),
            CommandId::CheckUpdates => crate::updater::check(window, cx),
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
            CommandId::SearchHistory => {
                self.open_palette(palette::PaletteKind::History, window, cx)
            }
            CommandId::LoadMoreHistory => self.load_more_history(window, cx),
            CommandId::LatestHistory => self.latest_history(window, cx),
            CommandId::ShowReferences => self.show_references(cx),
            CommandId::GoToReference => {
                self.open_palette(palette::PaletteKind::References, window, cx)
            }
            CommandId::RemoteHistorySources => {
                self.open_palette(palette::PaletteKind::Remotes, window, cx)
            }
            // The reference picker captures this with its rendered row snapshot.
            CommandId::ReviewSelectedReference => {}
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
            CommandId::ToggleWhitespaceDifferences
            | CommandId::WhitespaceExact
            | CommandId::WhitespaceTrailing
            | CommandId::WhitespaceChanges
            | CommandId::WhitespaceAll
            | CommandId::ToggleIgnoreLineEndings
            | CommandId::ToggleIgnoreBlankLines
            | CommandId::ToggleWordHighlight
            | CommandId::ToggleShowWhitespace
            | CommandId::ToggleShowLineEndings
            | CommandId::ToggleWhitespaceErrors => self.diff_option_command(command, window, cx),
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
                .expect("default visibility has no commit expression")
                .with_limit(self.settings.history.page_size)
                .expect("validated page size");
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

    #[gpui_test::test]
    fn recent_shortcut_is_available_in_the_repository_free_welcome_context(
        test_cx: &mut gpui_test::TestAppContext,
    ) {
        test_cx.update(|cx| {
            assert!(CommandId::OpenRecent.available_without_repository());
            let map = Keymap::new(configured_bindings(&Settings::default(), cx));
            // Welcome uses the same root context, without any repository panels.
            let contexts = [KeyContext::parse("Jaywalk").unwrap()];
            let bindings = map.bindings_for_input(
                &[Keystroke::parse_with_os("ctrl-r", jaywalk::keymap::native_os()).unwrap()],
                &contexts,
            );
            assert!(bindings.0.iter().any(|binding| {
                binding
                    .action()
                    .as_any()
                    .downcast_ref::<ExecuteCommand>()
                    .is_some_and(|action| action.command == CommandId::OpenRecent)
            }));
        });
    }

    #[gpui_test::test]
    fn new_window_shortcut_is_remappable_and_scoped_to_recents(
        test_cx: &mut gpui_test::TestAppContext,
    ) {
        test_cx.update(|cx| {
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
                let map = Keymap::new(configured_bindings(&settings, cx));
                let matches = |key: &str, recent: bool| {
                    let mut contexts = vec![KeyContext::parse("Jaywalk").unwrap()];
                    if recent {
                        contexts.push(KeyContext::parse("JaywalkRecent").unwrap());
                    }
                    contexts.extend([
                        KeyContext::parse("Command").unwrap(),
                        KeyContext::parse("Input").unwrap(),
                    ]);
                    map.bindings_for_input(
                        &[Keystroke::parse_with_os(key, jaywalk::keymap::native_os()).unwrap()],
                        &contexts,
                    )
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
        });
    }

    #[gpui_test::test]
    fn macos_shifted_bracket_events_navigate_history(test_cx: &mut gpui_test::TestAppContext) {
        test_cx.update(|cx| {
            let map = Keymap::new(configured_bindings(&Settings::parse("").unwrap(), cx));
            let contexts = [KeyContext::parse("Jaywalk").unwrap()];
            // GPUI's macOS adapter emits shifted punctuation with Shift consumed.
            for (event, expected) in [
                ("secondary-}", CommandId::NextRevision),
                ("secondary-{", CommandId::PreviousRevision),
            ] {
                let bindings = map
                    .bindings_for_input(
                        &[Keystroke::parse_with_os(event, jaywalk::keymap::native_os()).unwrap()],
                        &contexts,
                    )
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
        });
    }

    #[gpui_test::test]
    fn pristine_defaults_survive_remapping_removal_and_late_widget_registration(
        test_cx: &mut gpui_test::TestAppContext,
    ) {
        test_cx.update(|cx| {
            use gpui::base::input::{Copy, SelectAll};
            let defaults = vec![KeyBinding::new(
                if cfg!(target_os = "macos") {
                    "cmd-c"
                } else {
                    "ctrl-c"
                },
                Copy,
                Some("Input"),
            )];
            let rule = jaywalk::keymap::Override {
                action: "input::Copy".into(),
                keys: vec!["secondary-insert".into()],
                context: None,
                from: None,
            };
            let mut current = jaywalk::keymap::remap(&defaults, &[rule], cx).unwrap();
            current.push(KeyBinding::new(
                if cfg!(target_os = "macos") {
                    "cmd-a"
                } else {
                    "ctrl-a"
                },
                SelectAll,
                Some("Input"),
            ));
            let collected = super::collect_defaults(&defaults, current.into_iter());
            assert_eq!(collected.len(), 2);
            let restored = Keymap::new(jaywalk::keymap::remap(&collected, &[], cx).unwrap());
            let context = [KeyContext::parse("Input").unwrap()];
            assert!(
                !restored
                    .bindings_for_input(
                        &[
                            Keystroke::parse_with_os("secondary-c", jaywalk::keymap::native_os())
                                .unwrap()
                        ],
                        &context
                    )
                    .0
                    .is_empty()
            );
            assert!(
                !restored
                    .bindings_for_input(
                        &[
                            Keystroke::parse_with_os("secondary-a", jaywalk::keymap::native_os())
                                .unwrap()
                        ],
                        &context
                    )
                    .0
                    .is_empty()
            );
            assert!(
                restored
                    .bindings_for_input(
                        &[Keystroke::parse_with_os(
                            "secondary-insert",
                            jaywalk::keymap::native_os()
                        )
                        .unwrap()],
                        &context
                    )
                    .0
                    .is_empty()
            );
        });
    }

    #[gpui_test::test]
    fn change_keys_override_readonly_source_but_keep_editable_input_navigation(
        test_cx: &mut gpui_test::TestAppContext,
    ) {
        test_cx.update(|cx| {
            let settings = Settings::parse("").unwrap();
            let mut map = Keymap::new(vec![KeyBinding::new(
                if cfg!(target_os = "macos") {
                    "cmd-down"
                } else {
                    "ctrl-down"
                },
                gpui::base::input::MoveToEnd,
                Some("Input"),
            )]);
            map.add_bindings(configured_bindings(&settings, cx));
            let readonly = ["Jaywalk", "JaywalkDiff", "ReadonlyEditor", "Input"]
                .map(|context| KeyContext::parse(context).unwrap());
            let search = ["Jaywalk", "JaywalkDiff", "Input"]
                .map(|context| KeyContext::parse(context).unwrap());
            let input = [
                Keystroke::parse_with_os("secondary-down", jaywalk::keymap::native_os()).unwrap(),
            ];
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
        });
    }

    #[gpui_test::test]
    fn live_keymap_replacement_keeps_late_toolkit_bindings_and_removes_old_app_shortcuts(
        test_cx: &mut gpui_test::TestAppContext,
    ) {
        test_cx.update(|cx| {
            let initial = Settings::parse("").unwrap();
            let mut map = Keymap::new(configured_bindings(&initial, cx));
            // These can arrive after the application's initial startup snapshot.
            map.add_bindings([
                KeyBinding::new(
                    if cfg!(target_os = "macos") {
                        "cmd-a"
                    } else {
                        "ctrl-a"
                    },
                    gpui::base::input::SelectAll,
                    Some("Input"),
                ),
                KeyBinding::new(
                    if cfg!(target_os = "macos") {
                        "cmd-c"
                    } else {
                        "ctrl-c"
                    },
                    gpui::base::input::Copy,
                    Some("Input"),
                ),
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
            map.add_bindings(configured_bindings(&updated, cx));
            let contexts = [
                KeyContext::parse("Jaywalk").unwrap(),
                KeyContext::parse("Input").unwrap(),
            ];
            let matches = |key: &str| {
                map.bindings_for_input(
                    &[Keystroke::parse_with_os(key, jaywalk::keymap::native_os()).unwrap()],
                    &contexts,
                )
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
        });
    }
}
