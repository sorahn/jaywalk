//! GPUI ownership only. Views observe one shared Entity; they do not own token
//! refresh tasks or touch the credential store from the UI thread.
use super::{Error, HttpProvider, OsCredentialStore, Service, Snapshot};
use gpui_kit::{App, AppContext, Context, Entity, Global, Task};
use std::sync::Arc;

pub struct Accounts {
    pub backend: Result<Arc<Service>, Error>,
    pub state: Entity<AccountState>,
}
impl Global for Accounts {}
pub struct AccountState {
    pub snapshot: Snapshot,
    subscription: Option<Task<()>>,
    pub operation: Option<super::controller::OperationView>,
    pub notice: Option<super::controller::Notice>,
    pub workspaces: Option<Arc<super::WorkspaceListing>>,
    pub last_finished: Option<super::controller::Finished>,
    pub(super) serial: u64,
    pub(super) browser_url: Option<String>,
}
impl AccountState {
    pub(crate) fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            subscription: None,
            operation: None,
            notice: None,
            workspaces: None,
            last_finished: None,
            serial: 0,
            browser_url: None,
        }
    }
}
pub fn init(cx: &mut App) {
    assert!(
        !cx.is_test(),
        "simulated account tests must install a synthetic backend with init_with_backend"
    );
    let backend = HttpProvider::new()
        .map(|provider| Service::new(Arc::new(OsCredentialStore::new()), Arc::new(provider)));
    install(backend, cx);
}
/// Install an explicitly supplied backend, including deterministic test stores.
/// Production initialization always uses the OS credential store.
pub fn init_with_backend(backend: Arc<Service>, cx: &mut App) {
    install(Ok(backend), cx);
}
fn install(backend: Result<Arc<Service>, Error>, cx: &mut App) {
    let receiver = backend.as_ref().ok().map(|backend| backend.subscribe());
    let initial = Snapshot {
        store_error: backend.as_ref().err().copied(),
        ..Snapshot::default()
    };
    let state = cx.new(|cx: &mut Context<AccountState>| {
        let mut state = AccountState::new(initial);
        if let Some(receiver) = receiver {
            state.subscription = Some(cx.spawn(async move |this, cx| {
                while let Ok(snapshot) = receiver.recv().await {
                    if this
                        .update(cx, |state, cx| {
                            state.snapshot = snapshot;
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        state
    });
    cx.set_global(Accounts { backend, state });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{
        AccountKey, ClientRegistration, Control, CredentialStore, HostingProvider, Identity,
        Provider, Secret, StoredSession, Tokens,
    };
    struct LockedStore;
    impl CredentialStore for LockedStore {
        fn load(&self) -> Result<Vec<StoredSession>, Error> {
            Err(Error::StoreUnavailable)
        }
        fn save(&self, _: &StoredSession) -> Result<(), Error> {
            unreachable!("test only loads")
        }
        fn remove(&self, _: &AccountKey) -> Result<(), Error> {
            unreachable!("test only loads")
        }
        fn registration(&self, _: Provider, _: &str) -> Result<ClientRegistration, Error> {
            unreachable!("test only loads")
        }
    }
    struct OfflineProvider;
    impl HostingProvider for OfflineProvider {
        fn exchange(
            &self,
            _: &ClientRegistration,
            _: &str,
            _: &str,
            _: &Control,
        ) -> Result<Tokens, Error> {
            unreachable!("no network during initialization")
        }
        fn refresh(
            &self,
            _: &ClientRegistration,
            _: &Secret,
            _: &Control,
        ) -> Result<Tokens, Error> {
            unreachable!("no network during initialization")
        }
        fn identity(&self, _: Provider, _: &Secret, _: &Control) -> Result<Identity, Error> {
            unreachable!("no network during initialization")
        }
    }
    #[gpui_test::test]
    fn simulated_tests_reject_production_account_initialization(
        cx: &mut gpui_test::TestAppContext,
    ) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cx.update(init);
        }));
        let panic =
            result.expect_err("production initialization must fail before creating OS services");
        assert!(panic.downcast_ref::<&str>().is_some_and(|message| {
            message.contains("simulated account tests must install a synthetic backend")
        }));
        cx.update(|app| assert!(!app.has_global::<Accounts>()));
    }

    #[gpui_test::test]
    fn views_share_application_state_and_initialization_does_not_read_keychain(
        cx: &mut gpui_test::TestAppContext,
    ) {
        let backend = Service::new(Arc::new(LockedStore), Arc::new(OfflineProvider));
        let (first, second) = cx.update(|app| {
            install(Ok(backend.clone()), app);
            let first = app.global::<Accounts>().state.clone();
            let second = app.global::<Accounts>().state.clone();
            assert!(Arc::ptr_eq(
                app.global::<Accounts>().backend.as_ref().unwrap(),
                &backend
            ));
            assert_eq!(first.entity_id(), second.entity_id());
            (first, second)
        });
        cx.run_until_parked();
        cx.update(|app| assert!(first.read(app).snapshot.store_error.is_none()));
        assert_eq!(backend.restore(), Err(Error::StoreUnavailable));
        cx.run_until_parked();
        cx.update(|app| {
            assert_eq!(
                first.read(app).snapshot.store_error,
                Some(Error::StoreUnavailable)
            );
            assert_eq!(first.read(app).snapshot, second.read(app).snapshot);
        });
        drop(first);
        cx.update(|app| {
            assert_eq!(
                second.read(app).snapshot.store_error,
                Some(Error::StoreUnavailable)
            )
        });
    }
}
