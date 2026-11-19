use super::model::{RefreshState, now};
use super::{
    AccountKey, AccountView, BrowserAttempt, ClientRegistration, ConnectionState, Control,
    CredentialStore, Error, HostingProvider, Identity, Provider, Secret, StoredSession,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Condvar, Mutex, Weak},
    time::Duration,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub loaded: bool,
    pub accounts: Vec<AccountView>,
    pub signing_in: Vec<Provider>,
    pub store_error: Option<Error>,
}
struct Account {
    session: StoredSession,
    epoch: u64,
    verified: bool,
    dirty: bool,
    state: ConnectionState,
    flight: Option<Arc<Flight>>,
}
struct Pending {
    id: u64,
    control: Control,
}
#[derive(Default)]
struct State {
    accounts: BTreeMap<AccountKey, Account>,
    pending: BTreeMap<Provider, Pending>,
    loaded: bool,
    serial: u64,
    store_error: Option<Error>,
}
struct Flight {
    result: Mutex<Option<Result<Secret, Error>>>,
    ready: Condvar,
    control: Control,
}
impl Flight {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
            control: Control::new(Duration::from_secs(45)),
        }
    }
    fn finish(&self, result: Result<Secret, Error>) {
        *self.result.lock().expect("flight result mutex poisoned") = Some(result);
        self.ready.notify_all();
    }
    fn wait(&self, control: &Control) -> Result<Secret, Error> {
        let mut result = self.result.lock().expect("flight result mutex poisoned");
        loop {
            control.check()?;
            if let Some(result) = &*result {
                return result.clone();
            }
            (result, _) = self
                .ready
                .wait_timeout(result, Duration::from_millis(20))
                .expect("flight result mutex poisoned");
        }
    }
}

