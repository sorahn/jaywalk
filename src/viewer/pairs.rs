//! Pair gestures use the same review and request ownership as branch presets.
use super::*;
use jaywalk::branch_review::{Catalog, Target};
impl Viewer {
    pub(super) fn reveal_pair_endpoint(
        &mut self,
        right: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let review_context::ReviewSource::Target(Target::Pair { left, right: tip }) =
            &session.review.source
        else {
            return;
        };
        let commit = if right { tip.clone() } else { left.clone() };
        if let Some(index) = session
            .history
            .as_ref()
            .ok()
            .and_then(|h| h.revisions.iter().position(|r| r.commit_id == commit))
        {
            self.history_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
            cx.notify();
        } else if commit != "working tree" {
            self.open_reference_target(commit, false, window, cx);
        }
    }

    pub(super) fn compare_revision(
        &mut self,
        commit: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let Some(first) = self.selected_history_commit() else {
            return;
        };
        if first == commit {
            return;
        }
        let branch = session.review.aggregate.as_ref().clone();
        let second = commit.to_owned();
        let order = self.settings.comparison.pair_order;
        let limit = self.settings.diff.max_text_file_size_mb;
        self.checkpoint_navigation(cx);
        self.cancel_live_refresh();
        self.invalidate_comparison_request();
        let epoch = self.requests.comparison();
        self.review_picker.pending = Some(epoch);
        self.begin_loading(LoadingKind::Comparison, window, cx);
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let catalog = Catalog::capture(&branch)?;
                    let target = catalog.pair(&first, &second, order)?;
                    let comparison = catalog.resolve_with_limit(
                        &target,
                        jaywalk::text_content::TextLimit::from_mib(limit)?,
                    )?;
                    Ok::<_, String>((comparison, target))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != epoch {
                    return;
                }
                viewer.review_picker.pending = None;
                viewer.finish_loading(LoadingKind::Comparison);
                match result {
                    Ok((comparison, target)) => viewer.install_review(
                        comparison,
                        review_context::ReviewSource::Target(target),
                        window,
                        cx,
                    ),
                    Err(error) => {
                        viewer.live.notice = Some(live::LiveNotice::Diagnostic(format!(
                            "Comparison unchanged: {error}"
                        )));
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }
    pub(super) fn swap_comparison(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        if !matches!(
            session.review.source,
            review_context::ReviewSource::Target(Target::Pair { .. })
        ) {
            return;
        }
        let comparison = session.review.aggregate.as_ref().clone();
        self.checkpoint_navigation(cx);
        self.cancel_live_refresh();
        self.invalidate_comparison_request();
        let epoch = self.requests.comparison();
        self.review_picker.pending = Some(epoch);
        self.begin_loading(LoadingKind::Comparison, window, cx);
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { comparison.swapped() })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != epoch {
                    return;
                }
                viewer.review_picker.pending = None;
                viewer.finish_loading(LoadingKind::Comparison);
                match result {
                    Ok(comparison) => {
                        let target = Target::Pair {
                            left: comparison.base().into(),
                            right: comparison.tip().into(),
                        };
                        viewer.install_review(
                            comparison,
                            review_context::ReviewSource::Target(target),
                            window,
                            cx,
                        );
                    }
                    Err(error) => {
                        viewer.live.notice = Some(live::LiveNotice::Diagnostic(error));
                        cx.notify();
                    }
                }
            });
        }));
    }
}
