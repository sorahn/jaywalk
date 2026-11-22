use jaywalk::theme::{Part, Surface};
mod paging;
use super::*;
use gpui::component::Sizable;
use gpui::component::menu::ContextMenuExt as _;
use gpui::component::tooltip::{ManagedTooltipExt as _, Tooltip};
use gpui::prelude::FluentBuilder;
use jaywalk::history::History;

const GRAPH_MARGIN: f32 = 10.;
const NODE_RADIUS: f32 = 3.5;

#[derive(Clone, Copy)]
struct RowMetrics {
    width: f32,
    height: f32,
    spacing: f32,
    font_size: f32,
    edge: f32,
}

#[derive(Clone, Copy)]
struct RowColors {
    surface: Hsla,
    selection: Hsla,
    hover: Hsla,
}

fn history_row(
    history: Arc<History>,
    index: usize,
    metrics: RowMetrics,
    selected: bool,
    viewer: WeakEntity<Viewer>,
    colors: RowColors,
    badge: Option<AnyElement>,
) -> AnyElement {
    let RowColors {
        surface,
        selection,
        hover,
    } = colors;
    let RowMetrics {
        width,
        height,
        spacing,
        font_size,
        edge,
    } = metrics;
    let revision = &history.revisions[index];
    let commit_id = revision.commit_id.clone();
    let description = revision.summary();
    let target = history_menu::RevisionTarget::from_revision(revision);
    let menu_owner = viewer.clone();
    let hover_history = history.clone();
    div()
        .id(SharedString::from(format!("revision:{commit_id}")))
        .debug_selector(move || format!("history-row-{index}"))
        .h(px(height))
        .w_full()
        .h_flex()
        .items_center()
        .cursor_pointer()
        .bg(if selected { selection } else { surface })
        .hover(move |style| style.bg(hover))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    let graph = &history.graph[index];
                    let x = |lane| bounds.left() + px(GRAPH_MARGIN + lane as f32 * spacing);
                    let middle = bounds.center().y;
                    let palette = std::array::from_fn::<_, 5, _>(|i| {
                        crate::appearance::color(jaywalk::theme::Role::GraphLane(i as u8), cx)
                    });
                    for stroke in &graph.strokes {
                        paint_graph_stroke(
                            stroke,
                            bounds,
                            spacing,
                            palette[stroke.color % palette.len()],
                            1.5,
                            window,
                        );
                    }
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                point(x(graph.lane) - px(NODE_RADIUS), middle - px(NODE_RADIUS)),
                                size(px(NODE_RADIUS * 2.), px(NODE_RADIUS * 2.)),
                            ),
                            palette[graph.color % palette.len()],
                        )
                        .corner_radii(px(NODE_RADIUS)),
                    );
                },
            )
            .w(px(width))
            .h_full()
            .flex_shrink_0(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .pr(px(edge))
                .text_size(px(font_size))
                .truncate()
                .child(description),
        )
        .children(badge)
        .managed_tooltip(move |window, cx| {
            let history = hover_history.clone();
            Tooltip::element(move |_, cx| revision_hover(&history.revisions[index], cx))
                .max_w(rems(36.))
                .build(window, cx)
        })
        .on_click(move |event, window, cx| {
            let compare = if cfg!(target_os = "macos") {
                event.modifiers().platform
            } else {
                event.modifiers().control
            };
            let _ = viewer.update(cx, |viewer, cx| {
                if compare {
                    viewer.compare_revision(&commit_id, window, cx);
                } else {
                    viewer.select_revision(&commit_id, window, cx);
                }
            });
        })
        .context_menu(move |menu, _, cx| {
            let Some(owner) = menu_owner.upgrade() else {
                return menu;
            };
            let viewer = owner.read(cx);
            history_menu::revision_menu(
                menu.action_context(viewer.menu_focus()),
                target.clone(),
                menu_owner.clone(),
                viewer.supports_evolution(),
                viewer.is_hosted_review(),
            )
        })
        .into_any_element()
}

