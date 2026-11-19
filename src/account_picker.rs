//! Keyboard account flows over the shared asynchronous account controller.
use gpui::component::{
    Disableable,
    command::{Command, CommandItem, CommandState},
};
use gpui::*;
use jaywalk::{
    accounts::{
        AccountKey, AccountView, ConnectionState, Provider,
        application::{AccountState, Accounts},
        controller::{AccountAction, OperationHandle, Phase},
    },
    commands::CommandId,
};

pub(crate) enum Event {
    Leave,
    Input(Entity<CommandState>),
    Query(Entity<CommandState>, String),
    Browse,
}
#[derive(Clone)]
enum Step {
    Home,
    Providers,
    Accounts(CommandId),
    Actions(AccountKey),
    Disconnect(AccountKey),
    Workspaces(AccountKey),
    Import,
}
#[derive(Clone)]
enum Choice {
    Step(Step),
    Action(AccountAction),
    Back,
    Cancel,
    Browser,
    Browse,
}
struct Visit {
    step: Step,
    input: Entity<CommandState>,
}
pub(crate) struct AccountPicker {
    state: Entity<AccountState>,
    input: Entity<CommandState>,
    step: Step,
    stack: Vec<Visit>,
    owner: Option<OperationHandle>,
    loading_visible: bool,
    input_loading: bool,
    return_after_finish: bool,
    loading_delay: std::time::Duration,
    _subscription: Subscription,
}
impl EventEmitter<Event> for AccountPicker {}
impl AccountPicker {
    pub(crate) fn new(
        command: CommandId,
        input: Entity<CommandState>,
        delay_ms: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = cx.global::<Accounts>().state.clone();
        let subscription = cx.observe_in(&state, window, |picker, state, window, cx| {
            if let Some(finished) = state.read(cx).last_finished
                && picker
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner.id == finished.id)
            {
                picker.owner = None;
                let success = state
                    .read(cx)
                    .notice
                    .as_ref()
                    .is_some_and(|notice| !notice.error);
                if std::mem::take(&mut picker.return_after_finish)
                    && success
                    && !picker.stack.is_empty()
                {
                    picker.back(window, cx);
                } else {
                    picker.input.update(cx, |input, cx| input.focus(window, cx));
                }
            }
            cx.notify();
        });
        let step = match command {
            CommandId::ConnectAccount => Step::Providers,
            CommandId::ReconnectAccount
            | CommandId::DisconnectAccount
            | CommandId::CheckAccount
            | CommandId::ChooseHostingWorkspace => Step::Accounts(command),
            CommandId::ImportHostingSetup => Step::Import,
            _ => Step::Home,
        };
        let mut picker = Self {
            state,
            input,
            step,
            stack: vec![],
            owner: None,
            loading_visible: false,
            input_loading: false,
            return_after_finish: false,
            loading_delay: std::time::Duration::from_millis(delay_ms),
            _subscription: subscription,
        };
        picker.start(AccountAction::Load, window, cx);
        picker
    }
    fn start(&mut self, action: AccountAction, window: &mut Window, cx: &mut Context<Self>) {
        let return_after_finish = matches!(
            action,
            AccountAction::Disconnect(_) | AccountAction::SelectWorkspace { .. }
        );
        if let Some(owner) = self.state.update(cx, |state, cx| state.start(action, cx)) {
            self.return_after_finish = return_after_finish;
            let id = owner.id;
            self.owner = Some(owner);
            self.loading_visible = self.loading_delay.is_zero();
            let delay = self.loading_delay;
            cx.spawn_in(window, async move |picker, cx| {
                cx.background_executor().timer(delay).await;
                let _ = picker.update(cx, |picker, cx| {
                    if picker.owner.as_ref().is_some_and(|owner| owner.id == id) {
                        picker.loading_visible = true;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }
    fn enter(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        self.stack.push(Visit {
            step: self.step.clone(),
            input: self.input.clone(),
        });
        self.step = step;
        self.input = cx.new(|cx| CommandState::new(window, cx));
        self.input_loading = false;
        self.input.update(cx, |input, cx| input.focus(window, cx));
        cx.emit(Event::Input(self.input.clone()));
        cx.notify();
    }
    fn cancel_owned(&mut self, cx: &mut Context<Self>) {
        if let Some(owner) = self.owner.take() {
            let id = owner.id;
            self.state.update(cx, |state, cx| {
                if state
                    .operation
                    .as_ref()
                    .is_some_and(|operation| operation.id == id)
                {
                    state.cancel(cx);
                }
            });
            drop(owner);
        }
    }
    pub(crate) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.cancel_owned(cx);
    }
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.owner.is_some() {
            self.cancel_owned(cx);
            cx.notify();
            return;
        }
        while let Some(visit) = self.stack.pop() {
            if let Step::Actions(key) | Step::Disconnect(key) | Step::Workspaces(key) = &visit.step
                && !self
                    .state
                    .read(cx)
                    .snapshot
                    .accounts
                    .iter()
                    .any(|account| account.identity.key == *key)
            {
                continue;
            }
            self.step = visit.step;
            self.input = visit.input;
            self.input_loading = !self.loading_visible;
            self.input.update(cx, |input, cx| input.focus(window, cx));
            cx.emit(Event::Input(self.input.clone()));
            cx.notify();
            return;
        }
        cx.emit(Event::Leave);
    }
    fn title(&self) -> &'static str {
        match self.step {
            Step::Home => "Manage accounts",
            Step::Providers => "Connect account",
            Step::Accounts(CommandId::ChooseHostingWorkspace) => "Choose Bitbucket account",
            Step::Accounts(_) => "Choose account",
            Step::Actions(_) => "Account actions",
            Step::Disconnect(_) => "Confirm local disconnect",
            Step::Workspaces(_) => "Choose Bitbucket workspace",
            Step::Import => "Import saved development setup",
        }
    }
    fn rows(&self, cx: &App) -> Vec<(String, String, Choice)> {
        let state = self.state.read(cx);
        let mut rows = vec![];
        let account_row = |account: &AccountView, choice| {
            let status = match &account.state {
                ConnectionState::Stored => "Saved",
                ConnectionState::Checking => "Checking",
                ConnectionState::Refreshing => "Refreshing",
                ConnectionState::Connected => "Connected",
                ConnectionState::NeedsSignIn => "Needs sign-in",
                ConnectionState::Unavailable(_) => "Unavailable",
            };
            (
                format!(
                    "account:{}:{}",
                    account.identity.key.provider.name(),
                    account.identity.key.subject
                ),
                format!(
                    "{} · {} · {status}",
                    account.identity.key.provider.name(),
                    account.identity.label
                ),
                choice,
            )
        };
        match &self.step {
            Step::Home => {
                rows.push((
                    "connect".into(),
                    "Connect another account…".into(),
                    Choice::Step(Step::Providers),
                ));
                rows.extend(state.snapshot.accounts.iter().map(|account| {
                    account_row(
                        account,
                        Choice::Step(Step::Actions(account.identity.key.clone())),
                    )
                }));
                rows.push(("browse".into(), "Open Accounts page".into(), Choice::Browse));
                rows.push((
                    "import".into(),
                    "Import development setup…".into(),
                    Choice::Step(Step::Import),
                ));
            }
            Step::Providers => {
                for provider in [Provider::Github, Provider::Bitbucket] {
                    rows.push((
                        provider.name().into(),
                        format!("Connect {} in browser…", provider.name()),
                        Choice::Action(AccountAction::Connect {
                            provider,
                            replace: None,
                        }),
                    ));
                }
            }
            Step::Accounts(command) => {
                for account in &state.snapshot.accounts {
                    let key = account.identity.key.clone();
                    let choice = match command {
                        CommandId::ReconnectAccount => Choice::Action(AccountAction::Connect {
                            provider: key.provider,
                            replace: Some(key),
                        }),
                        CommandId::DisconnectAccount => Choice::Step(Step::Disconnect(key)),
                        CommandId::ChooseHostingWorkspace
                            if key.provider == Provider::Bitbucket =>
                        {
                            Choice::Step(Step::Workspaces(key))
                        }
                        CommandId::CheckAccount => Choice::Action(AccountAction::Verify(key)),
                        _ => continue,
                    };
                    rows.push(account_row(account, choice));
                }
            }
            Step::Actions(key)
                if state
                    .snapshot
                    .accounts
                    .iter()
                    .any(|account| account.identity.key == *key) =>
            {
                rows.push((
                    "check".into(),
                    "Check connection".into(),
                    Choice::Action(AccountAction::Verify(key.clone())),
                ));
                rows.push((
                    "reconnect".into(),
                    "Reconnect in browser…".into(),
                    Choice::Action(AccountAction::Connect {
                        provider: key.provider,
                        replace: Some(key.clone()),
                    }),
                ));
                if key.provider == Provider::Bitbucket {
                    rows.push((
                        "workspaces".into(),
                        "Choose workspace…".into(),
                        Choice::Step(Step::Workspaces(key.clone())),
                    ));
                }
                rows.push((
                    "disconnect".into(),
                    "Disconnect locally…".into(),
                    Choice::Step(Step::Disconnect(key.clone())),
                ));
            }
            Step::Disconnect(key) => {
                rows.push(("keep".into(), "Keep account".into(), Choice::Back));
                if state
                    .snapshot
                    .accounts
                    .iter()
                    .any(|account| account.identity.key == *key)
                {
                    rows.push((
                        "confirm-disconnect".into(),
                        "Confirm local disconnect".into(),
                        Choice::Action(AccountAction::Disconnect(key.clone())),
                    ));
                }
            }
            Step::Workspaces(key) => {
                if let Some(listing) = &state.workspaces
                    && listing.account == *key
                {
                    rows.extend(listing.workspaces.iter().map(|workspace| {
                        (
                            workspace.uuid.clone(),
                            workspace.slug.clone(),
                            Choice::Action(AccountAction::SelectWorkspace {
                                listing: listing.clone(),
                                uuid: workspace.uuid.clone(),
                            }),
                        )
                    }));
                }
                rows.push((
                    "reload-workspaces".into(),
                    "Reload workspaces".into(),
                    Choice::Action(AccountAction::Workspaces(key.clone())),
                ));
            }
            Step::Import => {
                rows.push(("keep-setup".into(), "Cancel import".into(), Choice::Back));
                rows.push((
                    "confirm-import".into(),
                    "Import development setup saved on this computer".into(),
                    Choice::Action(AccountAction::ImportDevelopment),
                ));
            }
            _ => {}
        }
        if state.operation.is_some() && self.owner.is_some() {
            rows.insert(
                0,
                (
                    "cancel-operation".into(),
                    "Cancel account action".into(),
                    Choice::Cancel,
                ),
            );
            if state
                .operation
                .as_ref()
                .is_some_and(|operation| operation.phase == Phase::WaitingForBrowser)
            {
                rows.insert(
                    0,
                    (
                        "browser-again".into(),
                        "Open authorization browser again".into(),
                        Choice::Browser,
                    ),
                );
            }
        }
        rows
    }
    fn choose(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        match choice {
            Choice::Step(step) => {
                let load = if let Step::Workspaces(key) = &step {
                    Some(key.clone())
                } else {
                    None
                };
                self.enter(step, window, cx);
                if let Some(key) = load {
                    self.start(AccountAction::Workspaces(key), window, cx);
                }
            }
            Choice::Action(action) => self.start(action, window, cx),
            Choice::Back => self.back(window, cx),
            Choice::Cancel => {
                self.cancel_owned(cx);
                cx.notify();
            }
            Choice::Browser => self
                .state
                .update(cx, |state, cx| state.open_browser_again(cx)),
            Choice::Browse => cx.emit(Event::Browse),
        }
    }
}
impl Render for AccountPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.read(cx);
        let busy = state.operation.is_some();
        let message = state.operation.as_ref().map(|op| op.label().to_owned()).or_else(|| state.notice.as_ref().map(|notice| notice.text.clone())).or_else(|| state.snapshot.store_error.map(|error| error.to_string())).unwrap_or_else(|| match self.step {
            Step::Disconnect(_) => "This removes the local connection; provider authorization stays unchanged.".into(),
            Step::Providers => "Authorization opens in your browser. No secrets are entered in this picker.".into(),
            Step::Import => "Developer registration only; copies saved client setup, not another account's grant.".into(),
            _ => "Choose an action · ? lists modes · Escape goes back or closes".into(),
        });
        let loading = busy && self.loading_visible;
        if loading != self.input_loading {
            self.input_loading = loading;
            self.input
                .update(cx, |input, cx| input.set_loading(loading, window, cx));
        }
        let query = self.input.read(cx).query(cx).to_string();
        let rows = self
            .rows(cx)
            .into_iter()
            .filter(|(_, label, choice)| account_row_matches(choice, label, &query))
            .collect::<Vec<_>>();
        let title = self.title().to_owned();
        let confirm = rows
            .iter()
            .map(|(_, _, choice)| choice.clone())
            .collect::<Vec<_>>();
        let input = self.input.clone();
        let owner = cx.entity().downgrade();
        let input_owner = owner.clone();
        let context = [title.clone(), message];
        let command = Command::new(&self.input)
            .bordered(false)
            .filterable(false)
            .max_h(px(360.))
            .placeholder(title)
            .description(move |_, _, cx| crate::picker_ui::context(context.clone(), cx))
            .empty(|_, _, _| {
                div().p_3().child(
                    "No matching accounts or actions. Use Connect account, or Escape to go back.",
                )
            })
            .items(rows.into_iter().map(|(key, label, choice)| {
                let query = query.clone();
                let label: SharedString = label.into();
                CommandItem::new()
                    .key(key)
                    .label(label.clone())
                    .disabled(busy && !matches!(choice, Choice::Cancel | Choice::Browser))
                    .child(move |_, cx| crate::picker_ui::matched_label(label.clone(), &query, cx))
            }))
            .on_query(move |query, _, cx| {
                let _ = input_owner.update(cx, |picker, cx| {
                    if picker.input == input {
                        cx.emit(Event::Query(input.clone(), query.to_owned()));
                        cx.notify();
                    }
                });
            })
            .on_confirm(move |index, window, cx| {
                if let Some(choice) = confirm.get(index.row) {
                    let _ =
                        owner.update(cx, |picker, cx| picker.choose(choice.clone(), window, cx));
                }
            });
        div()
            .debug_selector(|| "account-palette-picker".into())
            .capture_action(
                cx.listener(|picker, _: &gpui::base::input::Escape, window, cx| {
                    cx.stop_propagation();
                    picker.back(window, cx);
                }),
            )
            .capture_action(
                cx.listener(|picker, _: &gpui::base::actions::Cancel, window, cx| {
                    cx.stop_propagation();
                    picker.back(window, cx);
                }),
            )
            .child(command)
    }
}

#[cfg(test)]
mod tests;

fn account_row_matches(choice: &Choice, label: &str, query: &str) -> bool {
    matches!(choice, Choice::Cancel | Choice::Browser)
        || query.trim().is_empty()
        || !jaywalk::quick_picker::match_ranges(label, query).is_empty()
}
