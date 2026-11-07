use super::*;

const ROW_HEIGHT: f32 = 56.;

impl Viewer {
    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .v_flex()
            .size_full()
            .bg(rgb(0x242b30))
            .child(div().px_3().py_3().text_sm().child("JJ HISTORY"));
        let session = match &self.repository {
            LoadState::Loaded(session) => session,
            LoadState::Loading => {
                return panel
                    .child(div().p_3().child("Loading repository…"))
                    .into_any_element();
            }
            LoadState::Failed(error) => {
                return panel
                    .child(div().p_3().child(error.clone()))
                    .into_any_element();
            }
        };
        let history = match &session.history {
            Ok(history) => history,
            Err(error) => {
                return panel
                    .child(div().p_3().child(error.clone()))
                    .into_any_element();
            }
        };
        let mut rows = div()
            .id("history-rows")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        for (index, revision) in history.revisions.iter().enumerate() {
            let selected = matches!(&session.scope, ComparisonScope::Revision { commit_id, .. } if commit_id == &revision.commit_id);
            let commit_id = revision.commit_id.clone();
            let description = if revision.description.is_empty() {
                "(no description)"
            } else {
                &revision.description
            };
            let change = &revision.change_id[..revision.change_id.len().min(8)];
            let metadata = format!("{change}  ·  {}", revision.author);
            let graph = history.graph[index].clone();
            let width = history.lane_count as f32 * 12. + 16.;
            rows = rows.child(
                div()
                    .id(("revision", index))
                    .h(px(ROW_HEIGHT))
                    .h_flex()
                    .items_center()
                    .cursor_pointer()
                    .bg(rgb(if selected { 0x344956 } else { 0x242b30 }))
                    .hover(|style| style.bg(rgb(0x303e48)))
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let x = |lane| bounds.left() + px(10. + lane as f32 * 12.);
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
                                        window
                                            .paint_path(path, rgb(palette[*from % palette.len()]));
                                    }
                                }
                                if graph.incoming {
                                    let mut path = PathBuilder::stroke(px(1.5));
                                    path.move_to(point(x(graph.lane), bounds.top()));
                                    path.line_to(point(x(graph.lane), middle));
                                    if let Ok(path) = path.build() {
                                        window.paint_path(
                                            path,
                                            rgb(palette[graph.lane % palette.len()]),
                                        );
                                    }
                                }
                                window.paint_quad(
                                    fill(
                                        Bounds::new(
                                            point(x(graph.lane) - px(3.5), middle - px(3.5)),
                                            size(px(7.), px(7.)),
                                        ),
                                        rgb(palette[graph.lane % palette.len()]),
                                    )
                                    .corner_radii(px(3.5)),
                                );
                            },
                        )
                        .w(px(width))
                        .h_full()
                        .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .pr_2()
                            .text_sm()
                            .child(div().truncate().child(description.to_owned()))
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(rgb(0x9baab4))
                                    .child(metadata),
                            )
                            .children((!revision.bookmarks.is_empty()).then(|| {
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(rgb(0x8ac8a5))
                                    .child(revision.bookmarks.join(", "))
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_revision(&commit_id, window, cx)
                    })),
            );
        }
        panel = panel.child(rows).child(
            div()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(rgb(0x9baab4))
                .child(format!("{} recorded revisions", history.revisions.len())),
        );
        panel.into_any_element()
    }
}
