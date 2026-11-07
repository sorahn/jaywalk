use gpui::assets::IconName;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui::component::input::{Editor, EditorState};
use gpui::component::resizable::{h_resizable, resizable_panel};
use gpui::component::{Disableable, Selectable, StyledExt};
use gpui::*;
use jaywalk::diff_model::FileDiff;
use jaywalk::fluid::{DiffGeometry, FluidScroll};
use jaywalk::repository::Comparison;
use jaywalk::settings::{DiffSettings, DiffView as ViewMode, Settings};
use std::cell::Cell;
use std::rc::Rc;

use crate::Arguments;

mod comparison;
mod files;
mod fluid_view;
mod geometry;
mod history;
mod layout;
mod loading;
mod panels;
use geometry::{comparison_geometry, horizontal_wheel};

fn standard_mode(view: ViewMode) -> DiffMode {
    match view {
        ViewMode::Inline => DiffMode::Unified,
        ViewMode::SideBySide | ViewMode::Fluid => DiffMode::Split,
    }
}

#[derive(Clone, Copy)]
struct Wrapping {
    before: bool,
    after: bool,
    inline: bool,
}

impl Wrapping {
    fn from_settings(settings: &DiffSettings) -> Self {
        Self {
            before: settings.before.word_wrap,
            after: settings.after.word_wrap,
            inline: settings.inline.word_wrap,
        }
    }

    fn enabled(self, view: ViewMode) -> bool {
        match view {
            ViewMode::Inline => self.inline,
            ViewMode::SideBySide | ViewMode::Fluid => self.before && self.after,
        }
    }

    fn toggle(&mut self, view: ViewMode) {
        let enabled = !self.enabled(view);
        match view {
            ViewMode::Inline => self.inline = enabled,
            ViewMode::SideBySide | ViewMode::Fluid => {
                self.before = enabled;
                self.after = enabled;
            }
        }
    }
}

fn standard_patch(before: &str, after: &str) -> String {
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(before.lines().count().max(after.lines().count()))
        .header("a/source", "b/source")
        .to_string()
}

enum LoadState<T> {
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> LoadState<T> {
    fn ready(&self) -> Option<&T> {
        if let Self::Loaded(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum ComparisonScope {
    WholeBranch,
    Revision { commit_id: String, merge: bool },
}

struct RepositorySession {
    branch: Rc<Comparison>,
    history: Result<Rc<jaywalk::history::History>, String>,
    scope: ComparisonScope,
    comparison: LoadState<Rc<Comparison>>,
}

struct CodeComparison {
    before: Entity<EditorState>,
    after: Entity<EditorState>,
    model: Rc<FileDiff>,
    standard: Result<Option<Entity<DiffState>>, String>,
}

struct SelectedFile {
    index: usize,
    content: LoadState<CodeComparison>,
}

struct PreparedFile {
    before: String,
    after: String,
    model: FileDiff,
    standard: Result<Vec<DiffFile>, String>,
}

impl PreparedFile {
    fn load(
        comparison: &Comparison,
        file: &jaywalk::repository::ChangedFile,
    ) -> Result<Self, String> {
        let (before, after) = comparison.contents(file)?;
        let model = FileDiff::new(&before, &after);
        let standard =
            DiffFile::parse(&standard_patch(&before, &after)).map_err(|error| error.to_string());
        Ok(Self {
            before,
            after,
            model,
            standard,
        })
    }
}

gpui::actions!(jaywalk, [ShowWholeBranch]);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "secondary-shift-b",
        ShowWholeBranch,
        Some("Jaywalk"),
    )]);
}

pub(crate) struct Viewer {
    arguments: Arguments,
    settings: Settings,
    focus: FocusHandle,
    panels: panels::PanelLayout,
    history_scroll: UniformListScrollHandle,
    repository: LoadState<RepositorySession>,
    selected_file: Option<SelectedFile>,
    browser: files::FileBrowser,
    repository_generation: u64,
    file_generation: u64,
    repository_task: Option<Task<()>>,
    file_task: Option<Task<()>>,
    active_change: Option<usize>,
    navigation_target: Option<usize>,
    view_mode: ViewMode,
    wrapping: Wrapping,
    horizontal_position: f32,
    reveal_after_layout: bool,
    fluid: FluidScroll,
    editor_observers: Vec<Subscription>,
}

impl Viewer {
    fn comparison(&self) -> Option<&Comparison> {
        self.repository.ready()?.comparison.ready().map(Rc::as_ref)
    }

    fn comparison_state(&self) -> LoadState<&Comparison> {
        match &self.repository {
            LoadState::Loading => LoadState::Loading,
            LoadState::Failed(error) => LoadState::Failed(error.clone()),
            LoadState::Loaded(session) => match &session.comparison {
                LoadState::Loading => LoadState::Loading,
                LoadState::Failed(error) => LoadState::Failed(error.clone()),
                LoadState::Loaded(comparison) => LoadState::Loaded(comparison),
            },
        }
    }

    fn selected_index(&self) -> Option<usize> {
        self.selected_file.as_ref().map(|file| file.index)
    }

    fn code(&self) -> Option<&CodeComparison> {
        self.selected_file.as_ref()?.content.ready()
    }

    fn clear_file(&mut self) {
        self.file_generation += 1;
        self.file_task = None;
        self.selected_file = None;
        self.active_change = None;
        self.navigation_target = None;
        self.editor_observers.clear();
    }

