use super::AccountPicker;
use gpui::AppContext;
use gpui::component::{Root, command::CommandState};
use jaywalk::accounts::{
    AccountKey, ClientRegistration, Control, CredentialStore, Error, HostingProvider, Identity,
    Provider, Secret, Service, StoredSession, Tokens, Workspace,
    application::{self, Accounts},
};
use jaywalk::commands::CommandId;
use std::sync::{Arc, Mutex};
struct Store(Mutex<StoredSession>);
impl CredentialStore for Store {
    fn load(&self) -> Result<Vec<StoredSession>, Error> {
        Ok(vec![self.0.lock().unwrap().clone()])
    }
    fn save(&self, session: &StoredSession) -> Result<(), Error> {
        *self.0.lock().unwrap() = session.clone();
        Ok(())
    }
    fn remove(&self, _: &AccountKey) -> Result<(), Error> {
        unreachable!()
    }
    fn registration(&self, _: Provider, _: &str) -> Result<ClientRegistration, Error> {
        Err(Error::Registration)
    }
}
struct Adapter;
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
    fn identity(&self, _: Provider, _: &Secret, _: &Control) -> Result<Identity, Error> {
        Ok(Identity {
            key: AccountKey {
                provider: Provider::Bitbucket,
                subject: "synthetic-id".into(),
            },
            label: "synthetic-account".into(),
        })
    }
    fn workspaces(&self, _: &Secret, _: &Control) -> Result<Vec<Workspace>, Error> {
        Ok(vec![
            Workspace {
                uuid: "one".into(),
                slug: "alpha".into(),
            },
            Workspace {
                uuid: "two".into(),
                slug: "beta".into(),
            },
        ])
    }
}
fn draw(cx: &mut gpui_test::VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, app| window.draw(app).clear(app));
    }
}
#[gpui_test::test]
fn workspace_picker_loads_and_saves_through_the_shared_controller(
    test_cx: &mut gpui_test::TestAppContext,
) {
    let session: StoredSession = serde_json::from_value(serde_json::json!({"identity":{"key":{"provider":"bitbucket","subject":"synthetic-id"},"label":"synthetic-account"},"client_id":"synthetic-client","tokens":{"access":"synthetic-token","refresh":null,"expires_at":null},"refresh_state":"Ready"})).unwrap();
    let store = Arc::new(Store(Mutex::new(session)));
    test_cx.update(|app| {
        gpui_kit::init(app);
        application::init_with_backend(Service::new(store.clone(), Arc::new(Adapter)), app);
    });
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let input = app.new(|cx| CommandState::new(window, cx));
        input.update(app, |input, cx| input.focus(window, cx));
        let picker = app.new(|cx| {
            AccountPicker::new(CommandId::ChooseHostingWorkspace, input, 200, window, cx)
        });
        Root::new(picker, window, app)
    });
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("beta");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        cx.update(
            |_, app| app.global::<Accounts>().state.read(app).snapshot.accounts[0]
                .workspace
                .as_ref()
                .unwrap()
                .slug
                .clone()
        ),
        "beta"
    );
    assert_eq!(
        store.0.lock().unwrap().workspace.as_ref().unwrap().slug,
        "beta"
    );
}

#[test]
fn cancellation_and_browser_recovery_remain_reachable_for_any_query() {
    use super::{Choice, account_row_matches};
    assert!(account_row_matches(
        &Choice::Cancel,
        "Cancel account action",
        "github"
    ));
    assert!(account_row_matches(
        &Choice::Browser,
        "Open authorization browser again",
        "bitbucket"
    ));
    assert!(!account_row_matches(
        &Choice::Browse,
        "Open Accounts page",
        "unmatched"
    ));
}
