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
