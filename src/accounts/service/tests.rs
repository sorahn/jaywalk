use super::*;
use crate::accounts::{Tokens, model::RefreshState};
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    thread,
};

#[derive(Default)]
struct Store {
    sessions: Mutex<BTreeMap<AccountKey, StoredSession>>,
    saves: AtomicUsize,
    registrations: AtomicUsize,
    fail_save: AtomicUsize,
    unavailable: AtomicBool,
    fail_delete: AtomicBool,
    load_gate: Mutex<Option<Arc<Gate>>>,
    save_gate: Mutex<Option<Arc<Gate>>>,
}
impl CredentialStore for Store {
    fn load(&self) -> Result<Vec<StoredSession>, Error> {
        if let Some(gate) = self.load_gate.lock().unwrap().clone() {
            gate.enter();
        }
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(Error::StoreUnavailable);
        }
        Ok(self.sessions.lock().unwrap().values().cloned().collect())
    }
    fn save(&self, session: &StoredSession) -> Result<(), Error> {
        if let Some(gate) = self.save_gate.lock().unwrap().clone() {
            gate.enter();
        }
        let count = self.saves.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_save.load(Ordering::SeqCst) == count {
            return Err(Error::StoreUnavailable);
        }
        self.sessions
            .lock()
            .unwrap()
            .insert(session.identity.key.clone(), session.clone());
        Ok(())
    }
    fn remove(&self, key: &AccountKey) -> Result<(), Error> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(Error::StoreUnavailable);
        }
        self.sessions.lock().unwrap().remove(key);
        Ok(())
    }
    fn registration(&self, provider: Provider, _: &str) -> Result<ClientRegistration, Error> {
        self.registrations.fetch_add(1, Ordering::SeqCst);
        Ok(registration(provider))
    }
}
#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}
impl Gate {
    fn enter(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            let (next, timeout) = self
                .changed
                .wait_timeout(state, Duration::from_secs(5))
                .unwrap();
            state = next;
            assert!(
                !timeout.timed_out(),
                "test must release the provider operation"
            );
        }
    }
    fn entered(&self) {
        let state = self.state.lock().unwrap();
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.0)
            .unwrap();
        assert!(
            state.0 && !timeout.timed_out(),
            "provider operation must begin"
        );
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}
struct ProviderStub {
    store: Arc<Store>,
    calls: AtomicUsize,
    identities: AtomicUsize,
    hosting_calls: AtomicUsize,
    hosting_gate: Mutex<Option<Arc<Gate>>>,
    hosting_result: Mutex<Result<crate::hosting::Batch, Error>>,
    refresh_gate: Option<Arc<Gate>>,
    identity_gate: Option<Arc<Gate>>,
    refresh_result: Mutex<Result<Tokens, Error>>,
    identity_result: Mutex<Result<Identity, Error>>,
}
impl HostingProvider for ProviderStub {
    fn pull_requests(
        &self,
        _: &Secret,
        _: &Identity,
        _: &crate::hosting::Scope,
        _: &[crate::accounts::Workspace],
        _: &Control,
    ) -> Result<crate::hosting::Batch, Error> {
        self.hosting_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = self.hosting_gate.lock().unwrap().clone() {
            gate.enter();
        }
        self.hosting_result.lock().unwrap().clone()
    }

