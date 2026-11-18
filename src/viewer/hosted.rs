//! Hosted snapshots enter the same retained viewer as local comparisons.
use super::*;
impl Viewer {
    pub(super) fn open_hosted_review(
        &mut self,
        review: jaywalk::hosting::review::Review,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.checkpoint_page_departure(cx);
        self.cancel_live_refresh();
        self.live.poll = None;
        self.invalidate_history_reload();
        self.invalidate_comparison_request();
        self.clear_file();
        self.evolution.clear();
        self.file_evolution.clear();
        let metadata = review.metadata().clone();
        let comparison = Rc::new(review.into_comparison());
        let history = comparison.history(&self.history_query).map(Rc::new);
        self.repository = LoadState::Loaded(RepositorySession {
            home: comparison.clone(),
            review: review_context::ReviewContext {
                source: review_context::ReviewSource::Hosted(std::sync::Arc::new(metadata.clone())),
                aggregate: comparison.clone(),
            },
            history,
            scope: ComparisonScope::Aggregate,
            comparison: LoadState::Loaded(comparison),
            revision_comparison: None,
        });
        self.navigation.pinned = true;
        self.browser.scope = jaywalk::settings::TreeScope::Changed;
        self.live.notice = Some(live::LiveNotice::Diagnostic(if metadata.history_partial {
            "Published PR snapshot · commit history is limited by hosting.history_limit".into()
        } else {
            "Published PR snapshot · Refresh explicitly updates to the latest version".into()
        }));
        self.activate_page(pages::Page::Comparison, window, cx);
        self.rebuild_browser(cx);
        if let Some(file) = self.browser.files.first().cloned() {
            let comparison = self
                .comparison()
                .expect("installed hosted comparison")
                .clone();
            self.load_file(comparison, file, loading::FileLoad::Select, window, cx);
        }
        window.set_window_title(&format!("{} — Jaywalk", metadata.label()));
        cx.notify();
    }
    pub(super) fn update_hosted_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(comparison) = self.repository.ready().map(|s| s.review.aggregate.clone()) else {
            return;
        };
        let Some(contents) = comparison.hosted().cloned() else {
            return;
        };
        if let Some(control) = self.requests.hosted.take() {
            control.cancel();
        }
        let control = jaywalk::accounts::Control::new(std::time::Duration::from_secs(180));
        self.requests.hosted = Some(control.clone());
        self.live.notice = Some(live::LiveNotice::Diagnostic(
            "Updating published review…".into(),
        ));
        let expected = comparison.identity();
        let options = self.settings.hosting;
        let limit =
            jaywalk::text_content::TextLimit::from_mib(self.settings.diff.max_text_file_size_mb)
                .expect("validated text limit");
        self.requests.comparison_task = Some(cx.spawn_in(window, async move |this, cx| {
            let waiter = control.clone();
            let result = cx
                .background_spawn(async move { contents.newer(options, limit, &control) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if waiter.check().is_err()
                    || viewer
                        .repository
                        .ready()
                        .map(|s| s.review.aggregate.identity())
                        != Some(expected)
                {
                    return;
                }
                viewer.requests.hosted = None;
                match result {
                    Ok(review) => viewer.open_hosted_review(review, window, cx),
                    Err(error) => {
                        viewer.live.notice = Some(live::LiveNotice::Diagnostic(format!(
                            "Review unchanged: {error}"
                        )));
                        cx.notify();
                    }
                }
            });
        }));
        cx.notify();
    }
}