/// Uses the loaded history snapshot; hovering never queries or snapshots a repository.
fn revision_hover(revision: &jaywalk::history::Revision, cx: &App) -> AnyElement {
    let muted = crate::appearance::surface(Surface::Floating, Part::MutedForeground, cx);
    let mut details = Vec::new();
    if revision.is_virtual_working_copy() {
        details.push("Uncommitted working changes against HEAD".to_owned());
    } else {
        if !revision.author.is_empty() {
            details.push(format!("Author: {}", revision.author));
        }
        if !revision.timestamp.is_empty() {
            details.push(format!("Authored: {}", revision.timestamp));
        }
        details.push(format!("Commit: {}", revision.commit_id));
        if !revision.change_id.is_empty() && revision.change_id != revision.commit_id {
            details.push(format!("Change: {}", revision.change_id));
        }
        if revision.working_copy {
            details.push("Current working-copy revision".into());
        }
        if revision.parents.len() > 1 {
            details.push(format!("Merge: {} parents", revision.parents.len()));
        }
        details.extend(revision.references.iter().map(|reference| {
            format!(
                "{}: {}{}",
                if reference.remote().is_some() {
                    "Remote"
                } else {
                    "Local"
                },
                reference.display_name(),
                if reference.conflicted {
                    " (conflicted)"
                } else {
                    ""
                }
            )
        }));
    }
    let message = if revision.description.is_empty() {
        revision.summary()
    } else {
        revision.description.trim_end().to_owned()
    };
    div()
        .debug_selector(|| "commit-hover".into())
        .text_left()
        .v_flex()
        .gap(crate::spacing::edge(cx))
        .min_w_0()
        .max_w(rems(34.))
        .child(div().max_h(rems(16.)).overflow_hidden().child(message))
        .child(
            div()
                .v_flex()
                .gap(crate::spacing::interactive(cx))
                .text_xs()
                .text_color(muted)
                .children(details.into_iter().map(|line| div().child(line))),
        )
        .into_any_element()
}

