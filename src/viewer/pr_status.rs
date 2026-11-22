//! Mutable provider observations are separate from immutable review contents.
use super::*;
use gpui::component::Sizable;
use gpui::prelude::FluentBuilder;
use jaywalk::{
    accounts::Control,
    hosting::status::{Availability, Decision, Freshness, Snapshot},
    repository::ComparisonIdentity,
};
use std::{collections::VecDeque, sync::Arc, time::Duration};
#[derive(Default)]
pub(super) struct StatusState {
    key: Option<ComparisonIdentity>,
    pub(super) snapshot: Option<Arc<Snapshot>>,
    cache: VecDeque<(ComparisonIdentity, Arc<Snapshot>)>,
    pub(super) control: Option<Control>,
    task: Option<Task<()>>,
    error: Option<String>,
    expanded: bool,
}
impl StatusState {
    fn cancel(&mut self) {
        if let Some(control) = self.control.take() {
            control.cancel();
        }
        self.task = None;
    }
}
impl Drop for StatusState {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Viewer {
    pub(super) fn sync_pr_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self
            .repository
            .ready()
            .filter(|_| self.page == pages::Page::Comparison)
            .and_then(|s| {
                s.review
                    .aggregate
                    .hosted()
                    .map(|_| s.review.aggregate.identity())
            });
        if key == self.pr_status.key {
            return;
        }
        self.pr_status.cancel();
        self.pr_status.key = None;
        self.pr_status.snapshot = None;
        self.pr_status.error = None;
        self.pr_status.expanded = false;
        if key.is_some() {
            self.refresh_pr_status(false, window, cx);
        }
    }
    pub(super) fn refresh_pr_status(
        &mut self,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(comparison) = self.repository.ready().map(|s| s.review.aggregate.clone()) else {
            return;
        };
        let Some(contents) = comparison.hosted().cloned() else {
            return;
        };
        let key = comparison.identity();
        self.pr_status.cancel();
        self.pr_status.key = Some(key.clone());
        self.pr_status.error = None;
        let cached = self
            .pr_status
            .cache
            .iter()
            .rev()
            .find(|(id, _)| id == &key)
            .map(|(_, snapshot)| snapshot.clone());
        self.pr_status.snapshot = cached.clone();
        if !force
            && cached.is_some_and(|s| {
                s.fetched_at
                    .elapsed()
                    .is_ok_and(|age| age < Duration::from_secs(60))
            })
        {
            cx.notify();
            return;
        }
        let control = Control::new(Duration::from_secs(90));
        self.pr_status.control = Some(control.clone());
        self.pr_status.task = Some(cx.spawn_in(window, async move |this, cx| {
            let expected = control.clone();
            let result = cx
                .background_spawn(async move { contents.status(&control) })
                .await;
            let _ = this.update(cx, |viewer, cx| {
                if matches!(expected.check(), Err(jaywalk::accounts::Error::Cancelled))
                    || viewer.pr_status.key.as_ref() != Some(&key)
                {
                    return;
                }
                viewer.pr_status.control = None;
                match result {
                    Ok(snapshot) => {
                        let snapshot = Arc::new(snapshot);
                        viewer.pr_status.cache.retain(|(id, _)| id != &key);
                        viewer.pr_status.cache.push_back((key, snapshot.clone()));
                        while viewer.pr_status.cache.len() > 16 {
                            viewer.pr_status.cache.pop_front();
                        }
                        viewer.pr_status.snapshot = Some(snapshot);
                        viewer.pr_status.error = None;
                    }
                    Err(error) => {
                        viewer.pr_status.error = Some(format!(
                            "Status refresh failed: {}. Previous observations retained.",
                            jaywalk::hosting::status::Issue::Provider(error).label()
                        ))
                    }
                }
                viewer.remeasure_overview_header(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }
}
#[derive(Clone, Default)]
pub(super) struct Presentation {
    snapshot: Option<Arc<Snapshot>>,
    error: Option<String>,
    loading: bool,
    expanded: bool,
}
impl StatusState {
    pub(super) fn presentation(&self, key: Option<&ComparisonIdentity>) -> Presentation {
        if key != self.key.as_ref() {
            return Presentation {
                loading: true,
                ..Default::default()
            };
        }
        Presentation {
            snapshot: self.snapshot.clone(),
            error: self.error.clone(),
            loading: self.control.is_some(),
            expanded: self.expanded,
        }
    }
}
pub(super) fn render(
    presentation: &Presentation,
    viewer: WeakEntity<Viewer>,
    cx: &App,
) -> AnyElement {
    let muted = gpui::component::Theme::global(cx).muted_foreground;
    let loading = presentation.loading;
    let expanded = presentation.expanded;
    let toggle = viewer.clone();
    let refresh = viewer.clone();
    let mut panel = div()
        .v_flex()
        .gap(crate::spacing::interactive(cx))
        .text_sm()
        .py(crate::spacing::interactive(cx))
        .debug_selector(|| "pr-status".into());
    panel = panel.child(
        div()
            .h_flex()
            .items_center()
            .gap(crate::spacing::edge(cx))
            .child(
                Button::new("pr-status-details")
                    .ghost()
                    .small()
                    .compact()
                    .label(if expanded {
                        "Hide status details"
                    } else {
                        "PR status"
                    })
                    .icon(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .on_click(move |_, _, cx| {
                        let _ = toggle.update(cx, |viewer, cx| {
                            viewer.pr_status.expanded = !viewer.pr_status.expanded;
                            viewer.remeasure_overview_header(cx);
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("pr-status-refresh")
                    .ghost()
                    .small()
                    .compact()
                    .icon(IconName::RefreshCw)
                    .disabled(loading)
                    .command_tooltip(
                        "Refresh PR status without changing the diff",
                        jaywalk::commands::CommandId::RefreshReviewStatus,
                    )
                    .on_click(move |_, window, cx| {
                        let _ = refresh
                            .update(cx, |viewer, cx| viewer.refresh_pr_status(true, window, cx));
                    }),
            ),
    );
    if loading {
        panel = panel.child(
            div()
                .h_flex()
                .gap(crate::spacing::edge(cx))
                .text_color(muted)
                .child(gpui::component::spinner::Spinner::new())
                .child("Loading PR status…"),
        );
    }
    if let Some(error) = &presentation.error {
        panel = panel.child(
            div()
                .text_color(muted)
                .whitespace_normal()
                .child(error.clone()),
        );
    }
    let Some(snapshot) = &presentation.snapshot else {
        return panel
            .when(!loading && presentation.error.is_none(), |panel| {
                panel.child(div().text_color(muted).child("Status has not been loaded"))
            })
            .into_any_element();
    };
    let checked = snapshot
        .fetched_at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let seconds = d.as_secs() % 86400;
            format!("{:02}:{:02} UTC", seconds / 3600, (seconds % 3600) / 60)
        })
        .unwrap_or_else(|_| "time unavailable".into());
    panel = panel.child(div().text_xs().text_color(muted).child(format!(
        "CI for PR snapshot head {} · status snapshot checked {checked}",
        &snapshot.head[..12]
    )));
    match snapshot.freshness {
            Freshness::Current=>(),
            Freshness::NewVersion=>panel=panel.child(div().text_color(muted).whitespace_normal().child("The PR has a newer source or target. This diff stays pinned; update the published comparison to inspect it.")),
            Freshness::Unavailable(_)=>panel=panel.child(div().text_color(muted).child("Could not verify the PR's current version")),
        }
    for (index, group) in snapshot.checks.iter().enumerate() {
        panel = panel.child(div().whitespace_normal().child(group.summary()));
        if expanded && let Availability::Known(list) = &group.checks {
            for (row, check) in list.items.iter().take(8).enumerate() {
                let label = format!("{} · {}", check.name, check.state.label());
                panel = panel.child(if let Some(url) = &check.url {
                    let url = url.to_string();
                    Button::new(SharedString::from(format!("pr-check-{index}-{row}")))
                        .ghost()
                        .small()
                        .compact()
                        .label(label)
                        .tooltip("Open provider check details in browser")
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .into_any_element()
                } else {
                    div()
                        .pl(crate::spacing::edge(cx))
                        .text_color(muted)
                        .whitespace_normal()
                        .child(label)
                        .into_any_element()
                });
            }
            if list.items.len() > 8 {
                panel = panel.child(div().pl(crate::spacing::edge(cx)).text_color(muted).child(format!(
                    "Showing 8 of {} observations; open the PR in your browser for the full list",
                    list.items.len()
                )));
            }
        }
    }
    panel = panel.child(div().whitespace_normal().child(snapshot.review_summary()));
    if expanded && let Availability::Known(list) = &snapshot.reviews {
        for review in list.items.iter().take(12) {
            let earlier = review
                .commit
                .as_ref()
                .is_some_and(|commit| commit != &snapshot.head);
            let note = if earlier
                && matches!(
                    review.decision,
                    Decision::Approved | Decision::ChangesRequested
                ) {
                " · earlier commit; validity not inferred"
            } else {
                ""
            };
            panel = panel.child(
                div()
                    .pl(crate::spacing::edge(cx))
                    .text_color(muted)
                    .whitespace_normal()
                    .child(format!(
                        "{} · {}{}{}",
                        review.name,
                        review.decision.label(),
                        if review.requested {
                            " · requested reviewer"
                        } else {
                            ""
                        },
                        note
                    )),
            );
        }
        if list.items.len() > 12 {
            panel = panel.child(div().pl(crate::spacing::edge(cx)).text_color(muted).child(
                format!("Showing 12 of {} reviewer observations", list.items.len()),
            ));
        }
    }
    panel.into_any_element()
}
