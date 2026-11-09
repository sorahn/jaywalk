//! Toolkit command UI; actions and file results preserve their original subject.
use super::*;
use gpui::component::command::{Command, CommandItem, CommandState};

#[derive(Clone, Copy)]
pub(super) enum PaletteKind {
    Files,
    Commands,
}
#[derive(Clone)]
enum Choice {
    File {
        path: String,
        status: Option<String>,
        source: Rc<jaywalk::repository::ComparisonIdentity>,
    },
    Command(jaywalk::commands::CommandId),
}
pub(super) struct PaletteState {
    state: Entity<CommandState>,
    open: Option<PaletteKind>,
}
impl PaletteState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Viewer>) -> Self {
        Self {
            state: cx.new(|cx| CommandState::new(window, cx)),
            open: None,
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
    pub(super) fn render_palette(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let kind = self.palette.open?;
        let query = self.palette.state.read(cx).query(cx);
        let mut choices: Vec<(String, Choice)> = match kind {
            PaletteKind::Files => self
                .comparison()
                .map(|comparison| {
                    let source = Rc::new(comparison.identity());
                    comparison
                        .catalog
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
            PaletteKind::Commands => jaywalk::commands::CommandId::ALL
                .iter()
                .filter(|command| {
                    self.supports_evolution()
                        || !matches!(
                            command,
                            jaywalk::commands::CommandId::ToggleChangeEvolution
                                | jaywalk::commands::CommandId::ToggleFileEvolution
                                | jaywalk::commands::CommandId::VersionPatch
                                | jaywalk::commands::CommandId::SincePrevious
                        )
                })
                .map(|command| (command.label().to_owned(), Choice::Command(*command)))
                .collect(),
        };
        let mut ranked: Vec<_> = choices
            .drain(..)
            .filter_map(|(label, choice)| {
                let rank = match &choice {
                    Choice::File { path, status, .. } => file_rank(&query, path, status.is_some()),
                    Choice::Command(_) => fuzzy_score(&query, &label).map(|score| (score, false)),
                }?;
                Some((rank, label, choice))
            })
            .collect();
        ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let choices: Vec<_> = ranked
            .into_iter()
            .map(|(_, label, choice)| (label, choice))
            .collect();
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
                }
            },
        );
        let command = Command::new(&self.palette.state)
            .filterable(false)
            .placeholder(match kind {
                PaletteKind::Files => "Find any repository file…",
                PaletteKind::Commands => "Run a command…",
            })
            .items(choices.iter().map(|(label, choice)| {
                let label: SharedString = label.clone().into();
                let command = match choice {
                    Choice::Command(command) => Some(*command),
                    _ => None,
                };
                let status = match choice {
                    Choice::File { status, .. } => status.clone(),
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
                        div()
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
            .on_query(cx.listener(|_, _, _, cx| cx.notify()))
            .on_cancel(move |window, cx| cancel(&(), window, cx))
            .on_confirm(move |index, window, cx| confirm(&index, window, cx));
        Some(
            div()
                .id("palette-overlay")
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
                        .child(command),
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
            ComparisonScope::WholeBranch => None,
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
                self.show_whole_branch_overview(window, cx);
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
        if comparison.files.is_empty() {
            return;
        }
        let current = self
            .selected_changed_index()
            .map(|index| index as i32)
            .unwrap_or(if delta > 0 {
                -1
            } else {
                comparison.files.len() as i32
            });
        let next = (current + delta).clamp(0, comparison.files.len() as i32 - 1) as usize;
        self.select_file(next, window, cx);
    }
}
#[cfg(test)]
mod tests {
    use super::{file_rank, fuzzy_score};
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
