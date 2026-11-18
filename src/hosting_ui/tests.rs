use super::{AccountResult, Choice, PullRequestsPage};
use gpui::component::StyledExt;
use gpui::{AppContext, div, prelude::*, px, size};
use jaywalk::accounts::{
    AccountKey, ClientRegistration, Control, CredentialStore, Error, HostingProvider, Identity,
    Provider, Secret, Service, StoredSession, Tokens,
    application::{self, Accounts},
};
use jaywalk::hosting::{Batch, Locator, PullRequest, RepositoryName, Scope};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Store;
impl CredentialStore for Store {
    fn load(&self) -> Result<Vec<StoredSession>, Error> {
        (1..=2).map(|number|serde_json::from_value(serde_json::json!({"identity":{"key":{"provider":"github","subject":number.to_string()},"label":format!("person{number}")},"client_id":"synthetic-client","tokens":{"access":number.to_string(),"refresh":null,"expires_at":null},"refresh_state":"Ready"})).map_err(|_|Error::StoreInvalid)).collect()
    }
    fn save(&self, _: &StoredSession) -> Result<(), Error> {
        unreachable!("no refresh in picker tests")
    }
    fn remove(&self, _: &AccountKey) -> Result<(), Error> {
        Ok(())
    }
    fn registration(&self, _: Provider, _: &str) -> Result<ClientRegistration, Error> {
        Err(Error::Registration)
    }
}
struct Adapter {
    calls: AtomicUsize,
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
    fn identity(&self, _: Provider, token: &Secret, _: &Control) -> Result<Identity, Error> {
        Ok(Identity {
            key: AccountKey {
                provider: Provider::Github,
                subject: token.expose().into(),
            },
            label: format!("person{}", token.expose()),
        })
    }
    fn pull_requests(
        &self,
        _: &Secret,
        _: &Identity,
        scope: &Scope,
        _: &[jaywalk::accounts::Workspace],
        control: &Control,
    ) -> Result<Batch, Error> {
        control.check()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let number = match scope {
            Scope::Direct(locator) => locator.number,
            Scope::ReviewRequested => 99,
            _ => 42,
        };
        Ok(batch(number))
    }
}
pub(crate) fn fake_service() -> Arc<Service> {
    Service::new(
        Arc::new(Store),
        Arc::new(Adapter {
            calls: AtomicUsize::new(0),
        }),
    )
}
fn batch(number: u64) -> Batch {
    Batch {
        items: vec![PullRequest {
            locator: Locator {
                provider: Provider::Github,
                repository: RepositoryName::parse("team/widgets").unwrap(),
                number,
            },
            id: format!("PR_{number}"),
            repository_id: "R_widgets".into(),
            title: "Smooth wheel rendering".into(),
            author: "person".into(),
            branch: "fix/momentum".into(),
            state: "OPEN".into(),
        }],
        partial: false,
        coverage: vec!["synthetic".into()],
    }
}
fn setup(cx: &mut gpui_test::TestAppContext) -> (tempfile::TempDir, Arc<Adapter>, Arc<Service>) {
    let fixture = tempfile::tempdir().unwrap();
    let adapter = Arc::new(Adapter {
        calls: AtomicUsize::new(0),
    });
    let service = Service::new(Arc::new(Store), adapter.clone());
    cx.update(|app| {
        gpui_kit::init(app);
        super::init(app);
        crate::appearance::init(&jaywalk::settings::Settings::default(), app);
        application::init_with_backend(service.clone(), app);
    });
    (fixture, adapter, service)
}
fn draw(cx: &mut gpui_test::VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, app| window.draw(app).clear(app));
    }
}
#[gpui_test::test]
fn picker_filters_locally_disambiguates_accounts_and_purges_retired_results(
    test_cx: &mut gpui_test::TestAppContext,
) {
    let (fixture, adapter, service) = setup(test_cx);
    let path = fixture.path().join("recent.json");
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app.new(|cx| PullRequestsPage::new(None, path, vec![], window, cx));
        page = Some(view.clone());
        view.update(app, |page, cx| page.focus(window, cx));
        gpui::component::Root::new(view, window, app)
    });
    let page = page.unwrap();
    cx.simulate_resize(size(px(1100.), px(750.)));
    draw(cx);
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
    assert_eq!(cx.update(|_, app| page.read(app).choices(app).len()), 2);
    cx.simulate_input("momentum");
    draw(cx);
    assert_eq!(cx.update(|_, app| page.read(app).choices(app).len()), 2);
    assert_eq!(
        adapter.calls.load(Ordering::SeqCst),
        2,
        "typing must not request network"
    );
    cx.update(|window, app| {
        page.update(app, |page, cx| {
            page.input.update(cx, |state, cx| {
                state.set_query("team/widgets#12", window, cx)
            })
        })
    });
    draw(cx);
    let choices = cx.update(|_, app| page.read(app).choices(app));
    assert_eq!(choices.len(), 2);
    assert!(
        choices
            .iter()
            .all(|choice| matches!(choice, Choice::Lookup { .. }))
    );
    cx.update(|window, app| {
        page.update(app, |page, cx| page.choose(choices[0].clone(), window, cx))
    });
    draw(cx);
    assert!(cx.debug_bounds("pr-details").is_some());
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        cx.update(|_, app| page
            .read(app)
            .selected
            .as_ref()
            .unwrap()
            .item
            .locator
            .number),
        12
    );
    let key = cx.update(|_, app| {
        page.read(app)
            .selected
            .as_ref()
            .unwrap()
            .account
            .identity
            .key
            .clone()
    });
    service.disconnect(&key).unwrap();
    draw(cx);
    assert!(cx.debug_bounds("pr-details").is_none());
    assert!(cx.update(|_, app| page.read(app).choices(app).is_empty()));
}
#[gpui_test::test]
fn newer_scope_wins_and_errors_keep_cached_results(test_cx: &mut gpui_test::TestAppContext) {
    let (fixture, adapter, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app.new(|cx| {
            PullRequestsPage::new(None, fixture.path().join("recent.json"), vec![], window, cx)
        });
        view.update(app, |page, cx| {
            page.set_scope(Some(Scope::ReviewRequested), window, cx)
        });
        page = Some(view.clone());
        gpui::component::Root::new(view, window, app)
    });
    let page = page.unwrap();
    draw(cx);
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 2);
    cx.update(|window, app| {
        page.update(app, |page, cx| {
            let choices = page.choices(cx);
            assert_eq!(choices.len(), 2);
            for choice in choices {
                let Choice::Item { batch, index, .. } = choice else {
                    panic!()
                };
                assert_eq!(batch.items[index].locator.number, 99);
            }
            let account = cx.global::<Accounts>().state.read(cx).snapshot.accounts[0].clone();
            page.accept(account, Err(Error::Network), window, cx);
            assert_eq!(
                page.choices(cx).len(),
                2,
                "a failed refresh keeps the old snapshot"
            );
            assert!(
                page.views[&Scope::ReviewRequested]
                    .iter()
                    .any(|AccountResult { error, .. }| *error == Some(Error::Network))
            );
        })
    });
}

