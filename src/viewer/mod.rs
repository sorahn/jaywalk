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
use jaywalk::syntax::file_language;
use std::cell::Cell;
use std::rc::Rc;

use crate::Arguments;

mod chrome;
pub(crate) mod commands;
mod comparison;
mod context_menu;
mod editor_presentation;
mod evolution;
mod history_menu;
mod review_context;
#[cfg(test)]
use editor_presentation::standard_patch;
use editor_presentation::{CodeComparison, PreparedFile};
mod feedback;
mod file_icons;
mod files;
mod fluid_scrollbar;
mod fluid_view;
mod footer;
mod geometry;
mod history;
mod keyboard;
mod layout;
mod live;
mod loading;
mod navigation;
mod overview;
mod palette;
mod panels;
mod reference_browser;
mod references;
mod review;
mod session_layout;
pub(crate) use session_layout::SessionLayout;
#[cfg(test)]
mod interaction_tests;
mod settings;
mod typography;
mod welcome;
use geometry::{comparison_geometry, horizontal_wheel};
use jaywalk::loading_feedback::LoadingKind;

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
    Aggregate,
    Revision {
        commit_id: String,
        change_id: String,
        working_copy: bool,
        merge: bool,
    },
    Evolution {
        anchor: String,
        version: String,
        label: String,
        mode: evolution::ReviewMode,
    },
}

struct RepositorySession {
    home: Rc<Comparison>,
    review: review_context::ReviewContext,
    history: Result<Rc<jaywalk::history::History>, String>,
    scope: ComparisonScope,
    comparison: LoadState<Rc<Comparison>>,
    revision_comparison: Option<Rc<Comparison>>,
}

struct SelectedFile {
    file: jaywalk::repository_file::FileEntry,
    source: Comparison,
    content: LoadState<CodeComparison>,
}

pub(crate) fn init(arguments: &Arguments, settings: &Settings, cx: &mut App) -> Result<(), String> {
    reference_browser::init(cx);
    commands::init(arguments, settings, cx)
}

