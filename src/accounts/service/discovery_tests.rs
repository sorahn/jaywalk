//! Real-thread tests for the blocking backend; never run worker wakeups through
//! GPUI's deterministic foreground executor.
use super::*;
use crate::{
    accounts::{Tokens, preferences::AccountConfig},
    hosting::{Batch, Scope},
};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
struct Store(Vec<StoredSession>);
impl CredentialStore for Store {
    fn load(&self) -> Result<Vec<StoredSession>, Error> {
        Ok(self.0.clone())
    }
    fn save(&self, _: &StoredSession) -> Result<(), Error> {
        panic!("these checks must not change credentials")
    }
    fn remove(&self, _: &AccountKey) -> Result<(), Error> {
        Ok(())
    }
    fn registration(&self, _: Provider, _: &str) -> Result<ClientRegistration, Error> {
        Err(Error::Registration)
    }
}
#[derive(Default)]
struct Adapter {
    workspace_orders: Mutex<Vec<Vec<String>>>,
    active: AtomicUsize,
    peak: AtomicUsize,
    entered: Mutex<Vec<String>>,
    released: Mutex<HashSet<String>>,
    entered_changed: Condvar,
    released_changed: Condvar,
}
impl Adapter {
    fn wait_for(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut entered = self.entered.lock().unwrap();
        while entered.len() < count {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "independent accounts must reach the provider concurrently"
            );
            entered = self.entered_changed.wait_timeout(entered, left).unwrap().0;
        }
    }
    fn release(&self, id: &str) {
        self.released.lock().unwrap().insert(id.into());
        self.released_changed.notify_all();
    }
}
impl HostingProvider for Adapter {
    fn exchange(
        &self,
        _: &ClientRegistration,
        _: &str,
        _: &str,
        _: &Control,
    ) -> Result<Tokens, Error> {
        unreachable!()
    }
    fn refresh(&self, _: &ClientRegistration, _: &Secret, _: &Control) -> Result<Tokens, Error> {
        unreachable!()
    }
    fn identity(&self, provider: Provider, token: &Secret, _: &Control) -> Result<Identity, Error> {
        Ok(Identity {
            key: AccountKey {
                provider,
                subject: token.expose().into(),
            },
            label: format!("login-{}", token.expose()),
        })
    }
    fn workspaces(
        &self,
        _: &Secret,
        _: &Control,
    ) -> Result<Vec<crate::accounts::Workspace>, Error> {
        Ok(vec![
            crate::accounts::Workspace {
                uuid: "{old}".into(),
                slug: "old".into(),
            },
            crate::accounts::Workspace {
                uuid: "{new}".into(),
                slug: "new".into(),
            },
        ])
    }
    fn pull_requests(
        &self,
        _: &Secret,
        identity: &Identity,
        _: &Scope,
        workspaces: &[crate::accounts::Workspace],
        control: &Control,
    ) -> Result<Batch, Error> {
        assert_eq!(
            identity.label,
            format!("login-{}", identity.key.subject),
            "display aliases must not enter provider queries"
        );
        self.workspace_orders.lock().unwrap().push(
            workspaces
                .iter()
                .map(|workspace| workspace.slug.clone())
                .collect(),
        );
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        self.entered
            .lock()
            .unwrap()
            .push(identity.key.subject.clone());
        self.entered_changed.notify_all();
        let result = (|| {
            let mut released = self.released.lock().unwrap();
            while !released.contains(&identity.key.subject) {
                control.check()?;
                released = self
                    .released_changed
                    .wait_timeout(released, Duration::from_millis(20))
                    .unwrap()
                    .0;
            }
            Ok(Batch::default())
        })();
        self.active.fetch_sub(1, Ordering::SeqCst);
        result
    }
}
fn fixture(count: usize) -> (Arc<Service>, Arc<Adapter>) {
    let sessions=(1..=count).map(|id|serde_json::from_value(serde_json::json!({"identity":{"key":{"provider":"github","subject":id.to_string()},"label":format!("login-{id}")},"client_id":"public-client","tokens":{"access":id.to_string(),"refresh":null,"expires_at":null},"refresh_state":"Ready"})).unwrap()).collect();
    let provider = Arc::new(Adapter::default());
    let service = Service::new(Arc::new(Store(sessions)), provider.clone());
    service.restore().unwrap();
    (service, provider)
}
fn key(id: &str) -> AccountKey {
    AccountKey {
        provider: Provider::Github,
        subject: id.into(),
    }
}
fn start(
    service: &Arc<Service>,
    id: &str,
) -> std::thread::JoinHandle<Result<crate::hosting::Fetched, Error>> {
    let service = service.clone();
    let key = key(id);
    std::thread::spawn(move || {
        service.pull_requests(&key, &Scope::ForMe, &Control::new(Duration::from_secs(10)))
    })
}
#[test]
fn slow_account_does_not_block_another_accounts_request_or_cache() {
    let (service, provider) = fixture(2);
    let first = start(&service, "1");
    provider.wait_for(1);
    let second = start(&service, "2");
    provider.wait_for(2);
    provider.release("2");
    assert!(!second.join().unwrap().unwrap().cached);
    assert!(
        service
            .pull_requests(
                &key("2"),
                &Scope::ForMe,
                &Control::new(Duration::from_secs(1))
            )
            .unwrap()
            .cached
    );
    assert_eq!(provider.active.load(Ordering::SeqCst), 1);
    provider.release("1");
    first.join().unwrap().unwrap();
    assert_eq!(provider.peak.load(Ordering::SeqCst), 2);
}
#[test]
fn global_limit_bounds_independent_discoveries_and_releases_slots() {
    let (service, provider) = fixture(4);
    let threads = (1..=3)
        .map(|id| start(&service, &id.to_string()))
        .collect::<Vec<_>>();
    provider.wait_for(3);
    let fourth = start(&service, "4");
    {
        let entered = provider.entered.lock().unwrap();
        let (entered, _) = provider
            .entered_changed
            .wait_timeout(entered, Duration::from_millis(100))
            .unwrap();
        assert_eq!(
            entered.len(),
            3,
            "fourth request must wait for an application-wide slot"
        );
    }
    provider.release("1");
    provider.wait_for(4);
    for id in ["2", "3", "4"] {
        provider.release(id);
    }
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    fourth.join().unwrap().unwrap();
    assert_eq!(provider.peak.load(Ordering::SeqCst), 3);
}
#[test]
fn configured_order_and_aliases_do_not_change_authenticated_identity() {
    let (service, provider) = fixture(2);
    let entries = vec![
        AccountConfig {
            provider: Provider::Github,
            id: "2".into(),
            name: Some("Work".into()),
            client_id: Some("public-client".into()),
            workspace: None,
        },
        AccountConfig {
            provider: Provider::Github,
            id: "1".into(),
            name: Some("Personal".into()),
            client_id: Some("public-client".into()),
            workspace: None,
        },
    ];
    service
        .configure_accounts("/synthetic/config.toml".into(), entries, 1)
        .unwrap();
    let accounts = service.accounts_snapshot().accounts;
    assert_eq!(
        accounts
            .iter()
            .map(|account| account.label())
            .collect::<Vec<_>>(),
        ["Work", "Personal"]
    );
    assert_eq!(accounts[0].identity.label, "login-2");
    assert_eq!(service.account_keys(), [key("2"), key("1")]);
    let first = start(&service, "2");
    provider.wait_for(1);
    provider.release("2");
    first.join().unwrap().unwrap();
    assert_eq!(provider.entered.lock().unwrap().as_slice(), ["2"]);
}