    fn exchange(
        &self,
        _: &ClientRegistration,
        _: &str,
        _: &str,
        _: &Control,
    ) -> Result<Tokens, Error> {
        let mut tokens = self.refresh_result.lock().unwrap().clone()?;
        tokens.access = secret("signed-in-access");
        Ok(tokens)
    }
    fn refresh(&self, _: &ClientRegistration, _: &Secret, _: &Control) -> Result<Tokens, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // Prove crash protection precedes the grant, rather than just inspecting state later.
        assert!(
            self.store
                .sessions
                .lock()
                .unwrap()
                .values()
                .any(|session| session.refresh_state == RefreshState::Pending)
        );
        if let Some(gate) = &self.refresh_gate {
            gate.enter();
        }
        self.refresh_result.lock().unwrap().clone()
    }
    fn identity(&self, _: Provider, token: &Secret, _: &Control) -> Result<Identity, Error> {
        self.identities.fetch_add(1, Ordering::SeqCst);
        if self.calls.load(Ordering::SeqCst) > 0 && token.expose() == "rotated-access" {
            // The rotated pair must be durable BEFORE validation can perform network I/O.
            assert!(
                self.store
                    .sessions
                    .lock()
                    .unwrap()
                    .values()
                    .any(|session| session.tokens.access.expose() == token.expose()
                        && session.refresh_state == RefreshState::Ready)
            );
        }
        if let Some(gate) = &self.identity_gate {
            gate.enter();
        }
        self.identity_result.lock().unwrap().clone()
    }
}
fn secret(value: &str) -> Secret {
    Secret::new(value.into()).unwrap()
}
fn key() -> AccountKey {
    AccountKey {
        provider: Provider::Github,
        subject: "1234".into(),
    }
}
fn identity() -> Identity {
    Identity {
        key: key(),
        label: "synthetic-user".into(),
    }
}
fn registration(provider: Provider) -> ClientRegistration {
    ClientRegistration {
        provider,
        client_id: "synthetic-client".into(),
        secret: secret("synthetic-secret"),
    }
}
fn tokens(expired: bool) -> Tokens {
    Tokens {
        access: secret("old-access"),
        refresh: Some(secret("old-refresh")),
        expires_at: Some(if expired { 1 } else { now() + 3600 }),
    }
}
fn rotated() -> Tokens {
    Tokens {
        access: secret("rotated-access"),
        refresh: Some(secret("rotated-refresh")),
        expires_at: Some(now() + 3600),
    }
}
fn session(expired: bool) -> StoredSession {
    StoredSession {
        identity: identity(),
        authentication: Authentication::Oauth,
        client_id: Some("synthetic-client".into()),
        workspace: None,
        tokens: tokens(expired),
        refresh_state: RefreshState::Ready,
    }
}
fn fixture(
    expired: bool,
    refresh_gate: Option<Arc<Gate>>,
    identity_gate: Option<Arc<Gate>>,
) -> (Arc<Service>, Arc<Store>, Arc<ProviderStub>) {
    let store = Arc::new(Store::default());
    store
        .sessions
        .lock()
        .unwrap()
        .insert(key(), session(expired));
    let provider = Arc::new(ProviderStub {
        store: store.clone(),
        calls: AtomicUsize::new(0),
        identities: AtomicUsize::new(0),
        hosting_calls: AtomicUsize::new(0),
        hosting_gate: Mutex::new(None),
        hosting_result: Mutex::new(Ok(crate::hosting::Batch::default())),
        refresh_gate,
        identity_gate,
        refresh_result: Mutex::new(Ok(rotated())),
        identity_result: Mutex::new(Ok(identity())),
    });
    let service = Service::new(store.clone(), provider.clone());
    service.restore().unwrap();
    (service, store, provider)
}
fn control() -> Control {
    Control::new(Duration::from_secs(5))
}
fn spawn_token(
    service: &Arc<Service>,
    control: Control,
) -> thread::JoinHandle<Result<Secret, Error>> {
    let service = service.clone();
    thread::spawn(move || service.access_token(&key(), &control))
}
fn view(service: &Service) -> Snapshot {
    snapshot(&service.state.lock().unwrap())
}

