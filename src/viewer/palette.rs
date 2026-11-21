//! Toolkit command UI; actions and file results preserve their original subject.
mod files;
mod history;
mod references;
mod support;
mod symbols;
use super::*;
use gpui::component::Disableable;
use gpui::component::command::{Command, CommandItem, CommandState};
use gpui::prelude::FluentBuilder;
use jaywalk::quick_picker::{CommandUsage, Mode};
pub(super) use support::ShortcutHint;
use support::{mode_command, mode_description};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaletteKind {
    Files,
    Commands,
    Repositories,
    Profiles,
    LocalRepositories,
    CheckoutRecovery,
    Reviews,
    References,
    Remotes,
    PullRequests,
    Accounts,
    Line,
    Help,
    History,
    FileSymbols,
    ProjectSymbols,
}
impl PaletteKind {
    fn mode(self) -> Option<Mode> {
        Some(match self {
            Self::Files => Mode::Files,
            Self::Commands => Mode::Commands,
            Self::Repositories => Mode::Repositories,
            Self::PullRequests => Mode::PullRequests,
            Self::Line => Mode::Line,
            Self::Help => Mode::Help,
            Self::History => Mode::History,
            Self::FileSymbols => Mode::FileSymbols,
            Self::ProjectSymbols => Mode::ProjectSymbols,
            Self::References
            | Self::Remotes
            | Self::Reviews
            | Self::Profiles
            | Self::Accounts
            | Self::LocalRepositories
            | Self::CheckoutRecovery => return None,
        })
    }
    fn from_mode(mode: Mode) -> Self {
        match mode {
            Mode::Files => Self::Files,
            Mode::Commands => Self::Commands,
            Mode::Repositories => Self::Repositories,
            Mode::PullRequests => Self::PullRequests,
            Mode::Line => Self::Line,
            Mode::Help => Self::Help,
            Mode::History => Self::History,
            Mode::FileSymbols => Self::FileSymbols,
            Mode::ProjectSymbols => Self::ProjectSymbols,
        }
    }
}
#[derive(Clone)]
enum Choice {
    File {
        path: String,
        status: Option<String>,
        source: Rc<jaywalk::repository::ComparisonIdentity>,
    },
    History {
        revision: jaywalk::history::Revision,
        source: Option<std::sync::Arc<jaywalk::history::HistorySource>>,
        identity: Rc<jaywalk::repository::ComparisonIdentity>,
    },
    HistoryMore,
    Reference {
        reference: jaywalk::history::Reference,
        source: Rc<jaywalk::repository::ComparisonIdentity>,
    },
    Remote {
        remote: Option<String>,
        visible: bool,
        source: Rc<jaywalk::repository::ComparisonIdentity>,
    },
    Command(jaywalk::commands::CommandId),
    Mode(Mode),
    Line {
        path: String,
        line: usize,
        source: Rc<jaywalk::repository::ComparisonIdentity>,
        before: bool,
    },
    Symbol {
        entry: jaywalk::symbols::Entry,
        source: Rc<jaywalk::repository::ComparisonIdentity>,
    },
    Review(review_context::ReviewChoice),
    Profile(String),
    LocalRepository(std::path::PathBuf),
    RestoreCheckout(std::path::PathBuf),
    Repository {
        directory: std::path::PathBuf,
        order: usize,
    },
}
pub(super) struct PaletteState {
    state: Entity<CommandState>,
    pull_requests: Option<Entity<crate::hosting_ui::PullRequestsPage>>,
    accounts: Option<Entity<crate::account_picker::AccountPicker>>,
    open: Option<PaletteKind>,
    return_focus: Option<FocusHandle>,
    usage: CommandUsage,
    symbols: symbols::SymbolPicker,
    files: files::FilePicker,
    account_observer: Option<(
        Entity<jaywalk::accounts::application::AccountState>,
        Subscription,
    )>,
    history: history::HistorySearch,
    line_before: bool,
    pub(super) generation: u64,
    pub(super) review_loading: bool,
    pub(super) review_error: Option<String>,
    pub(super) profiles: Vec<String>,
}
impl PaletteState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Viewer>) -> Self {
        crate::command_usage::ensure(cx);
        Self {
            state: cx.new(|cx| CommandState::new(window, cx)),
            pull_requests: None,
            accounts: None,
            open: None,
            return_focus: None,
            usage: CommandUsage::default(),
            symbols: symbols::SymbolPicker::default(),
            files: files::FilePicker::default(),
            account_observer: None,
            history: history::HistorySearch::default(),
            line_before: false,
            generation: 0,
            review_loading: false,
            review_error: None,
            profiles: Vec::new(),
        }
    }
}
fn fuzzy_score(query: &str, label: &str) -> Option<usize> {
    let text = label.to_lowercase();
    let query = query.to_lowercase();
    let mut remaining = text.as_str();
    let mut score = 0;
    for needle in query.chars().filter(|character| !character.is_whitespace()) {
        let offset = remaining.find(needle)?;
        score += offset;
        remaining = &remaining[offset + needle.len_utf8()..];
    }
    Some(score)
}
fn repository_choices(directories: &[std::path::PathBuf]) -> Vec<(String, Choice)> {
    let mut choices: Vec<_> = directories
        .iter()
        .enumerate()
        .map(|(order, directory)| {
            (
                crate::recents::display_path(directory),
                Choice::Repository {
                    directory: directory.clone(),
                    order,
                },
            )
        })
        .collect();
    choices.push((
        jaywalk::commands::CommandId::OpenRepository
            .label()
            .to_owned(),
        Choice::Command(jaywalk::commands::CommandId::OpenRepository),
    ));
    choices
}
// Prefer the repository name; parent directories remain searchable as a fallback.
fn recent_rank(query: &str, directory: &std::path::Path) -> Option<(usize, usize)> {
    let query: String = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if query.is_empty() {
        return Some((0, 0));
    }
    if let Some(name) = directory.file_name() {
        let name = name.to_string_lossy().to_lowercase();
        if name == query {
            return Some((0, 0));
        }
        if name.starts_with(&query) {
            return Some((1, 0));
        }
        if let Some(offset) = name.find(&query) {
            return Some((2, offset));
        }
        if let Some(score) = fuzzy_score(&query, &name) {
            return Some((3, score));
        }
    }
    let display = crate::recents::display_path(directory);
    [
        fuzzy_score(&query, &display),
        fuzzy_score(&query, &directory.to_string_lossy()),
    ]
    .into_iter()
    .flatten()
    .min()
    .map(|score| (4, score))
}

