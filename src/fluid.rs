use crate::diff_model::FileDiff;

/// Intersect the complete changed-row span with the painted viewport. Do not
/// round to fully visible rows: both edges can contain part of a source row.
pub fn clipped_tint(
    changed: std::ops::Range<f32>,
    viewport: std::ops::Range<f32>,
) -> Option<std::ops::Range<f32>> {
    let top = changed.start.max(viewport.start);
    let bottom = changed.end.min(viewport.end);
    (bottom > top).then_some(top..bottom)
}

/// Attach at the change midpoint, sliding along tall blocks toward the visible
/// viewport. The endpoint must remain on the block even while it enters or
/// leaves the viewport, or a connector can point into unchanged source lines.
pub fn connector_attachment(change: std::ops::Range<f32>, viewport: std::ops::Range<f32>) -> f32 {
    let middle = (change.start + change.end) / 2.;
    let height = viewport.end - viewport.start;
    if change.end - change.start <= height {
        middle
    } else {
        let margin = height / 4.;
        middle
            .clamp(viewport.start + margin, viewport.end - margin)
            .clamp(change.start, change.end)
    }
}

/// Experimental centre-anchored comparison coordinate. Each pane clamps its
/// displayed offset independently; input continues until both reach the end.
#[derive(Default)]
pub struct FluidScroll {
    pub position: f32,
}

/// Pixel positions of source-line boundaries after editor layout, including
/// soft-wrapped continuation rows. Empty sides retain a single zero boundary.
pub struct DiffGeometry {
    pub before: Vec<f32>,
    pub after: Vec<f32>,
    /// Full editor heights, including a terminal empty row after a newline.
    /// Source boundaries above still describe diff blocks, not scroll padding.
    pub before_height: f32,
    pub after_height: f32,
}

impl DiffGeometry {
    fn segments(&self, diff: &FileDiff) -> Vec<(f32, f32)> {
        let mut result = Vec::new();
        for block in &diff.blocks {
            if block.changed {
                result.push((
                    self.before[block.before.end] - self.before[block.before.start],
                    self.after[block.after.end] - self.after[block.after.start],
                ));
            } else {
                // Corresponding unchanged lines can wrap to different heights.
                for (before, after) in block.before.clone().zip(block.after.clone()) {
                    result.push((
                        self.before[before + 1] - self.before[before],
                        self.after[after + 1] - self.after[after],
                    ));
                }
            }
        }
        // A final newline creates a visible empty editor row without adding a
        // diff line. Include it only in scrolling, never in connector geometry.
        result.push((
            (self.before_height - self.before.last().copied().unwrap_or(0.)).max(0.),
            (self.after_height - self.after.last().copied().unwrap_or(0.)).max(0.),
        ));
        result
    }

    pub fn scroll_extent(&self, diff: &FileDiff) -> f32 {
        self.segments(diff).iter().map(|(a, b)| a.max(*b)).sum()
    }

    /// Virtual coordinate at a source line, used when restoring a reading anchor.
    pub fn position_for_line(&self, diff: &FileDiff, line: usize, before: bool) -> f32 {
        let mut position = 0.;
        for block in &diff.blocks {
            let range = if before { &block.before } else { &block.after };
            if block.changed {
                let height = (self.before[block.before.end] - self.before[block.before.start])
                    .max(self.after[block.after.end] - self.after[block.after.start]);
                if range.contains(&line) {
                    return position
                        + height * (line - range.start) as f32 / range.len().max(1) as f32;
                }
                position += height;
            } else {
                for (left, right) in block.before.clone().zip(block.after.clone()) {
                    if (if before { left } else { right }) >= line {
                        return position;
                    }
                    position += (self.before[left + 1] - self.before[left])
                        .max(self.after[right + 1] - self.after[right]);
                }
            }
        }
        position
    }