#[test]
fn workspace_commands_write_config_and_never_rewrite_tokens() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("config.toml");
    std::fs::write(
        &path,
        "# keep
[[accounts]]
provider='bitbucket'
id='person'
name='Work'
workspace={uuid='{new}',slug='new'}
",
    )
    .unwrap();
    let session=serde_json::from_value(serde_json::json!({"identity":{"key":{"provider":"bitbucket","subject":"person"},"label":"login-person"},"client_id":"public-client","workspace":{"uuid":"{old}","slug":"old"},"tokens":{"access":"person","refresh":null,"expires_at":null},"refresh_state":"Ready"})).unwrap();
    let provider = Arc::new(Adapter::default());
    let service = Service::new(Arc::new(Store(vec![session])), provider.clone());
    let settings =
        crate::settings::Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    service
        .configure_accounts(path.clone(), settings.accounts, 3)
        .unwrap();
    service.restore().unwrap();
    let key = AccountKey {
        provider: Provider::Bitbucket,
        subject: "person".into(),
    };
    assert_eq!(
        service.accounts_snapshot().accounts[0]
            .workspace
            .as_ref()
            .unwrap()
            .slug,
        "new"
    );
    provider.release("person");
    service
        .pull_requests(
            &key,
            &Scope::ReviewRequested,
            &Control::new(Duration::from_secs(5)),
        )
        .unwrap();
    assert_eq!(provider.workspace_orders.lock().unwrap()[0], ["new", "old"]);
    let listing = service
        .workspaces(&key, &Control::new(Duration::from_secs(5)))
        .unwrap();
    service
        .select_workspace(&listing, "{old}", &Control::new(Duration::from_secs(5)))
        .unwrap();
    let saved = crate::settings::Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved.accounts[0].workspace.as_ref().unwrap().slug, "old");
    assert_eq!(
        service.accounts_snapshot().accounts[0]
            .workspace
            .as_ref()
            .unwrap()
            .slug,
        "old"
    );
    std::fs::write(&path, "[invalid").unwrap();
    assert_eq!(
        service.select_workspace(&listing, "{new}", &Control::new(Duration::from_secs(5))),
        Err(Error::Persistence)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[invalid");
    assert_eq!(
        service.accounts_snapshot().accounts[0]
            .workspace
            .as_ref()
            .unwrap()
            .slug,
        "old"
    );
    let cleared = crate::settings::Config::parse(
        "[[accounts]]
provider='bitbucket'
id='person'
",
    )
    .unwrap();
    service
        .configure_accounts(path, cleared.accounts, 3)
        .unwrap();
    assert!(service.accounts_snapshot().accounts[0].workspace.is_none());
}
#[test]
fn account_restore_exports_public_metadata_only_to_the_active_config() {
    let (service, _) = fixture(2);
    let fixture = tempfile::tempdir().unwrap();
    let active = fixture.path().join("config.toml");
    let other = fixture.path().join("other.toml");
    std::fs::write(
        &active,
        "# original
accounts=[]
",
    )
    .unwrap();
    std::fs::write(
        &other,
        "# inactive
",
    )
    .unwrap();
    service
        .configure_accounts(active.clone(), vec![], 3)
        .unwrap();
    service.synchronize_account_config(&other).unwrap();
    assert_eq!(
        std::fs::read_to_string(&other).unwrap(),
        "# inactive
"
    );
    service.synchronize_account_config(&active).unwrap();
    let saved = std::fs::read_to_string(&active).unwrap();
    let config = crate::settings::Config::parse(&saved).unwrap();
    assert_eq!(config.accounts.len(), 2);
    assert_eq!(
        config.accounts[0].client_id.as_deref(),
        Some("public-client")
    );
    assert!(!saved.contains("tokens"));
    assert!(!saved.contains("refresh"));
    assert!(saved.contains("# original"));
}
