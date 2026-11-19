//! Main-thread state transitions only; every store/provider/service call is on
//! the background executor. Atomic handles cancel waits without taking its locks.
use super::{
    AccountKey, Control, Error, Identity, Provider, WorkspaceListing,
    application::{AccountState, Accounts},
};
use gpui_kit::{AppContext, Context};
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub enum AccountAction {
    Load,
    Connect {
        provider: Provider,
        replace: Option<AccountKey>,
    },
    Verify(AccountKey),
    Disconnect(AccountKey),
    Workspaces(AccountKey),
    SelectWorkspace {
        listing: Arc<WorkspaceListing>,
        uuid: String,
    },
    ImportDevelopment,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Working,
    WaitingForBrowser,
    Finishing,
}
pub struct OperationView {
    pub id: u64,
    pub action: AccountAction,
    pub phase: Phase,
    control: Control,
}
impl OperationView {
    pub fn label(&self) -> &'static str {
        if self.phase == Phase::Finishing {
            return "Finishing action…";
        }
        if self.phase == Phase::WaitingForBrowser {
            return "Waiting for browser…";
        }
        match self.action {
            AccountAction::Load => "Checking saved accounts…",
            AccountAction::Connect { .. } => "Preparing sign-in…",
            AccountAction::Verify(_) => "Checking connection…",
            AccountAction::Disconnect(_) => "Disconnecting account…",
            AccountAction::Workspaces(_) => "Loading workspaces…",
            AccountAction::SelectWorkspace { .. } => "Saving workspace…",
            AccountAction::ImportDevelopment => "Importing local development setup…",
        }
    }
}
pub struct OperationHandle {
    pub id: u64,
    control: Control,
}
impl Drop for OperationHandle {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
#[derive(Clone)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}
#[derive(Clone, Copy)]
pub struct Finished {
    pub id: u64,
    pub sign_in: bool,
}
enum Outcome {
    Loaded,
    Connected(Identity),
    Verified,
    Disconnected,
    Workspaces(Arc<WorkspaceListing>),
    Selected,
    Imported(Vec<(Provider, Result<(), Error>)>),
}

impl AccountState {
    fn begin(&mut self, action: AccountAction) -> Option<OperationHandle> {
        if self
            .operation
            .as_ref()
            .is_some_and(|operation| operation.control.check() == Err(Error::Cancelled))
        {
            self.operation = None;
            self.browser_url = None;
        }
        if self.operation.is_some() {
            return None;
        }
        self.serial += 1;
        let id = self.serial;
        let control = Control::new(Duration::from_secs(
            if matches!(action, AccountAction::Connect { .. }) {
                600
            } else {
                120
            },
        ));
        if matches!(action, AccountAction::Workspaces(_)) {
            self.workspaces = None;
        }
        self.operation = Some(OperationView {
            id,
            action,
            control: control.clone(),
            phase: Phase::Working,
        });
        self.browser_url = None;
        self.notice = None;
        self.last_finished = None;
        Some(OperationHandle { id, control })
    }
    pub fn start(
        &mut self,
        action: AccountAction,
        cx: &mut Context<Self>,
    ) -> Option<OperationHandle> {
        let handle = self.begin(action.clone())?;
        let id = handle.id;
        let control = handle.control.clone();
        let backend = cx.global::<Accounts>().backend.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match backend {
                Err(error) => Err(error),
                Ok(backend) => match action {
                    AccountAction::Connect { provider, replace } => {
                        let attempt_control = control.clone();
                        let prepared = cx
                            .background_spawn(async move {
                                attempt_control.check()?;
                                let registration =
                                    backend.registration_for(provider, replace.as_ref())?;
                                attempt_control.check()?;
                                backend.begin_sign_in_controlled(
                                    registration,
                                    replace,
                                    attempt_control,
                                )
                            })
                            .await;
                        match prepared {
                            Err(error) => Err(error),
                            Ok(attempt) => {
                                let url = attempt.authorization_url().to_owned();
                                let proceed = this
                                    .update(cx, |state, cx| state.browser_ready(id, url, cx))
                                    .unwrap_or(false);
                                // SignIn's Drop may take the backend lock. Never
                                // drop a prepared/cancelled attempt on the UI thread.
                                cx.background_spawn(async move {
                                    if proceed {
                                        attempt.complete().map(Outcome::Connected)
                                    } else {
                                        drop(attempt);
                                        Err(Error::Cancelled)
                                    }
                                })
                                .await
                            }
                        }
                    }
                    action => {
                        cx.background_spawn(async move {
                            control.check()?;
                            match action {
                                AccountAction::Load => {
                                    backend.restore()?;
                                    for key in backend.account_keys() {
                                        control.check()?;
                                        // Account-specific failures stay on their rows.
                                        let _ = backend.access_token(&key, &control);
                                    }
                                    control.check()?;
                                    Ok(Outcome::Loaded)
                                }
                                AccountAction::Verify(key) => backend
                                    .access_token(&key, &control)
                                    .map(|_| Outcome::Verified),
                                AccountAction::Disconnect(key) => {
                                    control.commit()?;
                                    backend.disconnect(&key).map(|_| Outcome::Disconnected)
                                }
                                AccountAction::Workspaces(key) => backend
                                    .workspaces(&key, &control)
                                    .map(|listing| Outcome::Workspaces(Arc::new(listing))),
                                AccountAction::SelectWorkspace { listing, uuid } => backend
                                    .select_workspace(&listing, &uuid, &control)
                                    .map(|_| Outcome::Selected),
                                AccountAction::ImportDevelopment => {
                                    control.commit()?;
                                    let report = [Provider::Github, Provider::Bitbucket]
                                        .into_iter()
                                        .map(|provider| {
                                            (
                                                provider,
                                                backend.import_development_registration(provider),
                                            )
                                        })
                                        .collect();
                                    Ok(Outcome::Imported(report))
                                }
                                AccountAction::Connect { .. } => {
                                    unreachable!("browser preparation handled separately")
                                }
                            }
                        })
                        .await
                    }
                },
            };
            let _ = this.update(cx, |state, cx| {
                state.finish(id, result);
                cx.notify();
            });
        })
        .detach();
        Some(handle)
    }
    fn browser_ready(&mut self, id: u64, url: String, cx: &mut Context<Self>) -> bool {
        let Some(operation) = &mut self.operation else {
            return false;
        };
        if operation.id != id || operation.control.check().is_err() {
            return false;
        }
        operation.phase = Phase::WaitingForBrowser;
        self.browser_url = Some(url);
        self.open_browser_again(cx);
        cx.notify();
        true
    }
    pub fn open_browser_again(&self, cx: &mut Context<Self>) {
        if self
            .operation
            .as_ref()
            .is_some_and(|operation| operation.control.check().is_ok())
            && let Some(url) = &self.browser_url
        {
            cx.open_url(url);
        }
    }
    /// Always nonblocking, even if a worker is waiting in an OS credential API.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.cancel_current();
        cx.notify();
    }
    fn cancel_current(&mut self) {
        if let Some(operation) = &mut self.operation {
            if !operation.control.cancel() {
                operation.phase = Phase::Finishing;
                return;
            }
            self.operation = None;
            self.browser_url = None;
            self.notice = Some(Notice {
                text: "Cancelled.".into(),
                error: false,
            });
        }
    }
    fn finish(&mut self, id: u64, result: Result<Outcome, Error>) {
        let Some(operation) = &self.operation else {
            return;
        };
        if operation.id != id {
            return;
        }
        let sign_in = matches!(operation.action, AccountAction::Connect { .. });
        let cancelled = operation.control.check() == Err(Error::Cancelled);
        self.operation = None;
        self.browser_url = None;
        if cancelled {
            return;
        }
        self.last_finished = Some(Finished { id, sign_in });
        self.notice = Some(match result {
            Err(error) => Notice {
                text: error.to_string(),
                error: !matches!(error, Error::Cancelled),
            },
            Ok(outcome) => {
                let text = match outcome {
                    Outcome::Loaded => "Accounts checked.".into(),
                    Outcome::Connected(identity) => {
                        self.workspaces = None;
                        format!(
                            "Connected to {} as {}.",
                            identity.key.provider.name(),
                            identity.label
                        )
                    }
                    Outcome::Verified => "Connection verified.".into(),
                    Outcome::Disconnected => {
                        self.workspaces = None;
                        "Disconnected locally. Provider authorization remains unchanged.".into()
                    }
                    Outcome::Workspaces(listing) => {
                        self.workspaces = Some(listing);
                        "Choose a workspace below.".into()
                    }
                    Outcome::Selected => {
                        self.workspaces = None;
                        "Workspace selected.".into()
                    }
                    Outcome::Imported(report) => report
                        .into_iter()
                        .map(|(provider, result)| {
                            format!(
                                "{}: {}",
                                provider.name(),
                                match result {
                                    Ok(()) => "setup imported".into(),
                                    Err(error) => error.to_string(),
                                }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" · "),
                };
                Notice { text, error: false }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::Snapshot;
    fn state() -> AccountState {
        AccountState::new(Snapshot::default())
    }
    #[test]
    fn cancelled_completion_cannot_replace_a_newer_attempt_or_reopen_a_workspace_list() {
        let mut state = state();
        let first = state.begin(AccountAction::Load).unwrap();
        state.cancel_current();
        let second = state.begin(AccountAction::Load).unwrap();
        state.finish(first.id, Err(Error::Network));
        assert_eq!(state.operation.as_ref().unwrap().id, second.id);
        assert!(state.notice.is_none());
        state.finish(second.id, Ok(Outcome::Loaded));
        assert!(!state.notice.unwrap().error);
    }
    #[test]
    fn closing_an_owner_cancels_its_wait_and_new_views_can_start_again() {
        let mut state = state();
        let first = state.begin(AccountAction::Load).unwrap();
        let control = first.control.clone();
        drop(first);
        assert_eq!(control.check(), Err(Error::Cancelled));
        let second = state.begin(AccountAction::Load).unwrap();
        assert!(second.id > 1);
    }
    #[test]
    fn cancelling_after_the_commit_point_keeps_the_finishing_state() {
        let mut state = state();
        let handle = state.begin(AccountAction::Load).unwrap();
        handle.control.commit().unwrap();
        state.cancel_current();
        assert_eq!(state.operation.as_ref().unwrap().phase, Phase::Finishing);
        state.finish(handle.id, Ok(Outcome::Loaded));
        assert!(state.operation.is_none());
        assert!(!state.notice.unwrap().error);
    }
}
