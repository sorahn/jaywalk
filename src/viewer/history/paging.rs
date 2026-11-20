use super::*;
use jaywalk::accounts::Control;
use std::time::Duration;
impl Viewer {
    pub(in crate::viewer) fn latest_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.history_query =
            jaywalk::history::HistoryQuery::sources(self.references.visibility.clone(), None)
                .expect("no reveal expression")
                .with_limit(self.settings.history.page_size)
                .expect("validated page size");
        self.reload_history(None, window, cx);
    }
    pub(in crate::viewer) fn load_more_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.references.loading_more {
            return;
        }
        let Some(history) = self
            .repository
            .ready()
            .and_then(|s| s.history.as_ref().ok())
            .cloned()
            .filter(|h| h.has_more())
        else {
            return;
        };
        self.invalidate_history_reload();
        let generation = self.references.generation;
        let control = Control::new(Duration::from_secs(30));
        self.references.page_control = Some(control.clone());
        self.references.loading_more = true;
        self.references.error = None;
        let page_size = self.settings.history.page_size;
        self.references.task = Some(cx.spawn_in(window, async move |this, cx| {
            let source = history.clone();
            let result = cx
                .background_spawn(async move { source.load_more(page_size, &control) })
                .await;
            let _ = this.update_in(cx, |viewer, _, cx| {
                if viewer.references.generation != generation {
                    return;
                }
                viewer.references.loading_more = false;
                viewer.references.page_control = None;
                if !viewer
                    .repository
                    .ready()
                    .and_then(|s| s.history.as_ref().ok())
                    .is_some_and(|current| Arc::ptr_eq(current, &history))
                {
                    return;
                }
                match result {
                    Ok(next) => {
                        viewer.history_query = viewer
                            .history_query
                            .clone()
                            .with_limit(next.revisions.len().max(1))
                            .expect("positive history size");
                        if let LoadState::Loaded(session) = &mut viewer.repository {
                            session.history = Ok(Arc::new(next));
                        }
                    }
                    Err(error) => {
                        viewer.references.error = Some(format!("History retained: {error}"))
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
