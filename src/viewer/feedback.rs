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

/// Keep diagnostics literal and selectable; Copy retains the complete message,
/// including line breaks, even when only part of it fits in a narrow panel.
pub(super) fn error_message(id: &'static str, message: &str) -> AnyElement {
    let copied = message.to_owned();
    div()
        .id(id)
        .v_flex()
        .gap_2()
        .min_w_0()
        .child(
            div().child(
                Button::new(SharedString::from(format!("{id}-copy")))
                    .ghost()
                    .label("Copy error")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                    }),
            ),
        )
        .child(gpui::base::SelectableText::new(
            SharedString::from(format!("{id}-text")),
            message.to_owned(),
        ))
        .into_any_element()
}
