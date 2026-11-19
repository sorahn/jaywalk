//! Window-owned local-action requests; repository effects stay in the library.
use super::*;
use jaywalk::{
    accounts::Control,
    local_review::{
        self, Mode,
        checkout::{Lease, Recovery},
    },
    repository::ComparisonIdentity,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
#[derive(Default)]
pub(super) struct State {
    pub(super) checkout: Option<(ComparisonIdentity, Arc<Lease>)>,
    pub(super) repositories: Vec<PathBuf>,
    pub(super) recoveries: Vec<Recovery>,
    pub(super) message: Option<String>,
    serial: u64,
    control: Option<Control>,
    task: Option<Task<()>>,
}
impl State {
    fn cancel(&mut self) {
        self.serial += 1;
        if let Some(control) = self.control.take() {
            control.cancel();
        }
        self.task = None;
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel();
    }
}
struct Initialized;
impl Global for Initialized {}
pub(super) fn init(cx: &mut App) {
    if cx.try_global::<Initialized>().is_some() {
        return;
    }
    cx.set_global(Initialized);
    cx.on_app_quit(|_| local_review::checkout::restore_on_quit())
        .detach();
}
impl Viewer {
    fn receipts_directory(&self) -> Result<PathBuf, String> {
        self.arguments
            .config
            .clone()
            .map(Ok)
            .unwrap_or_else(jaywalk::settings::default_path)
            .map(|path| path.with_file_name("review-checkouts"))
    }
    fn retained_review(&self) -> Option<jaywalk::hosting::review::Review> {
        jaywalk::hosting::review::Review::retained(&self.repository.ready()?.review.aggregate)
    }
    fn local_source(&self) -> Option<ComparisonIdentity> {
        self.repository
            .ready()
            .map(|session| session.review.aggregate.identity())
    }
    fn begin_local_action(
        &mut self,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (u64, Control) {
        self.local_actions.cancel();
        if let Some(control) = self.requests.hosted.take() {
            control.cancel();
            self.requests.comparison_task = None;
        }
        self.local_actions.message = Some(message.into());
        self.local_actions.repositories.clear();
        self.local_actions.recoveries.clear();
        let control = Control::new(Duration::from_secs(180));
        self.local_actions.control = Some(control.clone());
        let serial = self.local_actions.serial;
        self.live.notice = Some(live::LiveNotice::Diagnostic(message.into()));
        let delay = Duration::from_millis(self.settings.ui.loading_delay_ms);
        cx.spawn_in(window, async move |viewer, cx| {
            cx.background_executor().timer(delay).await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.local_actions.serial == serial && viewer.local_actions.control.is_some() {
                    viewer.set_palette_loading(true, window, cx);
                }
            });
        })
        .detach();
        cx.notify();
        (serial, control)
    }
    pub(super) fn cancel_local_action(&mut self) {
        self.local_actions.cancel();
    }
    pub(super) fn release_inactive_checkout(&mut self) {
        if self
            .local_actions
            .checkout
            .as_ref()
            .is_some_and(|(source, lease)| {
                self.local_source().as_ref() != Some(source) || !lease.active()
            })
        {
            self.local_actions.checkout = None;
        }
    }
    pub(super) fn choose_local_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(review) = self.retained_review() else {
            self.report_open_error("Open a published PR to review it locally".into(), cx);
            return;
        };
        let expected = self.local_source();
        let candidates =
            crate::windows::repository_candidates(self.repository_directory().as_deref(), cx);
        self.open_palette(palette::PaletteKind::LocalRepositories, window, cx);
        let (serial, control) = self.begin_local_action(
            "Finding matching known repositories… · Escape cancels",
            window,
            cx,
        );
        self.local_actions.task = Some(cx.spawn_in(window, async move |viewer, cx| {
            let result = cx
                .background_spawn(async move {
                    jaywalk::hosting::associate(&review.metadata().locator, &candidates, &control)
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.local_actions.serial != serial || viewer.local_source() != expected {
                    return;
                }
                viewer.local_actions.control = None;
                viewer.set_palette_loading(false, window, cx);
                match result {
                    Ok(matches) => {
                        viewer.local_actions.message = Some(if matches.matches.is_empty() {
                            "No matching repository in opened/recent history".into()
                        } else {
                            format!(
                                "Choose a repository · {} · {} unreadable",
                                if matches.truncated {
                                    "search limited to 50 paths"
                                } else {
                                    "PR stays pinned"
                                },
                                matches.unreadable
                            )
                        });
                        viewer.local_actions.repositories = matches.matches;
                    }
                    Err(error) => viewer.local_actions.message = Some(error),
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn open_review_locally(
        &mut self,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(review) = self.retained_review() else {
            return;
        };
        let expected = self.local_source();
        let receipts = match self.receipts_directory() {
            Ok(path) => path,
            Err(error) => {
                self.report_open_error(error, cx);
                return;
            }
        };
        let mut options = self.settings.local_review;
        if options.mode == Mode::Hosted {
            options.mode = Mode::Isolated;
        }
        self.open_palette(palette::PaletteKind::LocalRepositories, window, cx);
        let (serial, control) = self.begin_local_action(
            "Acquiring pinned PR objects locally… · Escape cancels",
            window,
            cx,
        );
        self.local_actions.task = Some(cx.spawn_in(window, async move |viewer, cx| {
            let waiter = control.clone();
            let result = cx
                .background_spawn(
                    async move { review.locally(&root, options, &receipts, &control) },
                )
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.local_actions.serial != serial || viewer.local_source() != expected {
                    return;
                }
                viewer.local_actions.control = None;
                viewer.set_palette_loading(false, window, cx);
                let result = result.and_then(|review| {
                    waiter
                        .check()
                        .map(|_| review)
                        .map_err(|error| error.to_string())
                });
                match result {
                    Ok(review) => {
                        viewer.close_palette(window, cx);
                        viewer.open_hosted_review(review, window, cx);
                    }
                    Err(error) => {
                        viewer.local_actions.message = Some(format!(
                            "Local review refused: {error} · Existing view retained; Escape closes"
                        ));
                        viewer.live.notice = Some(live::LiveNotice::Diagnostic(error));
                    }
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn choose_checkout_recovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let directory = match self.receipts_directory() {
            Ok(directory) => directory,
            Err(error) => {
                self.report_open_error(error, cx);
                return;
            }
        };
        self.open_palette(palette::PaletteKind::CheckoutRecovery, window, cx);
        let (serial, _) = self.begin_local_action("Finding review checkout receipts…", window, cx);
        self.local_actions.task = Some(cx.spawn_in(window, async move |viewer, cx| {
            let result = cx
                .background_spawn(async move { local_review::checkout::recoveries(&directory) })
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.local_actions.serial != serial {
                    return;
                }
                viewer.local_actions.control = None;
                viewer.set_palette_loading(false, window, cx);
                match result {
                    Ok(receipts) => {
                        viewer.local_actions.message = Some(if receipts.is_empty() {
                            "No review checkout needs restoration".into()
                        } else {
                            "Restore only if still clean and unchanged · Otherwise leave it alone"
                                .into()
                        });
                        viewer.local_actions.recoveries = receipts;
                    }
                    Err(error) => viewer.local_actions.message = Some(error),
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn restore_checkout(
        &mut self,
        receipt: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette(palette::PaletteKind::CheckoutRecovery, window, cx);
        let (serial, _) =
            self.begin_local_action("Checking and restoring the original checkout…", window, cx);
        self.local_actions.task = Some(cx.spawn_in(window, async move |viewer, cx| {
            let result = local_review::checkout::restore_requested(receipt).await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.local_actions.serial != serial {
                    return;
                }
                viewer.local_actions.control = None;
                viewer.set_palette_loading(false, window, cx);
                let notice = result.unwrap_or_else(|error| format!("Restoration skipped: {error}"));
                viewer.local_actions.message = Some(notice.clone());
                viewer.live.notice = Some(live::LiveNotice::Diagnostic(notice));
                viewer.release_inactive_checkout();
                cx.notify();
            });
        }));
    }
}
