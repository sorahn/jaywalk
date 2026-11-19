//! Render-only controls: source strings, source offsets and clipboard data never change.
use std::ops::Range;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffDisplayOptions {
    pub word_highlight: bool,
    pub whitespace: bool,
    pub line_endings: bool,
    pub whitespace_errors: bool,
}
impl Default for DiffDisplayOptions {
    fn default() -> Self {
        Self {
            word_highlight: true,
            whitespace: false,
            line_endings: false,
            whitespace_errors: false,
        }
    }
}
/// Trailing ASCII spaces/tabs and mixed indentation with spaces before tabs.
pub fn whitespace_error_ranges(text: &str) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let trimmed = body.trim_end_matches([' ', '\t']).len();
        if trimmed < body.len() {
            result.push(offset + trimmed..offset + body.len());
        }
        let mut spaces = false;
        let mut mixed = false;
        let mut indent_end = 0;
        for (i, c) in body.char_indices() {
            match c {
                ' ' => spaces = true,
                '\t' => mixed |= spaces,
                _ => break,
            }
            indent_end = i + 1;
        }
        if mixed && indent_end.min(trimmed) > 0 {
            result.push(offset..offset + indent_end.min(trimmed));
        }
        offset += line.len();
    }
    result
}
