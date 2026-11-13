//! Toolkit command UI; actions and file results preserve their original subject.
use super::*;
use gpui::component::command::{Command, CommandItem, CommandState};
use gpui::prelude::FluentBuilder;

#[derive(Clone, Copy)]
pub(super) enum PaletteKind {
    Files,
    Commands,
    Repositories,
    Profiles,
    Reviews,
}
#[derive(Clone)]
enum Choice {
    File {
        path: String,
        status: Option<String>,
        source: Rc<jaywalk::repository::ComparisonIdentity>,
    },
    Command(jaywalk::commands::CommandId),
    Review(review_context::ReviewChoice),
    Profile(String),
    Repository {
        directory: std::path::PathBuf,
        order: usize,
    },
}
pub(super) struct PaletteState {
    state: Entity<CommandState>,
    open: Option<PaletteKind>,
    pub(super) profiles: Vec<String>,
}
impl PaletteState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Viewer>) -> Self {
        Self {
            state: cx.new(|cx| CommandState::new(window, cx)),
            open: None,
            profiles: Vec::new(),
        }
    }
}
fn fuzzy_score(query: &str, label: &str) -> Option<usize> {
    let text = label.to_lowercase();
    let query = query.to_lowercase();
    let mut remaining = text.as_str();
    let mut score = 0;
    for needle in query.chars().filter(|character| !character.is_whitespace()) {
        let offset = remaining.find(needle)?;
        score += offset;
        remaining = &remaining[offset + needle.len_utf8()..];
    }
    Some(score)
}
fn repository_choices(directories: &[std::path::PathBuf]) -> Vec<(String, Choice)> {
    let mut choices: Vec<_> = directories
        .iter()
        .enumerate()
        .map(|(order, directory)| {
            (
                crate::recents::display_path(directory),
                Choice::Repository {
                    directory: directory.clone(),
                    order,
                },
            )
        })
        .collect();
    choices.push((
        jaywalk::commands::CommandId::OpenRepository
            .label()
            .to_owned(),
        Choice::Command(jaywalk::commands::CommandId::OpenRepository),
    ));
    choices
}
// Prefer the repository name; parent directories remain searchable as a fallback.
fn recent_rank(query: &str, directory: &std::path::Path) -> Option<(usize, usize)> {
    let query: String = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if query.is_empty() {
        return Some((0, 0));
    }
    if let Some(name) = directory.file_name() {
        let name = name.to_string_lossy().to_lowercase();
        if name == query {
            return Some((0, 0));
        }
        if name.starts_with(&query) {
            return Some((1, 0));
        }
        if let Some(offset) = name.find(&query) {
            return Some((2, offset));
        }
        if let Some(score) = fuzzy_score(&query, &name) {
            return Some((3, score));
        }
    }
    let display = crate::recents::display_path(directory);
    [
        fuzzy_score(&query, &display),
        fuzzy_score(&query, &directory.to_string_lossy()),
    ]
    .into_iter()
    .flatten()
    .min()
    .map(|score| (4, score))
}

fn rank_choices(
    kind: PaletteKind,
    query: &str,
    choices: Vec<(String, Choice)>,
) -> Vec<(String, Choice)> {
    let mut ranked: Vec<_> = choices
        .into_iter()
        .filter_map(|(label, choice)| {
            let rank = match &choice {
                Choice::File { path, status, .. } => file_rank(query, path, status.is_some())
                    .map(|(score, unchanged)| (0, score, unchanged)),
                Choice::Command(jaywalk::commands::CommandId::OpenRepository)
                    if matches!(kind, PaletteKind::Repositories) =>
                {
                    Some((5, 0, false))
                }
                Choice::Command(_) | Choice::Profile(_) | Choice::Review(_) => {
                    fuzzy_score(query, &label).map(|score| (0, score, false))
                }
                Choice::Repository { directory, .. } => {
                    recent_rank(query, directory).map(|(category, score)| (category, score, false))
                }
            }?;
            Some((rank, label, choice))
        })
        .collect();
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            if matches!(kind, PaletteKind::Repositories) {
                let order = |choice: &Choice| match choice {
                    Choice::Repository { order, .. } => *order,
                    _ => usize::MAX,
                };
                order(&a.2).cmp(&order(&b.2))
            } else {
                a.1.cmp(&b.1)
            }
        })
    });
    ranked
        .into_iter()
        .map(|(_, label, choice)| (label, choice))
        .collect()
}

// Status is a tie-breaker only; neither status text nor UI labels are searchable.
fn file_rank(query: &str, path: &str, changed: bool) -> Option<(usize, bool)> {
    fuzzy_score(query, path).map(|score| (score, !changed))
}

#[derive(Debug, PartialEq, Eq)]
enum HistoryStep {
    Overview,
    Revision(usize),
}

