use super::*;

pub(super) fn mode_command(mode: Mode) -> Option<jaywalk::commands::CommandId> {
    use jaywalk::commands::CommandId;
    Some(match mode {
        Mode::Files => CommandId::FindFile,
        Mode::Commands => CommandId::ShowCommands,
        Mode::Line => CommandId::GoToLine,
        Mode::Repositories => CommandId::OpenRecent,
        Mode::PullRequests => CommandId::OpenPullRequest,
        Mode::Help => CommandId::PickerHelp,
        Mode::History => CommandId::SearchHistory,
        Mode::FileSymbols => CommandId::GoToFileSymbol,
        Mode::ProjectSymbols => CommandId::GoToProjectSymbol,
    })
}
pub(super) fn mode_description(mode: Mode) -> &'static str {
    match mode {
        Mode::Files => "Files in the captured comparison",
        Mode::Commands => "Actions and current settings",
        Mode::Line => "Source line in the focused file side",
        Mode::Repositories => "Open a recent repository",
        Mode::PullRequests => "Search PRs; Tab changes filters",
        Mode::Help => "Choose a mode to fill its prefix",
        Mode::History => "Commit IDs, messages, authors, references and date filters",
        Mode::FileSymbols => "Symbols on the focused source side",
        Mode::ProjectSymbols => "Symbols across available snapshot files",
    }
}
impl Viewer {
    pub(in crate::viewer) fn has_bitbucket_account(&self, cx: &App) -> bool {
        use jaywalk::accounts::{Provider, application::Accounts};
        if let Some(accounts) = cx.try_global::<Accounts>() {
            let snapshot = &accounts.state.read(cx).snapshot;
            if snapshot.loaded {
                return snapshot
                    .accounts
                    .iter()
                    .any(|account| account.identity.key.provider == Provider::Bitbucket);
            }
        }
        // Public config metadata is available before credential-store restoration.
        // Opening the palette must not trigger Keychain access.
        self.settings
            .accounts
            .iter()
            .any(|account| account.provider == Provider::Bitbucket)
    }

    pub(in crate::viewer::palette) fn observe_palette_accounts(&mut self, cx: &mut Context<Self>) {
        if self.palette.open != Some(PaletteKind::Commands) {
            return;
        }
        let Some(state) = cx
            .try_global::<jaywalk::accounts::application::Accounts>()
            .map(|accounts| accounts.state.clone())
        else {
            return;
        };
        if self
            .palette
            .account_observer
            .as_ref()
            .is_some_and(|(old, _)| *old == state)
        {
            return;
        }
        let subscription = cx.observe(&state, |viewer, _, cx| {
            if viewer.palette.open == Some(PaletteKind::Commands) {
                cx.notify();
            }
        });
        self.palette.account_observer = Some((state, subscription));
    }

    pub(in crate::viewer::palette) fn context_command_suggestions(
        &self,
    ) -> Vec<jaywalk::commands::CommandId> {
        use jaywalk::commands::CommandId as C;
        if self.page != pages::Page::Comparison {
            return vec![
                C::OpenRepository,
                C::OpenRecent,
                C::OpenPullRequest,
                C::ComparePaths,
                C::ConnectAccount,
                C::ShowSetup,
            ];
        }
        if self.is_hosted_review() {
            return vec![
                C::FindFile,
                C::UpdatePublishedReview,
                C::RefreshReviewStatus,
                C::OpenReviewInBrowser,
                C::ReviewPRLocally,
                C::Overview,
            ];
        }
        if self.code().is_some() {
            return vec![
                C::Overview,
                C::FindFile,
                C::GoToLine,
                C::ToggleWrap,
                C::SearchHistory,
                C::OpenPullRequest,
            ];
        }
        vec![
            C::FindFile,
            C::ChooseReview,
            C::SearchHistory,
            C::ShowReferences,
            C::OpenPullRequest,
            C::OpenRecent,
        ]
    }

