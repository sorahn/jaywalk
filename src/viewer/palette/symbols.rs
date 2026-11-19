//! Background index ownership and source-line navigation for @ / #.
use super::*;
use jaywalk::{
    repository::{ComparisonIdentity, FileSide},
    symbols::{Entry, Index, Scope},
};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, PartialEq, Eq)]
struct Key {
    source: ComparisonIdentity,
    scope: Scope,
}
#[derive(Default)]
pub(super) struct SymbolPicker {
    key: Option<Key>,
    index: Option<Arc<Index>>,
    cache: VecDeque<(Key, Arc<Index>)>,
    work: Option<(Arc<AtomicBool>, Task<()>)>,
    pending: Option<(jaywalk::review_requests::FileRequest, Entry)>,
}
impl SymbolPicker {
    pub(super) fn cancel(&mut self) {
        if let Some((flag, _task)) = self.work.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.key = None;
        self.index = None;
    }
}
impl Drop for SymbolPicker {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Viewer {
    pub(in crate::viewer::palette) fn start_symbol_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind @ (PaletteKind::FileSymbols | PaletteKind::ProjectSymbols)) =
            self.palette.open
        else {
            return;
        };
        let Some(comparison) = self
            .comparison()
            .cloned()
            .filter(|_| self.page == pages::Page::Comparison)
        else {
            return;
        };
        let scope = if kind == PaletteKind::FileSymbols {
            let Some(path) = self.selected_path().filter(|_| self.code().is_some()) else {
                return;
            };
            if !self
                .selected_file
                .as_ref()
                .is_some_and(|file| file.source.identity() == comparison.identity())
            {
                return;
            }
            Scope::File {
                path,
                before: self.palette.line_before,
            }
        } else {
            Scope::Project
        };
        let key = Key {
            source: comparison.identity(),
            scope,
        };
        if self.palette.symbols.key.as_ref() == Some(&key) {
            return;
        }
        self.palette.symbols.cancel();
        self.palette.symbols.key = Some(key.clone());
        if let Some((_, index)) = self
            .palette
            .symbols
            .cache
            .iter()
            .find(|(cached, _)| *cached == key)
        {
            self.palette.symbols.index = Some(index.clone());
            return;
        }
        let generation = self.palette.generation;
        let delay = std::time::Duration::from_millis(self.settings.ui.loading_delay_ms);
        cx.spawn_in(window, async move |viewer, cx| {
            cx.background_executor().timer(delay).await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.palette.generation == generation && viewer.palette.symbols.work.is_some()
                {
                    viewer
                        .palette
                        .state
                        .update(cx, |state, cx| state.set_loading(true, window, cx));
                }
            });
        })
        .detach();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let task = cx.spawn_in(window, async move |viewer, cx| {
            let scope = key.scope.clone();
            let index = cx
                .background_spawn(
                    async move { jaywalk::symbols::build(&comparison, &scope, &flag) },
                )
                .await;
            let _ = viewer.update_in(cx, |viewer, window, cx| {
                if viewer.palette.generation != generation
                    || viewer.palette.symbols.key.as_ref() != Some(&key)
                    || !viewer
                        .comparison()
                        .is_some_and(|comparison| comparison.identity() == key.source)
                {
                    return;
                }
                let index = Arc::new(index);
                viewer.palette.symbols.work = None;
                viewer.palette.symbols.index = Some(index.clone());
                // Keep only two source/scope indexes; never cache text or live paths.
                if index.skipped_files == 0 && !index.limited {
                    viewer.palette.symbols.cache.push_front((key, index));
                    viewer.palette.symbols.cache.truncate(2);
                }
                viewer
                    .palette
                    .state
                    .update(cx, |state, cx| state.set_loading(false, window, cx));
                cx.notify();
            });
        });
        self.palette.symbols.work = Some((cancel, task));
    }
    pub(in crate::viewer::palette) fn symbol_choices(&self, query: &str) -> Vec<(String, Choice)> {
        let state = &self.palette.symbols;
        let Some(key) = &state.key else {
            return Vec::new();
        };
        if !self
            .comparison()
            .is_some_and(|comparison| comparison.identity() == key.source)
        {
            return Vec::new();
        }
        let Some(index) = &state.index else {
            return Vec::new();
        };
        let search = jaywalk::quick_picker::CommandQuery::new(query);
        let mut matches: Vec<_> = index
            .entries
            .iter()
            .filter_map(|entry| {
                let rank = search
                    .rank(&entry.symbol.name, 0)
                    .map(|(category, _, _, score)| (category, score))
                    .or_else(|| {
                        search
                            .rank(&format!("{} {}", entry.symbol.container, entry.path), 0)
                            .map(|(_, _, _, score)| (4, score))
                    })?;
                Some((rank, entry))
            })
            .collect();
        matches.sort_by(|(rank_a, a), (rank_b, b)| {
            rank_a
                .cmp(rank_b)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.symbol.line.cmp(&b.symbol.line))
        });
        let source = Rc::new(key.source.clone());
        // Search every entry; only copy and mount the visible result budget.
        matches
            .into_iter()
            .take(200)
            .map(|(_, entry)| {
                (
                    format!(
                        "{} · {} · {}:{} · {}",
                        entry.symbol.label(),
                        entry.symbol.kind,
                        entry.path,
                        entry.symbol.line + 1,
                        if entry.side == FileSide::Before {
                            "Before"
                        } else {
                            "After"
                        }
                    ),
                    Choice::Symbol {
                        entry: entry.clone(),
                        source: source.clone(),
                    },
                )
            })
            .collect()
    }
    pub(in crate::viewer) fn symbol_picker_description(&self) -> String {
        let Some(key) = &self.palette.symbols.key else {
            return "Open a comparison/file to search symbols · ? lists modes".into();
        };
        if !self
            .comparison()
            .is_some_and(|comparison| comparison.identity() == key.source)
        {
            return "Comparison changed; reopen symbol search".into();
        }
        let coverage = if matches!(key.scope, Scope::Project)
            && self
                .comparison()
                .is_some_and(|comparison| comparison.hosted().is_some())
        {
            "PR changed files only"
        } else if matches!(key.scope, Scope::Project) {
            "Snapshot files; After side, removed files Before"
        } else if matches!(key.scope, Scope::File { before: true, .. }) {
            "Focused Before side; added files After"
        } else {
            "Focused After side; removed files Before"
        };
        let status = self.palette.symbols.index.as_ref().map_or_else(
            || "Indexing symbols… · Escape cancels".into(),
            |index| index.status(),
        );
        let limit_hint = if self
            .palette
            .symbols
            .index
            .as_ref()
            .is_some_and(|index| index.entries.len() > 200)
        {
            " · Up to 200 matches shown; refine search"
        } else {
            ""
        };
        format!("{coverage} · {status}{limit_hint}")
    }
    pub(in crate::viewer::palette) fn select_symbol(
        &mut self,
        entry: Entry,
        source: &ComparisonIdentity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .comparison()
            .is_some_and(|comparison| comparison.identity() == *source)
        {
            return;
        }
        self.checkpoint_navigation(cx);
        if self.selected_path().as_deref() == Some(&entry.path) && self.code().is_some() {
            self.go_to_source_line(
                entry.symbol.line,
                entry.side == FileSide::Before,
                window,
                cx,
            );
        } else {
            self.select_path(&entry.path, window, cx);
            self.palette.symbols.pending = Some((self.requests.file(), entry));
        }
    }
    pub(in crate::viewer) fn finish_symbol_navigation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((request, entry)) = self.palette.symbols.pending.take() else {
            return;
        };
        if request == self.requests.file()
            && self.selected_path().as_deref() == Some(&entry.path)
            && self.code().is_some()
        {
            self.live.pending_position = None;
            self.go_to_source_line(
                entry.symbol.line,
                entry.side == FileSide::Before,
                window,
                cx,
            );
        }
    }
}
