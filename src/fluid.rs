use crate::diff_model::FileDiff;

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

    pub fn change_position(&self, diff: &FileDiff, target: usize) -> f32 {
        let mut position = 0.;
        let mut change = 0;
        for block in &diff.blocks {
            let height = if block.changed {
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
            };
            if block.changed {
                if change == target {
                    return position + height / 2.;
                }
                change += 1;
            }
            position += height;
        }
        position
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