    fn block_height(&self, block: &crate::diff_model::Correspondence) -> f32 {
        if block.changed {
            (self.before[block.before.end] - self.before[block.before.start])
                .max(self.after[block.after.end] - self.after[block.after.start])
        } else {
            block
                .before
                .clone()
                .zip(block.after.clone())
                .map(|(before, after)| {
                    (self.before[before + 1] - self.before[before])
                        .max(self.after[after + 1] - self.after[after])
                })
                .sum()
        }
    }

    /// Changed spans in the shared virtual coordinate, including unequal wrapping
    /// of unchanged lines. The iterator borrows source blocks and allocates nothing.
    pub fn change_spans<'a>(
        &'a self,
        diff: &'a FileDiff,
    ) -> impl Iterator<Item = (&'a crate::diff_model::Correspondence, std::ops::Range<f32>)> + 'a
    {
        let mut position = 0.;
        diff.blocks.iter().filter_map(move |block| {
            let start = position;
            position += self.block_height(block);
            block.changed.then_some((block, start..position))
        })
    }

    pub fn change_position(&self, diff: &FileDiff, target: usize) -> f32 {
        self.change_spans(diff).nth(target).map_or_else(
            || self.position_for_line(diff, usize::MAX, true),
            |(_, span)| (span.start + span.end) / 2.,
        )
    }
}

