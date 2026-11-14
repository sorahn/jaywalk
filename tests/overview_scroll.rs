//! Headless GPUI state tests: no window is created or activated.
use gpui::{AppContext, ListOffset, TestAppContext, px};
use gpui_kit::component::diff::{DiffFile, DiffLinePosition, DiffSide, DiffState};
use gpui_test as gpui;

fn files() -> Vec<DiffFile> {
    let before = (1..=30)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    let after = before.replace("line 15\n", "changed 15\n");
    let patch = similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .context_radius(30)
        .header("a/review.txt", "b/review.txt")
        .to_string();
    DiffFile::parse(&patch).unwrap()
}

#[gpui::test]
fn expanding_and_folding_context_keeps_the_overview_header_in_view(cx: &mut TestAppContext) {
    let state = cx.new(|cx| DiffState::new(files(), cx).with_context_lines(Some(1)));
    state.update(cx, |state, cx| {
        for pixels in [0., 37.] {
            let start = ListOffset {
                item_ix: 0,
                offset_in_item: px(pixels),
            };
            state.restore_viewport_offset(start, cx);
            for context in [None, Some(1), None, Some(3)] {
                state.set_context_lines(context, cx);
                let actual = state.viewport_offset();
                assert_eq!(
                    actual.item_ix, 0,
                    "context {context:?} scrolled the header away"
                );
                assert_eq!(actual.offset_in_item, start.offset_in_item);
            }
        }
    });
}

#[gpui::test]
fn context_changes_still_follow_the_source_line_when_reading_code(cx: &mut TestAppContext) {
    let state = cx.new(|cx| DiffState::new(files(), cx).with_context_lines(None));
    state.update(cx, |state, cx| {
        let line = DiffLinePosition::new("review.txt", DiffSide::Modified, 15);
        state.scroll_to_line(line.clone(), cx);
        assert_eq!(state.viewport_position(), Some(line.clone()));
        for context in [Some(1), None, Some(3)] {
            state.set_context_lines(context, cx);
            assert_eq!(state.viewport_position(), Some(line.clone()));
        }
    });
}

struct ScrollHost {
    state: gpui::Entity<DiffState>,
}

impl gpui::Render for ScrollHost {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{ParentElement, Styled};
        gpui::div()
            .w(px(400.))
            .h(px(240.))
            .child(gpui_kit::component::diff::Diff::new(&self.state).size_full())
    }
}

#[gpui::test]
fn wide_overview_keeps_its_scrollbar_at_the_visible_edge(cx: &mut TestAppContext) {
    use gpui::{MouseButton, point};
    use gpui_kit::component::{Theme, scroll::ScrollbarMode};
    cx.update(|app| {
        gpui_kit::init(app);
        Theme::set_scrollbar_mode(ScrollbarMode::Always, app);
    });
    let before = (0..100)
        .map(|i| format!("{i} {}\n", "x".repeat(300)))
        .collect::<String>();
    let after = before.replace('x', "y");
    let patch = similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .context_radius(100)
        .header("a/wide.txt", "b/wide.txt")
        .to_string();
    let state =
        cx.new(|cx| DiffState::new(DiffFile::parse(&patch).unwrap(), cx).with_context_lines(None));
    let (_, cx) = cx.add_window_view({
        let state = state.clone();
        move |_, _| ScrollHost { state }
    });
    fn draw(cx: &mut gpui::VisualTestContext) {
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, app| window.draw(app).clear(app));
        }
    }
    draw(cx);
    let start = cx.update(|_, app| state.read(app).viewport_offset());
    let grab = point(px(393.), px(12.));
    let end = point(px(397.), px(200.));
    cx.simulate_mouse_move(grab, None, Default::default());
    draw(cx);
    cx.simulate_mouse_down(grab, MouseButton::Left, Default::default());
    draw(cx);
    cx.simulate_mouse_move(end, Some(MouseButton::Left), Default::default());
    draw(cx);
    cx.simulate_mouse_up(end, MouseButton::Left, Default::default());
    draw(cx);
    let finish = cx.update(|_, app| state.read(app).viewport_offset());
    assert!(
        finish.item_ix > start.item_ix + 10,
        "scrollbar did not advance: {start:?} -> {finish:?}"
    );
}

