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