fn rank_commands(
    query: &str,
    choices: Vec<(String, Choice)>,
    usage: Option<&CommandUsage>,
    suggestions: &[jaywalk::commands::CommandId],
) -> Vec<(String, Choice)> {
    let empty = query.trim().is_empty();
    let query = jaywalk::quick_picker::CommandQuery::new(query);
    let mut ranked: Vec<_> = choices
        .into_iter()
        .filter_map(|(label, choice)| {
            let score = match (&choice, usage) {
                (Choice::Command(command), Some(history)) => history.score(*command),
                _ => 0,
            };
            let rank = query.rank(&label, score)?;
            Some((rank, label, choice))
        })
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let mut rows: Vec<_> = ranked
        .into_iter()
        .map(|(_, label, choice)| (label, choice))
        .collect();
    if !empty {
        return rows;
    }
    let mut promoted = Vec::new();
    if let Some(usage) = usage {
        promoted.extend(
            rows.iter()
                .filter_map(|(_, choice)| match choice {
                    Choice::Command(command) if usage.score(*command) > 0 => Some(*command),
                    _ => None,
                })
                .take(2),
        );
    }
    for command in suggestions {
        if !promoted.contains(command) {
            promoted.push(*command);
        }
    }
    let mut result = Vec::with_capacity(rows.len());
    for command in promoted {
        if let Some(index) = rows.iter().position(
            |(_, choice)| matches!(choice, Choice::Command(candidate) if *candidate == command),
        ) {
            result.push(rows.remove(index));
        }
    }
    result.extend(rows);
    result
}

fn rank_choices(
    kind: PaletteKind,
    query: &str,
    choices: Vec<(String, Choice)>,
) -> Vec<(String, Choice)> {
    let mut ranked: Vec<_> = choices
        .into_iter()
        .filter_map(|(label, choice)| {
            let rank = match &choice {
                Choice::File { path, status, .. } => file_rank(query, path, status.is_some())
                    .map(|(score, unchanged)| (0, score, unchanged)),
                Choice::Command(jaywalk::commands::CommandId::OpenRepository)
                    if matches!(kind, PaletteKind::Repositories) =>
                {
                    Some((5, 0, false))
                }
                Choice::Line { .. } | Choice::History { .. } => Some((0, 0, false)),
                Choice::HistoryMore => Some((1, 0, false)),
                Choice::LocalRepository(_) | Choice::RestoreCheckout(_) => {
                    fuzzy_score(query, &label).map(|score| (0, score, false))
                }
                Choice::Remote { remote: None, .. } => {
                    fuzzy_score(query, &label).map(|score| (1, score, false))
                }
                Choice::Reference { .. }
                | Choice::Remote { .. }
                | Choice::Command(_)
                | Choice::Mode(_)
                | Choice::Profile(_)
                | Choice::Review(_)
                | Choice::Symbol { .. } => {
                    fuzzy_score(query, &label).map(|score| (0, score, false))
                }
                Choice::Repository { directory, .. } => {
                    recent_rank(query, directory).map(|(category, score)| (category, score, false))
                }
            }?;
            Some((rank, label, choice))
        })
        .collect();
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            if matches!(kind, PaletteKind::Repositories) {
                let order = |choice: &Choice| match choice {
                    Choice::Repository { order, .. } => *order,
                    _ => usize::MAX,
                };
                order(&a.2).cmp(&order(&b.2))
            } else {
                a.1.cmp(&b.1)
            }
        })
    });
    ranked
        .into_iter()
        .map(|(_, label, choice)| (label, choice))
        .collect()
}

// Status is a tie-breaker only; neither status text nor UI labels are searchable.
fn file_rank(query: &str, path: &str, changed: bool) -> Option<(usize, bool)> {
    fuzzy_score(query, path).map(|score| (score, !changed))
}

#[derive(Debug, PartialEq, Eq)]
enum HistoryStep {
    Overview,
    Revision(usize),
}

fn history_step(current: Option<usize>, delta: i32, count: usize) -> Option<HistoryStep> {
    if count == 0 {
        return None;
    }
    match current {
        None if delta > 0 => Some(HistoryStep::Revision(0)),
        None => None,
        Some(0) if delta < 0 => Some(HistoryStep::Overview),
        Some(index) => {
            let next = index.saturating_add_signed(delta as isize).min(count - 1);
            (next != index).then_some(HistoryStep::Revision(next))
        }
    }
}

