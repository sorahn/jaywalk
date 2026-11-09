use super::*;
use jaywalk::loading_feedback::LoadingFeedback;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Feedback {
    pending: LoadingFeedback,
    timers: HashMap<LoadingKind, Task<()>>,
}

impl Viewer {
    pub(super) fn begin_loading(
        &mut self,
        kind: LoadingKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.feedback.pending.begin(kind, Instant::now());
        self.schedule_feedback(kind, window, cx);
    }
    pub(super) fn finish_loading(&mut self, kind: LoadingKind) {
        self.feedback.pending.finish(kind);
        self.feedback.timers.remove(&kind);
    }
    pub(super) fn loading_visible(&self, kind: LoadingKind) -> bool {
        self.feedback.pending.visible(
            kind,
            Instant::now(),
            Duration::from_millis(self.settings.ui.loading_delay_ms),
        )
    }
    fn schedule_feedback(
        &mut self,
        kind: LoadingKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(started) = self.feedback.pending.started(kind) else {
            return;
        };
        let remaining = self
            .feedback
            .pending
            .remaining(
                kind,
                Instant::now(),
                Duration::from_millis(self.settings.ui.loading_delay_ms),
            )
            .expect("the pending request has a start time");
        self.feedback.timers.insert(
            kind,
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(remaining).await;
                let _ = this.update(cx, |viewer, cx| {
                    if viewer.feedback.pending.started(kind) == Some(started) {
                        cx.notify();
                    }
                });
            }),
        );
    }
    pub(super) fn reschedule_feedback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for kind in [
            LoadingKind::Comparison,
            LoadingKind::Overview,
            LoadingKind::File,
            LoadingKind::Evolution,
            LoadingKind::Live,
        ] {
            self.schedule_feedback(kind, window, cx);
        }
    }
}