struct StickyHost {
    state: gpui::Entity<DiffState>,
}
impl gpui::Render for StickyHost {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{ParentElement, Styled};
        gpui::div()
            .w(px(400.))
            .h(px(240.))
            .flex()
            .flex_col()
            .child(
                gpui::div()
                    .h(px(24.))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(gpui::rgb(0x303030)),
            )
            .child(
                gpui_kit::component::diff::Diff::new(&self.state)
                    .border_0()
                    .sticky_headers(true)
                    .size_full(),
            )
    }
}

#[gpui::test]
fn sticky_headers_keep_one_bottom_border_without_repeating_the_comparison_separator(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let mut documents = Vec::new();
    for name in ["first.txt", "second.txt", "third.txt"] {
        let before = (0..40).map(|i| format!("line {i}\n")).collect::<String>();
        let after = before.replace("line 15\n", "changed 15\n");
        let patch = similar::TextDiff::from_lines(&before, &after)
            .unified_diff()
            .context_radius(40)
            .header(&format!("a/{name}"), &format!("b/{name}"))
            .to_string();
        documents.extend(DiffFile::parse(&patch).unwrap());
    }
    let state = cx.new(|cx| DiffState::new(documents, cx).with_context_lines(None));
    let (_, cx) = cx.add_window_view({
        let state = state.clone();
        move |_, _| StickyHost { state }
    });
    fn draw(cx: &mut gpui::VisualTestContext) {
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|w, a| w.draw(a).clear(a));
        }
    }
    draw(cx);
    let painted_header = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, _| {
            let scale = window.scale_factor();
            // Transparent borders may be split into four masked quads. Count
            // only the primitive whose full-width bottom stroke is visible.
            let headers = window
                .painted_quads()
                .into_iter()
                .filter(|q| {
                    q.border_widths.bottom.as_f32() > 0.
                        && q.bounds.size.width.as_f32() / scale > 390.
                        && (q.bounds.top().as_f32() / scale - 24.).abs() < 0.1
                        && q.content_mask.bounds.size.width.as_f32() / scale > 390.
                        && q.content_mask.bounds.bottom().as_f32()
                            >= q.bounds.bottom().as_f32() - 0.1
                })
                .collect::<Vec<_>>();
            assert_eq!(
                headers.len(),
                1,
                "expected one visible header bottom stroke: {headers:?}"
            );
            (headers.into_iter().next().unwrap(), scale)
        })
    };
    for name in [
        "first.txt",
        "second.txt",
        "third.txt",
        "second.txt",
        "first.txt",
        "third.txt",
    ] {
        state.update(cx, |state, cx| state.scroll_to_file(name, cx));
        draw(cx);
        let natural = painted_header(cx).0;
        let header_offset = cx.update(|_, app| state.read(app).viewport_offset());
        for offset in [None, Some(0.25), Some(1.)] {
            state.update(cx, |state, cx| match offset {
                None => {
                    state.scroll_to_line(DiffLinePosition::new(name, DiffSide::Modified, 15), cx)
                }
                Some(offset) => state.restore_viewport_offset(
                    ListOffset {
                        item_ix: header_offset.item_ix,
                        offset_in_item: px(offset),
                    },
                    cx,
                ),
            });
            draw(cx);
            let (header, scale) = painted_header(cx);
            assert_eq!(
                header.border_widths.top.as_f32(),
                0.,
                "{name}: duplicate viewport separator"
            );
            assert_eq!(
                header.border_widths.bottom.as_f32() / scale,
                1.,
                "{name}: bottom stroke must stay one logical pixel"
            );
            assert_eq!(
                header.bounds.size.height, natural.bounds.size.height,
                "{name}: pinning changed header height"
            );
        }
    }
}