impl Viewer {
    #[cfg(test)]
    pub(super) fn palette_test_state(
        &self,
        cx: &App,
    ) -> (Option<PaletteKind>, String, Option<usize>) {
        let state = self.palette.state.read(cx);
        (
            self.palette.open,
            state.query(cx).to_string(),
            state.selected_index().map(|index| index.row),
        )
    }
    #[cfg(test)]
    pub(super) fn palette_test_count(&self, cx: &App) -> usize {
        self.palette.state.read(cx).matched_count()
    }
    pub(super) fn configure_picker_local_review(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = &self.palette.pull_requests {
            page.update(cx, |page, _| {
                page.configure_local_review(self.settings.local_review)
            });
        }
    }
    pub(super) fn close_local_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.palette.open,
            Some(PaletteKind::LocalRepositories | PaletteKind::CheckoutRecovery)
        ) {
            self.close_palette(window, cx);
        }
    }
    pub(super) fn set_palette_loading(
        &mut self,
        loading: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette
            .state
            .update(cx, |state, cx| state.set_loading(loading, window, cx));
    }
    pub(super) fn open_palette(
        &mut self,
        kind: PaletteKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(
            kind,
            PaletteKind::LocalRepositories | PaletteKind::CheckoutRecovery
        ) {
            self.cancel_local_action();
        }
        if self.palette.open.is_none() {
            self.palette.return_focus = window.focused(cx);
            self.palette.state = cx.new(|cx| CommandState::new(window, cx));
            self.palette.line_before = self.code().is_some_and(|code| {
                if self.view.effective() != ViewMode::Fluid
                    && let Ok(Some(state)) = &code.standard
                {
                    return state
                        .read(cx)
                        .selected_lines()
                        .map(|range| range.side())
                        .or_else(|| {
                            state
                                .read(cx)
                                .viewport_position()
                                .map(|position| position.side())
                        })
                        == Some(gpui::component::diff::DiffSide::Original);
                }
                code.before.focus_handle(cx).contains_focused(window, cx)
            });
        }
        self.palette.symbols.cancel();
        self.palette.history.cancel();
        self.palette.generation += 1;
        self.palette.review_loading = false;
        self.palette.review_error = None;
        self.palette.usage = cx.global::<crate::command_usage::Usage>().history.clone();
        self.retire_pr_picker(cx);
        self.palette.open = Some(kind);
        self.observe_palette_accounts(cx);
        self.palette.state.update(cx, |state, cx| {
            state.set_loading(false, window, cx);
            state.set_query(kind.mode().map_or("", Mode::prefix), window, cx);
            state.focus(window, cx);
        });
        if kind == PaletteKind::PullRequests {
            self.create_pr_picker(window, cx);
        }
        self.start_symbol_picker(window, cx);
        self.sync_history_picker(window, cx);
        cx.notify();
    }
    fn retire_pr_picker(&mut self, cx: &mut Context<Self>) {
        if let Some(picker) = self.palette.accounts.take() {
            picker.update(cx, |picker, cx| picker.cancel(cx));
        }
        if let Some(page) = self.palette.pull_requests.take() {
            page.update(cx, |page, cx| page.cancel_picker(cx));
        }
    }
    fn create_pr_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(page) = self.new_pull_requests(true, window, cx) {
            page.update(cx, |page, cx| {
                page.use_picker_input(
                    self.palette.state.clone(),
                    self.settings.ui.loading_delay_ms,
                    window,
                    cx,
                )
            });
            self.palette.pull_requests = Some(page);
        }
    }
    pub(super) fn palette_query_changed(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.palette.open else {
            return;
        };
        if self.palette.state.read(cx).query(cx).as_ref() != query {
            return;
        }
        let mode = Mode::parse(query).0;
        let next = if current.mode().is_none() && mode == Mode::Files {
            current
        } else {
            PaletteKind::from_mode(mode)
        };
        if next != current {
            if matches!(
                current,
                PaletteKind::LocalRepositories | PaletteKind::CheckoutRecovery
            ) {
                self.cancel_local_action();
            }
            self.palette.symbols.cancel();
            self.palette.history.cancel();
            self.palette.generation += 1;
            self.palette.review_loading = false;
            self.palette.review_error = None;
            self.retire_pr_picker(cx);
            self.palette.open = Some(next);
            self.palette
                .state
                .update(cx, |state, cx| state.set_loading(false, window, cx));
            if next == PaletteKind::PullRequests {
                self.create_pr_picker(window, cx);
            }
        }
        self.observe_palette_accounts(cx);
        self.start_symbol_picker(window, cx);
        self.sync_history_picker(window, cx);
        cx.notify();
    }
    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_local_action();
        self.palette.symbols.cancel();
        self.palette.history.cancel();
        self.palette.generation += 1;
        self.palette.review_loading = false;
        self.palette.open = None;
        self.retire_pr_picker(cx);
        if let Some(focus) = self.palette.return_focus.take() {
            window.focus(&focus, cx);
        } else {
            self.focus_page(window, cx);
        }
        cx.notify();
    }
    pub(super) fn open_selected_recent_in_new_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.palette.open, Some(PaletteKind::Repositories)) {
            return;
        }
        let state = self.palette.state.read(cx);
        let Some(index) = state.selected_index() else {
            return;
        };
        let choices = rank_choices(
            PaletteKind::Repositories,
            Mode::parse(&state.query(cx)).1,
            repository_choices(&cx.global::<crate::recents::Recents>().repositories),
        );
        let Some((_, choice)) = choices.get(index.row).cloned() else {
            return;
        };
        self.close_palette(window, cx);
        match choice {
            Choice::Repository { directory, .. } => {
                self.open_repository_path(directory, crate::windows::OpenDisposition::NewWindow, cx)
            }
            Choice::Command(jaywalk::commands::CommandId::OpenRepository) => {
                crate::choose_repository_in_new_window(
                    self.arguments.clone(),
                    Some(cx.entity().downgrade()),
                    cx,
                )
            }
            _ => {}
        }
    }
    pub(super) fn render_palette(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = self.palette.open?;
        if matches!(kind, PaletteKind::PullRequests | PaletteKind::Accounts) {
            let picker = if kind == PaletteKind::PullRequests {
                self.palette
                    .pull_requests
                    .as_ref()?
                    .clone()
                    .into_any_element()
            } else {
                self.palette.accounts.as_ref()?.clone().into_any_element()
            };
            return Some(
                div()
                    .id("palette-overlay")
                    .debug_selector(|| "palette-overlay".into())
                    .absolute()
                    .inset_0()
                    .bg(crate::appearance::color(jaywalk::theme::Role::Backdrop, cx))
                    .flex()
                    .justify_center()
                    .items_start()
                    .pt_12()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|viewer, _, window, cx| viewer.close_palette(window, cx)),
                    )
                    .child(
                        crate::picker_ui::frame(cx)
                            .w(rems(48.))
                            .max_w_full()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(picker),
                    )
                    .into_any_element(),
            );
        }
        let raw_query = self.palette.state.read(cx).query(cx);
        let query = if kind.mode().is_some() {
            Mode::parse(&raw_query).1
        } else {
            raw_query.as_ref()
        };
        let choices: Vec<(String, Choice)> = match kind {
            PaletteKind::PullRequests | PaletteKind::Accounts => {
                unreachable!("specialized picker renders its own rows")
            }
            PaletteKind::Help => Mode::ALL
                .into_iter()
                .map(|mode| {
                    (
                        format!(
                            "{}  {}",
                            if mode.prefix().is_empty() {
                                "(no prefix)"
                            } else {
                                mode.prefix()
                            },
                            mode.label()
                        ),
                        Choice::Mode(mode),
                    )
                })
                .collect(),
            PaletteKind::FileSymbols | PaletteKind::ProjectSymbols => self.symbol_choices(query),
            PaletteKind::LocalRepositories => self
                .local_actions
                .repositories
                .iter()
                .map(|path| {
                    (
                        path.display().to_string(),
                        Choice::LocalRepository(path.clone()),
                    )
                })
                .collect(),
            PaletteKind::CheckoutRecovery => self
                .local_actions
                .recoveries
                .iter()
                .map(|receipt| {
                    (
                        format!("Restore {}", receipt.root.display()),
                        Choice::RestoreCheckout(receipt.path.clone()),
                    )
                })
                .collect(),
            PaletteKind::Line => self.line_choice(query, cx).into_iter().collect(),
            PaletteKind::Files => self.file_palette_choices(query),
            PaletteKind::Repositories => {
                repository_choices(&cx.global::<crate::recents::Recents>().repositories)
            }
            PaletteKind::Profiles => self
                .palette
                .profiles
                .iter()
                .map(|name| (name.clone(), Choice::Profile(name.clone())))
                .collect(),
            PaletteKind::History => self.history_choices(),
            PaletteKind::References | PaletteKind::Remotes => self.reference_choices(kind),
            PaletteKind::Reviews => {
                let mut choices: Vec<_> = self
                    .review_picker
                    .choices
                    .iter()
                    .map(|(label, choice)| (label.clone(), Choice::Review(choice.clone())))
                    .collect();
                if self.palette.review_error.is_some() {
                    choices.push((
                        "Retry loading branches".into(),
                        Choice::Command(jaywalk::commands::CommandId::ChooseReview),
                    ));
                }
                choices
            }
            PaletteKind::Commands => jaywalk::commands::CommandId::ALL
                .iter()
                .filter(|command| {
                    !matches!(
                        command,
                        jaywalk::commands::CommandId::OpenInNewWindow
                            | jaywalk::commands::CommandId::ReviewSelectedReference
                    ) && (!self.hosted_changed_only()
                        || **command != jaywalk::commands::CommandId::TreeAll)
                        && (self.is_hosted_review()
                            || !matches!(
                                command,
                                jaywalk::commands::CommandId::UpdatePublishedReview
                                    | jaywalk::commands::CommandId::RefreshReviewStatus
                                    | jaywalk::commands::CommandId::ReviewPRLocally
                                    | jaywalk::commands::CommandId::OpenReviewInBrowser
                            ))
                        && (!self.is_hosted_review()
                            || !matches!(
                                command,
                                jaywalk::commands::CommandId::ChooseReview
                                    | jaywalk::commands::CommandId::ChooseReviewBase
                                    | jaywalk::commands::CommandId::CurrentWork
                                    | jaywalk::commands::CommandId::OpenedComparison
                                    | jaywalk::commands::CommandId::CompareRevision
                                    | jaywalk::commands::CommandId::DiffFromBranchPoint
                                    | jaywalk::commands::CommandId::BranchChanges
                                    | jaywalk::commands::CommandId::MergedBranch
                                    | jaywalk::commands::CommandId::MergeResult
                                    | jaywalk::commands::CommandId::SwapComparison
                                    | jaywalk::commands::CommandId::RevealComparisonLeft
                                    | jaywalk::commands::CommandId::RevealComparisonRight
                                    | jaywalk::commands::CommandId::GoToReference
                                    | jaywalk::commands::CommandId::RemoteHistorySources
                                    | jaywalk::commands::CommandId::ShowReferences
                            ))
                        && (**command != jaywalk::commands::CommandId::ChooseHostingWorkspace
                            || self.has_bitbucket_account(cx))
                        && (self.page == pages::Page::Comparison
                            || command.available_without_repository())
                        && (self.supports_evolution()
                            || !matches!(
                                command,
                                jaywalk::commands::CommandId::ToggleChangeEvolution
                                    | jaywalk::commands::CommandId::ToggleFileEvolution
                                    | jaywalk::commands::CommandId::VersionPatch
                                    | jaywalk::commands::CommandId::SincePrevious
                            ))
                })
                .map(|command| (command.label().to_owned(), Choice::Command(*command)))
                .collect(),
        };
        let choices = if kind == PaletteKind::Commands {
            rank_commands(
                query,
                choices,
                self.settings
                    .picker
                    .adaptive_command_order
                    .then_some(&self.palette.usage),
                &self.context_command_suggestions(),
            )
        } else if matches!(
            kind,
            PaletteKind::FileSymbols
                | PaletteKind::ProjectSymbols
                | PaletteKind::History
                | PaletteKind::Files
        ) {
            choices // Symbol rows are ranked once, before cloning the visible results.
        } else {
            rank_choices(kind, query, choices)
        };
        // Search the complete index, but mount a bounded set of picker rows.
        let choices = if matches!(
            kind,
            PaletteKind::FileSymbols
                | PaletteKind::ProjectSymbols
                | PaletteKind::References
                | PaletteKind::Remotes
        ) {
            choices.into_iter().take(200).collect()
        } else {
            choices
        };
        let choices = Rc::new(choices);
        let selected = choices.clone();
        let cancel = cx.listener(|this, _: &(), window, cx| this.close_palette(window, cx));
        let confirm = cx.listener(
            move |this, index: &gpui::component::IndexPath, window, cx| {
                let Some((_, choice)) = selected.get(index.row) else {
                    return;
                };
                let choice = choice.clone();
                if let Choice::Mode(mode) = choice {
                    this.open_palette(PaletteKind::from_mode(mode), window, cx);
                    return;
                }
                if let Choice::Remote {
                    remote,
                    visible,
                    source,
                } = choice
                {
                    this.apply_remote_choice(remote, visible, &source, window, cx);
                    return;
                }
                if matches!(choice, Choice::HistoryMore) {
                    this.load_more_history_matches(window, cx);
                    return;
                }
                this.close_palette(window, cx);
                match choice {
                    Choice::HistoryMore => unreachable!("search pagination keeps the picker open"),
                    Choice::History {
                        revision,
                        source,
                        identity,
                    } => this.open_history_result(revision, source, &identity, window, cx),
                    Choice::Remote { .. } => unreachable!("remote toggles keep the picker open"),
                    Choice::Reference { reference, source } => {
                        this.apply_reference_choice(reference, &source, false, window, cx);
                    }
                    Choice::File { path, source, .. } => {
                        if this
                            .comparison()
                            .is_some_and(|comparison| comparison.identity() == *source)
                        {
                            this.select_path(&path, window, cx);
                        }
                    }
                    Choice::Command(command) => {
                        if command != jaywalk::commands::CommandId::ClearCommandUsage
                            && this.settings.picker.adaptive_command_order
                        {
                            crate::command_usage::record(command, cx);
                        }
                        this.execute_command(command, window, cx);
                    }
                    Choice::Mode(_) => unreachable!("mode selections keep the picker open"),
                    Choice::Line {
                        path,
                        line,
                        source,
                        before,
                    } => {
                        if this.selected_path().as_deref() == Some(&path)
                            && this
                                .comparison()
                                .is_some_and(|comparison| comparison.identity() == *source)
                        {
                            this.go_to_source_line(line, before, window, cx);
                        }
                    }
                    Choice::Symbol { entry, source } => {
                        this.select_symbol(entry.clone(), source.as_ref(), window, cx)
                    }
                    Choice::Review(choice) => this.apply_review_choice(choice, window, cx),
                    Choice::LocalRepository(path) => this.open_review_locally(path, window, cx),
                    Choice::RestoreCheckout(path) => this.restore_checkout(path, window, cx),
                    Choice::Profile(name) => this.select_keybindings_profile(name, window, cx),
                    Choice::Repository { directory, .. } => this.open_repository_path(
                        directory,
                        crate::windows::OpenDisposition::Configured,
                        cx,
                    ),
                }
            },
        );
        let command =
            Command::new(&self.palette.state)
                .bordered(false)
                .filterable(false)
                .placeholder(match kind {
                    PaletteKind::PullRequests | PaletteKind::Accounts => {
                        unreachable!("specialized picker renders its own rows")
                    }
                    PaletteKind::LocalRepositories => "Choose a local repository…",
                    PaletteKind::CheckoutRecovery => "Choose a checkout to restore…",
                    PaletteKind::Help => "Search picker modes…",
                    PaletteKind::History => "Find SHA, change ID, message, author or reference…",
                    PaletteKind::Line => "Go to source line…",
                    PaletteKind::FileSymbols | PaletteKind::ProjectSymbols => "Search symbols…",
                    PaletteKind::Files => {
                        if self.hosted_changed_only() {
                            "Find changed PR file…"
                        } else {
                            "Find any repository file…"
                        }
                    }
                    PaletteKind::Commands => "Run a command…",
                    PaletteKind::Repositories => "Search recent repositories…",
                    PaletteKind::Profiles => "Select keybindings profile…",
                    PaletteKind::Reviews => "Choose review branch or comparison base…",
                    PaletteKind::References => "Search branches and bookmarks…",
                    PaletteKind::Remotes => "Search remote history sources…",
                })
                .items(choices.iter().enumerate().map(|(index, (label, choice))| {
                    let query = query.to_owned();
                    let viewer = cx.entity().downgrade();
                    let repository = match choice {
                        Choice::Repository { directory, .. } => Some(directory.clone()),
                        _ => None,
                    };
                    let label: SharedString = label.clone().into();
                    let command = match choice {
                        Choice::Command(command) => Some(*command),
                        Choice::Mode(mode) => mode_command(*mode),
                        _ => None,
                    };
                    let disabled =
                        matches!(choice, Choice::Mode(mode) if !self.picker_mode_available(*mode))
                            || matches!(choice, Choice::HistoryMore) && self.palette.history.loading;
                    let status = match choice {
                        Choice::File { status, .. } => status.clone(),
                        Choice::History { revision, .. } => Some(format!("{} · {}", revision.author, revision.timestamp.get(..10).unwrap_or(""))),
                        Choice::Reference { reference, .. } => {
                            Some(self.reference_choice_status(reference))
                        }
                        Choice::Remote {
                            remote: Some(remote),
                            ..
                        } => Some(
                            if self.references.visibility.includes(remote) {
                                "Visible"
                            } else {
                                "Hidden"
                            }
                            .into(),
                        ),
                        Choice::Command(command) => self.command_state_hint(*command),
                        Choice::Mode(mode) => Some(
                            if self.picker_mode_available(*mode) {
                                mode_description(*mode)
                            } else {
                                "Open a comparison/file first"
                            }
                            .into(),
                        ),
                        Choice::Profile(name) if name == &self.settings.keyboard.profile => {
                            Some("Active".into())
                        }
                        _ => None,
                    };
                    CommandItem::new()
                        .key(match choice { Choice::History { revision, .. } => format!("history:{}", revision.commit_id), Choice::HistoryMore => "history:more".into(), _ => format!("{kind:?}:{label}") })
                        .disabled(disabled)
                        .label(label.clone())
                        .child(move |window, cx| {
                            let shortcut = command.and_then(|command| {
                                gpui::component::kbd::Kbd::binding_for_action(
                                    &commands::ExecuteCommand { command },
                                    None,
                                    window,
                                )
                            });
                            let viewer = viewer.clone();
                            let repository = repository.clone();
                            div()
                                .id(("palette-item-content", index))
                                .on_click(move |event: &ClickEvent, window, cx| {
                                    if event.modifiers().secondary(cx)
                                        && let Some(directory) = &repository
                                    {
                                        cx.stop_propagation();
                                        let _ = viewer.update(cx, |viewer, cx| {
                                            viewer.close_palette(window, cx);
                                            viewer.open_repository_path(
                                                directory.clone(),
                                                crate::windows::OpenDisposition::NewWindow,
                                                cx,
                                            );
                                        });
                                    }
                                })
                                .h_flex()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .child(div().flex_1().min_w_0().truncate().child(
                                    crate::picker_ui::matched_label(label.clone(), &query, cx),
                                ))
                                .children(
                                    status.clone().map(|status| {
                                        div().flex_shrink_0().text_sm().child(status)
                                    }),
                                )
                                .children(shortcut)
                        })
                }))
                .when(matches!(kind, PaletteKind::Repositories), |command| {
                    command.footer(|_, window, _| {
                        div()
                            .h_flex()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .children(gpui::component::kbd::Kbd::binding_for_action(
                                &commands::ExecuteCommand {
                                    command: jaywalk::commands::CommandId::OpenInNewWindow,
                                },
                                None,
                                window,
                            ))
                            .child("Open in new window")
                    })
                })
                .description({
                    let message = self.picker_description(kind, cx);
                    move |_, _, cx| crate::picker_ui::context([message.clone()], cx)
                })
                .max_h(px(360.))
                .empty({
                    let message = self.picker_empty(kind);
                    move |_, _, _| div().p_3().child(message)
                })
                .on_query(cx.listener(|this, query: &str, window, cx| {
                    this.palette_query_changed(query, window, cx)
                }))
                .on_cancel(move |window, cx| cancel(&(), window, cx))
                .on_confirm(move |index, window, cx| confirm(&index, window, cx));
        Some(
            div()
                .id("palette-overlay")
                .debug_selector(|| "palette-overlay".into())
                .capture_action(
                    cx.listener(|this, _: &gpui::base::input::Escape, window, cx| {
                        cx.stop_propagation();
                        this.close_palette(window, cx);
                    }),
                )
                .capture_action(
                    cx.listener(|this, _: &gpui::base::actions::Cancel, window, cx| {
                        cx.stop_propagation();
                        this.close_palette(window, cx);
                    }),
                )
                .when(matches!(kind, PaletteKind::References), |panel| {
                    let choices = choices.clone();
                    panel
                        .key_context("JaywalkReferencePicker")
                        .capture_action(cx.listener(
                            move |this, action: &commands::ExecuteCommand, window, cx| {
                                if action.command
                                    != jaywalk::commands::CommandId::ReviewSelectedReference
                                {
                                    cx.propagate();
                                    return;
                                }
                                cx.stop_propagation();
                                let Some(index) = this
                                    .palette
                                    .state
                                    .read(cx)
                                    .selected_index()
                                    .map(|index| index.row)
                                else {
                                    return;
                                };
                                let Some((_, Choice::Reference { reference, source })) =
                                    choices.get(index)
                                else {
                                    return;
                                };
                                let (reference, source) = (reference.clone(), source.clone());
                                this.close_palette(window, cx);
                                this.apply_reference_choice(reference, &source, true, window, cx);
                            },
                        ))
                })
                .when(matches!(kind, PaletteKind::Repositories), |panel| {
                    panel.key_context("JaywalkRecent")
                })
                .absolute()
                .inset_0()
                .bg(crate::appearance::color(jaywalk::theme::Role::Backdrop, cx))
                .flex()
                .justify_center()
                .items_start()
                .pt_12()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
                )
                .child(
                    crate::picker_ui::frame(cx)
                        .w(rems(48.))
                        .max_w_full()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(command)
                        .children(
                            matches!(kind, PaletteKind::Repositories)
                                .then(|| cx.global::<crate::recents::Recents>().error.as_ref())
                                .flatten()
                                .map(|error| feedback::error_message("recent-picker-error", error)),
                        ),
                )
                .into_any_element(),
        )
    }
    pub(super) fn navigate_revision(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let Ok(history) = &session.history else {
            return;
        };
        if history.revisions.is_empty() {
            return;
        }
        let selected = match &session.scope {
            ComparisonScope::Aggregate => None,
            ComparisonScope::Revision { commit_id, .. } => Some(commit_id),
            ComparisonScope::Evolution { anchor, .. } => Some(anchor),
        };
        let current = selected.and_then(|id| {
            history
                .revisions
                .iter()
                .position(|revision| &revision.commit_id == id)
        });
        let Some(step) = history_step(current, delta, history.revisions.len()) else {
            return;
        };
        let index = match step {
            HistoryStep::Overview => {
                self.show_all_changes_overview(window, cx);
                return;
            }
            HistoryStep::Revision(index) => index,
        };
        let commit = history.revisions[index].commit_id.clone();
        self.history_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        self.select_revision(&commit, window, cx);
    }

    pub(super) fn navigate_file(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(comparison) = self.comparison() else {
            return;
        };
        if comparison.files().is_empty() {
            return;
        }
        let current = self
            .selected_changed_index()
            .map(|index| index as i32)
            .unwrap_or(if delta > 0 {
                -1
            } else {
                comparison.files().len() as i32
            });
        let next = (current + delta).clamp(0, comparison.files().len() as i32 - 1) as usize;
        self.select_file(next, window, cx);
    }
}
#[cfg(test)]
mod tests {
    use super::{
        Choice, PaletteKind, file_rank, fuzzy_score, rank_choices, rank_commands, recent_rank,
        repository_choices,
    };
    #[test]
    fn empty_command_query_mixes_context_with_at_most_two_learned_favorites() {
        use jaywalk::{commands::CommandId as C, quick_picker::CommandUsage};
        let choices = || {
            vec![
                ("Files".into(), Choice::Command(C::FindFile)),
                ("Overview".into(), Choice::Command(C::Overview)),
                ("Wrap".into(), Choice::Command(C::ToggleWrap)),
                ("Welcome".into(), Choice::Command(C::ShowWelcome)),
            ]
        };
        let mut usage = CommandUsage::default();
        usage.record(C::ToggleWrap);
        usage.record(C::Overview);
        usage.record(C::ShowWelcome);
        let context = [C::FindFile, C::Overview];
        let rows = rank_commands("", choices(), Some(&usage), &context);
        assert!(matches!(rows[0].1, Choice::Command(C::ShowWelcome)));
        assert!(matches!(rows[1].1, Choice::Command(C::Overview)));
        assert!(matches!(rows[2].1, Choice::Command(C::FindFile)));
        assert_eq!(rows.len(), 4);
        let rows = rank_commands("", choices(), None, &context);
        assert!(matches!(rows[0].1, Choice::Command(C::FindFile)));
        let rows = rank_commands("Wrap", choices(), Some(&usage), &context);
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0].1, Choice::Command(C::ToggleWrap)));
    }
    #[test]
    fn recent_picker_preserves_mru_order_and_offers_open_when_history_is_empty() {
        let paths = vec![
            std::path::PathBuf::from("/repositories/zebra"),
            "/repositories/alpha".into(),
        ];
        let choices = || repository_choices(&paths);
        let ordered = rank_choices(PaletteKind::Repositories, "", choices());
        assert!(
            matches!(&ordered[0].1, Choice::Repository { directory, .. } if directory == &paths[0])
        );
        assert!(
            matches!(&ordered[1].1, Choice::Repository { directory, .. } if directory == &paths[1])
        );
        assert!(matches!(
            ordered[2].1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
        let filtered = rank_choices(PaletteKind::Repositories, "ALPHA", choices());
        assert!(
            matches!(&filtered[..], [(_, Choice::Repository { directory, .. }), (_, Choice::Command(jaywalk::commands::CommandId::OpenRepository))] if directory == &paths[1])
        );
        let empty = rank_choices(PaletteKind::Repositories, "", repository_choices(&[]));
        assert_eq!(empty.len(), 1);
        assert!(matches!(
            empty[0].1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
    }
    #[test]
    fn recent_picker_prefers_names_and_keeps_open_as_a_fallback() {
        let paths = vec![
            std::path::PathBuf::from("/projects/other"),
            "/repos/p-r-o".into(),
            "/repos/man-protos".into(),
            "/repos/protos".into(),
            "/repos/pro".into(),
        ];
        let ordered = rank_choices(PaletteKind::Repositories, "pro", repository_choices(&paths));
        let repositories: Vec<_> = ordered
            .iter()
            .filter_map(|(_, choice)| match choice {
                Choice::Repository { directory, .. } => Some(directory),
                _ => None,
            })
            .collect();
        assert_eq!(
            repositories,
            vec![&paths[4], &paths[3], &paths[2], &paths[1], &paths[0]]
        );
        assert!(matches!(
            ordered.last().unwrap().1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
        let missing = rank_choices(
            PaletteKind::Repositories,
            "no-match",
            repository_choices(&paths),
        );
        assert_eq!(missing.len(), 1);
        assert!(matches!(
            missing[0].1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
    }

    #[test]
    fn recent_picker_matches_full_paths_and_unicode_names() {
        assert!(
            recent_rank(
                "/repositories/Δelta",
                std::path::Path::new("/repositories/Δelta")
            )
            .is_some()
        );
        assert!(recent_rank("δELTA", std::path::Path::new("/repositories/Δelta")).is_some());
        assert!(recent_rank("missing", std::path::Path::new("/repositories/Δelta")).is_none());
    }
    #[test]
    fn file_ranking_keeps_path_relevance_ahead_of_change_status() {
        assert!(
            file_rank("readme", "README.md", false).unwrap()
                < file_rank("readme", "docs/README.md", true).unwrap()
        );
        assert!(file_rank("", "a.rs", true).unwrap() < file_rank("", "b.rs", false).unwrap());
        assert!(file_rank("modified", "src/lib.rs", true).is_none());
    }

    #[test]
    fn fuzzy_paths_support_unicode_and_noncontiguous_queries() {
        assert!(fuzzy_score("svcfg", "src/viewer/config.rs").is_some());
        assert!(fuzzy_score("CAFÉ", "src/café.ts").is_some());
        assert!(fuzzy_score("zq", "src/main.rs").is_none());
    }
}

#[cfg(test)]
mod history_navigation_tests {
    use super::{HistoryStep, history_step};
    #[test]
    fn overview_precedes_the_first_history_entry_without_wrapping() {
        assert_eq!(history_step(None, 1, 3), Some(HistoryStep::Revision(0)));
        assert_eq!(history_step(Some(0), -1, 3), Some(HistoryStep::Overview));
        assert_eq!(history_step(None, -1, 3), None);
        assert_eq!(history_step(Some(0), 1, 3), Some(HistoryStep::Revision(1)));
        assert_eq!(history_step(Some(2), 1, 3), None);
        assert_eq!(history_step(None, 1, 0), None);
        assert_eq!(history_step(Some(0), -1, 1), Some(HistoryStep::Overview));
    }
}

#[cfg(test)]
impl Viewer {
    pub(super) fn palette_test_file_searches(&self) -> usize {
        self.palette.files.searches
    }
    pub(super) fn palette_test_line_before(&self) -> bool {
        self.palette.line_before
    }
    pub(super) fn palette_test_set_query(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette.state.update(cx, |state, cx| {
            state.set_query(query.to_owned(), window, cx)
        });
    }
}

impl Viewer {
    pub(super) fn open_account_palette(
        &mut self,
        command: jaywalk::commands::CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !cx.has_global::<jaywalk::accounts::application::Accounts>() {
            jaywalk::accounts::application::init(cx);
        }
        self.open_palette(PaletteKind::Accounts, window, cx);
        let input = self.palette.state.clone();
        let delay = self.settings.ui.loading_delay_ms;
        let picker = cx
            .new(|cx| crate::account_picker::AccountPicker::new(command, input, delay, window, cx));
        cx.subscribe_in(&picker, window, |viewer, picker, event, window, cx| {
            if viewer.palette.open != Some(PaletteKind::Accounts)
                || viewer.palette.accounts.as_ref() != Some(picker)
            {
                return;
            }
            match event {
                crate::account_picker::Event::Leave => viewer.close_palette(window, cx),
                crate::account_picker::Event::Input(input) => {
                    viewer.palette.state = input.clone();
                }
                crate::account_picker::Event::Query(input, query) => {
                    viewer.palette.state = input.clone();
                    viewer.palette_query_changed(query, window, cx);
                }
                crate::account_picker::Event::Browse => {
                    viewer.close_palette(window, cx);
                    viewer.show_accounts(jaywalk::commands::CommandId::Accounts, window, cx);
                }
            }
        })
        .detach();
        self.palette.accounts = Some(picker);
    }
}
