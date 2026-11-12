//! Prepare immutable file data off-thread; mount or update editor entities on the UI thread.
use super::*;

pub(super) fn standard_patch(before: &str, after: &str) -> String {
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(before.lines().count().max(after.lines().count()))
        .header("a/source", "b/source")
        .to_string()
}
pub(super) struct CodeComparison {
    pub(super) before: Entity<EditorState>,
    pub(super) after: Entity<EditorState>,
    pub(super) model: Rc<FileDiff>,
    pub(super) decorations: [gpui::base::input::TextDecorationCollection; 2],
    pub(super) standard: Result<Option<Entity<DiffState>>, String>,
}

pub(super) struct PreparedFile {
    pub(super) before: String,
    pub(super) after: String,
    pub(super) model: FileDiff,
    pub(super) syntax: [gpui::component::highlighter::PreparedInputHighlighter; 2],
    pub(super) standard: Result<Vec<DiffFile>, String>,
}

impl PreparedFile {
    pub(super) fn load(
        comparison: &Comparison,
        file: &jaywalk::repository_file::FileEntry,
        tab_size: usize,
    ) -> Result<Self, String> {
        let (before, after) = comparison.read_file(file)?;
        let model = FileDiff::new(&before, &after);
        let language = file_language(file.path());
        let syntax = [&before, &after].map(|text| {
            gpui::component::highlighter::PreparedInputHighlighter::new(language, text)
        });
        let standard = DiffFile::parse(&standard_patch(&before, &after))
            .map(|files| {
                files
                    .into_iter()
                    .map(|file| file.with_tab_size(tab_size))
                    .collect()
            })
            .map_err(|error| error.to_string());
        Ok(Self {
            before,
            after,
            model,
            syntax,
            standard,
        })
    }
}

fn word_decorations(
    ranges: &[std::ops::Range<usize>],
    color: Hsla,
) -> Vec<gpui::base::input::TextDecoration> {
    ranges
        .iter()
        .cloned()
        .map(|range| {
            gpui::base::input::TextDecoration::new(
                range,
                HighlightStyle {
                    background_color: Some(color),
                    ..Default::default()
                },
            )
        })
        .collect()
}

impl Viewer {
    pub(super) fn refresh_word_colors(&self, cx: &mut Context<Self>) {
        let Some(code) = self.code() else {
            return;
        };
        let colors = crate::appearance::diff_colors(cx);
        for (collection, ranges, color) in [
            (
                &code.decorations[0],
                &code.model.before_words,
                colors.word_removed,
            ),
            (
                &code.decorations[1],
                &code.model.after_words,
                colors.word_added,
            ),
        ] {
            collection.set(word_decorations(ranges, color.opacity(85. / 255.)), cx);
        }
    }

