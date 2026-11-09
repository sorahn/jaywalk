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
                    for (from, to) in &graph.edges {
                        let mut path = PathBuilder::stroke(px(1.5));
                        let start = if *from == graph.lane {
                            middle
                        } else {
                            bounds.top()
                        };
                        path.move_to(point(x(*from), start));
                        path.cubic_bezier_to(
                            point(x(*to), bounds.bottom()),
                            point(x(*from), middle),
                            point(x(*to), middle),
                        );
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(palette[*from % palette.len()]));
                        }
                    }
                    if graph.incoming {
                        let mut path = PathBuilder::stroke(px(1.5));
                        path.move_to(point(x(graph.lane), bounds.top()));
                        path.line_to(point(x(graph.lane), middle));
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(palette[graph.lane % palette.len()]));
                        }
                    }
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                point(x(graph.lane) - px(NODE_RADIUS), middle - px(NODE_RADIUS)),
                                size(px(NODE_RADIUS * 2.), px(NODE_RADIUS * 2.)),
                            ),
                            rgb(palette[graph.lane % palette.len()]),
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
                .child(self.remote_controls(cx))
                .child(self.panel_menu(jaywalk::panel_layout::PanelId::History))
                .child(
                    Button::new("toggle-evolution")
                        .ghost()
                        .small()
                        .compact()
                        .label("Evolution")
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
                        .child(div().p_3().child(error.clone()))
                        .into_any_element();
                }
            };
        let history = match &session.history {
            Ok(history) => history.clone(),
            Err(error) => {
                return panel
                    .child(div().p_3().child(error.clone()))
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
                            surface,
                            selection,
                            hover,
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
