use super::*;
use jaywalk::commands::CommandId;
use jaywalk::diff_options::Whitespace;
impl Viewer {
    pub(super) fn diff_option_command(
        &mut self,
        command: CommandId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.comparison().is_none() {
            return;
        }
        let before = self.diff_preferences;
        let p = &mut self.diff_preferences;
        match command {
            CommandId::ToggleWhitespaceDifferences => {
                p.comparison.whitespace = if p.comparison.whitespace == Whitespace::Exact {
                    self.ignored_whitespace_mode
                } else {
                    Whitespace::Exact
                }
            }
            CommandId::WhitespaceExact => p.comparison.whitespace = Whitespace::Exact,
            CommandId::WhitespaceTrailing => p.comparison.whitespace = Whitespace::IgnoreTrailing,
            CommandId::WhitespaceChanges => p.comparison.whitespace = Whitespace::IgnoreChanges,
            CommandId::WhitespaceAll => p.comparison.whitespace = Whitespace::IgnoreAll,
            CommandId::ToggleIgnoreLineEndings => {
                p.comparison.ignore_line_endings = !p.comparison.ignore_line_endings
            }
            CommandId::ToggleIgnoreBlankLines => {
                p.comparison.ignore_blank_lines = !p.comparison.ignore_blank_lines
            }
            CommandId::ToggleWordHighlight => p.word_highlight = !p.word_highlight,
            CommandId::ToggleShowWhitespace => p.show_whitespace = !p.show_whitespace,
            CommandId::ToggleShowLineEndings => p.show_line_endings = !p.show_line_endings,
            CommandId::ToggleWhitespaceErrors => {
                p.highlight_whitespace_errors = !p.highlight_whitespace_errors
            }
            _ => return,
        }
        self.apply_diff_preferences(before, window, cx);
    }
    pub(super) fn apply_diff_preferences(
        &mut self,
        before: jaywalk::settings::DiffPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.diff_preferences.comparison.whitespace != Whitespace::Exact {
            self.ignored_whitespace_mode = self.diff_preferences.comparison.whitespace;
        }
        if before.comparison != self.diff_preferences.comparison {
            self.prepare_display_reflow(cx);
            self.reload_open_file(window, cx);
            self.invalidate_overview();
        }
        if before.show_line_endings != self.diff_preferences.show_line_endings {
            let visible = self.diff_preferences.show_line_endings;
            if let Some(SelectedFile {
                content: LoadState::Loaded(code),
                ..
            }) = &mut self.selected_file
            {
                code.endings = [&code.before, &code.after].map(|s| {
                    if visible {
                        ending_markers(&s.read(cx).value())
                    } else {
                        Default::default()
                    }
                });
            }
        }
        self.refresh_word_colors(cx);
        if let Some(code) = self.code() {
            for state in [&code.before, &code.after] {
                state.update(cx, |editor, cx| {
                    editor.set_show_whitespaces(self.diff_preferences.show_whitespace, window, cx)
                });
            }
        }
        cx.notify();
    }
    pub(super) fn diff_render_options(&self) -> gpui::component::diff::DiffDisplayOptions {
        let p = self.diff_preferences;
        gpui::component::diff::DiffDisplayOptions {
            word_highlight: p.word_highlight,
            whitespace: p.show_whitespace,
            line_endings: p.show_line_endings,
            whitespace_errors: p.highlight_whitespace_errors,
        }
    }
}

pub(super) type EndingMarkers = std::sync::Arc<Vec<(usize, &'static str)>>;
pub(super) fn ending_markers(text: &str) -> EndingMarkers {
    let mut offset = 0;
    std::sync::Arc::new(
        text.split_inclusive('\n')
            .map(|line| {
                let (removed, label) = if line.ends_with("\r\n") {
                    (2, "CRLF")
                } else if line.ends_with('\n') {
                    (1, "LF")
                } else if line.ends_with('\r') {
                    (1, "CR · EOF")
                } else {
                    (0, "EOF")
                };
                offset += line.len();
                (offset - removed, label)
            })
            .collect(),
    )
}