#[gpui_test::test]
fn a_full_url_offers_its_provider_even_after_a_different_account_filter(
    test_cx: &mut gpui_test::TestAppContext,
) {
    let (fixture, adapter, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app.new(|cx| {
            PullRequestsPage::new(None, fixture.path().join("recent.json"), vec![], window, cx)
        });
        page = Some(view.clone());
        gpui::component::Root::new(view, window, app)
    });
    let page = page.unwrap();
    draw(cx);
    cx.update(|window, app| {
        let state = app.global::<Accounts>().state.clone();
        state.update(app, |state, cx| {
            state
                .snapshot
                .accounts
                .push(jaywalk::accounts::AccountView {
                    generation: 999,
                    identity: Identity {
                        key: AccountKey {
                            provider: Provider::Bitbucket,
                            subject: "{other}".into(),
                        },
                        label: "other-account".into(),
                    },
                    workspace: None,
                    state: jaywalk::accounts::ConnectionState::Connected,
                });
            cx.notify();
        });
        page.update(app, |page, cx| {
            page.account = Some(AccountKey {
                provider: Provider::Github,
                subject: "1".into(),
            });
            page.input.update(cx, |state, cx| {
                state.set_query(
                    "https://bitbucket.org/team/widgets/pull-requests/12",
                    window,
                    cx,
                )
            });
            let choices = page.choices(cx);
            assert_eq!(choices.len(), 1);
            let Choice::Lookup { account, locator } = &choices[0] else {
                panic!()
            };
            assert_eq!(account.identity.key.provider, Provider::Bitbucket);
            assert_eq!(locator.number, 12);
        });
    });
    assert_eq!(
        adapter.calls.load(Ordering::SeqCst),
        2,
        "typing a URL must not authenticate or fetch without choosing a candidate"
    );
}

struct PageContainer(gpui::Entity<PullRequestsPage>);
impl gpui::Render for PageContainer {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        div()
            .v_flex()
            .size_full()
            .child(div().h(px(40.)).flex_shrink_0())
            .child(div().flex_1().min_h_0().child(self.0.clone()))
    }
}

