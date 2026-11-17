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
}
pub fn init(cx: &mut App) {
    let backend = HttpProvider::new()
        .map(|provider| Service::new(Arc::new(OsCredentialStore::new()), Arc::new(provider)));
    install(backend, cx);
}
fn install(backend: Result<Arc<Service>, Error>, cx: &mut App) {
    let receiver = backend.as_ref().ok().map(|backend| backend.subscribe());
    let initial = Snapshot {
        store_error: backend.as_ref().err().copied(),
        ..Snapshot::default()
    };
    let state = cx.new(|cx: &mut Context<AccountState>| {
        let mut state = AccountState {
            snapshot: initial,
            subscription: None,
        };
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
