//! Central, read-only PR picker. Typing filters snapshots; only explicit scope,
//! refresh and lookup actions start I/O. Main-window ownership cancels waits.
mod filters;
mod recent;
use filters::Step;
use gpui::component::{
    Disableable, Selectable, StyledExt,
    button::{Button, ButtonVariants},
    command::{Command, CommandItem, CommandState},
    spinner::Spinner,
};
use gpui::prelude::FluentBuilder;
use gpui::*;
use jaywalk::{
    accounts::{AccountKey, AccountView, ConnectionState, Control, Error, application::Accounts},
    hosting::{self, Batch, EntryInput, Fetched, Locator, PullRequest, RemoteRepository, Scope},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

actions!(hosting_ui, [Cancel, Refresh]);
pub(crate) enum Event {
    Leave,
    Opened(hosting::review::Review),
    Accounts,
}
impl EventEmitter<Event> for PullRequestsPage {}
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Cancel, Some("JaywalkPullRequests")),
        KeyBinding::new("f5", Refresh, Some("JaywalkPullRequests")),
    ]);
}
struct AccountResult {
    account: AccountView,
    batch: Option<Arc<Batch>>,
    fetched: Option<Instant>,
    error: Option<Error>,
    cached: bool,
    saved_at: Option<u64>,
}
#[derive(Clone)]
enum Choice {
    Item {
        account: AccountView,
        batch: Arc<Batch>,
        index: usize,
    },
    Lookup {
        account: AccountView,
        locator: Locator,
    },
    Recent(Locator),
}
impl Choice {
    fn label(&self) -> String {
        match self {
            Self::Item {
                account,
                batch,
                index,
            } => format!(
                "{} · {}",
                batch.items[*index].label(),
                account.identity.label
            ),
            Self::Lookup { account, locator } => {
                format!("Look up {} · {}", locator.label(), account.identity.label)
            }
            Self::Recent(locator) => format!("Recent · {}", locator.label()),
        }
    }
}
enum Update {
    Setup {
        remotes: Result<Vec<RemoteRepository>, Error>,
        recent: Result<Vec<Locator>, Error>,
    },
    Account(AccountView, Result<Fetched, Error>),
    Saved(AccountView, hosting::snapshots::SavedBatch),
    Message(String),
    Notice(String),
    Done,
}
struct DiscoveryRequest {
    backend: Result<Arc<jaywalk::accounts::Service>, Error>,
    root: Option<PathBuf>,
    path: PathBuf,
    scope: Option<Scope>,
    account: Option<AccountKey>,
    force: bool,
}
impl DiscoveryRequest {
    fn run(self, sender: &async_channel::Sender<Update>, control: &Control) -> Result<(), Error> {
        control.check()?;
        let remotes = self
            .root
            .as_deref()
            .map(hosting::repository_remotes)
            .unwrap_or_else(|| Ok(vec![]));
        let recent = hosting::recent::load(&self.path);
        let _ = sender.try_send(Update::Setup { remotes, recent });
        let Some(scope) = self.scope else {
            return Ok(());
        };
        let backend = self.backend?;
        backend.restore()?;
        control.check()?;
        let accounts = backend.accounts_snapshot().accounts;
        if accounts.is_empty() {
            let _ = sender.try_send(Update::Notice(
                "Connect an account to discover pull requests.".into(),
            ));
        }
        if accounts.len() > 10 && self.account.is_none() {
            let _ = sender.try_send(Update::Notice(
                "Discovery is limited to the first ten accounts; select an account for more."
                    .into(),
            ));
        }
        let accounts = accounts
            .into_iter()
            .filter(|account| {
                self.account
                    .as_ref()
                    .is_none_or(|key| key == &account.identity.key)
            })
            .take(10)
            .collect::<Vec<_>>();
        let snapshot_path = self.path.with_file_name("pull-request-cache.json");
        match hosting::snapshots::load(&snapshot_path) {
            Ok(saved) => {
                for account in &accounts {
                    if !matches!(account.state, ConnectionState::NeedsSignIn)
                        && let Some(batch) = saved.get(&account.identity.key, &scope)
                    {
                        let _ = sender.try_send(Update::Saved(account.clone(), batch));
                    }
                }
            }
            Err(error) => {
                let _ = sender.try_send(Update::Notice(format!("Saved PR results: {error}")));
            }
        }
        for account in accounts {
            control.check()?;
            let _ = sender.try_send(Update::Message(format!(
                "Loading {} · {}…",
                account.identity.key.provider.name(),
                account.identity.label
            )));
            let result = if self.force {
                backend.refresh_pull_requests(&account.identity.key, &scope, control)
            } else {
                backend.pull_requests(&account.identity.key, &scope, control)
            };
            control.check()?;
            if let Ok(fetched) = &result
                && fetched.generation == account.generation
                && backend.accounts_snapshot().accounts.iter().any(|current| {
                    current.identity.key == account.identity.key
                        && current.generation == account.generation
                        && !matches!(current.state, ConnectionState::NeedsSignIn)
                })
                && let Err(error) = hosting::snapshots::save(
                    &snapshot_path,
                    &account.identity.key,
                    &scope,
                    &fetched.batch,
                    fetched.fetched_at,
                    control,
                )
            {
                let _ = sender.try_send(Update::Notice(format!(
                    "Could not save PR results: {error}"
                )));
            }
            control.check()?;
            let _ = sender.try_send(Update::Account(account, result));
        }
        Ok(())
    }
}

