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
    review_commits: Option<(String, String)>,
    calls: AtomicUsize,
    status_calls: Arc<AtomicUsize>,
    fail: std::sync::atomic::AtomicBool,
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
    fn review_request(
        &self,
        _: &Secret,
        request: jaywalk::hosting::review::Request,
        control: &Control,
    ) -> Result<jaywalk::hosting::review::Reply, Error> {
        use jaywalk::hosting::review::{Reply, Request};
        control.check()?;
        let Request::Json(url) = request else {
            return Err(Error::Unsupported);
        };
        let base = self
            .review_commits
            .as_ref()
            .map_or_else(|| "a".repeat(40), |(base, _)| base.clone());
        let head = self
            .review_commits
            .as_ref()
            .map_or_else(|| "b".repeat(40), |(_, head)| head.clone());
        let value = if url.path().contains("/compare/") {
            serde_json::json!({"merge_base_commit":{"sha":base}})
        } else if url.path().ends_with("/check-runs")
            || url.path().ends_with("/status")
            || url.path().ends_with("/reviews")
        {
            self.status_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                return Err(Error::Network);
            }
            if url.path().ends_with("/check-runs") {
                serde_json::json!({"total_count":1,"check_runs":[{"id":1,"name":"Synthetic CI","head_sha":head,"status":"completed","conclusion":"success"}]})
            } else if url.path().ends_with("/status") {
                serde_json::json!({"sha":head,"total_count":0,"statuses":[]})
            } else {
                serde_json::json!([])
            }
        } else {
            let number = url
                .path_segments()
                .unwrap()
                .next_back()
                .unwrap()
                .parse::<u64>()
                .map_err(|_| Error::InvalidResponse)?;
            serde_json::json!({"number":number,"id":number,"node_id":format!("PR_{number}"),"title":"Synthetic review","body":"Review description","user":{"login":"person"},"state":"open","changed_files":0,"commits":0,"head":{"sha":head,"repo":{"full_name":"team/widgets","node_id":"R"}},"base":{"sha":base,"repo":{"full_name":"team/widgets","node_id":"R"}}})
        };
        Ok(Reply::Json(value))
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
        if self.fail.load(Ordering::SeqCst) {
            return Err(Error::Network);
        }
        let number = match scope {
            Scope::Direct(locator) => locator.number,
            Scope::ReviewRequested => 99,
            _ => 42,
        };
        Ok(batch(number))
    }
}
pub(crate) fn fake_service() -> Arc<Service> {
    fake_service_with_status_counter().0
}
pub(crate) fn fake_service_with_status_counter() -> (Arc<Service>, Arc<AtomicUsize>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let service = Service::new(
        Arc::new(Store),
        Arc::new(Adapter {
            review_commits: None,
            calls: AtomicUsize::new(0),
            status_calls: counter.clone(),
            fail: std::sync::atomic::AtomicBool::new(false),
        }),
    );
    (service, counter)
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
        review_commits: None,
        calls: AtomicUsize::new(0),
        status_calls: Arc::new(AtomicUsize::new(0)),
        fail: std::sync::atomic::AtomicBool::new(false),
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
        let view = app.new(|cx| PullRequestsPage::new(None, path, window, cx));
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
    assert!(cx.debug_bounds("pr-details").is_none());
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 3);
    assert!(!cx.update(|_, app| page.read(app).opening));
    let key = cx.update(|_, app| {
        app.global::<Accounts>().state.read(app).snapshot.accounts[0]
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
        let view = app
            .new(|cx| PullRequestsPage::new(None, fixture.path().join("recent.json"), window, cx));
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
        let view = app
            .new(|cx| PullRequestsPage::new(None, fixture.path().join("recent.json"), window, cx));
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
fn published_review_does_not_add_a_metadata_card_to_the_browser(
    test_cx: &mut gpui_test::TestAppContext,
) {
    let (fixture, _, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app
            .new(|cx| PullRequestsPage::new(None, fixture.path().join("recent.json"), window, cx));
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
    assert!(cx.debug_bounds("pr-details").is_none());
    assert!(!cx.update(|_, app| page.read(app).opening));
}

#[gpui_test::test]
fn nested_filters_preserve_search_and_escape_restores_each_step(
    test_cx: &mut gpui_test::TestAppContext,
) {
    use super::filters::Step;
    let (fixture, adapter, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app
            .new(|cx| PullRequestsPage::new(None, fixture.path().join("recent.json"), window, cx));
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
    assert_eq!(
        cx.update(|_, app| page.read(app).filter_input.read(app).query(app).to_string()),
        "current"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.update(|_, app| page.read(app).step == Step::Filters));
    assert_eq!(
        cx.update(|_, app| page.read(app).filter_input.read(app).query(app).to_string()),
        "scope"
    );
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

#[gpui_test::test]
fn restart_snapshot_remains_visible_after_failed_in_palette_refresh(
    test_cx: &mut gpui_test::TestAppContext,
) {
    let (fixture, adapter, _) = setup(test_cx);
    adapter.fail.store(true, Ordering::SeqCst);
    let path = fixture.path().join("pull-request-cache.json");
    let key = AccountKey {
        provider: Provider::Github,
        subject: "1".into(),
    };
    let mut old = batch(42);
    old.items[0].title = "Old wheel snapshot".into();
    jaywalk::hosting::snapshots::save(
        &path,
        &key,
        &Scope::Authored,
        &old,
        std::time::SystemTime::now(),
        &Control::new(std::time::Duration::from_secs(10)),
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app
            .new(|cx| PullRequestsPage::new(None, fixture.path().join("recent.json"), window, cx));
        view.update(app, |page, cx| {
            page.as_picker();
            page.focus(window, cx);
        });
        page = Some(view.clone());
        gpui::component::Root::new(view, window, app)
    });
    let page = page.unwrap();
    draw(cx);
    assert_eq!(cx.update(|_, app| page.read(app).choices(app).len()), 1);
    assert!(cx.update(|_, app| {
        page.read(app).views[&Scope::Authored]
            .iter()
            .any(|result| result.saved_at.is_some() && result.error == Some(Error::Network))
    }));
    cx.simulate_input("wheel");
    cx.simulate_keystrokes("f5");
    draw(cx);
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 4);
    assert_eq!(cx.update(|_, app| page.read(app).choices(app).len()), 1);
    assert_eq!(
        cx.update(|_, app| page.read(app).input.read(app).query(app).to_string()),
        "wheel"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    adapter.fail.store(false, Ordering::SeqCst);
    cx.simulate_keystrokes("f5");
    draw(cx);
    assert_eq!(adapter.calls.load(Ordering::SeqCst), 6);
    assert_eq!(cx.update(|_, app| page.read(app).choices(app).len()), 2);
    assert!(cx.update(|_, app| {
        page.read(app).views[&Scope::Authored]
            .iter()
            .all(|result| result.saved_at.is_none() && result.error.is_none())
    }));
}

#[gpui_test::test]
fn refreshed_results_preserve_selected_identity_when_rows_move(
    test_cx: &mut gpui_test::TestAppContext,
) {
    let (fixture, _, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app
            .new(|cx| PullRequestsPage::new(None, fixture.path().join("recent.json"), window, cx));
        view.update(app, |page, cx| {
            page.as_picker();
            page.focus(window, cx);
        });
        page = Some(view.clone());
        gpui::component::Root::new(view, window, app)
    });
    let page = page.unwrap();
    draw(cx);
    cx.update(|window, app| {
        page.update(app, |page, cx| {
            let account = page.accounts(cx)[0].clone();
            let mut items = batch(42);
            items.items.push(batch(43).items.remove(0));
            page.accept(
                account.clone(),
                Ok(jaywalk::hosting::Fetched {
                    generation: account.generation,
                    batch: Arc::new(items),
                    cached: false,
                    fetched_at: std::time::SystemTime::now(),
                }),
                window,
                cx,
            );
        })
    });
    draw(cx);
    cx.simulate_keystrokes("down");
    draw(cx);
    cx.update(|window, app| {
        page.update(app, |page, cx| {
            let account = page.accounts(cx)[0].clone();
            let mut items = batch(40);
            items.items.extend(batch(42).items);
            let mut selected = batch(43).items.remove(0);
            selected.title = "Renamed result".into();
            selected.locator.repository = RepositoryName::parse("team/renamed-widgets").unwrap();
            items.items.push(selected);
            page.accept(
                account.clone(),
                Ok(jaywalk::hosting::Fetched {
                    generation: account.generation,
                    batch: Arc::new(items),
                    cached: false,
                    fetched_at: std::time::SystemTime::now(),
                }),
                window,
                cx,
            );
        })
    });
    draw(cx);
    assert!(cx.update(|_, app| { let page = page.read(app); let index = page.input.read(app).selected_index().unwrap().row; matches!(&page.choices(app)[index], Choice::Item { batch, index, .. } if batch.items[*index].locator.number == 43) }));
}

#[gpui_test::test]
fn cancelling_review_open_discards_late_completion(test_cx: &mut gpui_test::TestAppContext) {
    let (fixture, _, _) = setup(test_cx);
    let mut page = None;
    let (_, cx) = test_cx.add_window_view(|window, app| {
        let view = app
            .new(|cx| PullRequestsPage::new(None, fixture.path().join("recent.json"), window, cx));
        page = Some(view.clone());
        gpui::component::Root::new(view, window, app)
    });
    let page = page.unwrap();
    draw(cx);
    let opened = std::rc::Rc::new(std::cell::Cell::new(0));
    cx.update(|_, app| {
        let count = opened.clone();
        app.subscribe(&page, move |_, event, _| {
            if matches!(event, super::Event::Opened(_)) {
                count.set(count.get() + 1);
            }
        })
        .detach();
    });
    cx.update(|window, app| {
        page.update(app, |page, cx| {
            let choice = page.choices(cx).remove(0);
            page.choose(choice, window, cx);
            assert!(page.opening);
            page.cancel(cx);
        })
    });
    draw(cx);
    assert_eq!(opened.get(), 0);
    assert!(!cx.update(|_, app| page.read(app).opening));
    assert!(!cx.update(|_, app| page.read(app).choices(app).is_empty()));
}

pub(crate) fn fake_service_with_review_commits(base: String, head: String) -> Arc<Service> {
    Service::new(
        Arc::new(Store),
        Arc::new(Adapter {
            review_commits: Some((base, head)),
            calls: AtomicUsize::new(0),
            status_calls: Arc::new(AtomicUsize::new(0)),
            fail: std::sync::atomic::AtomicBool::new(false),
        }),
    )
}