    pub(super) fn picker_mode_available(&self, mode: Mode) -> bool {
        match mode {
            Mode::History => {
                self.page == pages::Page::Comparison
                    && self
                        .repository
                        .ready()
                        .and_then(|s| s.history.as_ref().ok())
                        .is_some()
            }
            Mode::Files => self.page == pages::Page::Comparison && self.comparison().is_some(),
            Mode::Line => self.page == pages::Page::Comparison && self.code().is_some(),
            Mode::FileSymbols => self.page == pages::Page::Comparison && self.code().is_some(),
            Mode::ProjectSymbols => {
                self.page == pages::Page::Comparison && self.comparison().is_some()
            }
            _ => true,
        }
    }
    pub(super) fn picker_description(&self, kind: PaletteKind, cx: &App) -> String {
        if kind == PaletteKind::Commands
            && let Some(error) = &cx.global::<crate::command_usage::Usage>().error
        {
            return format!("{error} · Clear command usage history to reset");
        }
        match kind {
            PaletteKind::Commands => "Suggestions for this view and frequently used actions · Type to search · ? lists modes".into(),
            PaletteKind::Files if self.palette.files.total > self.palette.files.shown => format!("Showing {} of {} matching files · Type to narrow · ? lists modes", self.palette.files.shown, self.palette.files.total),
            PaletteKind::LocalRepositories | PaletteKind::CheckoutRecovery => self
                .local_actions
                .message
                .clone()
                .unwrap_or_else(|| "Choose an action · Escape closes".into()),
            PaletteKind::FileSymbols | PaletteKind::ProjectSymbols => {
                self.symbol_picker_description()
            }
            PaletteKind::History => self.history_picker_description(),
            PaletteKind::Reviews => self.palette.review_error.clone().unwrap_or_else(|| {
                if self.palette.review_loading {
                    "Loading branches… · Escape closes".into()
                } else {
                    "Choose a branch/range · Escape closes · ? lists picker modes".into()
                }
            }),
            PaletteKind::References => "Enter opens history target · Review selected branch changes uses Shift+Enter by default · Escape closes".into(),
            PaletteKind::Remotes => "Enter toggles a remote · Search and selection stay in place · Escape closes".into(),
            PaletteKind::Profiles => {
                "Choose a keybinding profile · Escape closes · ? lists picker modes".into()
            }
            PaletteKind::Line => format!(
                "Source line on the {} side · Enter a positive line number · ? lists modes",
                if self.palette.line_before {
                    "before"
                } else {
                    "after"
                }
            ),
            _ => format!(
                "{} · ? lists modes and shortcuts · Escape closes",
                kind.mode().map_or("Choose", Mode::label)
            ),
        }
    }
    pub(super) fn picker_empty(&self, kind: PaletteKind) -> &'static str {
        match kind {
            PaletteKind::LocalRepositories | PaletteKind::CheckoutRecovery => {
                "No matching actions; see status above. Escape closes."
            }
            PaletteKind::Files if !self.picker_mode_available(Mode::Files) => {
                "Open a comparison to search its files. Type ? for available modes."
            }
            PaletteKind::Line if !self.picker_mode_available(Mode::Line) => {
                "Open a file first to go to a source line. Type ? for available modes."
            }
            PaletteKind::Line => "Enter a line number within this file, starting at 1.",
            PaletteKind::History => {
                "No matching history. Try a SHA, message, author:name, after:YYYY-MM-DD or before:YYYY-MM-DD."
            }
            PaletteKind::References => "No matching branches or bookmarks in this repository.",
            PaletteKind::Remotes => "No matching remotes in this repository.",
            PaletteKind::FileSymbols | PaletteKind::ProjectSymbols => {
                "No symbols here. Open a comparison/file with a supported language; type ? for modes."
            }
            _ => "No matching results. Type ? for available modes.",
        }
    }
    pub(super) fn line_choice(&self, query: &str, cx: &App) -> Option<(String, Choice)> {
        if !self.picker_mode_available(Mode::Line) {
            return None;
        }
        let line = query.trim().parse::<usize>().ok()?.checked_sub(1)?;
        let code = self.code()?;
        let before = self.palette.line_before;
        let offsets = if before {
            &code.model.before_offsets
        } else {
            &code.model.after_offsets
        };
        if line >= offsets.len().saturating_sub(1) {
            return None;
        }
        let source = Rc::new(self.comparison()?.identity());
        let _ = cx;
        Some((
            format!(
                "Go to line {} · {}",
                line + 1,
                if before { "Before" } else { "After" }
            ),
            Choice::Line {
                path: self.selected_path()?,
                line,
                source,
                before,
            },
        ))
    }
    pub(super) fn go_to_source_line(
        &mut self,
        line: usize,
        before_side: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigation_target = None;
        self.reveal_after_layout = false;
        if self.view.effective() != ViewMode::Fluid {
            if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
                state.update(cx, |state, cx| {
                    if let Some(path) = state.files().first().map(|file| file.path().clone()) {
                        let side = if before_side {
                            gpui::component::diff::DiffSide::Original
                        } else {
                            gpui::component::diff::DiffSide::Modified
                        };
                        state.set_selected_lines(
                            Some(gpui::component::diff::DiffLineRange::new(
                                path.clone(),
                                side,
                                line + 1,
                                line + 1,
                            )),
                            cx,
                        );
                        state.scroll_to_line(
                            gpui::component::diff::DiffLinePosition::new(path, side, line + 1),
                            cx,
                        );
                    }
                });
            }
        } else if let Some(code) = self.code() {
            let target = if before_side {
                code.before.clone()
            } else {
                code.after.clone()
            };
            target.update(cx, |editor, cx| {
                editor.set_cursor_position(
                    gpui::component::input::Position::new(line as u32, 0),
                    window,
                    cx,
                )
            });
            let code = self.code().expect("file remains loaded");
            let geometry =
                comparison_geometry(code.before.read(cx), code.after.read(cx), &code.model);
            let position = geometry.position_for_line(&code.model, line, before_side);
            self.scroll_comparison(position - self.fluid.position, cx);
        }
        self.checkpoint_navigation(cx);
        cx.notify();
    }
    pub(super) fn command_state_hint(
        &self,
        command: jaywalk::commands::CommandId,
    ) -> Option<String> {
        use jaywalk::commands::CommandId as C;
        let p = self.diff_preferences;
        let enabled = match command {
            C::ToggleWrap => Some(self.wrapping.enabled(if self.selected_file.is_none() {
                ViewMode::Inline
            } else {
                self.view.effective()
            })),
            C::ToggleLineNumbers => Some(self.line_numbers),
            C::ToggleUnchangedLines => Some(self.collapse_unchanged),
            C::ToggleIgnoreLineEndings => Some(p.comparison.ignore_line_endings),
            C::ToggleIgnoreBlankLines => Some(p.comparison.ignore_blank_lines),
            C::ToggleWordHighlight => Some(p.word_highlight),
            C::ToggleShowWhitespace => Some(p.show_whitespace),
            C::ToggleShowLineEndings => Some(p.show_line_endings),
            C::ToggleWhitespaceErrors => Some(p.highlight_whitespace_errors),
            C::ToggleHistory => Some(!self.layout.saved.history_hidden),
            C::ToggleFiles => Some(!self.layout.saved.files_hidden),
            C::ToggleChangeEvolution => Some(self.evolution.visible),
            C::ToggleFileEvolution => Some(self.file_evolution.visible),
            _ => None,
        };
        if let Some(enabled) = enabled {
            return Some(if enabled { "On" } else { "Off" }.into());
        }
        match command {
            C::ToggleWhitespaceDifferences => Some(format!("{:?}", p.comparison.whitespace)),
            C::WhitespaceExact
            | C::WhitespaceTrailing
            | C::WhitespaceChanges
            | C::WhitespaceAll => {
                use jaywalk::diff_options::Whitespace as W;
                let selected = match command {
                    C::WhitespaceExact => W::Exact,
                    C::WhitespaceTrailing => W::IgnoreTrailing,
                    C::WhitespaceChanges => W::IgnoreChanges,
                    _ => W::IgnoreAll,
                };
                (p.comparison.whitespace == selected).then(|| "Active".into())
            }
            C::ViewFluid | C::ViewInline | C::ViewSideBySide => {
                let selected = match command {
                    C::ViewFluid => ViewMode::Fluid,
                    C::ViewInline => ViewMode::Inline,
                    _ => ViewMode::SideBySide,
                };
                (self.view.preferred() == selected).then(|| "Active".into())
            }
            _ => None,
        }
    }
}