pub(crate) struct PullRequestsPage {
    picking: bool,
    input_loading: bool,
    step: Step,
    filter_stack: Vec<Step>,
    filter_input: Entity<CommandState>,
    input: Entity<CommandState>,
    scroll: ScrollHandle,
    root: Option<PathBuf>,
    recent_path: PathBuf,
    scope: Option<Scope>,
    account: Option<AccountKey>,
    views: HashMap<Scope, Vec<AccountResult>>,
    remotes: Vec<RemoteRepository>,
    operation: Option<Control>,
    association: Option<Control>,
    review_options: hosting::review::Options,
    text_limit: jaywalk::text_content::TextLimit,
    opening: bool,
    serial: u64,
    progress: Option<String>,
    notice: Option<String>,
    _accounts: Subscription,
}
impl Drop for PullRequestsPage {
    fn drop(&mut self) {
        if let Some(control) = &self.operation {
            control.cancel();
        }
        if let Some(control) = &self.association {
            control.cancel();
        }
    }
}
impl PullRequestsPage {
    pub(crate) fn new(
        root: Option<PathBuf>,
        recent_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        recent::init(cx);
        if !cx.has_global::<Accounts>() {
            jaywalk::accounts::application::init(cx);
        }
        let accounts = cx.global::<Accounts>().state.clone();
        let subscription = cx.observe_in(&accounts, window, |page, _, _, cx| {
            page.purge_retired(cx);
            cx.notify();
        });
        let input = cx.new(|cx| CommandState::new(window, cx));
        let mut page = Self {
            picking: false,
            input_loading: false,
            step: Step::Results,
            filter_stack: Vec::new(),
            filter_input: cx.new(|cx| CommandState::new(window, cx)),
            input,
            scroll: ScrollHandle::new(),
            root,
            recent_path,
            scope: Some(Scope::Authored),
            account: None,
            views: HashMap::new(),
            remotes: vec![],
            operation: None,
            association: None,
            review_options: Default::default(),
            text_limit: Default::default(),
            opening: false,
            serial: 0,
            progress: None,
            notice: None,
            _accounts: subscription,
        };
        page.refresh(window, cx);
        page
    }
    pub(crate) fn as_picker(&mut self) {
        self.picking = true;
    }
    pub(crate) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.focus(window, cx));
    }
    fn accounts(&self, cx: &App) -> Vec<AccountView> {
        cx.global::<Accounts>()
            .state
            .read(cx)
            .snapshot
            .accounts
            .clone()
    }
    fn active(&self, account: &AccountView, cx: &App) -> bool {
        self.accounts(cx).iter().any(|current| {
            current.identity.key == account.identity.key
                && current.generation == account.generation
                && !matches!(current.state, ConnectionState::NeedsSignIn)
        })
    }
    fn purge_retired(&mut self, cx: &App) {
        if !cx.global::<Accounts>().state.read(cx).snapshot.loaded {
            return;
        }
        let accounts = self.accounts(cx);
        for results in self.views.values_mut() {
            results.retain(|result| {
                accounts.iter().any(|account| {
                    account.identity.key == result.account.identity.key
                        && account.generation == result.account.generation
                })
            });
            for result in results.iter_mut() {
                if accounts.iter().any(|account| {
                    account.identity.key == result.account.identity.key
                        && matches!(account.state, ConnectionState::NeedsSignIn)
                }) {
                    result.batch = None;
                    result.error = Some(Error::NeedsSignIn);
                }
            }
        }
    }
    fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(control) = self.association.take() {
            control.cancel();
        }
        self.opening = false;
        if self.picking {
            cx.emit(Event::Leave);
            return;
        }
        if let Some(control) = self.operation.take() {
            control.cancel();
            self.serial += 1;
            self.progress = None;
            self.notice = Some("Cancelled. Previously loaded results remain available.".into());
            cx.notify();
        } else {
            cx.emit(Event::Leave);
        }
    }
    fn set_scope(&mut self, scope: Option<Scope>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(control) = self.association.take() {
            control.cancel();
        }
        self.opening = false;
        if matches!(scope, Some(Scope::Direct(_))) {
            self.views
                .retain(|scope, _| !matches!(scope, Scope::Direct(_)));
        }
        if self.views.len() >= 8
            && let Some(scope) = &scope
            && !self.views.contains_key(scope)
        {
            self.views
                .retain(|key, _| matches!(key, Scope::Authored | Scope::ReviewRequested));
        }
        self.scope = scope;
        self.input
            .update(cx, |state, cx| state.set_query("", window, cx));
        self.refresh(window, cx);
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load(false, window, cx);
    }
    pub(crate) fn force_refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load(true, window, cx);
    }
    fn load(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(control) = self.operation.take() {
            control.cancel();
        }
        self.serial += 1;
        let serial = self.serial;
        let control = Control::new(Duration::from_secs(180));
        self.operation = Some(control.clone());
        self.progress = Some("Loading saved accounts…".into());
        self.notice = None;
        let backend = cx.global::<Accounts>().backend.clone();
        let root = self.root.clone();
        let path = self.recent_path.clone();
        let scope = self.scope.clone();
        let filter = self.account.clone();
        let (sender, receiver) = async_channel::unbounded();
        let request = DiscoveryRequest {
            backend,
            root,
            path,
            scope,
            account: filter,
            force,
        };
        cx.spawn_in(window, async move |this, cx| {
            let worker_control = control.clone();
            let worker = cx.background_spawn(async move {
                if let Err(error) = request.run(&sender, &worker_control) {
                    let _ = sender.try_send(Update::Notice(error.to_string()));
                }
                let _ = sender.try_send(Update::Done);
            });
            while let Ok(event) = receiver.recv().await {
                let done = matches!(event, Update::Done);
                let result = this.update_in(cx, |page, window, cx| {
                    if serial != page.serial {
                        return;
                    }
                    match event {
                        Update::Setup { remotes, recent } => {
                            match remotes {
                                Ok(remotes) => page.remotes = remotes,
                                Err(error) => page.notice = Some(error.to_string()),
                            }
                            match recent {
                                Ok(items) => {
                                    let recents = cx.global_mut::<recent::Recents>();
                                    if recents.blocked.remove(&page.recent_path) {
                                        recents.lists.insert(page.recent_path.clone(), items);
                                    } else {
                                        recents
                                            .lists
                                            .entry(page.recent_path.clone())
                                            .or_insert(items);
                                    }
                                }
                                Err(error) => {
                                    cx.global_mut::<recent::Recents>()
                                        .blocked
                                        .insert(page.recent_path.clone());
                                    page.notice = Some(error.to_string());
                                }
                            }
                        }
                        Update::Saved(account, saved) => page.accept_saved(account, saved),
                        Update::Account(account, result) => {
                            page.accept(account, result, window, cx)
                        }
                        Update::Message(message) => page.progress = Some(message),
                        Update::Notice(message) => page.notice = Some(message),
                        Update::Done => {
                            page.operation = None;
                            if page.notice.is_none() {
                                page.notice = Some(
                                    "Ready. Results reflect the connected accounts’ access.".into(),
                                );
                            }
                            page.progress = None;
                        }
                    }
                    cx.notify();
                });
                if result.is_err() {
                    control.cancel();
                }
                if done {
                    break;
                }
            }
            worker.await;
        })
        .detach();
        cx.notify();
    }
    fn accept_saved(&mut self, account: AccountView, saved: hosting::snapshots::SavedBatch) {
        let Some(scope) = &self.scope else {
            return;
        };
        let results = self.views.entry(scope.clone()).or_default();
        if results.iter().any(|result| {
            result.account.identity.key == account.identity.key && result.batch.is_some()
        }) {
            return;
        }
        results.retain(|result| result.account.identity.key != account.identity.key);
        results.push(AccountResult {
            account,
            batch: Some(saved.batch),
            fetched: None,
            error: None,
            cached: true,
            saved_at: Some(saved.saved_at),
        });
    }
    fn accept(
        &mut self,
        account: AccountView,
        result: Result<Fetched, Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let results = self.views.entry(scope.clone()).or_default();
        let previous = results
            .iter()
            .position(|old| old.account.identity.key == account.identity.key);
        match result {
            Ok(fetched) if fetched.generation == account.generation => {
                let result = AccountResult {
                    account: account.clone(),
                    batch: Some(fetched.batch.clone()),
                    fetched: Some(Instant::now()),
                    error: None,
                    cached: fetched.cached,
                    saved_at: None,
                };
                if let Some(index) = previous {
                    results[index] = result;
                } else {
                    results.push(result);
                }
                if matches!(scope, Scope::Direct(_))
                    && !fetched.batch.items.is_empty()
                    && (!self.picking || self.step == Step::Results)
                {
                    self.input
                        .update(cx, |state, cx| state.set_query("", window, cx));
                    self.select(account, fetched.batch.items[0].clone(), window, cx);
                }
            }
            Ok(_) => (),
            Err(error) => {
                if let Some(index) = previous {
                    results[index].error = Some(error);
                } else {
                    results.push(AccountResult {
                        account,
                        batch: None,
                        fetched: None,
                        error: Some(error),
                        cached: false,
                        saved_at: None,
                    });
                }
            }
        }
    }
    fn choices(&self, cx: &App) -> Vec<Choice> {
        let query = self.input.read(cx).query(cx).to_string();
        let mut rows = vec![];
        let accounts = self.accounts(cx);
        if let Ok(input) = EntryInput::parse(&query) {
            let filter = self.account.as_ref().filter(|key| !matches!(&input, EntryInput::Url(locator) if locator.provider != key.provider));
            for account in accounts
                .iter()
                .filter(|account| filter.is_none_or(|key| key == &account.identity.key))
            {
                let locator = match &input {
                    EntryInput::Url(locator)
                        if locator.provider == account.identity.key.provider =>
                    {
                        Some(locator.clone())
                    }
                    EntryInput::Qualified { repository, number } => Some(Locator {
                        provider: account.identity.key.provider,
                        repository: repository.clone(),
                        number: *number,
                    }),
                    EntryInput::Number(number) => match &self.scope {
                        Some(Scope::Repository(provider, repository))
                            if *provider == account.identity.key.provider =>
                        {
                            Some(Locator {
                                provider: *provider,
                                repository: repository.clone(),
                                number: *number,
                            })
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(locator) = locator {
                    rows.push(Choice::Lookup {
                        account: account.clone(),
                        locator,
                    });
                }
            }
        }
        if let Some(scope) = &self.scope
            && let Some(results) = self.views.get(scope)
        {
            let mut ranked = vec![];
            for result in results.iter().filter(|result| {
                self.active(&result.account, cx)
                    && self
                        .account
                        .as_ref()
                        .is_none_or(|key| key == &result.account.identity.key)
            }) {
                if let Some(batch) = &result.batch {
                    for (index, item) in batch.items.iter().enumerate() {
                        if let Some(score) = item.score(&query) {
                            ranked.push((
                                score,
                                Choice::Item {
                                    account: result.account.clone(),
                                    batch: batch.clone(),
                                    index,
                                },
                            ));
                        }
                    }
                }
            }
            ranked.sort_by_key(|(score, _)| *score);
            rows.extend(ranked.into_iter().map(|(_, row)| row));
        }
        if self.scope.is_none()
            && let Some(items) = cx.global::<recent::Recents>().lists.get(&self.recent_path)
        {
            for locator in items {
                let mut seen = std::collections::HashSet::new();
                let mut known = false;
                for results in self.views.values() {
                    for result in results.iter().filter(|result| {
                        self.active(&result.account, cx)
                            && self
                                .account
                                .as_ref()
                                .is_none_or(|key| key == &result.account.identity.key)
                    }) {
                        if let Some(batch) = &result.batch
                            && let Some(index) =
                                batch.items.iter().position(|item| &item.locator == locator)
                        {
                            known = true;
                            if batch.items[index].score(&query).is_some()
                                && seen.insert(result.account.identity.key.clone())
                            {
                                rows.push(Choice::Item {
                                    account: result.account.clone(),
                                    batch: batch.clone(),
                                    index,
                                });
                            }
                        }
                    }
                }
                if !known {
                    let matches = match EntryInput::parse(&query) {
                        Ok(EntryInput::Number(number)) => number == locator.number,
                        _ => locator
                            .label()
                            .to_lowercase()
                            .contains(&query.to_lowercase()),
                    };
                    if matches {
                        rows.push(Choice::Recent(locator.clone()));
                    }
                }
            }
        }
        rows
    }
    fn choose(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        match choice {
            Choice::Item {
                account,
                batch,
                index,
            } => {
                let item = batch.items[index].clone();
                self.select(account, item, window, cx);
            }
            Choice::Lookup { account, locator } => {
                self.account = Some(account.identity.key);
                self.set_scope(Some(Scope::Direct(locator)), window, cx);
            }
            Choice::Recent(locator) => {
                self.input
                    .update(cx, |state, cx| state.set_query(locator.url(), window, cx));
                self.focus(window, cx);
                cx.notify();
            }
        }
    }
    fn command(&self, cx: &mut Context<Self>) -> Command {
        let choices = Arc::new(self.choices(cx));
        let confirm = choices.clone();
        let weak = cx.entity().downgrade();
        let cancel = cx.entity().downgrade();
        Command::new(&self.input)
            .filterable(false)
            .placeholder("Fuzzy title, repository, branch, #number, or paste a PR URL…")
            .items(choices.iter().map(|choice| {
                let key = match choice {
                    Choice::Item {
                        account,
                        batch,
                        index,
                    } => {
                        let item = &batch.items[*index];
                        serde_json::to_string(&(
                            "item",
                            &account.identity.key,
                            &item.repository_id,
                            &item.id,
                        ))
                    }
                    Choice::Lookup { account, locator } => {
                        serde_json::to_string(&("lookup", &account.identity.key, locator.url()))
                    }
                    Choice::Recent(locator) => serde_json::to_string(&("recent", locator.url())),
                }
                .expect("PR identity tuple serializes");
                CommandItem::new().key(key).label(choice.label())
            }))
            .on_query(cx.listener(|_, _, _, cx| cx.notify()))
            .on_confirm(move |index, window, cx| {
                if let Some(choice) = confirm.get(index.row) {
                    let _ = weak.update(cx, |page, cx| page.choose(choice.clone(), window, cx));
                }
            })
            .on_cancel(move |_, cx| {
                let _ = cancel.update(cx, |page, cx| page.cancel(cx));
            })
    }
    fn picker_context(&self, count: usize, cx: &App) -> [String; 2] {
        let results = self.scope.as_ref().and_then(|scope| self.views.get(scope));
        let cached = results.is_some_and(|results| results.iter().any(|result| result.cached));
        let partial = results.is_some_and(|results| {
            results
                .iter()
                .any(|result| result.batch.as_ref().is_some_and(|batch| batch.partial))
        });
        let scope = match &self.scope {
            Some(Scope::Authored) => "My open PRs".to_owned(),
            Some(Scope::ReviewRequested) => "Review requests".to_owned(),
            Some(Scope::Repository(provider, repo)) => format!("{} · {repo}", provider.name()),
            Some(Scope::Direct(locator)) => locator.label(),
            None => "Recent PRs".to_owned(),
        };
        let restored =
            results.is_some_and(|results| results.iter().any(|result| result.saved_at.is_some()));
        let account = self.account_label(cx);
        let summary = format!(
            "{scope} · {account} · {count} {} · Tab: filters · F5: refresh{}{}",
            if count == 1 { "match" } else { "matches" },
            if restored {
                " · saved snapshot"
            } else if cached {
                " · cached"
            } else {
                ""
            },
            if partial { " · partial results" } else { "" }
        );
        let error = results.and_then(|results| {
            results.iter().find_map(|result| {
                result.error.map(|error| {
                    format!(
                        "{} · {}: {error}",
                        result.account.identity.key.provider.name(),
                        result.account.identity.label
                    )
                })
            })
        });
        let query_error = EntryInput::parse(&self.input.read(cx).query(cx))
            .err()
            .map(|error| format!("Could not interpret this PR reference: {error}"));
        let detail = self
            .opening
            .then(|| "Opening published review… · Escape: cancel".to_owned())
            .or(query_error)
            .or(error)
            .or_else(|| cx.global::<recent::Recents>().error.clone())
            .or_else(|| self.progress.clone())
            .or_else(|| {
                self.notice
                    .clone()
                    .filter(|notice| !notice.starts_with("Ready."))
            })
            .unwrap_or_else(|| {
                if self.accounts(cx).is_empty() {
                    "No connected accounts · Use Accounts… to connect".into()
                } else if restored {
                    "Showing saved results until refreshed · PR details may have changed".into()
                } else if partial {
                    "Lists are incomplete · Paste a PR URL for direct lookup".into()
                } else if cached {
                    "Cached for up to 60s · Paste a PR URL for direct lookup".into()
                } else {
                    "Search titles, branches, repositories or numbers · Or paste a PR URL".into()
                }
            });
        [summary, detail]
    }
    pub(crate) fn configure_review(
        &mut self,
        options: hosting::review::Options,
        limit: jaywalk::text_content::TextLimit,
    ) {
        self.review_options = options;
        self.text_limit = limit;
    }
    fn select(
        &mut self,
        account: AccountView,
        item: PullRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active(&account, cx) {
            return;
        }
        if let Some(control) = self.association.take() {
            control.cancel();
        }
        let control = Control::new(Duration::from_secs(180));
        self.association = Some(control.clone());
        self.opening = true;
        self.notice = Some("Opening published review… · Escape: cancel".into());
        let backend = cx.global::<Accounts>().backend.clone();
        let options = self.review_options;
        let limit = self.text_limit;
        let expected = control.clone();
        let locator = item.locator.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    backend
                        .and_then(|backend| {
                            backend.open_review(
                                &account.identity.key,
                                &locator,
                                options,
                                limit,
                                &control,
                            )
                        })
                        .map(|review| (account, review))
                })
                .await;
            let _ = this.update(cx, |page, cx| {
                if expected.check().is_err() {
                    return;
                }
                page.association = None;
                page.opening = false;
                match result {
                    Ok((account, review)) if page.active(&account, cx) => {
                        recent::record(
                            page.recent_path.clone(),
                            review.metadata().locator.clone(),
                            cx,
                        );
                        page.notice = None;
                        cx.emit(Event::Opened(review));
                    }
                    Ok(_) => page.notice = Some("Account changed; reopen the review".into()),
                    Err(error) => {
                        page.notice = Some(format!(
                            "Could not open review: {error} · Select it again to retry"
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
impl Render for PullRequestsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.purge_retired(cx);
        if self.picking {
            let busy = (self.operation.is_some() || self.opening) && self.step == Step::Results;
            if self.input_loading != busy {
                self.input_loading = busy;
                self.input
                    .update(cx, |input, cx| input.set_loading(busy, window, cx));
            }
            let lines = if self.step == Step::Results {
                self.picker_context(self.choices(cx).len(), cx)
            } else {
                self.filter_context(cx)
            };
            let maximum = (f32::from(window.bounds().size.height) - 200.).clamp(80., 360.);
            let command = if self.step == Step::Results {
                self.command(cx)
            } else {
                self.filter_command(cx)
            };
            let command =
                command
                    .bordered(false)
                    .max_h(px(maximum))
                    .description(move |_, _, cx| {
                        let muted = gpui::component::Theme::global(cx).muted_foreground;
                        div()
                            .debug_selector(|| "pr-picker-context".into())
                            .v_flex()
                            .flex_shrink_0()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(muted)
                            .children(
                                lines
                                    .iter()
                                    .map(|line| div().truncate().child(line.clone())),
                            )
                    });
            return div()
                .w_full()
                .key_context("JaywalkPullRequests")
                .debug_selector(|| "pr-palette-picker".into())
                .capture_action(cx.listener(
                    |page, _: &gpui::base::input::IndentInline, window, cx| {
                        page.toggle_filters(window, cx)
                    },
                ))
                .capture_action(
                    cx.listener(|page, _: &gpui::base::input::Escape, window, cx| {
                        page.filter_escape(window, cx)
                    }),
                )
                .capture_action(
                    cx.listener(|page, _: &gpui::base::actions::Cancel, window, cx| {
                        page.filter_escape(window, cx)
                    }),
                )
                .capture_action(cx.listener(
                    |page, action: &crate::viewer::commands::ExecuteCommand, window, cx| {
                        if action.command == jaywalk::commands::CommandId::Refresh {
                            page.force_refresh(window, cx);
                            cx.stop_propagation();
                        } else {
                            cx.propagate();
                        }
                    },
                ))
                .on_action(
                    cx.listener(|page, _: &Refresh, window, cx| page.force_refresh(window, cx)),
                )
                .on_action(cx.listener(|page, _: &Cancel, _, cx| page.cancel(cx)))
                .child(command)
                .into_any_element();
        }
        let theme = gpui::component::Theme::global(cx);
        let muted = theme.muted_foreground;
        let busy = self.operation.is_some();
        let accounts = self.accounts(cx);
        let mut filters = div().h_flex().flex_wrap().gap_2();
        for (id, label, scope) in [
            ("pr-authored", "My open PRs", Some(Scope::Authored)),
            (
                "pr-reviews",
                "Review requests (GitHub)",
                Some(Scope::ReviewRequested),
            ),
            ("pr-recents", "Recent PRs", None),
        ] {
            let selected = self.scope == scope;
            filters = filters.child(
                Button::new(id)
                    .ghost()
                    .selected(selected)
                    .label(label)
                    .on_click(cx.listener(move |page, _, window, cx| {
                        page.set_scope(scope.clone(), window, cx)
                    })),
            );
        }
        for (index, remote) in self.remotes.iter().take(8).enumerate() {
            let scope = Scope::Repository(remote.provider, remote.repository.clone());
            filters = filters.child(
                Button::new(("pr-current", index))
                    .ghost()
                    .label(format!(
                        "Current: {} · {}",
                        remote.provider.name(),
                        remote.repository
                    ))
                    .selected(self.scope.as_ref() == Some(&scope))
                    .on_click(cx.listener(move |page, _, window, cx| {
                        page.set_scope(Some(scope.clone()), window, cx)
                    })),
            );
        }
        let mut account_filters = div().h_flex().flex_wrap().gap_2().child(
            Button::new("pr-all-accounts")
                .ghost()
                .label("All accounts")
                .selected(self.account.is_none())
                .on_click(cx.listener(|page, _, window, cx| {
                    page.account = None;
                    page.refresh(window, cx);
                })),
        );
        for (index, account) in accounts.iter().enumerate() {
            let key = account.identity.key.clone();
            account_filters = account_filters.child(
                Button::new(("pr-account", index))
                    .ghost()
                    .label(format!(
                        "{} · {}",
                        key.provider.name(),
                        account.identity.label
                    ))
                    .selected(self.account.as_ref() == Some(&key))
                    .on_click(cx.listener(move |page, _, window, cx| {
                        page.account = Some(key.clone());
                        page.refresh(window, cx);
                    })),
            );
        }
        let mut status = div().v_flex().gap_1().text_sm().text_color(muted);
        if let Some(progress) = &self.progress {
            status = status.child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_center()
                    .child(Spinner::new())
                    .child(progress.clone()),
            );
        } else if let Some(notice) = &self.notice {
            status = status.child(notice.clone());
        }
        if let Some(scope) = &self.scope
            && let Some(results) = self.views.get(scope)
        {
            for result in results {
                if let Some(error) = result.error {
                    status = status.child(format!(
                        "{} · {}: {error}",
                        result.account.identity.key.provider.name(),
                        result.account.identity.label
                    ));
                }
                if let Some(batch) = &result.batch {
                    let age = result.saved_at.map_or_else(
                        || result.fetched.map_or(0, |at| at.elapsed().as_secs()),
                        |saved| {
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |now| now.as_secs().saturating_sub(saved))
                        },
                    );
                    status = status.child(format!(
                        "{} · {}: {} PRs · {} · result age {}s{}{}",
                        result.account.identity.key.provider.name(),
                        result.account.identity.label,
                        batch.items.len(),
                        batch.coverage.join(", "),
                        age,
                        if result.saved_at.is_some() {
                            " · saved snapshot; awaiting refresh"
                        } else if result.cached {
                            " · cached (up to 60s old)"
                        } else {
                            ""
                        },
                        if batch.partial {
                            " · partial results; narrow the repository scope or use a URL"
                        } else {
                            ""
                        }
                    ));
                }
            }
        }
        if self.remotes.len() > 8 {
            status=status.child("Showing the first eight remote filters. Use a repository-qualified number or URL for another repository.");
        }
        let query = self.input.read(cx).query(cx).to_string();
        if let Err(error) = EntryInput::parse(&query) {
            status = status.child(format!("Could not interpret this PR reference: {error}"));
        }
        if let Some(error) = &cx.global::<recent::Recents>().error {
            status = status.child(error.clone());
        }
        let count = self.choices(cx).len();
        let picker = self.command(cx);
        let controls = div()
            .debug_selector(|| "pr-browser-controls".into())
            .h_flex()
            .gap_2()
            .child(
                Button::new("pr-refresh")
                    .label("Refresh")
                    .disabled(busy)
                    .on_click(cx.listener(|page, _, window, cx| page.force_refresh(window, cx))),
            )
            .when(busy, |row| {
                row.child(
                    Button::new("pr-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|page, _, _, cx| page.cancel(cx))),
                )
            })
            .child(
                Button::new("pr-accounts")
                    .ghost()
                    .label("Accounts…")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Accounts))),
            );
        let help = format!(
            "{count} matches · Numbers are repository-scoped. Cached lists may be incomplete. Direct lookup works for open or closed PRs."
        );
        let body = div()
            .v_flex()
            .flex_shrink_0()
            .w_full()
            .gap_3()
            .p_5()
            .child(filters)
            .child(account_filters)
            .child(controls)
            .child(status)
            .child(div().text_sm().text_color(muted).child(help))
            .child(
                div()
                    .id("pr-picker")
                    .debug_selector(|| "pr-picker".into())
                    .h(px(320.))
                    .flex_shrink_0()
                    .child(picker),
            );
        div()
            .id("pull-requests-scroll")
            .v_flex()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .key_context("JaywalkPullRequests")
            .debug_selector(|| "pull-requests-page".into())
            .size_full()
            .on_action(cx.listener(|page, _: &Refresh, window, cx| page.force_refresh(window, cx)))
            .on_action(cx.listener(|page, _: &Cancel, _, cx| page.cancel(cx)))
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
pub(crate) mod tests;