impl FluidScroll {
    pub fn advance(
        &mut self,
        delta: f32,
        diff: &FileDiff,
        geometry: &DiffGeometry,
        viewport: f32,
    ) -> (f32, f32) {
        let segments = geometry.segments(diff);
        let extent: f32 = segments
            .iter()
            .map(|(before, after)| before.max(*after))
            .sum();
        let limit = (extent - viewport).max(0.0);
        self.position = (self.position + delta).clamp(0.0, limit);
        let anchor = self.position + viewport / 2.0;
        let mut traversed = 0.0;
        let mut before_y = 0.0;
        let mut after_y = 0.0;
        for (before_height, after_height) in segments {
            let virtual_height = before_height.max(after_height);
            if anchor <= traversed + virtual_height && virtual_height > 0.0 {
                let fraction = ((anchor - traversed) / virtual_height).clamp(0.0, 1.0);
                before_y += before_height * fraction;
                after_y += after_height * fraction;
                break;
            }
            traversed += virtual_height;
            before_y += before_height;
            after_y += after_height;
        }
        let before_limit = (geometry.before_height - viewport).max(0.);
        let after_limit = (geometry.after_height - viewport).max(0.);
        if self.position == 0.0 {
            return (0.0, 0.0);
        }
        if self.position == limit {
            return (before_limit, after_limit);
        }
        (
            (before_y - viewport / 2.).clamp(0., before_limit),
            (after_y - viewport / 2.).clamp(0., after_limit),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry(diff: &FileDiff) -> DiffGeometry {
        DiffGeometry {
            before_height: (diff.before_offsets.len() - 1) as f32 * 20.,
            after_height: (diff.after_offsets.len() - 1) as f32 * 20.,
            before: (0..diff.before_offsets.len())
                .map(|line| line as f32 * 20.)
                .collect(),
            after: (0..diff.after_offsets.len())
                .map(|line| line as f32 * 20.)
                .collect(),
        }
    }

    #[test]
    fn mixed_empty_and_unequal_changes_scroll_continuously_and_reversibly() {
        let unchanged = |n: usize| (0..n).map(|i| format!("line {i}\n")).collect::<String>();
        let cases = [
            (String::new(), "added\n".repeat(100)),
            ("deleted\n".repeat(100), String::new()),
            (
                format!("{}{}{}", unchanged(30), "old\n".repeat(3), unchanged(40)),
                format!("{}{}{}", unchanged(30), "new\n".repeat(80), unchanged(40)),
            ),
            (
                format!("{}{}", unchanged(40), "removed\n".repeat(90)),
                unchanged(40),
            ),
        ];
        for (before, after) in cases {
            let diff = FileDiff::new(&before, &after);
            let mut layout = geometry(&diff);
            // Exercise different wrapping heights on corresponding lines too.
            for (positions, height) in [
                (&mut layout.before, &mut layout.before_height),
                (&mut layout.after, &mut layout.after_height),
            ] {
                let mut y = 0.;
                for (line, position) in positions.iter_mut().enumerate() {
                    *position = y;
                    y += if line % 7 == 0 { 80. } else { 20. };
                }
                *height = positions.last().copied().unwrap_or(0.) + 20.;
            }
            for viewport in [20., 200., 10000.] {
                let mut scroll = FluidScroll::default();
                let mut previous = (0., 0.);
                let limit = (layout.scroll_extent(&diff) - viewport).max(0.);
                for _ in 0..(limit / 7.).ceil() as usize + 1 {
                    let current = scroll.advance(7., &diff, &layout, viewport);
                    for (old, new) in [(previous.0, current.0), (previous.1, current.1)] {
                        assert!(
                            new >= old - 0.01 && new - old <= 7.01,
                            "{previous:?} -> {current:?}"
                        );
                    }
                    previous = current;
                }
                assert_eq!(
                    previous,
                    (
                        (layout.before_height - viewport).max(0.),
                        (layout.after_height - viewport).max(0.)
                    )
                );
                if limit > 100. {
                    scroll.advance(-limit / 2., &diff, &layout, viewport);
                    let original = scroll.advance(0., &diff, &layout, viewport);
                    scroll.advance(-17., &diff, &layout, viewport);
                    let restored = scroll.advance(17., &diff, &layout, viewport);
                    assert!((original.0 - restored.0).abs() < 0.01);
                    assert!((original.1 - restored.1).abs() < 0.01);
                }
                assert_eq!(scroll.advance(-100000., &diff, &layout, viewport), (0., 0.));
            }
        }
    }

    #[test]
    fn continues_when_one_document_cannot_scroll() {
        let diff = FileDiff::new("short\n", &"added\n".repeat(100));
        let mut scroll = FluidScroll::default();

        let first = scroll.advance(100.0, &diff, &geometry(&diff), 400.0);
        let next = scroll.advance(100.0, &diff, &geometry(&diff), 400.0);

        assert_eq!(first.0, 0.0);
        assert_eq!(next.0, 0.0);
        assert!(next.1 > first.1);
    }

    #[test]
    fn final_empty_rows_are_reachable_without_extending_diff_blocks() {
        for (left_newline, right_newline) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let source = |newline| {
                format!(
                    "{}last{}",
                    "line\n".repeat(30),
                    if newline { "\n" } else { "" }
                )
            };
            let diff = FileDiff::new(&source(left_newline), &source(right_newline));
            let mut geometry = geometry(&diff);
            let before_end = geometry.before_height;
            let after_end = geometry.after_height;
            geometry.before_height += if left_newline { 20. } else { 0. };
            geometry.after_height += if right_newline { 20. } else { 0. };
            let mut scroll = FluidScroll::default();
            let bottom = scroll.advance(10000., &diff, &geometry, 200.);
            assert_eq!(bottom.0 + 200., geometry.before_height);
            assert_eq!(bottom.1 + 200., geometry.after_height);
            assert_eq!(geometry.before.last().copied(), Some(before_end));
            assert_eq!(geometry.after.last().copied(), Some(after_end));
            assert_eq!(scroll.advance(-10000., &diff, &geometry, 200.), (0., 0.));
        }
    }

    #[test]
    fn clamps_both_ends_and_reverses_without_residual_overscroll() {
        let diff = FileDiff::new(&"before\n".repeat(60), &"after\n".repeat(100));
        let mut scroll = FluidScroll::default();

        let bottom = scroll.advance(100_000.0, &diff, &geometry(&diff), 400.0);
        let reversed = scroll.advance(-100.0, &diff, &geometry(&diff), 400.0);
        let top = scroll.advance(-100_000.0, &diff, &geometry(&diff), 400.0);

        assert_eq!(bottom, (800.0, 1600.0));
        assert!(reversed.0 <= bottom.0);
        assert!(reversed.1 < bottom.1);
        assert_eq!(top, (0.0, 0.0));
    }
}

#[cfg(test)]
mod wrapped_tests {
    use super::*;

