use super::*;
use gpui::component::Sizable;
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
}

#[derive(Clone, Copy)]
struct RowColors {
    surface: Hsla,
    selection: Hsla,
    hover: Hsla,
}

fn history_row(
    history: Rc<History>,
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
    } = metrics;
    let revision = &history.revisions[index];
    let commit_id = revision.commit_id.clone();
    let description = revision.summary();
    div()
        .id(SharedString::from(format!("revision:{commit_id}")))
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
                move |bounds, _, window, _| {
                    let graph = &history.graph[index];
                    let x = |lane| bounds.left() + px(GRAPH_MARGIN + lane as f32 * spacing);
                    let middle = bounds.center().y;
                    let palette = [0x5b9bd5, 0x7dc67c, 0xd9a066, 0xb18add, 0x66c4c7];
                    for stroke in &graph.strokes {
                        paint_graph_stroke(
                            stroke,
                            bounds,
                            spacing,
                            rgb(palette[stroke.color % palette.len()]).into(),
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
                            rgb(palette[graph.color % palette.len()]),
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
                .pr_2()
                .text_size(px(font_size))
                .truncate()
                .child(description),
        )
        .children(badge)
        .on_click(move |_, window, cx| {
            let _ = viewer.update(cx, |viewer, cx| {
                viewer.select_revision(&commit_id, window, cx)
            });
        })
        .into_any_element()
}

impl Viewer {
    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = gpui::component::Theme::global(cx);
        let surface = theme.sidebar;
        let stripe = surface.blend(
            theme
                .foreground
                .opacity(self.settings.history.stripe_opacity),
        );
        let selection = theme.list_active;
        let hover = theme.secondary_hover;
        let mut panel = div().v_flex().size_full().bg(surface).child(
            self.panel_header(self.settings.panels.history.font.size, cx)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(self.settings.panels.history.font.size))
                        .child(if self.supports_evolution() {
                            "JJ HISTORY"
                        } else {
                            "GIT HISTORY"
                        }),
                )
                .child(self.reference_controls(cx))
                .child(self.remote_controls(cx))
                .child(self.panel_menu(jaywalk::panel_layout::PanelId::History))
                .child(
                    Button::new("toggle-evolution")
                        .ghost()
                        .small()
                        .compact()
                        .icon(IconName::GitCompareArrows)
                        .when(!self.supports_evolution(), |button| button.hidden())
                        .selected(self.evolution.visible)
                        .tooltip("Show change evolution")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_evolution(window, cx)),
                        ),
                ),
        );
        let session =
            match &self.repository {
                LoadState::Loaded(session) => session,
                LoadState::Loading => {
                    return panel
                        .child(div().p_3().child(
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
                                .p_3()
                                .child(feedback::error_message("history-error", error)),
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
                            .p_3()
                            .child(feedback::error_message("history-error", error)),
                    )
                    .into_any_element();
            }
        };
        let selected_commit = match &session.scope {
            ComparisonScope::WholeBranch => None,
            ComparisonScope::Revision { commit_id, .. } => Some(commit_id.clone()),
            ComparisonScope::Evolution { anchor, .. } => Some(anchor.clone()),
        };
        let count = history.revisions.len();
        let status = if let Some(error) = &self.references.error {
            error.clone()
        } else if self
            .selected_history_commit()
            .is_some_and(|commit| !history.revisions.iter().any(|r| r.commit_id == commit))
        {
            format!("{count} revisions · inspected revision outside graph")
        } else {
            format!("{count} recorded revisions")
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
        let rows = uniform_list("history-rows", count, move |visible, _, _| {
            let width = GRAPH_MARGIN * 2.
                + (history.visible_lane_count(visible.clone()) - 1) as f32 * spacing;
            visible
                .map(|index| {
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
                        if labels {
                            super::references::reference_badge(
                                &history,
                                index,
                                &visibility,
                                viewer.clone(),
                                badge_width,
                            )
                        } else {
                            None
                        },
                    )
                })
                .collect()
        })
        .size_full()
        .track_scroll(&self.history_scroll);
        panel = panel.child(div().flex_1().min_h_0().child(rows)).child(
            div()
                .px_3()
                .py_1()
                .text_size(px(self.settings.panels.history.font.size * 0.85))
                .text_color(rgb(0x9baab4))
                .truncate()
                .child(status),
        );
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