fn history_step(current: Option<usize>, delta: i32, count: usize) -> Option<HistoryStep> {
    if count == 0 {
        return None;
    }
    match current {
        None if delta > 0 => Some(HistoryStep::Revision(0)),
        None => None,
        Some(0) if delta < 0 => Some(HistoryStep::Overview),
        Some(index) => {
            let next = index.saturating_add_signed(delta as isize).min(count - 1);
            (next != index).then_some(HistoryStep::Revision(next))
        }
    }
}

impl Viewer {
    #[cfg(test)]
    pub(super) fn palette_test_state(
        &self,
        cx: &App,
    ) -> (Option<PaletteKind>, String, Option<usize>) {
        let state = self.palette.state.read(cx);
        (
            self.palette.open,
            state.query(cx).to_string(),
            state.selected_index().map(|index| index.row),
        )
    }
    pub(super) fn open_palette(
        &mut self,
        kind: PaletteKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette.open = Some(kind);
        self.palette
            .state
            .update(cx, |state, cx| state.set_query("", window, cx));
        self.palette
            .state
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }
    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette.open = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }
    pub(super) fn open_selected_recent_in_new_window(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.palette.open, Some(PaletteKind::Repositories)) {
            return;
        }
        let state = self.palette.state.read(cx);
        let Some(index) = state.selected_index() else {
            return;
        };
        let choices = rank_choices(
            PaletteKind::Repositories,
            &state.query(cx),
            repository_choices(&cx.global::<crate::recents::Recents>().repositories),
        );
        let Some((_, choice)) = choices.get(index.row).cloned() else {
            return;
        };
        self.close_palette(window, cx);
        match choice {
            Choice::Repository { directory, .. } => {
                self.open_repository_path(directory, crate::windows::OpenDisposition::NewWindow, cx)
            }
            Choice::Command(jaywalk::commands::CommandId::OpenRepository) => {
                crate::choose_repository_in_new_window(
                    self.arguments.clone(),
                    Some(cx.entity().downgrade()),
                    cx,
                )
            }
            _ => {}
        }
    }
    pub(super) fn render_palette(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = self.palette.open?;
        let query = self.palette.state.read(cx).query(cx);
        let choices: Vec<(String, Choice)> = match kind {
            PaletteKind::Files => self
                .comparison()
                .map(|comparison| {
                    let source = Rc::new(comparison.identity());
                    comparison
                        .catalog()
                        .entries()
                        .map(|file| {
                            (
                                file.path().to_owned(),
                                Choice::File {
                                    path: file.path().to_owned(),
                                    status: file.status().map(str::to_owned),
                                    source: source.clone(),
                                },
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            PaletteKind::Repositories => {
                repository_choices(&cx.global::<crate::recents::Recents>().repositories)
            }
            PaletteKind::Profiles => self
                .palette
                .profiles
                .iter()
                .map(|name| (name.clone(), Choice::Profile(name.clone())))
                .collect(),
            PaletteKind::Reviews => self
                .review_picker
                .choices
                .iter()
                .map(|(label, choice)| (label.clone(), Choice::Review(choice.clone())))
                .collect(),
            PaletteKind::Commands => jaywalk::commands::CommandId::ALL
                .iter()
                .filter(|command| {
                    !matches!(command, jaywalk::commands::CommandId::OpenInNewWindow)
                        && (!self.is_welcome() || command.available_without_repository())
                        && (self.supports_evolution()
                            || !matches!(
                                command,
                                jaywalk::commands::CommandId::ToggleChangeEvolution
                                    | jaywalk::commands::CommandId::ToggleFileEvolution
                                    | jaywalk::commands::CommandId::VersionPatch
                                    | jaywalk::commands::CommandId::SincePrevious
                            ))
                })
                .map(|command| (command.label().to_owned(), Choice::Command(*command)))
                .collect(),
        };
        let choices = rank_choices(kind, &query, choices);
        let choices = Rc::new(choices);
        let selected = choices.clone();
        let cancel = cx.listener(|this, _: &(), window, cx| this.close_palette(window, cx));
        let confirm = cx.listener(
            move |this, index: &gpui::component::IndexPath, window, cx| {
                let Some((_, choice)) = selected.get(index.row) else {
                    return;
                };
                let choice = choice.clone();
                this.close_palette(window, cx);
                match choice {
                    Choice::File { path, source, .. } => {
                        if this
                            .comparison()
                            .is_some_and(|comparison| comparison.identity() == *source)
                        {
                            this.select_path(&path, window, cx);
                        }
                    }
                    Choice::Command(command) => this.execute_command(command, window, cx),
                    Choice::Review(choice) => this.apply_review_choice(choice, window, cx),
                    Choice::Profile(name) => this.select_keybindings_profile(name, window, cx),
                    Choice::Repository { directory, .. } => this.open_repository_path(
                        directory,
                        crate::windows::OpenDisposition::Configured,
                        cx,
                    ),
                }
            },
        );
        let command = Command::new(&self.palette.state)
            .filterable(false)
            .placeholder(match kind {
                PaletteKind::Files => "Find any repository file…",
                PaletteKind::Commands => "Run a command…",
                PaletteKind::Repositories => "Search recent repositories…",
                PaletteKind::Profiles => "Select keybindings profile…",
                PaletteKind::Reviews => "Choose review branch or comparison base…",
            })
            .items(choices.iter().enumerate().map(|(index, (label, choice))| {
                let viewer = cx.entity().downgrade();
                let repository = match choice {
                    Choice::Repository { directory, .. } => Some(directory.clone()),
                    _ => None,
                };
                let label: SharedString = label.clone().into();
                let command = match choice {
                    Choice::Command(command) => Some(*command),
                    _ => None,
                };
                let status = match choice {
                    Choice::File { status, .. } => status.clone(),
                    Choice::Profile(name) if name == &self.settings.keyboard.profile => {
                        Some("Active".into())
                    }
                    _ => None,
                };
                CommandItem::new()
                    .label(label.clone())
                    .child(move |window, _| {
                        let shortcut = command.and_then(|command| {
                            gpui::component::kbd::Kbd::binding_for_action(
                                &commands::ExecuteCommand { command },
                                None,
                                window,
                            )
                        });
                        let viewer = viewer.clone();
                        let repository = repository.clone();
                        div()
                            .id(("palette-item-content", index))
                            .on_click(move |event: &ClickEvent, window, cx| {
                                if event.modifiers().secondary()
                                    && let Some(directory) = &repository
                                {
                                    cx.stop_propagation();
                                    let _ = viewer.update(cx, |viewer, cx| {
                                        viewer.close_palette(window, cx);
                                        viewer.open_repository_path(
                                            directory.clone(),
                                            crate::windows::OpenDisposition::NewWindow,
                                            cx,
                                        );
                                    });
                                }
                            })
                            .h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                            .children(
                                status
                                    .clone()
                                    .map(|status| div().flex_shrink_0().text_sm().child(status)),
                            )
                            .children(shortcut)
                    })
            }))
            .when(matches!(kind, PaletteKind::Repositories), |command| {
                command.footer(|_, window, _| {
                    div()
                        .h_flex()
                        .gap_2()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .children(gpui::component::kbd::Kbd::binding_for_action(
                            &commands::ExecuteCommand {
                                command: jaywalk::commands::CommandId::OpenInNewWindow,
                            },
                            None,
                            window,
                        ))
                        .child("Open in new window")
                })
            })
            .on_query(cx.listener(|_, _, _, cx| cx.notify()))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |index, window, cx| confirm(&index, window, cx));
        Some(
            div()
                .id("palette-overlay")
                .when(matches!(kind, PaletteKind::Repositories), |panel| {
                    panel.key_context("JaywalkRecent")
                })
                .absolute()
                .inset_0()
                .bg(rgba(0x00000070))
                .flex()
                .justify_center()
                .items_start()
                .pt_12()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
                )
                .child(
                    div()
                        .w(rems(40.))
                        .max_w_full()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(command)
                        .children(
                            matches!(kind, PaletteKind::Repositories)
                                .then(|| cx.global::<crate::recents::Recents>().error.as_ref())
                                .flatten()
                                .map(|error| feedback::error_message("recent-picker-error", error)),
                        ),
                )
                .into_any_element(),
        )
    }
    pub(super) fn navigate_revision(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.repository.ready() else {
            return;
        };
        let Ok(history) = &session.history else {
            return;
        };
        if history.revisions.is_empty() {
            return;
        }
        let selected = match &session.scope {
            ComparisonScope::Aggregate => None,
            ComparisonScope::Revision { commit_id, .. } => Some(commit_id),
            ComparisonScope::Evolution { anchor, .. } => Some(anchor),
        };
        let current = selected.and_then(|id| {
            history
                .revisions
                .iter()
                .position(|revision| &revision.commit_id == id)
        });
        let Some(step) = history_step(current, delta, history.revisions.len()) else {
            return;
        };
        let index = match step {
            HistoryStep::Overview => {
                self.show_all_changes_overview(window, cx);
                return;
            }
            HistoryStep::Revision(index) => index,
        };
        let commit = history.revisions[index].commit_id.clone();
        self.history_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        self.select_revision(&commit, window, cx);
    }

    pub(super) fn navigate_file(
        &mut self,
        delta: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(comparison) = self.comparison() else {
            return;
        };
        if comparison.files().is_empty() {
            return;
        }
        let current = self
            .selected_changed_index()
            .map(|index| index as i32)
            .unwrap_or(if delta > 0 {
                -1
            } else {
                comparison.files().len() as i32
            });
        let next = (current + delta).clamp(0, comparison.files().len() as i32 - 1) as usize;
        self.select_file(next, window, cx);
    }
}
#[cfg(test)]
mod tests {
    use super::{
        Choice, PaletteKind, file_rank, fuzzy_score, rank_choices, recent_rank, repository_choices,
    };
    #[test]
    fn recent_picker_preserves_mru_order_and_offers_open_when_history_is_empty() {
        let paths = vec![
            std::path::PathBuf::from("/repositories/zebra"),
            "/repositories/alpha".into(),
        ];
        let choices = || repository_choices(&paths);
        let ordered = rank_choices(PaletteKind::Repositories, "", choices());
        assert!(
            matches!(&ordered[0].1, Choice::Repository { directory, .. } if directory == &paths[0])
        );
        assert!(
            matches!(&ordered[1].1, Choice::Repository { directory, .. } if directory == &paths[1])
        );
        assert!(matches!(
            ordered[2].1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
        let filtered = rank_choices(PaletteKind::Repositories, "ALPHA", choices());
        assert!(
            matches!(&filtered[..], [(_, Choice::Repository { directory, .. }), (_, Choice::Command(jaywalk::commands::CommandId::OpenRepository))] if directory == &paths[1])
        );
        let empty = rank_choices(PaletteKind::Repositories, "", repository_choices(&[]));
        assert_eq!(empty.len(), 1);
        assert!(matches!(
            empty[0].1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
    }
    #[test]
    fn recent_picker_prefers_names_and_keeps_open_as_a_fallback() {
        let paths = vec![
            std::path::PathBuf::from("/projects/other"),
            "/repos/p-r-o".into(),
            "/repos/man-protos".into(),
            "/repos/protos".into(),
            "/repos/pro".into(),
        ];
        let ordered = rank_choices(PaletteKind::Repositories, "pro", repository_choices(&paths));
        let repositories: Vec<_> = ordered
            .iter()
            .filter_map(|(_, choice)| match choice {
                Choice::Repository { directory, .. } => Some(directory),
                _ => None,
            })
            .collect();
        assert_eq!(
            repositories,
            vec![&paths[4], &paths[3], &paths[2], &paths[1], &paths[0]]
        );
        assert!(matches!(
            ordered.last().unwrap().1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
        let missing = rank_choices(
            PaletteKind::Repositories,
            "no-match",
            repository_choices(&paths),
        );
        assert_eq!(missing.len(), 1);
        assert!(matches!(
            missing[0].1,
            Choice::Command(jaywalk::commands::CommandId::OpenRepository)
        ));
    }

    #[test]
    fn recent_picker_matches_full_paths_and_unicode_names() {
        assert!(
            recent_rank(
                "/repositories/Δelta",
                std::path::Path::new("/repositories/Δelta")
            )
            .is_some()
        );
        assert!(recent_rank("δELTA", std::path::Path::new("/repositories/Δelta")).is_some());
        assert!(recent_rank("missing", std::path::Path::new("/repositories/Δelta")).is_none());
    }
    #[test]
    fn file_ranking_keeps_path_relevance_ahead_of_change_status() {
        assert!(
            file_rank("readme", "README.md", false).unwrap()
                < file_rank("readme", "docs/README.md", true).unwrap()
        );
        assert!(file_rank("", "a.rs", true).unwrap() < file_rank("", "b.rs", false).unwrap());
        assert!(file_rank("modified", "src/lib.rs", true).is_none());
    }

    #[test]
    fn fuzzy_paths_support_unicode_and_noncontiguous_queries() {
        assert!(fuzzy_score("svcfg", "src/viewer/config.rs").is_some());
        assert!(fuzzy_score("CAFÉ", "src/café.ts").is_some());
        assert!(fuzzy_score("zq", "src/main.rs").is_none());
    }
}

#[cfg(test)]
mod history_navigation_tests {
    use super::{HistoryStep, history_step};
    #[test]
    fn overview_precedes_the_first_history_entry_without_wrapping() {
        assert_eq!(history_step(None, 1, 3), Some(HistoryStep::Revision(0)));
        assert_eq!(history_step(Some(0), -1, 3), Some(HistoryStep::Overview));
        assert_eq!(history_step(None, -1, 3), None);
        assert_eq!(history_step(Some(0), 1, 3), Some(HistoryStep::Revision(1)));
        assert_eq!(history_step(Some(2), 1, 3), None);
        assert_eq!(history_step(None, 1, 0), None);
        assert_eq!(history_step(Some(0), -1, 1), Some(HistoryStep::Overview));
    }
}