impl Viewer {
    pub(super) fn history_footer_status(&self) -> String {
        if self.references.loading_more {
            return "Loading history…".into();
        }
        if let Some(error) = &self.references.error {
            error.clone()
        } else if let Some(history) = self
            .repository
            .ready()
            .and_then(|session| session.history.as_ref().ok())
        {
            let virtual_working_copy = history
                .revisions
                .iter()
                .any(|revision| revision.is_virtual_working_copy());
            let count = history
                .revisions
                .iter()
                .filter(|revision| !revision.is_virtual_working_copy())
                .count();
            let mut status = if virtual_working_copy {
                format!("{count} commits loaded · working copy")
            } else {
                format!("{count} recorded revisions loaded")
            };
            if history.has_more() {
                status.push_str(" · more available");
            }
            if history
                .source()
                .is_some_and(|source| source.context().is_some())
            {
                status.push_str(" · selected history context");
            }
            if self.selected_history_commit().is_some_and(|commit| {
                !history
                    .revisions
                    .iter()
                    .any(|revision| revision.commit_id == commit)
            }) {
                status.push_str(" · inspected revision outside graph");
            }
            status
        } else {
            String::new()
        }
    }

    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = gpui::component::Theme::global(cx);
        let surface = crate::appearance::surface(Surface::History, Part::Background, cx);
        let stripe = surface.blend(
            theme
                .foreground
                .opacity(self.settings.history.stripe_opacity),
        );
        let accent = theme.primary;
        let selection = theme.list_active;
        let hover = theme.secondary_hover;
        let mut panel = super::chrome::Panel::new(
            "history-panel",
            self.panel_header(
                jaywalk::panel_layout::PanelId::History,
                self.settings.panels.history.font.size,
                cx,
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(if self.supports_evolution() {
                        "JJ HISTORY"
                    } else {
                        "GIT HISTORY"
                    }),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(crate::spacing::interactive(cx))
                    .child(self.panel_menu(jaywalk::panel_layout::PanelId::History))
                    .child(
                        Button::new("search-history")
                            .ghost()
                            .small()
                            .compact()
                            .icon(IconName::Search)
                            .command_tooltip(
                                "Find in history",
                                jaywalk::commands::CommandId::SearchHistory,
                            )
                            .on_click(cx.listener(|viewer, _, window, cx| {
                                viewer.open_palette(palette::PaletteKind::History, window, cx)
                            })),
                    )
                    .child(self.reference_controls(cx))
                    .child(self.remote_controls(cx))
                    .when(
                        self.repository
                            .ready()
                            .and_then(|s| s.history.as_ref().ok())
                            .and_then(|h| h.source())
                            .is_some_and(|source| source.context().is_some()),
                        |row| {
                            row.child(
                                Button::new("latest-history")
                                    .ghost()
                                    .small()
                                    .compact()
                                    .icon(IconName::ArrowUp)
                                    .command_tooltip(
                                        "Show latest history",
                                        jaywalk::commands::CommandId::LatestHistory,
                                    )
                                    .on_click(cx.listener(|viewer, _, window, cx| {
                                        viewer.latest_history(window, cx)
                                    })),
                            )
                        },
                    ),
            ),
            div().bg(surface).text_color(crate::appearance::surface(
                Surface::History,
                Part::Foreground,
                cx,
            )),
        );
        let session = match &self.repository {
            LoadState::Loaded(session) => session,
            LoadState::Loading => {
                return panel
                    .child(div().p(crate::spacing::edge(cx)).child(
                        if self.loading_visible(LoadingKind::Comparison) {
                            "Loading repository…"
                        } else {
                            ""
                        },
                    ))
                    .into_any_element();
            }
            LoadState::Failed(error) => {
                return panel
                    .child(
                        div()
                            .p(crate::spacing::edge(cx))
                            .child(feedback::error_message("history-error", error, cx)),
                    )
                    .into_any_element();
            }
        };
        let history = match &session.history {
            Ok(history) => history.clone(),
            Err(error) => {
                return panel
                    .child(
                        div()
                            .p(crate::spacing::edge(cx))
                            .child(feedback::error_message("history-error", error, cx)),
                    )
                    .into_any_element();
            }
        };
        let selected_commit = match &session.scope {
            ComparisonScope::Aggregate => None,
            ComparisonScope::Revision { commit_id, .. } => Some(commit_id.clone()),
            ComparisonScope::Evolution { anchor, .. } => Some(anchor.clone()),
        };
        let commit_count = history.revisions.len();
        let count = commit_count + usize::from(history.has_more());
        let automatic = self.settings.history.page_on_scroll && self.references.error.is_none();
        let loading = self.references.loading_more;
        let load_label = if loading {
            "Loading older commits…".to_owned()
        } else if self.references.error.is_some() {
            "Retry loading older commits".to_owned()
        } else {
            format!("Load {} older commits", self.next_history_page_size())
        };
        let viewer = cx.entity().downgrade();
        let height = self.settings.row_height(
            self.settings.history.row_height,
            self.settings.panels.history.font.size,
        );
        let spacing = self.settings.history.lane_spacing;
        let font_size = self.settings.panels.history.font.size;
        let labels = self.references.labels;
        let badge_width = self.settings.history.reference_badge_width;
        let visibility = self.references.visibility.clone();
        let endpoints = match &session.review.source {
            review_context::ReviewSource::Target(jaywalk::branch_review::Target::Pair {
                left,
                right,
            }) => Some((left.clone(), right.clone())),
            _ => None,
        };
        let row_edge = self.settings.ui.spacing.edge_px();
        let rows = uniform_list("history-rows", count, move |visible, window, cx| {
            if automatic && history.has_more() && visible.end >= commit_count && !loading {
                let owner = viewer.clone();
                let snapshot = history.clone();
                window.defer(cx, move |window, cx| {
                    let _ = owner.update(cx, |viewer, cx| {
                        if viewer.settings.history.page_on_scroll
                            && viewer.references.error.is_none()
                            && viewer
                                .repository
                                .ready()
                                .and_then(|s| s.history.as_ref().ok())
                                .is_some_and(|h| Arc::ptr_eq(h, &snapshot))
                        {
                            viewer.load_more_history(window, cx);
                        }
                    });
                });
            }
            let width = GRAPH_MARGIN * 2.
                + (history.visible_lane_count(visible.clone()) - 1) as f32 * spacing;
            visible
                .map(|index| {
                    if index == commit_count {
                        let owner = viewer.clone();
                        return div()
                            .debug_selector(|| "history-load-more".into())
                            .h(px(height))
                            .w_full()
                            .child(
                                Button::new("load-older-history")
                                    .ghost()
                                    .small()
                                    .w_full()
                                    .label(load_label.clone())
                                    .disabled(loading)
                                    .on_click(move |_, window, cx| {
                                        let _ = owner
                                            .update(cx, |v, cx| v.load_more_history(window, cx));
                                    }),
                            )
                            .into_any_element();
                    }
                    let selected =
                        selected_commit.as_ref() == Some(&history.revisions[index].commit_id);
                    history_row(
                        history.clone(),
                        index,
                        RowMetrics {
                            width,
                            height,
                            spacing,
                            font_size,
                            edge: row_edge,
                        },
                        selected,
                        viewer.clone(),
                        RowColors {
                            surface: if index.is_multiple_of(2) {
                                surface
                            } else {
                                stripe
                            },
                            selection,
                            hover,
                        },
                        Some(
                            div()
                                .h_flex()
                                .items_center()
                                .gap(crate::spacing::interactive(cx))
                                .children(endpoints.as_ref().and_then(|(left, right)| {
                                    let id = &history.revisions[index].commit_id;
                                    let marker = match (id == left, id == right) {
                                        (true, true) => "L/R",
                                        (true, false) => "L",
                                        (false, true) => "R",
                                        _ => return None,
                                    };
                                    Some(
                                        div()
                                            .text_xs()
                                            .text_color(accent)
                                            .px(crate::spacing::interactive(cx))
                                            .child(marker),
                                    )
                                }))
                                .children(if labels {
                                    super::references::reference_badge(
                                        &history,
                                        index,
                                        &visibility,
                                        viewer.clone(),
                                        badge_width,
                                        font_size,
                                        cx,
                                    )
                                } else {
                                    None
                                })
                                .into_any_element(),
                        ),
                    )
                })
                .collect()
        })
        .size_full()
        .track_scroll(&self.history_scroll);
        panel = panel.child(div().relative().flex_1().min_h_0().child(rows).child(
            gpui::component::scroll::Scrollbar::vertical(&self.history_scroll),
        ));
        panel.into_any_element()
    }
}