    #[test]
    fn unchanged_wrapped_lines_keep_their_correspondence() {
        let diff = FileDiff::new("one\ntwo\nthree", "one\ntwo\nthree");
        let geometry = DiffGeometry {
            before: vec![0., 100., 120., 140.],
            after: vec![0., 20., 80., 100.],
            before_height: 140.,
            after_height: 100.,
        };
        let mut scroll = FluidScroll::default();
        // At virtual row 100 the first source line ends on both sides.
        assert_eq!(scroll.advance(90., &diff, &geometry, 20.), (90., 10.));
        assert_eq!(scroll.advance(10000., &diff, &geometry, 20.), (120., 80.));
    }
}

#[cfg(test)]
mod tint_tests {
    use super::clipped_tint;
    #[test]
    fn partial_rows_retain_tint_without_spilling_into_unchanged_rows() {
        assert_eq!(clipped_tint(140.0..180.0, 100.0..157.0), Some(140.0..157.0));
        assert_eq!(clipped_tint(90.0..120.0, 100.0..157.0), Some(100.0..120.0));
        assert_eq!(clipped_tint(90.0..180.0, 100.0..157.0), Some(100.0..157.0));
        assert_eq!(clipped_tint(120.0..145.0, 100.0..157.0), Some(120.0..145.0));
        assert_eq!(clipped_tint(157.0..180.0, 100.0..157.0), None);
        assert_eq!(clipped_tint(90.0..100.0, 100.0..157.0), None);
        assert_eq!(clipped_tint(120.0..120.0, 100.0..157.0), None);
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::connector_attachment;

    #[test]
    fn tall_change_entering_or_leaving_never_attaches_to_unchanged_lines() {
        let viewport = 100.0..700.0;
        assert_eq!(connector_attachment(650.0..2650.0, viewport.clone()), 650.0);
        assert_eq!(
            connector_attachment(-1950.0..150.0, viewport.clone()),
            150.0
        );
        assert_eq!(connector_attachment(-1000.0..2000.0, viewport), 500.0);
    }

    #[test]
    fn short_changes_and_empty_sides_keep_their_midpoints() {
        assert_eq!(connector_attachment(650.0..690.0, 100.0..700.0), 670.0);
        assert_eq!(connector_attachment(720.0..720.0, 100.0..700.0), 720.0);
        assert_eq!(connector_attachment(-200.0..-180.0, 100.0..700.0), -190.0);
    }

    #[test]
    fn tall_attachment_tracks_scrolling_continuously_and_stays_on_its_edge() {
        let mut previous: Option<f32> = None;
        for offset in -3000..1000 {
            let top = offset as f32;
            let bottom = top + 2000.;
            let y = connector_attachment(top..bottom, 100.0..700.0);
            assert!((top..=bottom).contains(&y));
            if let Some(previous) = previous {
                assert!((y - previous).abs() <= 1.0);
            }
            previous = Some(y);
        }
    }
}

/// Shared scrollbar coordinates in track-local pixels. The virtual comparison
/// extent includes unequal changes, so neither pane's length defines the thumb.
pub struct SharedScrollTrack {
    pub thumb: std::ops::Range<f32>,
    limit: f32,
    travel: f32,
    inset: f32,
}
impl SharedScrollTrack {
    pub fn new(extent: f32, viewport: f32, track: f32, thumb_height: f32, position: f32) -> Self {
        let limit = (extent - viewport).max(0.);
        let height = thumb_height.min(track);
        let remaining = (track - height).max(0.);
        // Keep both ends discoverable. Tiny tracks retain at least half their
        // remaining travel rather than losing the ability to drag altogether.
        let inset = (track * 0.05).min(remaining / 4.);
        let travel = remaining - 2. * inset;
        let top = inset
            + if limit > 0. {
                position.clamp(0., limit) / limit * travel
            } else {
                0.
            };
        Self {
            thumb: top..top + height,
            limit,
            travel,
            inset,
        }
    }
    pub fn scrollable(&self) -> bool {
        self.limit > 0. && self.travel > 0.
    }
    /// Track-local mark for a virtual changed span. Subtracting half a viewport
    /// puts the thumb over the mark when that change is centered in the editors.
    pub fn change_marker(
        &self,
        change: std::ops::Range<f32>,
        viewport: f32,
        minimum_height: f32,
    ) -> std::ops::Range<f32> {
        let height = self.thumb.end - self.thumb.start;
        let track_height = self.travel + 2. * self.inset + height;
        let center_at = |position: f32| {
            self.inset
                + height / 2.
                + if self.limit > 0. {
                    position.clamp(0., self.limit) / self.limit * self.travel
                } else {
                    0.
                }
        };
        let start = center_at(change.start - viewport / 2.);
        let end = center_at(change.end - viewport / 2.);
        let half = (end - start).max(minimum_height).min(track_height) / 2.;
        let center = ((start + end) / 2.).clamp(half, track_height - half);
        center - half..center + half
    }

