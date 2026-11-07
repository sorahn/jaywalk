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
use std::cell::Cell;
use std::rc::Rc;

use crate::Arguments;

mod comparison;
mod files;
mod fluid_view;
mod geometry;
mod history;
mod layout;
use geometry::{comparison_geometry, horizontal_wheel};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ViewMode {
    Inline,
    SideBySide,
    #[default]
    Fluid,
}

impl ViewMode {
    fn standard(self) -> DiffMode {
        match self {
            Self::Inline => DiffMode::Unified,
            Self::SideBySide | Self::Fluid => DiffMode::Split,
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
    repository: LoadState<RepositorySession>,
    selected_file: Option<SelectedFile>,
    repository_generation: u64,
    file_generation: u64,
    repository_task: Option<Task<()>>,
    file_task: Option<Task<()>>,
    active_change: Option<usize>,
    navigation_target: Option<usize>,
    view_mode: ViewMode,
    wrap: bool,
    horizontal_position: f32,
    reveal_after_layout: bool,
    fluid: FluidScroll,
    editor_observers: Vec<Subscription>,
}

impl Viewer {
    pub(crate) fn new(arguments: Arguments, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let preferred_path = arguments.file.clone();
        let mut viewer = Self {
            arguments,
            repository: LoadState::Loading,
            selected_file: None,
            repository_generation: 0,
            file_generation: 0,
            repository_task: None,
            file_task: None,
            active_change: None,
            navigation_target: None,
            view_mode: ViewMode::default(),
            wrap: false,
            horizontal_position: 0.,
            reveal_after_layout: false,
            fluid: FluidScroll::default(),
            editor_observers: Vec::new(),
        };
        viewer.load_repository(preferred_path, window, cx);
        viewer
    }

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

    fn load_repository(
        &mut self,
        preferred_path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.clear_file();
        self.repository = LoadState::Loading;
        let arguments = self.arguments.clone();
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let comparison = Comparison::load(
                        &arguments.directory,
                        arguments.base.as_deref(),
                        &arguments.tip,
                    )?;
                    let history = comparison.history();
                    Ok::<_, String>((comparison, history))
                })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
                viewer.repository = match result {
                    Ok((comparison, history)) => {
                        let branch = Rc::new(comparison);
                        LoadState::Loaded(RepositorySession {
                            branch: branch.clone(),
                            history: history.map(Rc::new),
                            scope: ComparisonScope::WholeBranch,
                            comparison: LoadState::Loaded(branch),
                        })
                    }
                    Err(error) => LoadState::Failed(error),
                };
                if let Some(path) = preferred_path {
                    viewer.select_path(&path, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_path(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.comparison().and_then(|comparison| {
            comparison
                .files
                .iter()
                .position(|file| file.after_path == path)
        }) {
            self.select_file(index, window, cx);
        }
    }

    fn select_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(comparison) = self.comparison() else {
            return;
        };
        let Some(file) = comparison.files.get(index).cloned() else {
            return;
        };
        let comparison = comparison.clone();
        self.clear_file();
        let generation = self.file_generation;
        self.selected_file = Some(SelectedFile {
            index,
            content: LoadState::Loading,
        });
        self.fluid = FluidScroll::default();
        self.horizontal_position = 0.;
        self.reveal_after_layout = false;
        self.file_task = Some(cx.spawn_in(window, async move |this, cx| {
            let extension = std::path::Path::new(&file.after_path)
                .extension()
                .and_then(|value| value.to_str());
            let language = match extension {
                Some("ts") => "typescript",
                Some("tsx") => "tsx",
                Some("js" | "jsx") => "javascript",
                _ => "plain",
            };
            let result = cx
                .background_spawn(async move { PreparedFile::load(&comparison, &file) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.file_generation != generation {
                    return;
                }
                let content = match result {
                    Err(error) => LoadState::Failed(error),
                    Ok(prepared) => {
                        let standard = prepared.standard.map(|files| {
                            if files.is_empty() {
                                return None;
                            }
                            let mode = viewer.view_mode.standard();
                            Some(cx.new(|cx| {
                                DiffState::new(
                                    files.into_iter().map(|file| file.with_language(language)),
                                    cx,
                                )
                                .with_context_lines(None)
                                .with_mode(mode)
                            }))
                        });
                        let mut make_editor = |text, cx: &mut Context<Self>| {
                            cx.new(|cx| {
                                EditorState::new(window, cx)
                                    .language(language)
                                    .soft_wrap(viewer.wrap)
                                    .folding(false)
                                    .default_value(text)
                            })
                        };
                        let before = make_editor(prepared.before, cx);
                        let after = make_editor(prepared.after, cx);
                        viewer.editor_observers = vec![
                            cx.observe(&before, |_, _, cx| cx.notify()),
                            cx.observe(&after, |_, _, cx| cx.notify()),
                        ];
                        LoadState::Loaded(CodeComparison {
                            before,
                            after,
                            model: Rc::new(prepared.model),
                            standard,
                        })
                    }
                };
                viewer.selected_file = Some(SelectedFile { index, content });
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn show_whole_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferred_path = self
            .selected_index()
            .and_then(|index| self.comparison()?.files.get(index))
            .map(|file| file.after_path.clone());
        self.repository_generation += 1;
        self.repository_task = None;
        self.clear_file();
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::WholeBranch;
            session.comparison = LoadState::Loaded(session.branch.clone());
        }
        if let Some(path) = preferred_path {
            self.select_path(&path, window, cx);
        }
        cx.notify();
    }

    fn select_revision(&mut self, commit_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let Ok(history) = &session.history else {
            return;
        };
        let Some(revision) = history
            .revisions
            .iter()
            .find(|revision| revision.commit_id == commit_id)
            .cloned()
        else {
            return;
        };
        let branch = session.branch.as_ref().clone();
        let preferred_path = self
            .selected_index()
            .and_then(|index| self.comparison()?.files.get(index))
            .map(|file| file.after_path.clone());
        self.repository_generation += 1;
        let generation = self.repository_generation;
        self.clear_file();
        if let LoadState::Loaded(session) = &mut self.repository {
            session.scope = ComparisonScope::Revision {
                commit_id: revision.commit_id.clone(),
                merge: revision.parents.len() > 1,
            };
            session.comparison = LoadState::Loading;
        }
        self.repository_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { branch.for_revision(&revision) })
                .await;
            let _ = this.update_in(cx, |viewer, window, cx| {
                if viewer.repository_generation != generation {
                    return;
                }
                if let LoadState::Loaded(session) = &mut viewer.repository {
                    session.comparison = match result {
                        Ok(comparison) => LoadState::Loaded(Rc::new(comparison)),
                        Err(error) => LoadState::Failed(error),
                    };
                }
                if let Some(path) = preferred_path {
                    viewer.select_path(&path, window, cx);
                }
                cx.notify();
            });
        }));
        cx.notify();
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

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferred_path = self
            .selected_index()
            .and_then(|index| self.comparison()?.files.get(index))
            .map(|file| file.after_path.clone());
        self.load_repository(preferred_path, window, cx);
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
        self.wrap = !self.wrap;
        self.horizontal_position = 0.;
        self.reveal_after_layout = self.code().is_some();
        if let Some(CodeComparison { before, after, .. }) = self.code() {
            for state in [before, after] {
                state.update(cx, |state, cx| {
                    state.set_soft_wrap(self.wrap, window, cx);
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
        if self.wrap {
            return;
        }
        if self.view_mode == ViewMode::Fluid {
            self.horizontal_position = (self.horizontal_position + delta).max(0.);
            if let Some(CodeComparison { before, after, .. }) = self.code() {
                for state in [before, after] {
                    state.update(cx, |state, cx| {
                        let offset = state.scroll_offset();
                        state.set_scroll_offset(point(px(-self.horizontal_position), offset.y), cx);
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
            state.update(cx, |state, cx| state.set_mode(mode.standard(), cx));
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
