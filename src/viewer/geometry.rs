use super::*;

pub(super) fn line_positions(state: &EditorState, boundary_count: usize) -> Vec<f32> {
    let map = state.display_map();
    let height = f32::from(state.line_height().unwrap_or(px(20.)));
    (0..boundary_count)
        .map(|line| {
            let row = if line < map.buffer_line_count() {
                map.buffer_line_to_display_row(line)
            } else {
                map.display_row_count()
            };
            row as f32 * height
        })
        .collect()
}

fn editor_height(state: &EditorState) -> f32 {
    state.display_map().display_row_count() as f32
        * f32::from(state.line_height().unwrap_or(px(20.)))
}

pub(super) fn comparison_geometry(
    before: &EditorState,
    after: &EditorState,
    model: &FileDiff,
) -> DiffGeometry {
    DiffGeometry {
        before: line_positions(before, model.before_offsets.len()),
        after: line_positions(after, model.after_offsets.len()),
        before_height: editor_height(before),
        after_height: editor_height(after),
    }
}

pub(super) fn horizontal_wheel(event: &ScrollWheelEvent, line_height: Pixels) -> Option<f32> {
    let delta = event.delta.pixel_delta(line_height);
    if event.modifiers.shift {
        // macOS may already have moved Shift+wheel input to the X axis.
        Some(-f32::from(if delta.x.abs() > delta.y.abs() {
            delta.x
        } else {
            delta.y
        }))
    } else if delta.x.abs() > delta.y.abs() {
        Some(-f32::from(delta.x))
    } else {
        None
    }
}

/// Off-screen selections are revealed before stepping; boundary arrows still
/// work for a single change or an endpoint scrolled away by the user.
pub(super) fn change_target(
    active: Option<usize>,
    direction: i32,
    count: usize,
    visible: bool,
) -> Option<usize> {
    let last = count.checked_sub(1)?;
    let current = active.unwrap_or(0).min(last);
    if active.is_none() || !visible {
        return Some(current);
    }
    Some(current.saturating_add_signed(direction as isize).min(last))
}
#[cfg(test)]
mod navigation_tests {
    use super::change_target;
    #[test]
    fn either_arrow_reveals_an_offscreen_selection_before_advancing() {
        for direction in [-1, 1] {
            assert_eq!(change_target(Some(0), direction, 1, false), Some(0));
            assert_eq!(change_target(Some(1), direction, 3, false), Some(1));
        }
        assert_eq!(change_target(Some(1), -1, 3, true), Some(0));
        assert_eq!(change_target(Some(1), 1, 3, true), Some(2));
        assert_eq!(change_target(Some(0), -1, 3, true), Some(0));
        assert_eq!(change_target(Some(2), 1, 3, true), Some(2));
        assert_eq!(change_target(None, 1, 3, false), Some(0));
        assert_eq!(change_target(None, 1, 0, false), None);
    }
}