impl Viewer {
    pub(in crate::viewer) fn palette_owns_pr(
        &self,
        page: &Entity<crate::hosting_ui::PullRequestsPage>,
    ) -> bool {
        self.palette.open == Some(PaletteKind::PullRequests)
            && self.palette.pull_requests.as_ref() == Some(page)
    }
}

#[derive(IntoElement)]
pub(crate) struct ShortcutHint(pub(crate) jaywalk::commands::CommandId);
impl RenderOnce for ShortcutHint {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        div().children(gpui::component::kbd::Kbd::binding_for_action(
            &commands::ExecuteCommand { command: self.0 },
            None,
            window,
        ))
    }
}

impl Viewer {
    pub(in crate::viewer) fn begin_picker_review_load(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> u64 {
        if self.review_picker.epoch != Some(self.requests.comparison()) {
            self.review_picker.choices.clear();
        }
        self.open_palette(PaletteKind::Reviews, window, cx);
        self.palette.review_loading = true;
        let generation = self.palette.generation;
        let delay = std::time::Duration::from_millis(self.settings.ui.loading_delay_ms);
        cx.spawn_in(window, async move |viewer, cx| {
            cx.background_executor().timer(delay).await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.owns_picker_review_load(generation) && viewer.palette.review_loading {
                    viewer
                        .palette
                        .state
                        .update(cx, |state, cx| state.set_loading(true, window, cx));
                    cx.notify();
                }
            });
        })
        .detach();
        generation
    }
    pub(in crate::viewer) fn owns_picker_review_load(&self, generation: u64) -> bool {
        self.palette.generation == generation && self.palette.open == Some(PaletteKind::Reviews)
    }
    pub(in crate::viewer) fn finish_picker_review_load(
        &mut self,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette.review_loading = false;
        self.palette.review_error = error;
        self.palette
            .state
            .update(cx, |state, cx| state.set_loading(false, window, cx));
    }
    pub(in crate::viewer) fn review_picker_visible(&self) -> bool {
        self.palette.open == Some(PaletteKind::Reviews)
    }
}