#[gpui_test::test]
fn metadata_remains_reachable_by_scrolling_in_a_short_window(
    test_cx: &mut gpui_test::TestAppContext,
) {
    let (fixture, _, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app.new(|cx| {
            PullRequestsPage::new(None, fixture.path().join("recent.json"), vec![], window, cx)
        });
        page = Some(view.clone());
        let container = app.new(|_| PageContainer(view));
        gpui::component::Root::new(container, window, app)
    });
    let page = page.unwrap();
    cx.simulate_resize(size(px(1000.), px(550.)));
    draw(cx);
    cx.update(|window, app| {
        page.update(app, |page, cx| {
            let choice = page.choices(cx).remove(0);
            page.choose(choice, window, cx);
        })
    });
    draw(cx);
    let (maximum, origin) = cx.update(|_, app| {
        let page = page.read(app);
        (page.scroll.max_offset(), page.scroll.offset())
    });
    assert!(
        maximum.y > px(0.),
        "the detail card must contribute to the scroll extent"
    );
    cx.simulate_event(gpui_test::ScrollEvent {
        position: gpui_test::point(px(10.), px(220.)),
        delta: gpui_test::ScrollDelta::Pixels(gpui_test::point(px(0.), px(-1000.))),
        ..Default::default()
    });
    draw(cx);
    let offset = cx.update(|_, app| page.read(app).scroll.offset());
    assert!(offset.y < origin.y);
    let detail = cx.debug_bounds("pr-details").unwrap();
    assert!(
        detail.bottom() <= px(551.),
        "the metadata card must be reachable inside the viewport"
    );
}

#[gpui_test::test]
fn nested_filters_preserve_search_and_escape_restores_each_step(
    test_cx: &mut gpui_test::TestAppContext,
) {
    use super::filters::Step;
    let (fixture, adapter, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app.new(|cx| {
            PullRequestsPage::new(None, fixture.path().join("recent.json"), vec![], window, cx)
        });
        view.update(app, |page, cx| {
            page.as_picker();
            page.focus(window, cx);
        });
        page = Some(view.clone());
        gpui::component::Root::new(view, window, app)
    });
    let page = page.unwrap();
    draw(cx);
    cx.simulate_input("momentum");
    cx.simulate_keystrokes("tab");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).step == Step::Filters));
    cx.simulate_input("scope");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).step == Step::Scope));
    cx.simulate_input("review");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).scope == Some(Scope::ReviewRequested)));
    assert!(cx.update(|_, app| page.read(app).step == Step::Results));
    assert_eq!(
        cx.update(|_, app| page.read(app).input.read(app).query(app).to_string()),
        "momentum"
    );
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 4);

    cx.simulate_keystrokes("tab");
    draw(cx);
    cx.simulate_input("account");
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("person2");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| {
        page.read(app)
            .account
            .as_ref()
            .is_some_and(|key| key.subject == "2")
    }));
    assert_eq!(cx.update(|_, app| page.read(app).choices(app).len()), 1);
    assert_eq!(
        cx.update(|_, app| page.read(app).input.read(app).query(app).to_string()),
        "momentum"
    );

    cx.simulate_keystrokes("tab");
    draw(cx);
    cx.simulate_input("scope");
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("current");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).step == Step::Repository));
    cx.simulate_input("unmatched");
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).step == Step::Scope));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).step == Step::Filters));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).step == Step::Results));
    assert_eq!(
        cx.update(|_, app| page.read(app).input.read(app).query(app).to_string()),
        "momentum"
    );
    cx.update(|_, app| {
        page.update(app, |page, _| {
            page.remotes = vec![jaywalk::hosting::RemoteRepository {
                provider: Provider::Github,
                repository: RepositoryName::parse("team/widgets").unwrap(),
            }];
        })
    });
    cx.simulate_keystrokes("tab");
    draw(cx);
    cx.simulate_input("repository");
    cx.simulate_keystrokes("enter");
    draw(cx);
    cx.simulate_input("widgets");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| matches!(&page.read(app).scope, Some(Scope::Repository(Provider::Github, repo)) if repo.to_string() == "team/widgets")));
    let serial = cx.update(|_, app| page.read(app).serial);
    cx.simulate_keystrokes("tab");
    draw(cx);
    cx.simulate_input("refresh");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).serial > serial));
    assert!(cx.update(|_, app| page.read(app).step == Step::Results));
    assert_eq!(
        cx.update(|_, app| page.read(app).input.read(app).query(app).to_string()),
        "momentum"
    );
}