pub(crate) struct Viewer {
    arguments: Arguments,
    home_source: review_context::HomeSource,
    review_picker: review_context::ReviewPicker,
    welcome: Option<welcome::WelcomeState>,
    recent_recorded: bool,
    settings: Settings,
    focus: FocusHandle,
    panels: panels::PanelLayout,
    overview: overview::OverviewState,
    navigation: navigation::NavigationState,
    history_scroll: UniformListScrollHandle,
    history_query: jaywalk::history::HistoryQuery,
    references: references::ReferenceState,
    evolution: evolution::EvolutionPanel,
    evolution_header: Option<evolution::HeaderPresentation>,
    file_evolution: evolution::EvolutionPanel,
    active_evolution: evolution::EvolutionKind,
    repository: LoadState<RepositorySession>,
    selected_file: Option<SelectedFile>,
    pending_file: Option<String>,
    browser: files::FileBrowser,
    requests: review::ReviewRequests,
    active_change: Option<usize>,
    navigation_target: Option<usize>,
    view: jaywalk::view_choice::ViewChoice,
    code_font_size: f32,
    line_numbers: bool,
    collapse_unchanged: bool,
    wrapping: Wrapping,
    horizontal_position: f32,
    reveal_after_layout: bool,
    fluid: FluidScroll,
    fluid_drag: Option<f32>,
    editor_observers: Vec<Subscription>,
    live: live::LiveState,
    feedback: feedback::Feedback,
    palette: palette::PaletteState,
    layout: session_layout::SessionLayout,
    config: settings::ConfigState,
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
        let file = &self.selected_file.as_ref()?.file;
        jaywalk::repository_file::find_entry(&self.browser.files, file.path())
    }

    fn selected_changed_index(&self) -> Option<usize> {
        let file = &self.selected_file.as_ref()?.file;
        jaywalk::file_browser::find_file(self.comparison()?.files(), file.path())
    }

    fn selected_path(&self) -> Option<String> {
        self.selected_file
            .as_ref()
            .map(|selected| selected.file.path().to_owned())
    }

    fn code(&self) -> Option<&CodeComparison> {
        self.selected_file.as_ref()?.content.ready()
    }

    fn clear_file(&mut self) {
        self.config.pending_standard = None;
        self.config.pending_horizontal = None;
        self.requests.invalidate_file();
        self.requests.file_task = None;
        self.finish_loading(LoadingKind::File);
        self.selected_file = None;
        self.view
            .present(jaywalk::view_choice::FilePresentation::Comparison);
        self.live.pending_position = None;
        self.live.restore = None;
        self.active_change = None;
        self.navigation_target = None;
        self.editor_observers.clear();
    }

    fn is_interdiff(&self) -> bool {
        matches!(
            self.repository.ready().map(|session| &session.scope),
            Some(ComparisonScope::Evolution {
                mode: evolution::ReviewMode::SincePrevious,
                ..
            })
        )
    }

    fn supports_evolution(&self) -> bool {
        self.repository
            .ready()
            .is_some_and(|session| session.home.supports_evolution())
    }

    fn scope_label(&self) -> String {
        if self.arguments.diff.is_some() {
            return "External diff".into();
        }
        match self.repository.ready().map(|session| &session.scope) {
            Some(ComparisonScope::Aggregate) => self
                .repository
                .ready()
                .expect("loaded scope has a session")
                .review
                .source
                .label(),
            Some(ComparisonScope::Revision {
                working_copy: true, ..
            }) if !self.supports_evolution() => {
                format!(
                    "Working copy · {}",
                    self.repository
                        .ready()
                        .expect("loaded working copy has a review context")
                        .review
                        .source
                        .label()
                )
            }
            Some(ComparisonScope::Revision {
                commit_id, merge, ..
            }) => format!(
                "Commit {}{} · {}",
                &commit_id[..commit_id.len().min(8)],
                if *merge { " · first parent" } else { "" },
                self.repository
                    .ready()
                    .expect("loaded commit has a review context")
                    .review
                    .source
                    .label()
            ),
            Some(ComparisonScope::Evolution { label, version, .. }) => {
                format!("{label} · {}", &version[..version.len().min(8)])
            }
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

    fn editor_wraps(&self) -> [bool; 2] {
        if self.view.is_single_file() {
            [self.wrapping.inline; 2]
        } else {
            [self.wrapping.before, self.wrapping.after]
        }
    }

    fn toggle_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preserve_reflow_position(cx);
        self.wrapping.toggle(if self.selected_file.is_none() {
            ViewMode::Inline
        } else {
            self.view.effective()
        });
        self.horizontal_position = 0.;
        self.reveal_after_layout = false;
        if let Some(CodeComparison { before, after, .. }) = self.code() {
            for (state, wrap) in [
                (before, self.editor_wraps()[0]),
                (after, self.editor_wraps()[1]),
            ] {
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
        self.save_layout(cx);
    }

    fn scroll_horizontal(&mut self, delta: f32, cx: &mut Context<Self>) {
        if self.wrapping.enabled(self.view.effective()) {
            return;
        }
        if self.view.effective() == ViewMode::Fluid {
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
        } else if let Some(code) = self.code() {
            code.before.update(cx, |editor, cx| {
                let offset = editor.scroll_offset();
                editor.set_scroll_offset(point((offset.x - px(delta)).min(px(0.)), offset.y), cx);
            });
        }
        cx.notify();
    }

    fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        if self.view.preferred() == mode {
            return;
        }
        let line = self.code().map(|code| {
            if self.view.effective() != ViewMode::Fluid
                && let Ok(Some(state)) = &code.standard
                && let Some(position) = state.read(cx).viewport_position()
            {
                return position.line().saturating_sub(1);
            }
            let state = if self.view.is_single_file() {
                code.before.read(cx)
            } else {
                code.after.read(cx)
            };
            state.visible_row_range().map_or(0, |range| range.start)
        });
        self.view.select(mode);
        let mode = self.view.effective();
        if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
            state.update(cx, |state, cx| {
                state.set_mode(standard_mode(mode), cx);
                if let Some(line) = line
                    && mode != ViewMode::Fluid
                {
                    let path = state.files().first().map(|file| file.path().clone());
                    if let Some(path) = path {
                        state.restore_viewport_position(
                            gpui::component::diff::DiffLinePosition::new(
                                path,
                                gpui::component::diff::DiffSide::Modified,
                                line + 1,
                            ),
                            cx,
                        );
                    }
                }
            });
        }
        if mode == ViewMode::Fluid
            && let Some(line) = line
        {
            let before_line = self
                .code()
                .map_or(line, |code| code.model.corresponding_line(line, false));
            self.live.pending_position = Some((before_line, line, self.horizontal_position));
        }
        cx.notify();
        self.save_layout(cx);
    }

    fn reveal_initial_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.reveal_after_layout {
            return;
        }
        self.reveal_after_layout = false;
        let generation = self.requests.file();
        let viewer = cx.entity().downgrade();
        // Editor paint can clear a scroll requested during that same paint.
        // Wait until measured wrapping and the widget's scroll state settle.
        window.defer(cx, move |_, cx| {
            let _ = viewer.update(cx, |viewer, cx| {
                if viewer.requests.file() == generation {
                    viewer.navigate_change(0, cx);
                }
            });
        });
    }

    fn standard_context_collapsed(&self) -> bool {
        self.selected_file.is_some()
            && self.view.effective() != ViewMode::Fluid
            && self.collapse_unchanged
    }

    fn apply_standard_context(&mut self, cx: &mut Context<Self>) {
        if let Some(Ok(Some(state))) = self.code().map(|code| &code.standard) {
            state.update(cx, |state, cx| {
                state.set_context_lines(
                    self.collapse_unchanged
                        .then_some(self.settings.diff.context_lines),
                    cx,
                );
                state.set_expansion_lines(self.settings.diff.expansion_lines, cx);
            });
        }
        self.reveal_after_layout = false;
        cx.notify();
    }

    fn toggle_unchanged_lines(&mut self, cx: &mut Context<Self>) {
        if self.view.effective() == ViewMode::Fluid || self.code().is_none() {
            return;
        }
        self.collapse_unchanged = !self.collapse_unchanged;
        self.apply_standard_context(cx);
        self.save_layout(cx);
    }

    fn navigate_change(&mut self, direction: i32, cx: &mut Context<Self>) {
        if self.standard_context_collapsed() {
            return;
        }
        if self.selected_file.is_none() {
            self.navigate_overview_change(direction, cx);
            return;
        }
        if self.view.effective() != ViewMode::Fluid {
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
        let geometry = comparison_geometry(before.read(cx), after.read(cx), model);
        let current = self.active_change.unwrap_or(0).min(count - 1);
        let block = model
            .blocks
            .iter()
            .filter(|block| block.changed)
            .nth(current)
            .expect("a clamped change index has a corresponding block");
        let intersects =
            |state: &EditorState, positions: &[f32], lines: &std::ops::Range<usize>| {
                !lines.is_empty()
                    && positions[lines.end] + f32::from(state.scroll_offset().y) > 0.
                    && positions[lines.start] + f32::from(state.scroll_offset().y)
                        < f32::from(state.input_bounds().size.height)
            };
        let visible = intersects(before.read(cx), &geometry.before, &block.before)
            || intersects(after.read(cx), &geometry.after, &block.after);
        let target = geometry::change_target(self.active_change, direction, count, visible)
            .expect("a nonempty change list has a navigation target");
        let viewport = f32::from(before.read(cx).input_bounds().size.height);
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

#[cfg(test)]
mod performance_tests;
