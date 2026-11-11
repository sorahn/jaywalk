//! Request ownership and reconciliation are independent of constructing UI elements.
use super::*;
use jaywalk::review_requests::{ComparisonRequest, FileRequest, RequestClock};
#[derive(Default)]
pub(super) struct ReviewRequests {
    clock: RequestClock,
    pub(super) comparison_task: Option<Task<()>>,
    pub(super) file_task: Option<Task<()>>,
    observer: Option<Subscription>,
}
impl ReviewRequests {
    pub(super) fn comparison(&self) -> ComparisonRequest {
        self.clock.comparison()
    }
    pub(super) fn file(&self) -> FileRequest {
        self.clock.file()
    }
    pub(super) fn invalidate_comparison(&mut self) {
        self.clock.invalidate_comparison();
        self.comparison_task = None;
        self.file_task = None;
    }
    pub(super) fn invalidate_file(&mut self) {
        self.clock.invalidate_file();
        self.file_task = None;
    }
}
impl Viewer {
    pub(super) fn invalidate_comparison_request(&mut self) {
        self.requests.invalidate_comparison();
        self.review_picker.pending = None;
        self.finish_loading(LoadingKind::File);
    }

    pub(super) fn observe_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.requests.observer = Some(cx.observe_in(
            &cx.entity(),
            window,
            |viewer, _, window, cx| viewer.reconcile_review(window, cx),
        ));
    }
    fn reconcile_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = commands::refresh_bindings(cx) {
            self.config.error = Some(format!("Could not reapply keymap: {error}"));
        }
        if !self.is_welcome() {
            self.sync_overview(window, cx);
            self.sync_navigation(cx);
        }
    }
}