/// Paint one half-row route; the list clips the completed row without leaving
/// curves dependent on the next virtualized row being painted.
pub(super) fn paint_graph_stroke(
    stroke: &jaywalk::graph::GraphStroke,
    bounds: Bounds<Pixels>,
    spacing: f32,
    color: Hsla,
    width: f32,
    window: &mut Window,
) {
    let height = f32::from(bounds.size.height);
    let incoming = stroke.half == jaywalk::graph::GraphHalf::Incoming;
    let offset = if incoming { -height / 2. } else { height / 2. };
    let start = [GRAPH_MARGIN + stroke.from as f32 * spacing, 0.];
    let end = [GRAPH_MARGIN + stroke.to as f32 * spacing, height];
    let screen = |p: [f32; 2]| point(bounds.left() + px(p[0]), bounds.top() + px(p[1] + offset));
    let mut path = PathBuilder::stroke(px(width));
    if stroke.from == stroke.to {
        let y = if incoming { height / 2. } else { 0. };
        path.move_to(screen([start[0], y]));
        path.line_to(screen([end[0], y + height / 2.]));
    } else {
        let curves = jaywalk::graph::branch_curve(start, end);
        let half = if incoming { &curves[3..] } else { &curves[..3] };
        path.move_to(screen(half[0].start));
        for curve in half {
            path.cubic_bezier_to(
                screen(curve.end),
                screen(curve.control1),
                screen(curve.control2),
            );
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
