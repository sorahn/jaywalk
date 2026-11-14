//! Reference controls reload only the graph; the open comparison stays pinned.
use super::*;
use gpui::component::Sizable;
use gpui::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui::component::popover::Popover;
use gpui::component::tag::Tag;
use gpui::prelude::FluentBuilder;
use jaywalk::history::{HistoryQuery, RemoteVisibility};
use jaywalk::references::ReferenceBadgeKind;

pub(super) struct ReferenceState {
    pub(super) visibility: RemoteVisibility,
    pub(super) labels: bool,
    pub(super) open: bool,
    pub(super) inspected: Option<jaywalk::history::Reference>,
    generation: u64,
    task: Option<Task<()>>,
    pub(super) error: Option<String>,
}
impl ReferenceState {
    pub(super) fn new(settings: &jaywalk::settings::HistorySettings) -> Self {
        Self {
            visibility: settings.remotes.clone(),
            labels: settings.show_reference_labels,
            open: false,
            inspected: None,
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
            ComparisonScope::Aggregate => {
                let tip = self.repository.ready()?.review.aggregate.tip();
                (tip == "working tree"
                    || (tip.len() >= 8 && tip.bytes().all(|b| b.is_ascii_hexdigit())))
                .then(|| tip.to_owned())
            }
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
            if let Some(index) = self
                .repository
                .ready()
                .and_then(|session| session.history.as_ref().ok())
                .and_then(|history| history.revisions.iter().position(|r| r.commit_id == commit))
            {
                self.history_scroll
                    .scroll_to_item(index, ScrollStrategy::Nearest);
            }
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
    pub(super) fn invalidate_history_reload(&mut self) {
        self.references.generation += 1;
        self.references.task = None;
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
        let branch = session.home.as_ref().clone();
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
        let selection_generation = self.requests.comparison();
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
                        if let Some((commit, true)) = target
                            && viewer.requests.comparison() == selection_generation
                        {
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
        let labels = self.references.labels;
        let viewer = cx.entity().downgrade();
        Popover::new("remote-history-sources")
            .trigger(
                Button::new("remote-history-toggle")
                    .ghost()
                    .small()
                    .compact()
                    .icon(IconName::ListFilter)
                    .tooltip("Choose remote history sources"),
            )
            .content(move |_, _, _| {
                let mut content = div()
                    .v_flex()
                    .gap_1()
                    .p_2()
                    .w(px(320.))
                    .max_h(px(450.))
                    .id("remote-sources-list")
                    .overflow_y_scroll()
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
                let toggle_viewer = viewer.clone();
                content = content.child(
                    Button::new("toggle-reference-labels")
                        .ghost()
                        .small()
                        .label(if labels {
                            "Hide reference labels"
                        } else {
                            "Show reference labels"
                        })
                        .on_click(move |_, _, cx| {
                            let _ = toggle_viewer
                                .update(cx, |viewer, cx| viewer.toggle_reference_labels(cx));
                        }),
                );
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

impl Viewer {
    pub(super) fn toggle_reference_labels(&mut self, cx: &mut Context<Self>) {
        self.references.labels = !self.references.labels;
        self.save_layout(cx);
        cx.notify();
    }
    pub(super) fn show_references(&mut self, cx: &mut Context<Self>) {
        self.layout.saved.history_hidden = false;
        self.rebuild_sidebar_layout(cx);
        self.references.open = true;
        self.save_layout(cx);
        cx.notify();
    }
    pub(super) fn reference_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(history) = self
            .repository
            .ready()
            .and_then(|s| s.history.as_ref().ok())
        else {
            return div().into_any_element();
        };
        let snapshot = history.references.clone();
        let viewer = cx.entity().downgrade();
        let visibility = self.references.visibility.clone();
        let inspected = self
            .references
            .inspected
            .clone()
            .filter(|r| Some(&r.target) == self.selected_history_commit().as_ref());
        Popover::new("all-reference-details")
            .trigger(
                Button::new("references-header")
                    .ghost()
                    .small()
                    .compact()
                    .icon(IconName::Tags)
                    .tooltip("Browse local and remote references"),
            )
            .open(self.references.open)
            .on_open_change(cx.listener(|viewer, open, _, cx| {
                viewer.references.open = *open;
                cx.notify();
            }))
            .content(move |_, window, cx| {
                reference_content(
                    &snapshot,
                    None,
                    &visibility,
                    inspected.as_ref(),
                    viewer.clone(),
                    window,
                    cx,
                )
            })
            .into_any_element()
    }
}

fn reference_content(
    snapshot: &std::sync::Arc<jaywalk::references::ReferenceSnapshot>,
    target: Option<&str>,
    visibility: &RemoteVisibility,
    inspected: Option<&jaywalk::history::Reference>,
    viewer: WeakEntity<Viewer>,
    window: &mut Window,
    cx: &mut Context<gpui::component::popover::PopoverState>,
) -> AnyElement {
    let popup = cx.entity().downgrade();
    let state = window.use_keyed_state("reference-browser-state", cx, |window, cx| {
        super::reference_browser::ReferenceBrowser::new(viewer, popup.clone(), window, cx)
    });
    state.update(cx, |state, cx| {
        state.configure(snapshot.clone(), target, visibility, inspected, popup, cx)
    });
    state.into_any_element()
}

pub(super) fn reference_row(
    reference: &jaywalk::history::Reference,
    inspected: Option<&jaywalk::history::Reference>,
    viewer: WeakEntity<Viewer>,
    popup: WeakEntity<gpui::component::popover::PopoverState>,
) -> AnyElement {
    let target = reference.target.clone();
    let label = reference.display_name();
    let tracking = match &reference.kind {
        jaywalk::history::ReferenceKind::Local => "local".to_owned(),
        jaywalk::history::ReferenceKind::Remote {
            tracking: Some(jaywalk::history::TrackingState::Untracked),
            ..
        } => "untracked".into(),
        jaywalk::history::ReferenceKind::Remote { .. } if !reference.tracked_by.is_empty() => {
            format!("tracked by {}", reference.tracked_by.join(", "))
        }
        _ => "tracking unknown".into(),
    };
    let detail = format!(
        "{label}\n{target}\n{tracking}{}",
        if reference.conflicted {
            " · conflicted target"
        } else {
            ""
        }
    );
    let navigation_viewer = viewer.clone();
    let selected = inspected == Some(reference);
    let reference = reference.clone();
    let copy = label.clone();
    let copy_id = format!("copy:{copy}:{target}");
    let review_name = jaywalk::branch_review::RefName::from_reference(&reference);
    let menu_owner = viewer.clone();
    let menu_popup = popup.clone();
    div()
        .id(SharedString::from(format!(
            "review-reference:{label}:{target}"
        )))
        .h_flex()
        .items_center()
        .gap_1()
        .context_menu(move |menu, _, _| {
            let owner = menu_owner.clone();
            let reference = review_name.clone();
            let popup = menu_popup.clone();
            menu.item(
                PopupMenuItem::new("Review branch changes")
                    .action(Box::new(commands::ExecuteCommand {
                        command: jaywalk::commands::CommandId::ChooseReview,
                    }))
                    .on_click(move |_, window, cx| {
                        let _ = popup.update(cx, |popup, cx| popup.dismiss(window, cx));
                        let _ = owner.update(cx, |viewer, cx| {
                            viewer.review_reference(reference.clone(), window, cx)
                        });
                    }),
            )
        })
        .child(
            Button::new(SharedString::from(format!("open:{label}:{target}")))
                .ghost()
                .small()
                .selected(selected)
                .flex_1()
                .min_w_0()
                .label(label)
                .tooltip(detail)
                .on_click(move |_, window, cx| {
                    let _ = popup.update(cx, |popup, cx| popup.dismiss(window, cx));
                    let _ = navigation_viewer.update(cx, |viewer, cx| {
                        viewer.references.inspected = Some(reference.clone());
                        viewer.open_reference_target(target.clone(), true, window, cx);
                    });
                }),
        )
        .child(
            Button::new(SharedString::from(copy_id))
                .ghost()
                .small()
                .compact()
                .label("Copy")
                .tooltip("Copy qualified reference name")
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                }),
        )
        .into_any_element()
}

pub(super) fn reference_badge(
    history: &Rc<jaywalk::history::History>,
    index: usize,
    visibility: &RemoteVisibility,
    viewer: WeakEntity<Viewer>,
    badge_width: f32,
    font_size: f32,
    cx: &App,
) -> Option<AnyElement> {
    let revision = &history.revisions[index];
    let visible: Vec<_> = revision
        .references
        .iter()
        .filter(|r| r.remote().is_none_or(|remote| visibility.includes(remote)))
        .collect();
    let label = jaywalk::references::compact_label(&visible)?;
    let summary = visible
        .iter()
        .map(|r| {
            format!(
                "{}: {}{}",
                if r.remote().is_some() {
                    "Remote"
                } else {
                    "Local"
                },
                r.display_name(),
                if r.conflicted {
                    " · conflicted target"
                } else {
                    ""
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let theme = gpui::component::Theme::global(cx);
    let (icon, tint) = match label.kind {
        ReferenceBadgeKind::Remote => (IconName::Cloud, theme.success),
        ReferenceBadgeKind::Local | ReferenceBadgeKind::Shared => (IconName::GitBranch, theme.info),
    };
    let shared = label.kind == ReferenceBadgeKind::Shared;
    let icon_size = px(font_size * 0.75);
    // At the narrowest configured widths, combine the two scope glyphs and
    // reserve the remaining space for the count instead of overflowing the tag.
    let stacked = shared && badge_width < font_size * 4.;
    let leading_icon = div()
        .relative()
        .size(icon_size)
        .flex_shrink_0()
        .child(gpui::component::Icon::new(icon).size(icon_size))
        .when(stacked, |icon| {
            icon.child(
                gpui::component::Icon::new(IconName::Cloud)
                    .size(icon_size * 0.65)
                    .absolute()
                    .right_0()
                    .bottom_0(),
            )
        });
    let target = revision.commit_id.clone();
    let snapshot = history.references.clone();
    let visibility = visibility.clone();
    Some(
        Popover::new(SharedString::from(format!("references:{target}")))
            .trigger(
                Button::new(SharedString::from(format!("reference-label:{target}")))
                    .ghost()
                    .small()
                    .compact()
                    .max_w(px(badge_width))
                    .flex_shrink_0()
                    .p_0()
                    .mr_1()
                    .child(
                        Tag::custom(tint.opacity(0.14), tint, tint.opacity(0.5))
                            .small()
                            .max_w(px(badge_width))
                            .min_w_0()
                            .px_1()
                            .py_0p5()
                            .text_size(px(font_size * 0.75))
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_0p5()
                                    .min_w_0()
                                    .child(leading_icon)
                                    .when(!stacked, |row| {
                                        row.child(div().min_w_0().truncate().child(label.name))
                                    })
                                    .when(shared && !stacked, |row| {
                                        row.child(
                                            gpui::component::Icon::new(IconName::Cloud)
                                                .size(icon_size)
                                                .flex_shrink_0(),
                                        )
                                    })
                                    .when(label.additional > 0, |row| {
                                        row.child(
                                            div()
                                                .flex_shrink_0()
                                                .child(format!("+{}", label.additional)),
                                        )
                                    }),
                            ),
                    )
                    .tooltip(summary),
            )
            .content(move |_, window, cx| {
                reference_content(
                    &snapshot,
                    Some(&target),
                    &visibility,
                    None,
                    viewer.clone(),
                    window,
                    cx,
                )
            })
            .into_any_element(),
    )
}
