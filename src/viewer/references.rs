//! Reference controls reload only the graph; the open comparison stays pinned.
use super::*;
use gpui::component::Sizable;
use gpui::component::popover::Popover;
use jaywalk::history::{HistoryQuery, RemoteVisibility};

pub(super) struct ReferenceState {
    pub(super) visibility: RemoteVisibility,
    generation: u64,
    task: Option<Task<()>>,
    pub(super) error: Option<String>,
}
impl ReferenceState {
    pub(super) fn new() -> Self {
        Self {
            visibility: Default::default(),
            generation: 0,
            task: None,
            error: None,
        }
    }
}
impl Viewer {
    pub(super) fn selected_history_commit(&self) -> Option<String> {
        match &self.repository.ready()?.scope {
            ComparisonScope::Revision { commit_id, .. } => Some(commit_id.clone()),
            ComparisonScope::Evolution { anchor, .. } => Some(anchor.clone()),
            ComparisonScope::WholeBranch => Some(self.repository.ready()?.branch.tip.clone()),
        }
    }
    pub(super) fn set_remote_visibility(
        &mut self,
        remote: Option<String>,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(remote) = remote {
            self.references.visibility.set(remote, visible);
        } else {
            self.references.visibility = RemoteVisibility::all(visible);
        }
        self.history_query = HistoryQuery::sources(self.references.visibility.clone(), None)
            .expect("no reveal target");
        self.reload_history(None, window, cx);
        self.save_layout(cx);
    }
    pub(super) fn reveal_history_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(commit) = self.selected_history_commit() {
            self.open_reference_target(commit, false, window, cx);
        }
    }
    pub(super) fn open_reference_target(
        &mut self,
        commit: String,
        select: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if select
            && self
                .repository
                .ready()
                .and_then(|s| s.history.as_ref().ok())
                .is_some_and(|h| h.revisions.iter().any(|r| r.commit_id == commit))
        {
            self.select_revision(&commit, window, cx);
            return;
        }
        match HistoryQuery::sources(self.references.visibility.clone(), Some(commit.clone())) {
            Ok(query) => {
                self.history_query = query;
                self.reload_history(Some((commit, select)), window, cx);
            }
            Err(error) => {
                self.references.error = Some(error);
                cx.notify();
            }
        }
    }
    pub(super) fn reload_history(
        &mut self,
        target: Option<(String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let branch = session.branch.as_ref().clone();
        let top = self
            .history_scroll
            .0
            .borrow()
            .base_handle
            .logical_scroll_top()
            .0;
        let top_commit = session
            .history
            .as_ref()
            .ok()
            .and_then(|history| history.revisions.get(top))
            .map(|r| r.commit_id.clone());
        let query = self.history_query.clone();
        self.references.generation += 1;
        let generation = self.references.generation;
        self.references.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { branch.current_history(&query) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if generation != viewer.references.generation {
                    return;
                }
                match result {
                    Ok(history) => {
                        let scroll_target = target
                            .as_ref()
                            .map(|(commit, _)| commit)
                            .or(top_commit.as_ref());
                        let index = scroll_target.and_then(|commit| {
                            history
                                .revisions
                                .iter()
                                .position(|r| &r.commit_id == commit)
                        });
                        if let LoadState::Loaded(session) = &mut viewer.repository {
                            session.history = Ok(Rc::new(history));
                        }
                        viewer.references.error = None;
                        if let Some(index) = index {
                            viewer.history_scroll.scroll_to_item(
                                index,
                                if target.is_some() {
                                    ScrollStrategy::Nearest
                                } else {
                                    ScrollStrategy::Top
                                },
                            );
                        }
                        if let Some((commit, true)) = target {
                            viewer.select_revision(&commit, window, cx);
                        }
                    }
                    Err(error) => {
                        viewer.references.error = Some(format!("History retained: {error}"))
                    }
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn remote_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(history) = self
            .repository
            .ready()
            .and_then(|s| s.history.as_ref().ok())
        else {
            return div().into_any_element();
        };
        let remotes = history.references.remotes.clone();
        let visibility = self.references.visibility.clone();
        let viewer = cx.entity().downgrade();
        Popover::new("remote-history-sources")
            .trigger(
                Button::new("remote-history-toggle")
                    .ghost()
                    .small()
                    .compact()
                    .label("Remotes")
                    .tooltip("Choose remote history sources"),
            )
            .content(move |_, _, _| {
                let mut content = div()
                    .v_flex()
                    .gap_1()
                    .p_2()
                    .w(px(300.))
                    .child("Remote history");
                for (label, remote, visible) in [
                    ("Show all".to_string(), None, true),
                    ("Hide all".to_string(), None, false),
                ]
                .into_iter()
                .chain(remotes.iter().cloned().map(|remote| {
                    let show = !visibility.includes(&remote);
                    (
                        format!("{} {remote}", if show { "☐" } else { "☑" }),
                        Some(remote),
                        show,
                    )
                })) {
                    let viewer = viewer.clone();
                    content = content.child(
                        Button::new(SharedString::from(label.clone()))
                            .ghost()
                            .small()
                            .label(label)
                            .on_click(move |_, window, cx| {
                                let _ = viewer.update(cx, |viewer, cx| {
                                    viewer.set_remote_visibility(
                                        remote.clone(),
                                        visible,
                                        window,
                                        cx,
                                    )
                                });
                            }),
                    );
                }
                let viewer = viewer.clone();
                content.child(
                    Button::new("reveal-selected")
                        .ghost()
                        .small()
                        .label("Reveal inspected revision")
                        .on_click(move |_, window, cx| {
                            let _ = viewer.update(cx, |viewer, cx| {
                                viewer.reveal_history_selection(window, cx)
                            });
                        }),
                )
            })
            .into_any_element()
    }
}