    /// One viewport toward the click, in virtual comparison coordinates.
    pub fn page_at(&self, pointer: f32, position: f32, viewport: f32) -> f32 {
        let delta = if pointer < self.thumb.start {
            -viewport
        } else if pointer >= self.thumb.end {
            viewport
        } else {
            0.
        };
        (position + delta).clamp(0., self.limit)
    }
    pub fn position_at(&self, pointer: f32, grab_fraction: f32) -> f32 {
        if !self.scrollable() {
            return 0.;
        }
        let top = pointer - grab_fraction * (self.thumb.end - self.thumb.start);
        ((top - self.inset) / self.travel).clamp(0., 1.) * self.limit
    }
}
#[cfg(test)]
mod shared_scroll_tests {
    use super::*;
    #[test]
    fn thumb_tracks_virtual_extent_and_reaches_both_ends() {
        let track = SharedScrollTrack::new(4000., 800., 800., 28., 1600.);
        assert_eq!(track.thumb, 386.0..414.0);
        assert_eq!(track.position_at(400., 0.5), 1600.);
        assert_eq!(track.position_at(-100., 0.5), 0.);
        assert_eq!(track.position_at(1000., 0.5), 3200.);
    }
    #[test]
    fn fixed_thumb_preserves_pointer_grab_and_handles_short_tracks() {
        let track = SharedScrollTrack::new(100000., 800., 800., 28., 0.);
        assert_eq!(track.thumb, 40.0..68.0);
        assert_eq!(
            SharedScrollTrack::new(1000., 800., 800., 28., 0.).thumb,
            track.thumb
        );
        assert_eq!(track.position_at(47., 0.25), 0.);
        assert_eq!(track.position_at(739., 0.25), 99200.);
        assert!(!SharedScrollTrack::new(100000., 800., 10., 28., 0.).scrollable());
        assert!(!SharedScrollTrack::new(200., 800., 800., 28., 0.).scrollable());
    }
    #[test]
    fn inset_thumb_keeps_full_range_and_preserves_every_grab_point() {
        for position in [0., 1600., 3200.] {
            let track = SharedScrollTrack::new(4000., 800., 800., 28., position);
            assert!(track.thumb.start >= 40. && track.thumb.end <= 760.);
            for grab in [0., 0.25, 0.5, 1.] {
                let pointer = track.thumb.start + grab * (track.thumb.end - track.thumb.start);
                assert!((track.position_at(pointer, grab) - position).abs() < 0.01);
            }
        }
        let short = SharedScrollTrack::new(4000., 800., 30., 28., 3200.);
        assert!(short.scrollable());
        assert_eq!(short.thumb, 1.5..29.5);
        assert_eq!(short.position_at(0., 0.5), 0.);
        assert_eq!(short.position_at(30., 0.5), 3200.);
    }
    #[test]
    fn track_paging_moves_one_viewport_without_jumping_to_the_pointer() {
        let track = SharedScrollTrack::new(4000., 800., 800., 28., 1600.);
        assert_eq!(track.page_at(0., 1600., 800.), 800.);
        assert_eq!(track.page_at(800., 1600., 800.), 2400.);
        assert_eq!(track.page_at(400., 1600., 800.), 1600.);
        assert_eq!(track.page_at(0., 100., 800.), 0.);
        assert_eq!(track.page_at(800., 3000., 800.), 3200.);
    }
    #[test]
    fn large_append_uses_shared_extent_after_short_pane_stops() {
        let diff = FileDiff::new(
            "unchanged\n",
            &format!("unchanged\n{}", "added\n".repeat(100)),
        );
        let geometry = DiffGeometry {
            before: vec![0., 20.],
            after: (0..102).map(|n| n as f32 * 20.).collect(),
            before_height: 20.,
            after_height: 2020.,
        };
        let extent = geometry.scroll_extent(&diff);
        assert_eq!(extent, 2020.);
        let track = SharedScrollTrack::new(extent, 400., 400., 28., 0.);
        let mut scroll = FluidScroll::default();
        let end = scroll.advance(track.position_at(400., 0.5), &diff, &geometry, 400.);
        assert_eq!(end, (0., 1620.));
    }
}

#[cfg(test)]
mod change_map_tests {
    use super::{DiffGeometry, SharedScrollTrack};
    use crate::diff_model::FileDiff;

