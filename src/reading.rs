//! Content anchors survive inserted/deleted lines without retaining whole files.
#[derive(Clone, Debug)]
pub struct ReadingAnchor {
    pub line: usize,
    text: String,
}
impl ReadingAnchor {
    pub fn capture(text: &str, line: usize) -> Self {
        Self {
            line,
            text: text.lines().nth(line).unwrap_or("").to_owned(),
        }
    }
    pub fn resolve(&self, text: &str) -> usize {
        text.lines()
            .enumerate()
            .filter(|(_, line)| *line == self.text)
            .min_by_key(|(line, _)| line.abs_diff(self.line))
            .map_or_else(
                || self.line.min(text.lines().count().saturating_sub(1)),
                |(line, _)| line,
            )
    }
}
#[cfg(test)]
mod tests {
    use super::ReadingAnchor;
    #[test]
    fn preserves_content_across_insertions_and_clamps_deleted_anchors() {
        let anchor = ReadingAnchor::capture("a\nfocus\nz", 1);
        assert_eq!(anchor.resolve("new\na\nfocus\nz"), 2);
        assert_eq!(anchor.resolve("a"), 0);
        assert_eq!(anchor.resolve(""), 0);
    }
}
