//! Account page content; the main window owns its lifetime across navigation.
use gpui::component::{
    Disableable, StyledExt,
    button::{Button, ButtonVariants},
    spinner::Spinner,
};
use gpui::prelude::FluentBuilder;
use gpui::*;
use jaywalk::{
    accounts::{
        AccountKey, AccountView, ConnectionState, Provider,
        application::{AccountState, Accounts},
        controller::{AccountAction, OperationHandle, Phase},
    },
    commands::CommandId,
};

actions!(account_ui, [Cancel]);
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Accounts,
    Connect,
    Reconnect,
    Disconnect,
    Workspaces,
    Development,
    Verify,
}
impl Mode {
    fn command(command: CommandId) -> Self {
        match command {
            CommandId::ConnectAccount => Self::Connect,
            CommandId::ReconnectAccount => Self::Reconnect,
            CommandId::DisconnectAccount => Self::Disconnect,
            CommandId::ChooseHostingWorkspace => Self::Workspaces,
            CommandId::ImportHostingSetup => Self::Development,
            CommandId::CheckAccount => Self::Verify,
            _ => Self::Accounts,
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Accounts => "Accounts",
            Self::Connect => "Connect account",
            Self::Reconnect => "Reconnect account",
            Self::Disconnect => "Disconnect account",
            Self::Workspaces => "Choose Bitbucket workspace",
            Self::Development => "Import development setup",
            Self::Verify => "Check connection",
        }
    }
}
pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Cancel, Some("JaywalkAccounts"))]);
}
pub(crate) fn cancel_sign_in(cx: &mut App) {
    if let Some(accounts) = cx.try_global::<Accounts>() {
        let state = accounts.state.clone();
        state.update(cx, |state, cx| {
            if state
                .operation
                .as_ref()
                .is_some_and(|operation| matches!(operation.action, AccountAction::Connect { .. }))
            {
                state.cancel(cx);
            }
        });
    }
}

pub(crate) struct LeavePage;
impl EventEmitter<LeavePage> for AccountPanel {}