    fn scope_label(&self) -> String {
        match self.repository.ready().map(|session| &session.scope) {
            Some(ComparisonScope::WholeBranch) => "Whole branch".into(),
            Some(ComparisonScope::Revision { commit_id, merge }) => format!(
                "Commit {}{}",
                &commit_id[..commit_id.len().min(8)],
                if *merge { " · first parent" } else { "" }
            ),
            None => "Repository".into(),
        }
    }

    fn scroll_comparison(&mut self, delta: f32, cx: &mut Context<Self>) {
        if let Some(CodeComparison {
            before,
            after,
            model,
            ..
        }) = self.code()
        {
            let geometry = comparison_geometry(before.read(cx), after.read(cx), model);
            let viewport = f32::from(before.read(cx).input_bounds().size.height);
            let before = before.clone();
            let after = after.clone();
            let model = model.clone();
            let (before_y, after_y) = self.fluid.advance(delta, &model, &geometry, viewport);
            before.update(cx, |editor, cx| {
                let offset = editor.scroll_offset();
                editor.set_scroll_offset(point(offset.x, px(-before_y)), cx);
            });
            after.update(cx, |editor, cx| {
                let offset = editor.scroll_offset();
                editor.set_scroll_offset(point(offset.x, px(-after_y)), cx);
            });
        }
        cx.notify();
    }

    fn toggle_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.wrapping.toggle(self.view_mode);
        self.horizontal_position = 0.;
        self.reveal_after_layout = self.code().is_some();
        if let Some(CodeComparison { before, after, .. }) = self.code() {
            for (state, wrap) in [(before, self.wrapping.before), (after, self.wrapping.after)] {
                state.update(cx, |state, cx| {
                    state.set_soft_wrap(wrap, window, cx);
                    let offset = state.scroll_offset();
                    state.set_scroll_offset(point(px(0.), offset.y), cx);
                });
            }
        }
        if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
            state.update(cx, |state, cx| state.scroll_horizontal(px(-1e9), cx));
        }
        cx.notify();
    }

    fn scroll_horizontal(&mut self, delta: f32, cx: &mut Context<Self>) {
        if self.wrapping.enabled(self.view_mode) {
            return;
        }
        if self.view_mode == ViewMode::Fluid {
            self.horizontal_position = (self.horizontal_position + delta).max(0.);
            if let Some(CodeComparison { before, after, .. }) = self.code() {
                for (state, wrap) in [(before, self.wrapping.before), (after, self.wrapping.after)]
                {
                    state.update(cx, |state, cx| {
                        let offset = state.scroll_offset();
                        let x = if wrap {
                            px(0.)
                        } else {
                            px(-self.horizontal_position)
                        };
                        state.set_scroll_offset(point(x, offset.y), cx);
                    });
                }
            }
        } else if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
            state.update(cx, |state, cx| state.scroll_horizontal(px(delta), cx));
        }
        cx.notify();
    }

    fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.view_mode = mode;
        if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
            state.update(cx, |state, cx| state.set_mode(standard_mode(mode), cx));
        }
        cx.notify();
    }

    fn navigate_change(&mut self, direction: i32, cx: &mut Context<Self>) {
        if self.view_mode != ViewMode::Fluid {
            if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
                state.update(cx, |state, cx| {
                    if direction < 0 {
                        state.previous_change(cx);
                    } else {
                        state.next_change(cx);
                    }
                });
            }
            return;
        }
        let Some(CodeComparison {
            before,
            after,
            model,
            ..
        }) = self.code()
        else {
            return;
        };
        let count = model.blocks.iter().filter(|block| block.changed).count();
        if count == 0 {
            return;
        }
        let target = (self.active_change.unwrap_or(0) as i32 + direction).clamp(0, count as i32 - 1)
            as usize;
        let viewport = f32::from(before.read(cx).input_bounds().size.height);
        let geometry = comparison_geometry(before.read(cx), after.read(cx), model);
        let position = geometry.change_position(model, target);
        let delta = position - viewport / 2. - self.fluid.position;
        self.navigation_target = Some(target);
        self.active_change = Some(target);
        self.scroll_comparison(delta, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{DiffFile, horizontal_wheel, standard_patch};
    use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent, point, px};

    #[test]
    fn shift_wheel_handles_native_axis_remapping_without_doubling() {
        let mut event = ScrollWheelEvent {
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            delta: ScrollDelta::Pixels(point(px(0.), px(-60.))),
            ..Default::default()
        };
        assert_eq!(horizontal_wheel(&event, px(20.)), Some(60.));
        event.delta = ScrollDelta::Pixels(point(px(-60.), px(0.)));
        assert_eq!(horizontal_wheel(&event, px(20.)), Some(60.));
        event.modifiers.shift = false;
        event.delta = ScrollDelta::Pixels(point(px(0.), px(-60.)));
        assert_eq!(horizontal_wheel(&event, px(20.)), None);
    }

    #[test]
    fn standard_widget_parses_full_context_and_unterminated_unicode_changes() {
        for (before, after, additions, deletions) in [
            ("same\nhéllo", "same\nworld", 1, 1),
            ("", "added", 1, 0),
            ("removed", "", 0, 1),
        ] {
            let files = DiffFile::parse(&standard_patch(before, after)).unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].additions(), additions);
            assert_eq!(files[0].deletions(), deletions);
        }
    }
}