/// One application instance owns this service. Network and OS-store work runs
/// only on workers; snapshots carry no credentials and can be observed by every
/// window. The state lock linearizes store commits with account retirement.
/// Keychain permission UI itself cannot be interrupted by a Rust cancellation flag.
pub struct Service {
    store: Arc<dyn CredentialStore>,
    provider: Arc<dyn HostingProvider>,
    state: Mutex<State>,
    hosting_cache: Mutex<crate::hosting::Cache>,
    subscribers: Mutex<Vec<async_channel::Sender<Snapshot>>>,
}
impl Service {
    pub fn new(store: Arc<dyn CredentialStore>, provider: Arc<dyn HostingProvider>) -> Arc<Self> {
        Arc::new(Self {
            store,
            provider,
            state: Mutex::new(State::default()),
            hosting_cache: Mutex::default(),
            subscribers: Mutex::new(Vec::new()),
        })
    }
    pub fn subscribe(&self) -> async_channel::Receiver<Snapshot> {
        let state = self.state.lock().expect("account state mutex poisoned");
        let (sender, receiver) = async_channel::unbounded();
        sender
            .try_send(snapshot(&state))
            .expect("new snapshot receiver is open");
        self.subscribers
            .lock()
            .expect("account subscribers mutex poisoned")
            .push(sender);
        receiver
    }
    fn publish(&self, state: &State) {
        let value = snapshot(state);
        self.subscribers
            .lock()
            .expect("account subscribers mutex poisoned")
            .retain(|sender| sender.try_send(value.clone()).is_ok());
    }
    /// Lazy initialization avoids Keychain prompts just from launching a local diff.
    /// Call on a worker when account features are first requested.
    pub fn restore(&self) -> Result<(), Error> {
        let mut state = self.state.lock().expect("account state mutex poisoned");
        if state.loaded {
            return Ok(());
        }
        let sessions = match self.store.load() {
            Ok(sessions) => sessions,
            Err(error) => {
                state.store_error = Some(error);
                self.publish(&state);
                return Err(error);
            }
        };
        let mut accounts = BTreeMap::new();
        for session in sessions {
            session.validate()?;
            state.serial += 1;
            let account = Account {
                state: if session.refresh_state == RefreshState::Ready {
                    ConnectionState::Stored
                } else {
                    ConnectionState::Unavailable(Error::RefreshUncertain)
                },
                session,
                epoch: state.serial,
                verified: false,
                dirty: false,
                flight: None,
            };
            if accounts
                .insert(account.session.identity.key.clone(), account)
                .is_some()
            {
                return Err(Error::StoreInvalid);
            }
        }
        state.accounts = accounts;
        state.loaded = true;
        state.store_error = None;
        self.publish(&state);
        Ok(())
    }
    pub fn begin_sign_in(
        self: &Arc<Self>,
        registration: ClientRegistration,
        replace: Option<AccountKey>,
        timeout: Duration,
    ) -> Result<SignIn, Error> {
        self.begin_sign_in_controlled(registration, replace, Control::new(timeout))
    }
    /// The caller creates this handle before any worker can block in Keychain.
    pub fn begin_sign_in_controlled(
        self: &Arc<Self>,
        registration: ClientRegistration,
        replace: Option<AccountKey>,
        control: Control,
    ) -> Result<SignIn, Error> {
        let provider = registration.provider;
        let client_id = registration.client_id.clone();
        self.begin_with_control(registration, replace, control, || {
            BrowserAttempt::new(provider, &client_id)
        })
    }
    #[cfg(test)]
    fn begin_with_browser(
        self: &Arc<Self>,
        registration: ClientRegistration,
        replace: Option<AccountKey>,
        timeout: Duration,
        browser: impl FnOnce() -> Result<BrowserAttempt, Error>,
    ) -> Result<SignIn, Error> {
        self.begin_with_control(registration, replace, Control::new(timeout), browser)
    }
    fn begin_with_control(
        self: &Arc<Self>,
        registration: ClientRegistration,
        replace: Option<AccountKey>,
        control: Control,
        browser: impl FnOnce() -> Result<BrowserAttempt, Error>,
    ) -> Result<SignIn, Error> {
        control.check()?;
        registration.validate()?;
        self.restore()?;
        let provider = registration.provider;
        control.check()?;
        let mut state = self.state.lock().expect("account state mutex poisoned");
        if state.pending.contains_key(&provider) {
            return Err(Error::Busy);
        }
        if let Some(key) = &replace
            && (key.provider != provider || !state.accounts.contains_key(key))
        {
            return Err(Error::NotConnected);
        }
        // Bind before publishing or opening a browser; failure leaves no pending attempt.
        let browser = browser()?;
        state.serial += 1;
        let id = state.serial;
        state.pending.insert(
            provider,
            Pending {
                id,
                control: control.clone(),
            },
        );
        self.publish(&state);
        Ok(SignIn {
            service: Arc::downgrade(self),
            provider,
            id,
            control,
            registration,
            replace,
            browser: Some(browser),
        })
    }
    fn retire_sign_in(&self, provider: Provider, id: u64) {
        let mut state = self.state.lock().expect("account state mutex poisoned");
        if state
            .pending
            .get(&provider)
            .is_some_and(|pending| pending.id == id)
        {
            if let Some(pending) = state.pending.remove(&provider) {
                pending.control.cancel();
            }
            self.publish(&state);
        }
    }
    /// Cancels the authorization in every window. Dropping its SignIn handle
    /// also retires it, including when its originating window closes.
    pub fn cancel_sign_in(&self, provider: Provider) {
        let id = self
            .state
            .lock()
            .expect("account state mutex poisoned")
            .pending
            .get(&provider)
            .map(|pending| pending.id);
        if let Some(id) = id {
            self.retire_sign_in(provider, id);
        }
    }
    pub fn disconnect(&self, key: &AccountKey) -> Result<(), Error> {
        let mut state = self.state.lock().expect("account state mutex poisoned");
        let account = state.accounts.get_mut(key).ok_or(Error::NotConnected)?;
        // Retire in-memory work even if credential deletion fails. The UI then
        // reports failure and permits retry; it does not claim durable sign-out.
        if let Some(flight) = account.flight.take() {
            flight.control.cancel();
            flight.finish(Err(Error::Superseded));
        }
        account.epoch += 1;
        account.verified = false;
        account.state = ConnectionState::Unavailable(Error::StoreUnavailable);
        if let Some(pending) = state.pending.remove(&key.provider) {
            pending.control.cancel();
        }
        let result = self.store.remove(key);
        match result {
            Ok(()) => {
                state.accounts.remove(key);
            }
            Err(error) => {
                state
                    .accounts
                    .get_mut(key)
                    .expect("retained account exists")
                    .state = ConnectionState::Unavailable(error);
            }
        }
        self.publish(&state);
        result
    }
    /// A resource request may reject a token that was valid before revocation.
    /// Invalidate only the exact pair it used; a late 401 must not expire a newer
    /// session. The API caller may obtain a fresh token and retry once.
    pub fn reject_access_token(&self, key: &AccountKey, used: &Secret) -> Result<(), Error> {
        let mut state = self.state.lock().expect("account state mutex poisoned");
        let account = state.accounts.get_mut(key).ok_or(Error::NotConnected)?;
        if account.session.tokens.access.expose() == used.expose() {
            account.verified = false;
            account.session.tokens.expires_at = Some(0);
            account.state = if account.session.tokens.refresh.is_some() {
                ConnectionState::Stored
            } else {
                ConnectionState::NeedsSignIn
            };
            self.publish(&state);
        }
        Ok(())
    }
    /// Waiters share one account operation, but cancelling one waiter never
    /// cancels rotation for another window. A service-owned worker persists any
    /// replacement pair even after all waiters close their windows.
    pub fn access_token(
        self: &Arc<Self>,
        key: &AccountKey,
        control: &Control,
    ) -> Result<Secret, Error> {
        control.check()?;
        let mut state = self.state.lock().expect("account state mutex poisoned");
        let account = state.accounts.get_mut(key).ok_or(Error::NotConnected)?;
        if account.state == ConnectionState::NeedsSignIn {
            return Err(Error::NeedsSignIn);
        }
        if account.session.refresh_state != RefreshState::Ready && account.flight.is_none() {
            return Err(Error::RefreshUncertain);
        }
        if account.verified && !account.dirty && account.session.tokens.usable(now()) {
            return Ok(account.session.tokens.access.clone());
        }
        let flight = if let Some(flight) = &account.flight {
            flight.clone()
        } else {
            let flight = Arc::new(Flight::new());
            account.flight = Some(flight.clone());
            account.state = ConnectionState::Checking;
            let epoch = account.epoch;
            let service = self.clone();
            let worker_key = key.clone();
            let work = flight.clone();
            self.publish(&state);
            if std::thread::Builder::new()
                .name("hosting-account".into())
                .spawn(move || {
                    let result = service.prepare_token(&worker_key, epoch, &work.control);
                    service.finish_token(&worker_key, epoch, &work, result);
                })
                .is_err()
            {
                // No network/store operation began, so this is safe to retry.
                let account = state.accounts.get_mut(key).expect("flight account exists");
                account.flight = None;
                flight.finish(Err(Error::Network));
            }
            flight
        };
        drop(state);
        flight.wait(control)
    }
    /// Worker-only, read-only hosted requests bound to the original account generation.
    pub(crate) fn review_request(
        self: &Arc<Self>,
        key: &AccountKey,
        epoch: u64,
        request: crate::hosting::review::Request,
        control: &Control,
    ) -> Result<crate::hosting::review::Reply, Error> {
        let url = match &request {
            crate::hosting::review::Request::Json(url)
            | crate::hosting::review::Request::Bytes(url, _) => url,
        };
        let host = match key.provider {
            Provider::Github => "api.github.com",
            Provider::Bitbucket => "api.bitbucket.org",
        };
        if url.host_str() != Some(host) {
            return Err(Error::InvalidResponse);
        }
        {
            let mut state = self.state.lock().expect("account mutex poisoned");
            current(&mut state, key, epoch)?;
        }
        {
            let cache = loop {
                control.check()?;
                match self.hosting_cache.try_lock() {
                    Ok(cache) => break cache,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        panic!("hosting cache mutex poisoned")
                    }
                }
            };
            cache.check_rate(key)?;
        }
        let mut token = self.access_token(key, control)?;
        let mut result = self
            .provider
            .review_request(&token, request.clone(), control);
        if matches!(result, Err(Error::NeedsSignIn)) {
            self.reject_access_token(key, &token)?;
            token = self.access_token(key, control)?;
            result = self.provider.review_request(&token, request, control);
            if matches!(result, Err(Error::NeedsSignIn)) {
                self.reject_access_token(key, &token)?;
            }
        }
        if matches!(result, Err(Error::RateLimited)) {
            self.hosting_cache
                .lock()
                .expect("hosting cache mutex poisoned")
                .limited(key.clone());
        }
        control.check()?;
        let mut state = self.state.lock().expect("account mutex poisoned");
        current(&mut state, key, epoch)?;
        result
    }
    pub fn open_review(
        self: &Arc<Self>,
        key: &AccountKey,
        locator: &crate::hosting::Locator,
        options: crate::hosting::review::Options,
        limit: crate::text_content::TextLimit,
        control: &Control,
    ) -> Result<crate::hosting::review::Review, Error> {
        options.validate().map_err(|_| Error::InvalidResponse)?;
        self.restore()?;
        if key.provider != locator.provider {
            return Err(Error::Unsupported);
        }
        let epoch = self
            .state
            .lock()
            .expect("account mutex poisoned")
            .accounts
            .get(key)
            .ok_or(Error::NotConnected)?
            .epoch;
        let access = crate::hosting::review::Access {
            service: Arc::downgrade(self),
            account: key.clone(),
            generation: epoch,
        };
        let data = crate::hosting::review::resolve(locator, options, control, |request| {
            access.request(request, control)
        })?;
        control.check()?;
        let contents = crate::hosting::review::Contents::new(data, access, options, limit);
        let comparison = contents.comparison().map_err(|_| Error::InvalidResponse)?;
        Ok(crate::hosting::review::Review {
            comparison,
            checkout: None,
            local_notice: None,
        })
    }
    pub fn accounts_snapshot(&self) -> Snapshot {
        snapshot(&self.state.lock().expect("account state mutex poisoned"))
    }
    /// Worker-only: coalesce reads behind a bounded, token-free cache. UI code
    /// reads its own snapshots and never takes this lock.
    pub fn pull_requests(
        self: &Arc<Self>,
        key: &AccountKey,
        scope: &crate::hosting::Scope,
        control: &Control,
    ) -> Result<crate::hosting::Fetched, Error> {
        self.pull_requests_with_policy(key, scope, false, control)
    }
    /// Explicit refresh bypasses existing cached data while retaining it on
    /// failure. A fresh result from an already-running read can satisfy a waiter.
    pub fn refresh_pull_requests(
        self: &Arc<Self>,
        key: &AccountKey,
        scope: &crate::hosting::Scope,
        control: &Control,
    ) -> Result<crate::hosting::Fetched, Error> {
        self.pull_requests_with_policy(key, scope, true, control)
    }
    fn pull_requests_with_policy(
        self: &Arc<Self>,
        key: &AccountKey,
        scope: &crate::hosting::Scope,
        refresh: bool,
        control: &Control,
    ) -> Result<crate::hosting::Fetched, Error> {
        let started = std::time::Instant::now();
        if !scope.supports(key.provider) {
            return Err(Error::Unsupported);
        }
        let mut cache = loop {
            control.check()?;
            match self.hosting_cache.try_lock() {
                Ok(cache) => break cache,
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(std::sync::TryLockError::Poisoned(_)) => panic!("hosting cache mutex poisoned"),
            }
        };
        let epoch = self
            .state
            .lock()
            .expect("account state mutex poisoned")
            .accounts
            .get(key)
            .ok_or(Error::NotConnected)?
            .epoch;
        let cached = if refresh {
            cache.updated_since(key, epoch, scope, started)
        } else {
            cache.get(key, epoch, scope)
        };
        if let Some((batch, fetched_at)) = cached {
            let mut state = self.state.lock().expect("account state mutex poisoned");
            let account = current(&mut state, key, epoch)?;
            if !matches!(account.state, ConnectionState::NeedsSignIn) {
                return Ok(crate::hosting::Fetched {
                    batch,
                    generation: epoch,
                    cached: true,
                    fetched_at,
                });
            }
        }
        cache.check_rate(key)?;
        let mut token = self.access_token(key, control)?;
        let mut result = self.read_pull_requests(key, epoch, &token, scope, control);
        if matches!(result, Err(Error::NeedsSignIn)) {
            self.reject_access_token(key, &token)?;
            token = self.access_token(key, control)?;
            result = self.read_pull_requests(key, epoch, &token, scope, control);
            if matches!(result, Err(Error::NeedsSignIn)) {
                self.reject_access_token(key, &token)?;
            }
        }
        if matches!(result, Err(Error::RateLimited)) {
            cache.limited(key.clone());
        }
        let batch = Arc::new(result?);
        control.check()?;
        let mut state = self.state.lock().expect("account state mutex poisoned");
        current(&mut state, key, epoch)?;
        let fetched_at = std::time::SystemTime::now();
        cache.put(key.clone(), epoch, scope.clone(), batch.clone(), fetched_at);
        Ok(crate::hosting::Fetched {
            batch,
            generation: epoch,
            cached: false,
            fetched_at,
        })
    }
    fn read_pull_requests(
        &self,
        key: &AccountKey,
        epoch: u64,
        token: &Secret,
        scope: &crate::hosting::Scope,
        control: &Control,
    ) -> Result<crate::hosting::Batch, Error> {
        let (identity, selected) = {
            let mut state = self.state.lock().expect("account state mutex poisoned");
            let account = current(&mut state, key, epoch)?;
            (
                account.session.identity.clone(),
                account.session.workspace.clone(),
            )
        };
        let mut workspaces = if key.provider == Provider::Bitbucket
            && matches!(scope, crate::hosting::Scope::Authored)
        {
            self.provider.workspaces(token, control)?
        } else {
            vec![]
        };
        if let Some(selected) = selected {
            workspaces.sort_by_key(|workspace| workspace.uuid != selected.uuid);
        }
        self.provider
            .pull_requests(token, &identity, scope, &workspaces, control)
    }
    /// Worker-only snapshot access, never called from a rendering callback.
    pub fn account_keys(&self) -> Vec<AccountKey> {
        self.state
            .lock()
            .expect("account state mutex poisoned")
            .accounts
            .keys()
            .cloned()
            .collect()
    }
    pub fn selected_registration(&self, provider: Provider) -> Result<ClientRegistration, Error> {
        self.store.selected_registration(provider)
    }
    pub fn import_development_registration(&self, provider: Provider) -> Result<(), Error> {
        self.store.import_development_registration(provider)
    }
    pub fn workspaces(
        self: &Arc<Self>,
        key: &AccountKey,
        control: &Control,
    ) -> Result<super::WorkspaceListing, Error> {
        if key.provider != Provider::Bitbucket {
            return Err(Error::Unsupported);
        }
        let epoch = self
            .state
            .lock()
            .expect("account state mutex poisoned")
            .accounts
            .get(key)
            .ok_or(Error::NotConnected)?
            .epoch;
        let token = self.access_token(key, control)?;
        let workspaces = self.provider.workspaces(&token, control)?;
        control.check()?;
        let mut state = self.state.lock().expect("account state mutex poisoned");
        current(&mut state, key, epoch)?;
        Ok(super::WorkspaceListing {
            account: key.clone(),
            workspaces,
            epoch,
        })
    }
    pub fn select_workspace(
        &self,
        listing: &super::WorkspaceListing,
        uuid: &str,
        control: &Control,
    ) -> Result<(), Error> {
        control.check()?;
        let workspace = listing
            .workspaces
            .iter()
            .find(|workspace| workspace.uuid == uuid)
            .ok_or(Error::InvalidResponse)?;
        workspace.validate()?;
        let mut state = self.state.lock().expect("account state mutex poisoned");
        let account = current(&mut state, &listing.account, listing.epoch)?;
        let mut session = account.session.clone();
        session.workspace = Some(workspace.clone());
        control.commit()?;
        self.store.save(&session)?;
        account.session = session;
        self.publish(&state);
        Ok(())
    }
    fn prepare_token(
        &self,
        key: &AccountKey,
        epoch: u64,
        control: &Control,
    ) -> Result<Secret, Error> {
        let mut state = self.state.lock().expect("account state mutex poisoned");
        let account = current(&mut state, key, epoch)?;
        control.check()?;
        if account.dirty {
            self.store
                .save(&account.session)
                .map_err(|_| Error::Persistence)?;
            account.dirty = false;
        }
        if !account.session.tokens.usable(now()) {
            let refresh = account
                .session
                .tokens
                .refresh
                .clone()
                .ok_or(Error::NeedsSignIn)?;
            let registration = self
                .store
                .registration(key.provider, &account.session.client_id)?;
            account.session.refresh_state = RefreshState::Pending;
            // A persisted intent prevents replay after a crash or lost response.
            if let Err(error) = self.store.save(&account.session) {
                account.session.refresh_state = RefreshState::Ready;
                return Err(error);
            }
            account.state = ConnectionState::Refreshing;
            self.publish(&state);
            drop(state);
            let rotated = self.provider.refresh(&registration, &refresh, control);
            state = self.state.lock().expect("account state mutex poisoned");
            let account = current(&mut state, key, epoch)?;
            match rotated {
                Ok(tokens) => {
                    tokens.validate()?;
                    // Both supported browser providers rotate refresh tokens.
                    // A missing replacement cannot justify replaying the old one.
                    if tokens.refresh.is_none() {
                        return Err(Error::RefreshUncertain);
                    }
                    account.session.tokens = tokens;
                    account.session.refresh_state = RefreshState::Ready;
                    account.verified = false;
                    account.dirty = true;
                    // Do not check waiter cancellation between rotation and save.
                    self.store
                        .save(&account.session)
                        .map_err(|_| Error::Persistence)?;
                    account.dirty = false;
                }
                Err(error) => {
                    account.session.refresh_state = RefreshState::Blocked;
                    // Pending on disk is already fail-closed if this write fails.
                    let _ = self.store.save(&account.session);
                    return Err(match error {
                        Error::NeedsSignIn | Error::Registration => error,
                        _ => Error::RefreshUncertain,
                    });
                }
            }
        }
        let account = current(&mut state, key, epoch)?;
        let token = account.session.tokens.access.clone();
        drop(state);
        let identity = self.provider.identity(key.provider, &token, control)?;
        identity.validate()?;
        if &identity.key != key {
            return Err(Error::IdentityMismatch);
        }
        let mut state = self.state.lock().expect("account state mutex poisoned");
        let account = current(&mut state, key, epoch)?;
        control.check()?;
        account.session.identity = identity;
        account.verified = true;
        Ok(token)
    }
    fn finish_token(
        &self,
        key: &AccountKey,
        epoch: u64,
        flight: &Flight,
        result: Result<Secret, Error>,
    ) {
        let mut state = self.state.lock().expect("account state mutex poisoned");
        let result = match current(&mut state, key, epoch) {
            Ok(account) => {
                let result = result.and_then(|token| {
                    if account.session.tokens.access.expose() != token.expose()
                        || account
                            .session
                            .tokens
                            .expires_at
                            .is_some_and(|expiry| expiry <= now())
                    {
                        account.verified = false;
                        Err(Error::Superseded)
                    } else {
                        Ok(token)
                    }
                });
                account.flight = None;
                account.state = match &result {
                    Ok(_) => ConnectionState::Connected,
                    Err(Error::NeedsSignIn | Error::IdentityMismatch) => {
                        ConnectionState::NeedsSignIn
                    }
                    Err(error) => ConnectionState::Unavailable(*error),
                };
                result
            }
            Err(_) => Err(Error::Superseded),
        };
        flight.finish(result);
        self.publish(&state);
    }
}
fn current<'a>(
    state: &'a mut State,
    key: &AccountKey,
    epoch: u64,
) -> Result<&'a mut Account, Error> {
    state
        .accounts
        .get_mut(key)
        .filter(|account| account.epoch == epoch)
        .ok_or(Error::Superseded)
}
fn snapshot(state: &State) -> Snapshot {
    Snapshot {
        loaded: state.loaded,
        accounts: state
            .accounts
            .values()
            .map(|account| AccountView {
                generation: account.epoch,
                identity: account.session.identity.clone(),
                workspace: account.session.workspace.clone(),
                state: account.state.clone(),
            })
            .collect(),
        signing_in: state.pending.keys().copied().collect(),
        store_error: state.store_error,
    }
}

