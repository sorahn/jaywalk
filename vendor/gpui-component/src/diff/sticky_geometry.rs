/// The expanded overview scrolls before the first file reaches the top.
/// Later file headers use the same pinning and push behavior.
pub(crate) fn file_header_top(
    natural_top: Option<f32>,
    next_top: Option<f32>,
    height: f32,
) -> Option<f32> {
    if natural_top.is_some_and(|top| top >= 0.) {
        return None;
    }
    Some(push_offset(next_top, height))
}

/// The next file's natural header pushes the sticky header off the viewport.
/// Both inputs use pixels relative to the viewport's top edge.
pub(crate) fn push_offset(next_top: Option<f32>, height: f32) -> f32 {
    next_top.map_or(0., |top| (top - height).min(0.))
}

/// macOS can move Shift+wheel deltas onto X before delivery.
pub(crate) fn horizontal_delta(x: f32, y: f32, shift: bool) -> Option<f32> {
    if shift || x.abs() > y.abs() {
        Some(if x.abs() > y.abs() { x } else { y })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_message_scrolls_away_before_the_first_file_pins() {
        assert_eq!(file_header_top(Some(800.), None, 32.), None);
        assert_eq!(file_header_top(Some(1.), None, 32.), None);
        assert_eq!(file_header_top(Some(0.), None, 32.), None);
        assert_eq!(file_header_top(Some(-1.), None, 32.), Some(0.));
        assert_eq!(file_header_top(None, None, 32.), Some(0.));
        assert_eq!(file_header_top(None, Some(20.), 32.), Some(-12.));
    }
    #[test]
    fn header_wheel_supports_vertical_horizontal_and_remapped_shift_input() {
        assert_eq!(horizontal_delta(0., 20., false), None);
        assert_eq!(horizontal_delta(20., 0., false), Some(20.));
        assert_eq!(horizontal_delta(0., -20., true), Some(-20.));
        assert_eq!(horizontal_delta(-20., 0., true), Some(-20.));
    }
    #[test]
    fn next_header_pushes_only_when_it_reaches_the_sticky_header() {
        assert_eq!(push_offset(None, 32.), 0.);
        assert_eq!(push_offset(Some(60.), 32.), 0.);
        assert_eq!(push_offset(Some(32.), 32.), 0.);
        assert_eq!(push_offset(Some(20.), 32.), -12.);
        assert_eq!(push_offset(Some(0.), 32.), -32.);
        assert_eq!(push_offset(Some(20.), 48.), -28.);
    }
}
