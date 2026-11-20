//! Repository searches are cancellable background work, independent of loaded pages.
use super::*;
use jaywalk::{
    accounts::Control,
    history::{History, HistorySource, Revision, SearchPage, SearchQuery},
    repository::ComparisonIdentity,
};
use std::{sync::Arc, time::Duration};

struct Key {
    identity: Rc<ComparisonIdentity>,
    query: String,
    source: Option<Arc<HistorySource>>,
    loaded: Arc<History>,
}
#[derive(Default)]
pub(super) struct HistorySearch {
    key: Option<Rc<Key>>,
    matches: Vec<Revision>,
    next: Option<usize>,
    error: Option<String>,
    pub(super) loading: bool,
    request: u64,
    work: Option<(Control, Task<()>)>,
}
impl HistorySearch {
    pub(super) fn cancel(&mut self) {
        if let Some((control, _)) = self.work.take() {
            control.cancel();
        }
        self.key = None;
        self.matches.clear();
        self.next = None;
        self.error = None;
        self.loading = false;
        self.request += 1;
    }
}
impl Drop for HistorySearch {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Viewer {
    fn history_key_current(&self, key: &Key) -> bool {
        self.palette.open == Some(PaletteKind::History)
            && self.page == pages::Page::Comparison
            && self.repository.ready().is_some_and(|session| {
                session.home.identity() == *key.identity
                    && session.history.as_ref().is_ok_and(|history| {
                        match (history.source().map(|s| s.repository_source()), &key.source) {
                            (Some(a), Some(b)) => Arc::ptr_eq(&a, b),
                            (None, None) => Arc::ptr_eq(history, &key.loaded),
                            _ => false,
                        }
                    })
            })
    }
    pub(in crate::viewer) fn sync_history_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.open != Some(PaletteKind::History) {
            return;
        }
        let Some(session) = self
            .repository
            .ready()
            .filter(|_| self.page == pages::Page::Comparison)
        else {
            return;
        };
        let Ok(loaded) = &session.history else {
            return;
        };
        let raw = self.palette.state.read(cx).query(cx);
        let query = Mode::parse(&raw).1.to_owned();
        if self
            .palette
            .history
            .key
            .as_ref()
            .is_some_and(|key| key.query == query && self.history_key_current(key))
        {
            return;
        }
        let key = Rc::new(Key {
            identity: Rc::new(session.home.identity()),
            query,
            source: loaded.source().map(|s| s.repository_source()),
            loaded: loaded.clone(),
        });
        self.palette.history.cancel();
        self.palette.history.key = Some(key.clone());
        let query = match SearchQuery::parse(&key.query) {
            Ok(query) => query,
            Err(error) => {
                self.palette.history.error = Some(error);
                self.set_palette_loading(false, window, cx);
                cx.notify();
                return;
            }
        };
        // Bounded immediate feedback. The background query searches the full source.
        let cached = query.in_memory(
            key.loaded.revisions.iter().take(5000).map(Arc::as_ref),
            0,
            self.settings.history.search_page_size,
            &Control::new(Duration::from_secs(1)),
        );
        if let Ok(page) = cached {
            self.palette.history.matches = page.matches;
        }
        self.start_history_search(query, false, window, cx);
    }
    fn start_history_search(
        &mut self,
        query: SearchQuery,
        append: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.palette.history.key.clone() else {
            return;
        };
        if !self.history_key_current(&key) {
            return;
        }
        let offset = if append {
            let Some(offset) = self.palette.history.next else {
                return;
            };
            offset
        } else {
            0
        };
        if let Some((control, _)) = self.palette.history.work.take() {
            control.cancel();
        }
        self.palette.history.request += 1;
        let request = self.palette.history.request;
        self.palette.history.loading = true;
        self.palette.history.error = None;
        let control = Control::new(Duration::from_secs(30));
        let flag = control.clone();
        let source = key.source.clone();
        let available = key.loaded.clone();
        let size = self.settings.history.search_page_size;
        let delay = Duration::from_millis(self.settings.ui.loading_delay_ms);
        cx.spawn_in(window, async move |viewer, cx| {
            cx.background_executor().timer(delay).await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.palette.open == Some(PaletteKind::History)
                    && viewer.palette.history.request == request
                    && viewer.palette.history.loading
                {
                    viewer.set_palette_loading(true, window, cx);
                }
            });
        })
        .detach();
        let task = cx.spawn_in(window, async move |viewer, cx| {
            let result = cx
                .background_spawn(async move {
                    match source {
                        Some(source) => source.search(&query, offset, size, &flag),
                        None => query.in_memory(
                            available.revisions.iter().map(Arc::as_ref),
                            offset,
                            size,
                            &flag,
                        ),
                    }
                })
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.palette.history.request != request || !viewer.history_key_current(&key) {
                    return;
                }
                viewer.palette.history.work = None;
                viewer.palette.history.loading = false;
                match result {
                    Ok(SearchPage {
                        matches,
                        next_offset,
                        ..
                    }) => {
                        if append {
                            viewer.palette.history.matches.extend(matches);
                        } else {
                            viewer.palette.history.matches = matches;
                        }
                        viewer.palette.history.next = next_offset;
                    }
                    Err(error) => viewer.palette.history.error = Some(error),
                }
                viewer.set_palette_loading(false, window, cx);
                cx.notify();
            });
        });
        self.palette.history.work = Some((control, task));
        cx.notify();
    }
    pub(super) fn load_more_history_matches(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.history.loading {
            return;
        }
        if let Some(key) = &self.palette.history.key
            && let Ok(query) = SearchQuery::parse(&key.query)
        {
            self.start_history_search(query, self.palette.history.next.is_some(), window, cx);
        }
    }
    pub(super) fn history_choices(&self) -> Vec<(String, Choice)> {
        let Some(key) = &self.palette.history.key else {
            return vec![];
        };
        if !self.history_key_current(key) {
            return vec![];
        }
        let mut choices: Vec<_> = self
            .palette
            .history
            .matches
            .iter()
            .map(|revision| {
                (
                    format!(
                        "{}  {}",
                        if revision.is_virtual_working_copy() {
                            "Working copy".into()
                        } else {
                            revision.commit_id.chars().take(10).collect::<String>()
                        },
                        revision.summary()
                    ),
                    Choice::History {
                        revision: revision.clone(),
                        source: key.source.clone(),
                        identity: key.identity.clone(),
                    },
                )
            })
            .collect();
        if self.palette.history.next.is_some()
            || self.palette.history.error.is_some() && SearchQuery::parse(&key.query).is_ok()
        {
            choices.push((
                if self.palette.history.loading {
                    "Loading more matches…".into()
                } else if self.palette.history.error.is_some() {
                    "Retry history search".into()
                } else {
                    "Load more matches…".into()
                },
                Choice::HistoryMore,
            ));
        }
        choices
    }
    pub(super) fn history_picker_description(&self) -> String {
        if let Some(error) = &self.palette.history.error {
            return error.clone();
        }
        let scope = if self.is_hosted_review() {
            "available PR commits"
        } else {
            "repository history"
        };
        if self.palette.history.loading {
            return format!("Searching {scope}… · Escape cancels");
        }
        format!(
            "{} matches{} · {scope} · author:name after:YYYY-MM-DD before:YYYY-MM-DD",
            self.palette.history.matches.len(),
            if self.palette.history.next.is_some() {
                " shown; more available"
            } else {
                ""
            }
        )
    }
    pub(super) fn open_history_result(
        &mut self,
        revision: Revision,
        source: Option<Arc<HistorySource>>,
        identity: &ComparisonIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .repository
            .ready()
            .is_some_and(|s| s.home.identity() == *identity)
        {
            return;
        }
        if let Some(index) = self
            .repository
            .ready()
            .and_then(|s| s.history.as_ref().ok())
            .and_then(|h| {
                h.revisions
                    .iter()
                    .position(|r| r.commit_id == revision.commit_id)
            })
        {
            self.history_scroll
                .scroll_to_item(index, ScrollStrategy::Nearest);
            self.select_revision(&revision.commit_id, window, cx);
            return;
        }
        let Some(source) = source else {
            return;
        };
        self.invalidate_history_reload();
        let generation = self.references.generation;
        let selection = self.requests.comparison();
        let identity = identity.clone();
        let control = Control::new(Duration::from_secs(30));
        self.references.page_control = Some(control.clone());
        self.references.loading_more = true;
        let size = self.settings.history.page_size;
        self.references.task = Some(cx.spawn_in(window, async move |viewer, cx| {
            let target = revision.commit_id.clone();
            let result = cx
                .background_spawn(async move { source.around(&target, size, &control) })
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.references.generation != generation {
                    return;
                }
                viewer.references.page_control = None;
                viewer.references.loading_more = false;
                if viewer.requests.comparison() != selection
                    || !viewer
                        .repository
                        .ready()
                        .is_some_and(|s| s.home.identity() == identity)
                {
                    return;
                }
                match result {
                    Ok(history) => {
                        if let LoadState::Loaded(session) = &mut viewer.repository {
                            session.history = Ok(Arc::new(history));
                        }
                        viewer.history_scroll.scroll_to_item(0, ScrollStrategy::Top);
                        viewer.select_revision(&revision.commit_id, window, cx);
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