/// Owns the callback socket and a scoped authorization generation. Dropping it
/// cannot leave a valid attempt behind that a late callback could resurrect.
pub struct SignIn {
    service: Weak<Service>,
    provider: Provider,
    id: u64,
    control: Control,
    registration: ClientRegistration,
    replace: Option<AccountKey>,
    browser: Option<BrowserAttempt>,
}
impl SignIn {
    pub fn control(&self) -> Control {
        self.control.clone()
    }
    pub fn authorization_url(&self) -> &str {
        self.browser
            .as_ref()
            .expect("unconsumed sign-in has a browser attempt")
            .authorization_url()
    }
    pub fn cancel(&self) {
        self.control.cancel();
        if let Some(service) = self.service.upgrade() {
            service.retire_sign_in(self.provider, self.id);
        }
    }
    pub fn complete(mut self) -> Result<Identity, Error> {
        let service = self.service.upgrade().ok_or(Error::Cancelled)?;
        let browser = self.browser.take().expect("sign-in completes only once");
        let (code, verifier) = browser.receive(&self.control)?;
        self.control.check()?;
        let tokens =
            service
                .provider
                .exchange(&self.registration, &code, &verifier, &self.control)?;
        self.control.check()?;
        let identity = service
            .provider
            .identity(self.provider, &tokens.access, &self.control)?;
        self.control.check()?;
        identity.validate()?;
        tokens.validate()?;
        if self
            .replace
            .as_ref()
            .is_some_and(|key| *key != identity.key)
        {
            return Err(Error::IdentityMismatch);
        }
        let session = StoredSession {
            identity: identity.clone(),
            client_id: self.registration.client_id.clone(),
            workspace: None,
            tokens,
            refresh_state: RefreshState::Ready,
        };
        let mut state = service.state.lock().expect("account state mutex poisoned");
        if !state
            .pending
            .get(&self.provider)
            .is_some_and(|pending| pending.id == self.id)
        {
            return Err(Error::Superseded);
        }
        self.control.commit()?;
        service.store.save(&session)?;
        if let Some(old) = state
            .accounts
            .get(&identity.key)
            .and_then(|account| account.flight.as_ref())
        {
            old.control.cancel();
            old.finish(Err(Error::Superseded));
        }
        state.serial += 1;
        let epoch = state.serial;
        state.accounts.insert(
            identity.key.clone(),
            Account {
                session,
                epoch,
                verified: true,
                dirty: false,
                state: ConnectionState::Connected,
                flight: None,
            },
        );
        state.pending.remove(&self.provider);
        service.publish(&state);
        Ok(identity)
    }
}
impl Drop for SignIn {
    fn drop(&mut self) {
        self.control.cancel();
        if let Some(service) = self.service.upgrade() {
            service.retire_sign_in(self.provider, self.id);
        }
    }
}

#[cfg(test)]
mod tests;
