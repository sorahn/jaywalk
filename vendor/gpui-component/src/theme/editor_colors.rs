//! Code-surface colors are distinct from shell inputs and active controls.
use super::Theme;
use gpui::Hsla;

/// A resolved code surface. Per-view overrides leave the global shell untouched.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorColors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub border: Hsla,
    pub selection: Hsla,
    pub caret: Hsla,
    pub line_number: Hsla,
    pub gutter_background: Option<Hsla>,
}
impl Theme {
    pub fn editor_colors(&self) -> EditorColors {
        let style = &self.highlight_theme.style;
        EditorColors {
            background: self.editor_background(),
            foreground: style.editor_foreground.unwrap_or(self.foreground),
            muted_foreground: style.editor_line_number.unwrap_or(self.muted_foreground),
            border: style.editor_border.unwrap_or(self.border),
            selection: style.editor_selection.unwrap_or(self.selection),
            caret: style.editor_caret.unwrap_or(self.caret),
            line_number: style.editor_line_number.unwrap_or(self.muted_foreground),
            gutter_background: style.editor_gutter_background,
        }
    }
}