    #[test]
    fn marks_use_shared_wrapped_geometry_and_the_navigation_coordinate() {
        let diff = FileDiff::new("keep\nold a\nold b\ntail\n", "keep\nnew\ntail\n");
        // The same unchanged source line wraps to different heights in each pane.
        let geometry = DiffGeometry {
            before: vec![0., 20., 50., 90., 110.],
            after: vec![0., 60., 80., 100.],
            before_height: 110.,
            after_height: 100.,
        };
        let spans: Vec<_> = geometry.change_spans(&diff).map(|(_, span)| span).collect();
        assert_eq!(spans, vec![60.0..130.0]);
        assert_eq!(geometry.change_position(&diff, 0), 95.);
        let track = SharedScrollTrack::new(geometry.scroll_extent(&diff), 40., 500., 12., 0.);
        let marker = track.change_marker(spans[0].clone(), 40., 2.);
        let target = track.position_at((marker.start + marker.end) / 2., 0.5);
        assert!((target - (95. - 20.)).abs() < 0.01);
    }

    #[test]
    fn map_targets_reach_endpoint_changes_and_stay_inside_short_tracks() {
        for height in [1., 13., 500.] {
            let track = SharedScrollTrack::new(4000., 800., height, 12., 0.);
            for span in [0.0..1.0, 1800.0..1801.0, 3999.0..4000.0] {
                let marker = track.change_marker(span, 800., 2.);
                assert!(marker.start >= 0.);
                assert!(marker.end <= height);
                assert!(marker.end > marker.start);
            }
        }
        let track = SharedScrollTrack::new(4000., 800., 500., 12., 0.);
        for (span, expected) in [(0.0..1.0, 0.), (3999.0..4000.0, 3200.)] {
            let marker = track.change_marker(span, 800., 2.);
            assert_eq!(
                track.position_at((marker.start + marker.end) / 2., 0.5),
                expected
            );
        }
    }

    #[test]
    fn empty_sides_and_large_appends_keep_their_shared_targets() {
        for (before, after) in [("", "added\n".repeat(100)), ("deleted\n", String::new())] {
            let diff = FileDiff::new(before, &after);
            let before_lines = diff.before_offsets.len() - 1;
            let after_lines = diff.after_offsets.len() - 1;
            let geometry = DiffGeometry {
                before: (0..=before_lines).map(|n| n as f32 * 20.).collect(),
                after: (0..=after_lines).map(|n| n as f32 * 20.).collect(),
                before_height: before_lines as f32 * 20.,
                after_height: after_lines as f32 * 20.,
            };
            let spans: Vec<_> = geometry.change_spans(&diff).collect();
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].0.before.is_empty(), before.is_empty());
            assert_eq!(spans[0].0.after.is_empty(), after.is_empty());
            assert_eq!(spans[0].1.end, geometry.scroll_extent(&diff));
        }
    }
}
