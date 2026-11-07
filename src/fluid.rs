use crate::diff_model::FileDiff;

/// Experimental centre-anchored comparison coordinate. Each pane clamps its
/// displayed offset independently; input continues until both reach the end.
#[derive(Default)]
pub struct FluidScroll {
    pub position: f32,
}

impl FluidScroll {
    pub fn advance(
        &mut self,
        delta: f32,
        diff: &FileDiff,
        line_height: f32,
        viewport: f32,
    ) -> (f32, f32) {
        let extent: f32 = diff
            .blocks
            .iter()
            .map(|block| block.before.len().max(block.after.len()) as f32 * line_height)
            .sum();
        let limit = (extent - viewport).max(0.0);
        self.position = (self.position + delta).clamp(0.0, limit);
        let anchor = self.position + viewport / 2.0;
        let mut traversed = 0.0;
        let mut before_y = 0.0;
        let mut after_y = 0.0;

        for block in &diff.blocks {
            let before_height = block.before.len() as f32 * line_height;
            let after_height = block.after.len() as f32 * line_height;
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

        let before_limit = ((diff.before_offsets.len().saturating_sub(1)) as f32 * line_height
            - viewport)
            .max(0.0);
        let after_limit =
            ((diff.after_offsets.len().saturating_sub(1)) as f32 * line_height - viewport).max(0.0);
        if self.position == 0.0 {
            return (0.0, 0.0);
        }
        if self.position == limit {
            return (before_limit, after_limit);
        }
        (
            (before_y - viewport / 2.0).clamp(0.0, before_limit),
            (after_y - viewport / 2.0).clamp(0.0, after_limit),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continues_when_one_document_cannot_scroll() {
        let diff = FileDiff::new("short\n", &"added\n".repeat(100));
        let mut scroll = FluidScroll::default();

        let first = scroll.advance(100.0, &diff, 20.0, 400.0);
        let next = scroll.advance(100.0, &diff, 20.0, 400.0);

        assert_eq!(first.0, 0.0);
        assert_eq!(next.0, 0.0);
        assert!(next.1 > first.1);
    }

    #[test]
    fn clamps_both_ends_and_reverses_without_residual_overscroll() {
        let diff = FileDiff::new(&"before\n".repeat(60), &"after\n".repeat(100));
        let mut scroll = FluidScroll::default();

        let bottom = scroll.advance(100_000.0, &diff, 20.0, 400.0);
        let reversed = scroll.advance(-100.0, &diff, 20.0, 400.0);
        let top = scroll.advance(-100_000.0, &diff, 20.0, 400.0);

        assert_eq!(bottom, (800.0, 1600.0));
        assert!(reversed.0 <= bottom.0);
        assert!(reversed.1 < bottom.1);
        assert_eq!(top, (0.0, 0.0));
    }
}