    pub(super) fn mount_file_editors(
        &mut self,
        prepared: PreparedFile,
        language: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> CodeComparison {
        let standard = self.make_standard_diff(prepared.standard, language.into(), cx);
        let [before_wrap, after_wrap] = self.editor_wraps();
        let mut make_editor = |text, wrap, cx: &mut Context<Self>| {
            cx.new(|cx| {
                EditorState::new(window, cx)
                    .language(language)
                    .line_number(self.line_numbers)
                    .tab_size(gpui::base::input::TabSize {
                        tab_size: self.settings.editor.tab_size,
                        hard_tabs: false,
                    })
                    .soft_wrap(wrap)
                    .folding(false)
                    .default_value(text)
            })
        };
        let before = make_editor(prepared.before, before_wrap, cx);
        let after = make_editor(prepared.after, after_wrap, cx);
        for (state, syntax) in [&before, &after].into_iter().zip(prepared.syntax) {
            state.update(cx, |editor, cx| {
                editor.set_highlighter_factory(syntax.into_factory(), cx)
            });
        }
        let colors = crate::appearance::diff_colors(cx);
        let decorations = [
            (
                &before,
                &prepared.model.before_words,
                colors.word_removed.opacity(85. / 255.),
            ),
            (
                &after,
                &prepared.model.after_words,
                colors.word_added.opacity(85. / 255.),
            ),
        ]
        .map(|(state, ranges, color)| {
            state.update(cx, |editor, cx| {
                editor.create_decorations_collection(word_decorations(ranges, color), cx)
            })
        });
        self.restore_standard_diff(&standard, cx);
        self.editor_observers = vec![
            cx.observe(&before, |_, _, cx| cx.notify()),
            cx.observe(&after, |_, _, cx| cx.notify()),
        ];
        CodeComparison {
            before,
            after,
            model: Rc::new(prepared.model),
            decorations,
            standard,
        }
    }
    pub(super) fn update_followed_file(
        &mut self,
        prepared: PreparedFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = self.selected_path().expect("a followed file has a path");
        let code = self.code().expect("a followed file has mounted editors");
        let before = code.before.clone();
        let after = code.after.clone();
        let wraps = self.editor_wraps();
        for (state, wrap) in [(&before, wraps[0]), (&after, wraps[1])] {
            state.update(cx, |editor, cx| editor.set_soft_wrap(wrap, window, cx));
        }
        if before.read(cx).value().as_ref() == prepared.before
            && after.read(cx).value().as_ref() == prepared.after
        {
            self.restore_prepared_reading(&path, &prepared.before, &prepared.after);
            return;
        }
        self.restore_prepared_reading(&path, &prepared.before, &prepared.after);
        for ((state, text), syntax) in [(&before, prepared.before), (&after, prepared.after)]
            .into_iter()
            .zip(prepared.syntax)
        {
            if state.read(cx).value().as_ref() != text {
                state.update(cx, |editor, cx| {
                    editor.set_value(text, window, cx);
                    editor.set_highlighter_factory(syntax.into_factory(), cx);
                });
            }
        }
        let code = self
            .code()
            .expect("updating text keeps its editor entities");
        let colors = crate::appearance::diff_colors(cx);
        for (collection, ranges, color) in [
            (
                &code.decorations[0],
                &prepared.model.before_words,
                colors.word_removed.opacity(85. / 255.),
            ),
            (
                &code.decorations[1],
                &prepared.model.after_words,
                colors.word_added.opacity(85. / 255.),
            ),
        ] {
            collection.set(word_decorations(ranges, color), cx);
        }
        let presentation_changed = self
            .selected_file
            .as_ref()
            .is_some_and(|selected| selected.file.kind().is_none())
            != self.view.is_single_file();
        // A different pane width needs the new frame's measured wrap map. Leave
        // its source anchors for the fluid/standard overlay to restore together.
        if !presentation_changed
            && (self.view.effective() == ViewMode::Fluid || self.view.is_single_file())
            && let Some((left, right, horizontal)) = self.live.pending_position.take()
        {
            // set_value updates the cached wrap map synchronously. Restore
            // against it before prepaint, rather than after a frame at line 1.
            let geometry = comparison_geometry(before.read(cx), after.read(cx), &prepared.model);
            self.fluid.position = geometry.position_for_line(&prepared.model, left, true);
            for (state, positions, line) in [
                (
                    &before,
                    &geometry.before,
                    if self.view.is_single_file() {
                        right
                    } else {
                        left
                    },
                ),
                (&after, &geometry.after, right),
            ] {
                state.update(cx, |editor, cx| {
                    editor.set_scroll_offset(
                        point(
                            px(-horizontal),
                            px(-positions[line.min(positions.len() - 1)]),
                        ),
                        cx,
                    )
                });
            }
            self.horizontal_position = horizontal;
        }
        let standard =
            self.make_standard_diff(prepared.standard, before.read(cx).language_name(), cx);
        self.restore_standard_diff(&standard, cx);
        if let Some(SelectedFile {
            content: LoadState::Loaded(code),
            ..
        }) = &mut self.selected_file
        {
            code.model = Rc::new(prepared.model);
            code.standard = standard;
        }
        self.active_change = None;
        self.navigation_target = None;
    }

    fn make_standard_diff(
        &self,
        files: Result<Vec<DiffFile>, String>,
        language: SharedString,
        cx: &mut Context<Self>,
    ) -> Result<Option<Entity<DiffState>>, String> {
        files.map(|files| {
            if files.is_empty() {
                return None;
            }
            let mode = standard_mode(self.view.effective());
            Some(cx.new(|cx| {
                DiffState::new(
                    files
                        .into_iter()
                        .map(|file| file.with_language(language.clone())),
                    cx,
                )
                .with_context_lines(None)
                .with_mode(mode)
            }))
        })
    }
    fn restore_standard_diff(
        &mut self,
        standard: &Result<Option<Entity<DiffState>>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.view.effective() == ViewMode::Fluid {
            return;
        }
        let Ok(Some(state)) = standard else {
            return;
        };
        let Some(path) = state
            .read(cx)
            .files()
            .first()
            .map(|file| file.path().clone())
        else {
            return;
        };
        let Some((left, right, horizontal)) = self.live.pending_position.take() else {
            return;
        };
        let side = self.live.standard_side;
        let line = if side == gpui::component::diff::DiffSide::Original {
            left
        } else {
            right
        };
        state.update(cx, |state, cx| {
            state.scroll_to_line(
                gpui::component::diff::DiffLinePosition::new(path, side, line + 1),
                cx,
            )
        });
        self.config.pending_horizontal = Some(horizontal);
    }
}