#[test]
fn simultaneous_windows_share_rotation_and_one_cancelled_waiter_does_not_lose_it() {
    let gate = Arc::new(Gate::default());
    let (service, store, provider) = fixture(true, Some(gate.clone()), None);
    let first_observer = service.subscribe();
    let second_observer = service.subscribe();
    let cancelled = control();
    let first = spawn_token(&service, cancelled.clone());
    gate.entered();
    let second = spawn_token(&service, control());
    cancelled.cancel();
    assert!(matches!(first.join().unwrap(), Err(Error::Cancelled)));
    gate.release();
    assert_eq!(second.join().unwrap().unwrap().expose(), "rotated-access");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.sessions.lock().unwrap()[&key()]
            .tokens
            .refresh
            .as_ref()
            .unwrap()
            .expose(),
        "rotated-refresh"
    );
    assert_eq!(
        first_observer.try_recv().unwrap(),
        second_observer.try_recv().unwrap()
    );
    let a: Vec<_> = std::iter::from_fn(|| first_observer.try_recv().ok()).collect();
    let b: Vec<_> = std::iter::from_fn(|| second_observer.try_recv().ok()).collect();
    assert_eq!(a, b);
    assert_eq!(
        a.last().unwrap().accounts[0].state,
        ConnectionState::Connected
    );
}
#[test]
fn disconnect_during_rotation_cannot_restore_credentials_or_an_account() {
    let gate = Arc::new(Gate::default());
    let (service, store, _) = fixture(true, Some(gate.clone()), None);
    let waiter = spawn_token(&service, control());
    gate.entered();
    service.disconnect(&key()).unwrap();
    assert!(matches!(waiter.join().unwrap(), Err(Error::Superseded)));
    let events = service.subscribe();
    events.try_recv().unwrap();
    gate.release();
    // The stale worker publishes its final snapshot after rejecting the late response.
    let completed = events.recv_blocking().unwrap();
    assert!(completed.accounts.is_empty());
    assert!(store.load().unwrap().is_empty());
}
#[test]
fn locked_store_does_not_start_a_refresh_or_report_a_successful_disconnect() {
    let (service, store, provider) = fixture(true, None, None);
    store.fail_save.store(1, Ordering::SeqCst);
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::StoreUnavailable)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    store.fail_delete.store(true, Ordering::SeqCst);
    assert_eq!(service.disconnect(&key()), Err(Error::StoreUnavailable));
    assert_eq!(view(&service).accounts.len(), 1);
    assert_eq!(store.load().unwrap().len(), 1);
    store.fail_delete.store(false, Ordering::SeqCst);
    service.disconnect(&key()).unwrap();
    assert!(view(&service).accounts.is_empty());
}
#[test]
fn rotation_save_failure_keeps_new_pair_and_retries_persistence_without_another_grant() {
    let (service, store, provider) = fixture(true, None, None);
    store.fail_save.store(2, Ordering::SeqCst);
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::Persistence)
    ));
    assert_eq!(
        store.sessions.lock().unwrap()[&key()].refresh_state,
        RefreshState::Pending
    );
    assert_eq!(provider.identities.load(Ordering::SeqCst), 0);
    let token = service.access_token(&key(), &control()).unwrap();
    assert_eq!(token.expose(), "rotated-access");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.sessions.lock().unwrap()[&key()]
            .tokens
            .refresh
            .as_ref()
            .unwrap()
            .expose(),
        "rotated-refresh"
    );
}
#[test]
fn lost_response_and_restart_never_replay_the_consumed_refresh_token() {
    let (service, store, provider) = fixture(true, None, None);
    *provider.refresh_result.lock().unwrap() = Err(Error::Network);
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::RefreshUncertain)
    ));
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::RefreshUncertain)
    ));
    let restarted = Service::new(store, provider.clone());
    restarted.restore().unwrap();
    assert!(matches!(
        restarted.access_token(&key(), &control()),
        Err(Error::RefreshUncertain)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
#[test]
fn crash_marker_is_enough_to_disable_refresh_after_restart() {
    let (service, store, provider) = fixture(true, None, None);
    drop(service);
    store
        .sessions
        .lock()
        .unwrap()
        .get_mut(&key())
        .unwrap()
        .refresh_state = RefreshState::Pending;
    let restarted = Service::new(store, provider.clone());
    restarted.restore().unwrap();
    assert!(matches!(
        restarted.access_token(&key(), &control()),
        Err(Error::RefreshUncertain)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn revoked_refresh_is_actionable_and_network_failure_retains_the_known_identity() {
    let (service, _, provider) = fixture(true, None, None);
    *provider.refresh_result.lock().unwrap() = Err(Error::NeedsSignIn);
    for _ in 0..2 {
        assert!(matches!(
            service.access_token(&key(), &control()),
            Err(Error::NeedsSignIn)
        ));
    }
    assert_eq!(
        view(&service).accounts[0].state,
        ConnectionState::NeedsSignIn
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let (service, _, provider) = fixture(false, None, None);
    *provider.identity_result.lock().unwrap() = Err(Error::Network);
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::Network)
    ));
    assert_eq!(view(&service).accounts[0].identity, identity());
    assert_eq!(
        view(&service).accounts[0].state,
        ConnectionState::Unavailable(Error::Network)
    );
    *provider.identity_result.lock().unwrap() = Ok(identity());
    service.access_token(&key(), &control()).unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn restart_checks_identity_and_mismatch_never_exposes_a_token() {
    let (service, _, provider) = fixture(false, None, None);
    let mut other = identity();
    other.key.subject = "another-account".into();
    *provider.identity_result.lock().unwrap() = Ok(other);
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::IdentityMismatch)
    ));
    assert_eq!(
        view(&service).accounts[0].state,
        ConnectionState::NeedsSignIn
    );
    assert_eq!(provider.identities.load(Ordering::SeqCst), 1);
}
#[test]
fn unavailable_store_is_reported_and_can_be_recovered_without_a_plaintext_fallback() {
    let (old, store, provider) = fixture(false, None, None);
    drop(old);
    store.unavailable.store(true, Ordering::SeqCst);
    let service = Service::new(store.clone(), provider);
    assert_eq!(service.restore(), Err(Error::StoreUnavailable));
    assert_eq!(view(&service).store_error, Some(Error::StoreUnavailable));
    store.unavailable.store(false, Ordering::SeqCst);
    service.restore().unwrap();
    assert_eq!(view(&service).accounts.len(), 1);
    assert_eq!(view(&service).store_error, None);
}
fn attempt(service: &Arc<Service>) -> (SignIn, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let attempt = service
        .begin_with_browser(
            registration(Provider::Github),
            Some(key()),
            Duration::from_secs(5),
            || BrowserAttempt::with_listener(Provider::Github, "synthetic", listener),
        )
        .unwrap();
    (attempt, address)
}
fn callback(attempt: &SignIn, address: std::net::SocketAddr, extra: &str) {
    let url = reqwest::Url::parse(attempt.authorization_url()).unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .write_all(
            format!("GET /callback?state={state}&{extra} HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
}
#[test]
fn denied_and_dropped_reconnections_preserve_the_previous_session() {
    let (service, store, _) = fixture(false, None, None);
    let (denied, address) = attempt(&service);
    callback(&denied, address, "error=access_denied");
    assert_eq!(denied.complete(), Err(Error::Denied));
    let (dropped, address) = attempt(&service);
    drop(dropped);
    assert!(TcpStream::connect(address).is_err());
    assert!(view(&service).signing_in.is_empty());
    assert_eq!(
        store.sessions.lock().unwrap()[&key()]
            .tokens
            .access
            .expose(),
        "old-access"
    );
    assert_eq!(store.saves.load(Ordering::SeqCst), 0);
}
#[test]
fn closing_the_initiating_window_during_identity_verification_retires_the_attempt() {
    let gate = Arc::new(Gate::default());
    let (service, store, _) = fixture(false, None, Some(gate.clone()));
    let (attempt, address) = attempt(&service);
    callback(&attempt, address, "code=synthetic");
    let completion = thread::spawn(move || attempt.complete());
    gate.entered();
    service.cancel_sign_in(Provider::Github);
    gate.release();
    assert_eq!(completion.join().unwrap(), Err(Error::Cancelled));
    assert_eq!(
        store.sessions.lock().unwrap()[&key()]
            .tokens
            .access
            .expose(),
        "old-access"
    );
}
#[test]
fn failed_or_wrong_account_reconnect_does_not_replace_saved_credentials() {
    let (service, store, provider) = fixture(false, None, None);
    let mut other = identity();
    other.key.subject = "5678".into();
    *provider.identity_result.lock().unwrap() = Ok(other);
    let (wrong, address) = attempt(&service);
    callback(&wrong, address, "code=synthetic");
    assert_eq!(wrong.complete(), Err(Error::IdentityMismatch));
    *provider.identity_result.lock().unwrap() = Ok(identity());
    store.fail_save.store(1, Ordering::SeqCst);
    let (failed, address) = attempt(&service);
    callback(&failed, address, "code=synthetic");
    assert_eq!(failed.complete(), Err(Error::StoreUnavailable));
    assert_eq!(
        store.sessions.lock().unwrap()[&key()]
            .tokens
            .access
            .expose(),
        "old-access"
    );
}

#[test]
fn stale_api_rejection_cannot_expire_a_newly_rotated_session() {
    let (service, _, provider) = fixture(false, None, None);
    let old = service.access_token(&key(), &control()).unwrap();
    service.reject_access_token(&key(), &old).unwrap();
    assert_eq!(
        service.access_token(&key(), &control()).unwrap().expose(),
        "rotated-access"
    );
    service.reject_access_token(&key(), &old).unwrap();
    service.access_token(&key(), &control()).unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
#[test]
fn missing_replacement_refresh_token_is_never_treated_as_permission_to_replay_old_one() {
    let (service, store, provider) = fixture(true, None, None);
    provider
        .refresh_result
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .refresh = None;
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::RefreshUncertain)
    ));
    assert!(matches!(
        service.access_token(&key(), &control()),
        Err(Error::RefreshUncertain)
    ));
    assert_eq!(
        store.sessions.lock().unwrap()[&key()].refresh_state,
        RefreshState::Pending
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn reconnect_supersedes_an_old_refresh_without_late_persistence() {
    let gate = Arc::new(Gate::default());
    let (service, store, _) = fixture(true, Some(gate.clone()), None);
    let old = spawn_token(&service, control());
    gate.entered();
    let (new, address) = attempt(&service);
    callback(&new, address, "code=synthetic");
    new.complete().unwrap();
    assert!(matches!(old.join().unwrap(), Err(Error::Superseded)));
    let events = service.subscribe();
    events.try_recv().unwrap();
    gate.release();
    events.recv_blocking().unwrap();
    assert_eq!(store.saves.load(Ordering::SeqCst), 2);
    assert_eq!(view(&service).accounts[0].state, ConnectionState::Connected);
}
#[test]
fn token_rejected_while_identity_check_is_pending_is_not_returned_to_a_waiter() {
    let gate = Arc::new(Gate::default());
    let (service, _, _) = fixture(false, None, Some(gate.clone()));
    let waiter = spawn_token(&service, control());
    gate.entered();
    service
        .reject_access_token(&key(), &secret("old-access"))
        .unwrap();
    gate.release();
    assert!(matches!(waiter.join().unwrap(), Err(Error::Superseded)));
}
#[test]
fn an_expired_sign_in_deadline_does_not_publish_or_persist_an_attempt() {
    let (service, store, _) = fixture(false, None, None);
    assert!(matches!(
        service.begin_sign_in(registration(Provider::Github), Some(key()), Duration::ZERO),
        Err(Error::TimedOut)
    ));
    assert!(view(&service).signing_in.is_empty());
    assert_eq!(store.saves.load(Ordering::SeqCst), 0);
}

#[test]
fn ui_cancellation_does_not_wait_for_delayed_credential_restore() {
    let (old, store, provider) = fixture(false, None, None);
    drop(old);
    let gate = Arc::new(Gate::default());
    *store.load_gate.lock().unwrap() = Some(gate.clone());
    let service = Service::new(store.clone(), provider.clone());
    let cancelled = control();
    let worker_control = cancelled.clone();
    let worker = thread::spawn(move || {
        service.begin_sign_in_controlled(registration(Provider::Github), None, worker_control)
    });
    gate.entered();
    let (sent, received) = std::sync::mpsc::channel();
    thread::spawn(move || {
        sent.send(cancelled.cancel()).unwrap();
    });
    assert!(received.recv_timeout(Duration::from_secs(1)).unwrap());
    gate.release();
    assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
    assert_eq!(provider.identities.load(Ordering::SeqCst), 0);
    assert_eq!(store.saves.load(Ordering::SeqCst), 0);
}
#[test]
fn cancellation_reports_finishing_after_the_credential_commit_point() {
    let (service, store, _) = fixture(false, None, None);
    let gate = Arc::new(Gate::default());
    *store.save_gate.lock().unwrap() = Some(gate.clone());
    let (attempt, address) = attempt(&service);
    let control = attempt.control();
    callback(&attempt, address, "code=synthetic");
    let worker = thread::spawn(move || attempt.complete());
    gate.entered();
    assert!(!control.cancel());
    assert!(control.is_committing());
    gate.release();
    assert!(worker.join().unwrap().is_ok());
    assert_eq!(view(&service).accounts[0].state, ConnectionState::Connected);
}

#[test]
fn workspace_selection_is_durable_preserves_tokens_and_rejects_stale_listings() {
    let (_, store, provider) = fixture(false, None, None);
    let mut saved = session(false);
    saved.identity.key.provider = Provider::Bitbucket;
    saved.identity.key.subject = "{synthetic-account}".into();
    let account_key = saved.identity.key.clone();
    store.sessions.lock().unwrap().clear();
    store
        .sessions
        .lock()
        .unwrap()
        .insert(account_key.clone(), saved.clone());
    *provider.identity_result.lock().unwrap() = Ok(saved.identity.clone());
    // A new process restores an account and chooses from its authenticated listing.
    let service = Service::new(store.clone(), provider.clone());
    service.restore().unwrap();
    let epoch = service.state.lock().unwrap().accounts[&account_key].epoch;
    let workspace = super::super::Workspace {
        uuid: "{synthetic-workspace}".into(),
        slug: "example-team".into(),
    };
    let listing = super::super::WorkspaceListing {
        account: account_key.clone(),
        workspaces: vec![workspace.clone()],
        epoch,
    };
    assert_eq!(
        service.select_workspace(&listing, "unknown", &control()),
        Err(Error::InvalidResponse)
    );
    service
        .select_workspace(&listing, &workspace.uuid, &control())
        .unwrap();
    let persisted = store.sessions.lock().unwrap()[&account_key].clone();
    assert_eq!(persisted.workspace, Some(workspace.clone()));
    assert_eq!(
        persisted.tokens.access.expose(),
        saved.tokens.access.expose()
    );
    assert_eq!(
        persisted.tokens.refresh.as_ref().unwrap().expose(),
        saved.tokens.refresh.as_ref().unwrap().expose()
    );
    // Force a token rotation after selection; it must retain account preferences.
    service
        .state
        .lock()
        .unwrap()
        .accounts
        .get_mut(&account_key)
        .unwrap()
        .session
        .tokens
        .expires_at = Some(1);
    service.access_token(&account_key, &control()).unwrap();
    assert_eq!(
        store.sessions.lock().unwrap()[&account_key].workspace,
        Some(workspace)
    );
    service.disconnect(&account_key).unwrap();
    assert_eq!(
        service.select_workspace(&listing, &listing.workspaces[0].uuid, &control()),
        Err(Error::Superseded)
    );
}

#[test]
fn workspace_save_failure_keeps_previous_selection() {
    let (service, store, _) = fixture(false, None, None);
    let listing = super::super::WorkspaceListing {
        account: key(),
        workspaces: vec![super::super::Workspace {
            uuid: "{team}".into(),
            slug: "team".into(),
        }],
        epoch: service.state.lock().unwrap().accounts[&key()].epoch,
    };
    store.fail_save.store(1, Ordering::SeqCst);
    assert_eq!(
        service.select_workspace(&listing, "{team}", &control()),
        Err(Error::StoreUnavailable)
    );
    assert!(view(&service).accounts[0].workspace.is_none());
    assert!(store.sessions.lock().unwrap()[&key()].workspace.is_none());
}

#[test]
fn discovery_reads_coalesce_across_windows_and_queued_waiters_can_cancel() {
    let (service, _, provider) = fixture(false, None, None);
    let gate = Arc::new(Gate::default());
    *provider.hosting_gate.lock().unwrap() = Some(gate.clone());
    let first_service = service.clone();
    let first = thread::spawn(move || {
        first_service.pull_requests(&key(), &crate::hosting::Scope::Authored, &control())
    });
    gate.entered();
    let second_control = control();
    let waiter = second_control.clone();
    let second_service = service.clone();
    let second = thread::spawn(move || {
        second_service.pull_requests(&key(), &crate::hosting::Scope::Authored, &waiter)
    });
    second_control.cancel();
    assert!(matches!(second.join().unwrap(), Err(Error::Cancelled)));
    gate.release();
    let first = first.join().unwrap().unwrap();
    assert!(!first.cached);
    let second = service
        .pull_requests(&key(), &crate::hosting::Scope::Authored, &control())
        .unwrap();
    assert!(second.cached);
    assert!(Arc::ptr_eq(&first.batch, &second.batch));
    assert_eq!(provider.hosting_calls.load(Ordering::SeqCst), 1);
}
#[test]
fn late_discovery_cannot_publish_after_disconnect_and_rate_limits_back_off() {
    let (service, _, provider) = fixture(false, None, None);
    let gate = Arc::new(Gate::default());
    *provider.hosting_gate.lock().unwrap() = Some(gate.clone());
    let request = service.clone();
    let worker = thread::spawn(move || {
        request.pull_requests(&key(), &crate::hosting::Scope::Authored, &control())
    });
    gate.entered();
    service.disconnect(&key()).unwrap();
    gate.release();
    assert!(matches!(worker.join().unwrap(), Err(Error::Superseded)));
    let (service, _, provider) = fixture(false, None, None);
    *provider.hosting_result.lock().unwrap() = Err(Error::RateLimited);
    assert!(matches!(
        service.pull_requests(&key(), &crate::hosting::Scope::Authored, &control()),
        Err(Error::RateLimited)
    ));
    assert!(matches!(
        service.pull_requests(&key(), &crate::hosting::Scope::ReviewRequested, &control()),
        Err(Error::RateLimited)
    ));
    assert_eq!(provider.hosting_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn explicit_discovery_refresh_bypasses_cache_and_retains_it_on_failure() {
    let (service, _, provider) = fixture(false, None, None);
    let scope = crate::hosting::Scope::Authored;
    let initial = service.pull_requests(&key(), &scope, &control()).unwrap();
    assert!(!initial.cached);
    assert!(
        service
            .pull_requests(&key(), &scope, &control())
            .unwrap()
            .cached
    );
    let fresh = service
        .refresh_pull_requests(&key(), &scope, &control())
        .unwrap();
    assert!(!fresh.cached);
    assert_eq!(provider.hosting_calls.load(Ordering::SeqCst), 2);
    *provider.hosting_result.lock().unwrap() = Err(Error::Network);
    assert!(matches!(
        service.refresh_pull_requests(&key(), &scope, &control()),
        Err(Error::Network)
    ));
    let retained = service.pull_requests(&key(), &scope, &control()).unwrap();
    assert!(Arc::ptr_eq(&fresh.batch, &retained.batch));
    assert_eq!(fresh.fetched_at, retained.fetched_at);
    *provider.hosting_result.lock().unwrap() = Err(Error::RateLimited);
    assert!(matches!(
        service.refresh_pull_requests(&key(), &scope, &control()),
        Err(Error::RateLimited)
    ));
    let calls = provider.hosting_calls.load(Ordering::SeqCst);
    assert!(matches!(
        service.refresh_pull_requests(&key(), &scope, &control()),
        Err(Error::RateLimited)
    ));
    assert_eq!(provider.hosting_calls.load(Ordering::SeqCst), calls);
}

#[test]
fn personal_tokens_restore_without_registration_or_refresh_and_reject_revocation() {
    let (service, store, provider) = fixture(false, None, None);
    service.restore().unwrap();
    service
        .connect_token(
            secret("personal-token"),
            Some(key()),
            &Control::new(Duration::from_secs(2)),
        )
        .unwrap();
    let saved = store.sessions.lock().unwrap()[&key()].clone();
    assert_eq!(saved.authentication, Authentication::Token);
    assert!(saved.client_id.is_none() && saved.tokens.refresh.is_none());
    let restored = Service::new(store.clone(), provider.clone());
    restored.restore().unwrap();
    let control = Control::new(Duration::from_secs(2));
    assert_eq!(
        restored.access_token(&key(), &control).unwrap().expose(),
        "personal-token"
    );
    assert_eq!(store.registrations.load(Ordering::SeqCst), 0);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    restored
        .reject_access_token(&key(), &secret("personal-token"))
        .unwrap();
    assert_eq!(
        restored.access_token(&key(), &control).unwrap_err(),
        Error::NeedsSignIn
    );
    assert_eq!(store.registrations.load(Ordering::SeqCst), 0);
}
#[test]
fn token_replacement_requires_explicit_same_identity_and_preserves_previous_connection_on_failure()
{
    let (service, store, provider) = fixture(false, None, None);
    service.restore().unwrap();
    let previous = store.sessions.lock().unwrap()[&key()]
        .tokens
        .access
        .expose()
        .to_owned();
    assert_eq!(
        service.connect_token(
            secret("personal-token"),
            None,
            &Control::new(Duration::from_secs(2))
        ),
        Err(Error::AccountExists)
    );
    *provider.identity_result.lock().unwrap() = Err(Error::Denied);
    assert_eq!(
        service.connect_token(
            secret("bad-token"),
            Some(key()),
            &Control::new(Duration::from_secs(2))
        ),
        Err(Error::Denied)
    );
    let mut other = identity();
    other.key.subject = "different".into();
    *provider.identity_result.lock().unwrap() = Ok(other);
    assert_eq!(
        service.connect_token(
            secret("wrong-token"),
            Some(key()),
            &Control::new(Duration::from_secs(2))
        ),
        Err(Error::IdentityMismatch)
    );
    *provider.identity_result.lock().unwrap() = Ok(identity());
    store.fail_save.store(1, Ordering::SeqCst);
    assert_eq!(
        service.connect_token(
            secret("unsaved-token"),
            Some(key()),
            &Control::new(Duration::from_secs(2))
        ),
        Err(Error::Persistence)
    );
    assert_eq!(
        store.sessions.lock().unwrap()[&key()]
            .tokens
            .access
            .expose(),
        previous
    );
    assert_eq!(
        service
            .access_token(&key(), &Control::new(Duration::from_secs(2)))
            .unwrap()
            .expose(),
        previous
    );
}
#[test]
fn disconnect_during_token_validation_cannot_resurrect_the_account() {
    let gate = Arc::new(Gate::default());
    let (service, store, _) = fixture(false, None, Some(gate.clone()));
    service.restore().unwrap();
    let worker = service.clone();
    let job = thread::spawn(move || {
        worker.connect_token(
            secret("late-token"),
            Some(key()),
            &Control::new(Duration::from_secs(2)),
        )
    });
    gate.entered();
    service.disconnect(&key()).unwrap();
    gate.release();
    assert!(matches!(
        job.join().unwrap(),
        Err(Error::Cancelled | Error::Superseded)
    ));
    assert!(store.sessions.lock().unwrap().is_empty());
    assert!(service.accounts_snapshot().accounts.is_empty());
}
#[test]
fn legacy_oauth_records_migrate_and_token_records_reject_oauth_fields() {
    let old = serde_json::to_value(session(false)).unwrap();
    let mut legacy = old.clone();
    legacy.as_object_mut().unwrap().remove("authentication");
    let migrated: StoredSession = serde_json::from_value(legacy).unwrap();
    assert_eq!(migrated.authentication, Authentication::Oauth);
    migrated.validate().unwrap();
    let mut invalid = old;
    invalid["authentication"] = serde_json::json!("token");
    let invalid: StoredSession = serde_json::from_value(invalid).unwrap();
    assert_eq!(invalid.validate(), Err(Error::StoreInvalid));
}
