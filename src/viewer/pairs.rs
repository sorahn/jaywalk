//! Pair gestures use the same review and request ownership as branch presets.
use super::*;
use jaywalk::branch_review::{Catalog, Target};
impl Viewer {
    pub(super) fn pair_caption(&self, left: &str, right: &str) -> String {
        let label = |id: &str| {
            if id == "working tree" {
                return "Working copy".to_owned();
            }
            let names = self
                .repository
                .ready()
                .and_then(|s| s.history.as_ref().ok())
                .and_then(|h| h.revisions.iter().find(|r| r.commit_id == id))
                .map(|r| {
                    r.references
                        .iter()
                        .filter(|r| !r.conflicted)
                        .take(2)
                        .map(|r| r.display_name())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if names.is_empty() {
                jaywalk::branch_review::endpoint_label(id).into()
            } else {
                format!(
                    "{} ({})",
                    names.join(", "),
                    jaywalk::branch_review::endpoint_label(id)
                )
            }
        };
        format!("{} → {}", label(left), label(right))
    }

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

impl Viewer {
    pub(crate) fn update_difftool(
        &mut self,
        arguments: Arguments,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.arguments = arguments.clone();
        let initial = self.repository.ready().is_none();
        let partial = arguments
            .difftool
            .as_ref()
            .is_some_and(|c| c.received < c.total);
        let limit = self.settings.diff.max_text_file_size_mb;
        self.invalidate_comparison_request();
        let epoch = self.requests.comparison();
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Coalesce rapid callbacks before scanning/parsing the growing
            // directory. The final delivery is prepared immediately.
            if partial {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(80))
                    .await;
            }
            if initial {
                let _ = this.update_in(cx, |viewer, window, cx| {
                    if viewer.requests.comparison() == epoch {
                        viewer.load_repository(None, window, cx);
                    }
                });
                return;
            }
            let result = cx
                .background_spawn(async move {
                    arguments
                        .comparison_with_limit(jaywalk::text_content::TextLimit::from_mib(limit)?)
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.requests.comparison() != epoch {
                    return;
                }
                match result {
                    Ok(comparison) => {
                        let source = if [comparison.base(), comparison.tip()].iter().all(|id| {
                            *id == "working tree"
                                || (id.len() >= 40 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                        }) {
                            review_context::ReviewSource::Target(Target::Pair {
                                left: comparison.base().into(),
                                right: comparison.tip().into(),
                            })
                        } else {
                            review_context::ReviewSource::Home(review_context::HomeSource::Opened)
                        };
                        // Incoming files extend the captured aggregate without
                        // replacing a commit the user is currently inspecting.
                        if viewer
                            .repository
                            .ready()
                            .is_some_and(|s| !s.review.aggregate.is_external())
                        {
                            if let LoadState::Loaded(s) = &mut viewer.repository {
                                s.home = Rc::new(comparison);
                            }
                            cx.notify();
                            return;
                        }
                        let inspecting = viewer
                            .repository
                            .ready()
                            .is_some_and(|s| !matches!(s.scope, ComparisonScope::Aggregate));
                        if inspecting {
                            if let LoadState::Loaded(s) = &mut viewer.repository {
                                s.home = Rc::new(comparison.clone());
                                s.review = review_context::ReviewContext {
                                    source,
                                    aggregate: Rc::new(comparison),
                                };
                            }
                            cx.notify();
                        } else {
                            viewer.install_review(comparison, source, window, cx);
                        }
                    }
                    Err(error) => {
                        viewer.live.notice = Some(live::LiveNotice::Diagnostic(format!(
                            "Difftool update failed: {error}"
                        )));
                        cx.notify();
                    }
                }
            });
        }));
    }
}