pub(crate) struct AccountPanel {
    focus: FocusHandle,
    state: Entity<AccountState>,
    mode: Mode,
    owner: Option<OperationHandle>,
    disconnect: Option<AccountKey>,
    _subscription: Subscription,
}
impl AccountPanel {
    pub(crate) fn new(command: CommandId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mode = Mode::command(command);
        let state = cx.global::<Accounts>().state.clone();
        let subscription = cx.observe_in(&state, window, |this, state, window, cx| {
            let finished = state.read(cx).last_finished;
            if let Some(finished) = finished
                && this
                    .owner
                    .as_ref()
                    .is_some_and(|owner| owner.id == finished.id)
            {
                if finished.sign_in {
                    window.activate_window();
                    cx.activate(true);
                }
                this.owner = None;
            }
            cx.notify();
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut panel = Self {
            focus,
            state,
            mode,
            owner: None,
            disconnect: None,
            _subscription: subscription,
        };
        panel.start(AccountAction::Load, cx);
        panel
    }
    pub(crate) fn show(&mut self, command: CommandId, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = Mode::command(command);
        self.disconnect = None;
        self.focus(window, cx);
        cx.notify();
    }
    pub(crate) fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }
    fn start(&mut self, action: AccountAction, cx: &mut Context<Self>) {
        if let Some(owner) = self.state.update(cx, |state, cx| state.start(action, cx)) {
            self.owner = Some(owner);
        }
    }
    fn cancel(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.read(cx).operation.is_some() {
            self.state.update(cx, |state, cx| state.cancel(cx));
        } else if self.disconnect.take().is_some() {
            cx.notify();
        } else {
            cx.emit(LeavePage);
        }
    }
    fn row(&self, account: &AccountView, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = gpui::component::Theme::global(cx);
        let key = account.identity.key.clone();
        let name = match key.provider {
            Provider::Github => "GitHub",
            Provider::Bitbucket => "Bitbucket",
        };
        let status = match &account.state {
            ConnectionState::Stored => "Saved · not checked".into(),
            ConnectionState::Checking => "Checking connection…".into(),
            ConnectionState::Refreshing => "Refreshing connection…".into(),
            ConnectionState::Connected => "Connected".into(),
            ConnectionState::NeedsSignIn => "Needs sign-in".into(),
            ConnectionState::Unavailable(error) => error.to_string(),
        };
        let mut actions = div().h_flex().gap_2();
        if matches!(self.mode, Mode::Accounts | Mode::Verify) {
            let key = key.clone();
            actions = actions.child(
                Button::new(SharedString::from(format!(
                    "verify-{}-{}",
                    key.provider.name(),
                    key.subject
                )))
                .ghost()
                .label("Check")
                .disabled(busy)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.start(AccountAction::Verify(key.clone()), cx)
                })),
            );
        }
        if matches!(self.mode, Mode::Accounts | Mode::Reconnect) {
            let key = key.clone();
            actions = actions.child(
                Button::new(SharedString::from(format!(
                    "reconnect-{}-{}",
                    key.provider.name(),
                    key.subject
                )))
                .ghost()
                .label("Reconnect")
                .disabled(busy)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.start(
                        AccountAction::Connect {
                            provider: key.provider,
                            replace: Some(key.clone()),
                        },
                        cx,
                    )
                })),
            );
        }
        if matches!(self.mode, Mode::Accounts | Mode::Disconnect) {
            let key = key.clone();
            actions = actions.child(
                Button::new(SharedString::from(format!(
                    "disconnect-{}-{}",
                    key.provider.name(),
                    key.subject
                )))
                .ghost()
                .debug_selector(|| "account-disconnect".into())
                .label("Disconnect")
                .disabled(busy)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.disconnect = Some(key.clone());
                    cx.notify();
                })),
            );
        }
        if key.provider == Provider::Bitbucket
            && matches!(self.mode, Mode::Accounts | Mode::Workspaces)
        {
            actions = actions.child(
                Button::new(SharedString::from(format!("workspaces-{}", key.subject)))
                    .ghost()
                    .label("Choose workspace…")
                    .disabled(busy)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.mode = Mode::Workspaces;
                        view.start(AccountAction::Workspaces(key.clone()), cx);
                    })),
            );
        }
        div()
            .v_flex()
            .gap_2()
            .py_3()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .font_medium()
                    .child(format!("{name} · {}", account.label())),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(status),
            )
            .children(account.workspace.as_ref().map(|workspace| {
                div()
                    .text_sm()
                    .child(format!("Workspace: {}", workspace.slug))
            }))
            .child(actions)
            .into_any_element()
    }
}
impl Render for AccountPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = gpui::component::Theme::global(cx);
        let muted = theme.muted_foreground;
        let border = theme.border;
        let background = theme.background;
        let foreground = theme.foreground;
        let danger = theme.danger;
        let state = self.state.read(cx);
        let busy = state.operation.is_some();
        let phase = state.operation.as_ref().map(|operation| operation.phase);
        let working = state.operation.as_ref().map(|operation| operation.label());
        let notice = state.notice.clone();
        let snapshot = state.snapshot.clone();
        let listing = state.workspaces.clone();
        let mut content = div().v_flex().gap_3();
        if let Some(label) = working {
            content = content
                .child(
                    div()
                        .h_flex()
                        .gap_2()
                        .items_center()
                        .child(Spinner::new())
                        .child(label),
                )
                .child(
                    div()
                        .h_flex()
                        .gap_2()
                        .when(phase == Some(Phase::WaitingForBrowser), |row| {
                            row.child(
                                Button::new("accounts-reopen-browser")
                                    .ghost()
                                    .label("Open browser again")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.state
                                            .update(cx, |state, cx| state.open_browser_again(cx))
                                    })),
                            )
                        })
                        .child(
                            Button::new("accounts-cancel")
                                .ghost()
                                .label("Cancel")
                                .disabled(phase == Some(Phase::Finishing))
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.cancel(window, cx)),
                                ),
                        ),
                );
        }
        if let Some(notice) = notice {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(if notice.error { danger } else { muted })
                    .child(if notice.error {
                        crate::feedback::error_message("accounts-notice", &notice.text)
                    } else {
                        div().child(notice.text).into_any_element()
                    }),
            );
        }
        if let Some(error) = snapshot.store_error {
            content =
                content
                    .child(div().text_sm().text_color(danger).child(
                        crate::feedback::error_message("accounts-store-error", &error.to_string()),
                    ))
                    .child(
                        Button::new("accounts-retry")
                            .label("Retry")
                            .disabled(busy)
                            .on_click(
                                cx.listener(|view, _, _, cx| view.start(AccountAction::Load, cx)),
                            ),
                    );
        }
        if self.mode == Mode::Development {
            content = content
                .child(div().text_sm().child(
                    "Reuse developer app registrations saved by the local authentication probe. Account tokens are not imported.",
                ))
                .child(Button::new("accounts-import-setup")
                    .label("Import development setup")
                    .disabled(busy)
                    .on_click(cx.listener(|view, _, _, cx| view.start(AccountAction::ImportDevelopment, cx))));
        } else {
            if let Some(key) = &self.disconnect {
                let key = key.clone();
                let label = snapshot
                    .accounts
                    .iter()
                    .find(|account| account.identity.key == key)
                    .map_or("this account", |account| account.label())
                    .to_owned();
                let confirmation = div().v_flex().gap_2().p_3().border_1().border_color(border)
                    .child(format!("Disconnect {label}?"))
                    .child(div().text_sm().text_color(muted).child(
                        "This removes the local connection. Authorization on the provider stays unchanged.",
                    ))
                    .child(div().h_flex().gap_2()
                        .child(Button::new("accounts-keep").ghost().label("Keep account")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.disconnect = None;
                                cx.notify();
                            })))
                        .child(Button::new("accounts-confirm-disconnect").debug_selector(|| "accounts-confirm-disconnect".into()).label("Disconnect").disabled(busy)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.disconnect = None;
                                view.start(AccountAction::Disconnect(key.clone()), cx);
                            }))));
                content = content.child(confirmation);
            }
            if snapshot.accounts.is_empty() {
                content = content.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("No accounts connected."),
                );
            }
            for account in &snapshot.accounts {
                content = content.child(self.row(account, busy, cx));
            }
            if self.mode == Mode::Workspaces
                && let Some(listing) = listing
            {
                let count = listing.workspaces.len();
                content = content
                    .child(div().text_sm().font_medium().child(format!(
                            "Workspaces for {}",
                            snapshot
                                .accounts
                                .iter()
                                .find(|account| account.identity.key == listing.account)
                                .map_or("Bitbucket account", |account| account
                                    .identity
                                    .label
                                    .as_str())
                        )))
                    .when(count == 0, |column| {
                        column.child("No accessible workspaces were returned.")
                    })
                    .child(
                        uniform_list(
                            "account-workspace-rows",
                            count,
                            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let selected = listing.clone();
                                        let uuid = listing.workspaces[index].uuid.clone();
                                        div()
                                            .h(px(36.))
                                            .child(
                                                Button::new(("account-workspace", index))
                                                    .ghost()
                                                    .label(listing.workspaces[index].slug.clone())
                                                    .disabled(busy)
                                                    .on_click(cx.listener(
                                                        move |view, _, _, cx| {
                                                            view.start(
                                                                AccountAction::SelectWorkspace {
                                                                    listing: selected.clone(),
                                                                    uuid: uuid.clone(),
                                                                },
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                            .into_any_element()
                                    })
                                    .collect()
                            }),
                        )
                        .h(px(180.)),
                    );
            }
            content = content.child(
                div()
                    .h_flex()
                    .gap_2()
                    .mt_2()
                    .child(
                        Button::new("accounts-connect-github")
                            .label("Connect GitHub…")
                            .disabled(busy)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.start(
                                    AccountAction::Connect {
                                        provider: Provider::Github,
                                        replace: None,
                                    },
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("accounts-connect-bitbucket")
                            .label("Connect Bitbucket…")
                            .disabled(busy)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.start(
                                    AccountAction::Connect {
                                        provider: Provider::Bitbucket,
                                        replace: None,
                                    },
                                    cx,
                                )
                            })),
                    ),
            );
            if let Some(url) = jaywalk::accounts::publisher::github_install_url() {
                content = content.child(
                    div().v_flex().gap_1()
                        .child(div().text_sm().text_color(muted).child("To review private GitHub repositories, choose which repositories Jaywalk can access."))
                        .child(div().child(Button::new("accounts-github-repositories").ghost().label("Choose GitHub repositories…").on_click(move |_, _, cx| cx.open_url(&url))))
                );
            }
            content = content.child(
                Button::new("accounts-developer-setup")
                    .ghost()
                    .label("Import development setup…")
                    .disabled(busy)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.mode = Mode::Development;
                        cx.notify();
                    })),
            );
        }
        div()
            .key_context("JaywalkAccounts")
            .debug_selector(|| "accounts-page".into())
            .track_focus(&self.focus)
            .size_full()
            .v_flex()
            .bg(background)
            .text_color(foreground)
            .on_action(cx.listener(|view, _: &Cancel, window, cx| view.cancel(window, cx)))
            .child(
                div()
                    .id("accounts-scroll")
                    .debug_selector(|| "accounts-scroll".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_8()
                    .child(
                        div()
                            .mx_auto()
                            .w_full()
                            .max_w(rems(48.))
                            .when(self.mode != Mode::Accounts, |column| {
                                column.child(
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_3()
                                        .mb_4()
                                        .child(
                                            Button::new("accounts-back")
                                                .ghost()
                                                .label("Accounts")
                                                .on_click(cx.listener(|view, _, _, cx| {
                                                    view.mode = Mode::Accounts;
                                                    view.disconnect = None;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div().text_lg().font_medium().child(self.mode.title()),
                                        ),
                                )
                            })
                            .child(content),
                    ),
            )
            .child(
                div()
                    .px_5()
                    .py_3()
                    .border_t_1()
                    .border_color(border)
                    .h_flex()
                    .justify_end()
                    .child(
                        Button::new("accounts-leave")
                            .ghost()
                            .label("Back")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(LeavePage))),
                    ),
            )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{AccountPanel, Mode, init};
    use gpui::{AppContext, px, size};
    use jaywalk::accounts::application::Accounts;
    use jaywalk::accounts::controller::AccountAction;
    use jaywalk::accounts::{
        AccountKey, ClientRegistration, Control, CredentialStore, Error, HostingProvider, Identity,
        Provider, Secret, Service, StoredSession, Tokens,
    };
    use jaywalk::commands::CommandId;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    pub(crate) struct Store {
        sessions: Mutex<Vec<StoredSession>>,
        removals: AtomicUsize,
    }
    impl CredentialStore for Store {
        fn load(&self) -> std::result::Result<Vec<StoredSession>, Error> {
            Ok(self.sessions.lock().unwrap().clone())
        }
        fn save(&self, _: &StoredSession) -> std::result::Result<(), Error> {
            unreachable!("these UI tests do not save credentials")
        }
        fn remove(&self, key: &AccountKey) -> std::result::Result<(), Error> {
            self.removals.fetch_add(1, Ordering::SeqCst);
            self.sessions
                .lock()
                .unwrap()
                .retain(|session| session.identity.key != *key);
            Ok(())
        }
        fn registration(
            &self,
            _: Provider,
            _: &str,
        ) -> std::result::Result<ClientRegistration, Error> {
            Err(Error::Registration)
        }
    }
    struct ProviderStub;
    impl HostingProvider for ProviderStub {
        fn exchange(
            &self,
            _: &ClientRegistration,
            _: &str,
            _: &str,
            _: &Control,
        ) -> std::result::Result<Tokens, Error> {
            unreachable!("no browser flow in UI tests")
        }
        fn refresh(
            &self,
            _: &ClientRegistration,
            _: &Secret,
            _: &Control,
        ) -> std::result::Result<Tokens, Error> {
            unreachable!("no expired token in UI tests")
        }
        fn identity(
            &self,
            _: Provider,
            _: &Secret,
            _: &Control,
        ) -> std::result::Result<Identity, Error> {
            Ok(identity())
        }
    }
    fn identity() -> Identity {
        Identity {
            key: AccountKey {
                provider: Provider::Github,
                subject: "1234".into(),
            },
            label: "synthetic-user".into(),
        }
    }
    pub(crate) fn setup(cx: &mut gpui_test::TestAppContext) -> Arc<Store> {
        let store = Arc::new(Store {
            sessions: Mutex::new(vec![serde_json::from_value(serde_json::json!({
                "identity": {"key":{"provider":"github","subject":"1234"},"label":"synthetic-user"},
                "client_id":"synthetic-client", "tokens":{"access":"synthetic-token","refresh":null,"expires_at":null},
                "refresh_state":"Ready"
            })).unwrap()]),
            removals: AtomicUsize::new(0),
        });
        let backend = Service::new(store.clone(), Arc::new(ProviderStub));
        cx.update(|app| {
            gpui::init(app);
            init(app);
            crate::appearance::init(&jaywalk::settings::Settings::default(), app);
            jaywalk::accounts::application::init_with_backend(backend, app);
        });
        store
    }
    fn draw(cx: &mut gpui_test::VisualTestContext) {
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, app| window.draw(app).clear(app));
        }
    }
    #[gpui_test::test]
    fn disconnect_requires_confirmation_and_escape_keeps_the_account(
        cx: &mut gpui_test::TestAppContext,
    ) {
        let store = setup(cx);
        let mut panel = None;
        let (_, cx) = cx.add_window_view(|window, app| {
            let view = app.new(|cx| AccountPanel::new(CommandId::DisconnectAccount, window, cx));
            panel = Some(view.clone());
            gpui::component::Root::new(view, window, app)
        });
        let panel = panel.unwrap();
        cx.simulate_resize(size(px(580.), px(540.)));
        draw(cx);
        assert_eq!(
            cx.update(|_, app| panel.read(app).state.read(app).snapshot.accounts.len()),
            1
        );
        let button = cx.debug_bounds("account-disconnect").unwrap();
        cx.simulate_click(button.center(), Default::default());
        draw(cx);
        assert!(cx.debug_bounds("accounts-confirm-disconnect").is_some());
        assert_eq!(store.removals.load(Ordering::SeqCst), 0);
        cx.simulate_keystrokes("escape");
        draw(cx);
        assert!(cx.debug_bounds("accounts-confirm-disconnect").is_none());
        assert_eq!(store.removals.load(Ordering::SeqCst), 0);
        cx.simulate_click(button.center(), Default::default());
        draw(cx);
        let confirm = cx.debug_bounds("accounts-confirm-disconnect").unwrap();
        cx.simulate_click(confirm.center(), Default::default());
        draw(cx);
        assert_eq!(store.removals.load(Ordering::SeqCst), 1);
        assert!(cx.update(|_, app| panel.read(app).state.read(app).snapshot.accounts.is_empty()));
    }
    #[gpui_test::test]
    fn missing_setup_is_actionable_without_opening_another_window(
        cx: &mut gpui_test::TestAppContext,
    ) {
        setup(cx);
        let mut panel = None;
        let (_, cx) = cx.add_window_view(|window, app| {
            let view = app.new(|cx| AccountPanel::new(CommandId::ConnectAccount, window, cx));
            panel = Some(view.clone());
            gpui::component::Root::new(view, window, app)
        });
        let panel = panel.unwrap();
        draw(cx);
        cx.update(|window, app| {
            panel.update(app, |view, cx| {
                assert!(view.mode == Mode::Connect);
                view.start(
                    AccountAction::Connect {
                        provider: Provider::Github,
                        replace: None,
                    },
                    cx,
                );
                view.focus(window, cx);
            })
        });
        draw(cx);
        cx.update(|_, app| {
            let state = app.global::<Accounts>().state.read(app);
            assert!(state.operation.is_none());
            assert!(state.notice.as_ref().unwrap().text.contains("publisher"));
        });
    }
    #[gpui_test::test]
    fn account_error_copy_keeps_full_literal_multiline_message(cx: &mut gpui_test::TestAppContext) {
        setup(cx);
        let (_, cx) = cx.add_window_view(|window, app| {
            let panel = app.new(|cx| AccountPanel::new(CommandId::ConnectAccount, window, cx));
            gpui::component::Root::new(panel, window, app)
        });
        let message = "Provider setup is missing.\n<literal> & diagnostic details\n".to_owned();
        cx.update(|_, app| {
            let state = app.global::<Accounts>().state.clone();
            state.update(app, |state, cx| {
                state.notice = Some(jaywalk::accounts::controller::Notice {
                    text: message.clone(),
                    error: true,
                });
                cx.notify();
            });
        });
        cx.simulate_resize(size(px(400.), px(500.)));
        draw(cx);
        let copy = cx.debug_bounds("accounts-notice-copy").unwrap();
        cx.simulate_click(copy.center(), Default::default());
        draw(cx);
        assert_eq!(
            cx.update(|_, app| app.read_from_clipboard().and_then(|item| item.text())),
            Some(message)
        );
    }

    #[gpui_test::test]
    fn account_pages_in_two_windows_observe_one_local_disconnect(
        test_cx: &mut gpui_test::TestAppContext,
    ) {
        let store = setup(test_cx);
        let first = {
            let mut page = None;
            let (_, cx) = test_cx.add_window_view(|window, app| {
                let view = app.new(|cx| AccountPanel::new(CommandId::Accounts, window, cx));
                page = Some(view.clone());
                gpui::component::Root::new(view, window, app)
            });
            draw(cx);
            page.unwrap()
        };
        let mut second = None;
        let (_, cx) = test_cx.add_window_view(|window, app| {
            let view = app.new(|cx| AccountPanel::new(CommandId::Accounts, window, cx));
            second = Some(view.clone());
            gpui::component::Root::new(view, window, app)
        });
        let second = second.unwrap();
        draw(cx);
        assert!(cx.debug_bounds("account-disconnect").is_some());
        cx.update(|_, app| {
            first.update(app, |page, cx| {
                page.start(AccountAction::Disconnect(identity().key), cx)
            })
        });
        draw(cx);
        assert_eq!(store.removals.load(Ordering::SeqCst), 1);
        assert!(cx.debug_bounds("account-disconnect").is_none());
        cx.update(|_, app| {
            assert_eq!(
                first.read(app).state.entity_id(),
                second.read(app).state.entity_id()
            );
            assert!(first.read(app).state.read(app).snapshot.accounts.is_empty());
            assert!(
                second
                    .read(app)
                    .state
                    .read(app)
                    .snapshot
                    .accounts
                    .is_empty()
            );
        });
    }
}
