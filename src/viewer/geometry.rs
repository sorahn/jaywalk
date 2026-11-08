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

pub(super) fn comparison_geometry(
    before: &EditorState,
    after: &EditorState,
    model: &FileDiff,
) -> DiffGeometry {
    DiffGeometry {
        before: line_positions(before, model.before_offsets.len()),
        after: line_positions(after, model.after_offsets.len()),
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
