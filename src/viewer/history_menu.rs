//! History actions retain the full identity and metadata of the pressed row.
use super::*;
use gpui::component::menu::{PopupMenu, PopupMenuItem};
use jaywalk::commands::CommandId;

#[derive(Clone)]
pub(super) struct RevisionTarget {
    pub(super) commit: String,
    pub(super) change: String,
    pub(super) message: String,
    pub(super) parents: Vec<String>,
}
impl RevisionTarget {
    pub(super) fn from_revision(revision: &jaywalk::history::Revision) -> Self {
        Self {
            commit: revision.commit_id.clone(),
            change: revision.change_id.clone(),
            message: revision.description.clone(),
            parents: revision.parents.clone(),
        }
    }
    pub(super) fn copy_text(&self, command: CommandId) -> Option<&str> {
        match command {
            CommandId::CopyCommitId => Some(&self.commit),
            CommandId::CopyChangeId => (!self.change.is_empty()).then_some(self.change.as_str()),
            CommandId::CopyCommitMessage => Some(&self.message),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(super) struct VersionTarget {
    pub(super) revision: RevisionTarget,
    pub(super) operation_id: Option<String>,
    pub(super) kind: evolution::EvolutionKind,
    pub(super) operation: String,
    pub(super) file_path: Option<String>,
    pub(super) index: usize,
    pub(super) has_predecessors: bool,
}

impl VersionTarget {
    pub(super) fn matches(&self, operation: &str, file_path: Option<&str>, commit: &str) -> bool {
        self.operation == operation
            && self.file_path.as_deref() == file_path
            && self.revision.commit == commit
    }
}

pub(super) fn revision_menu(
    mut menu: PopupMenu,
    target: RevisionTarget,
    viewer: WeakEntity<Viewer>,
    jj: bool,
) -> PopupMenu {
    for command in [
        CommandId::InspectRevision,
        CommandId::BranchChanges,
        CommandId::MergedBranch,
        CommandId::MergeResult,
        CommandId::ShowChangeEvolution,
        CommandId::CopyCommitId,
        CommandId::CopyChangeId,
        CommandId::CopyCommitMessage,
    ] {
        if target.parents.len() != 2
            && matches!(command, CommandId::MergedBranch | CommandId::MergeResult)
        {
            continue;
        }
        if !jj
            && matches!(
                command,
                CommandId::CopyChangeId | CommandId::ShowChangeEvolution
            )
        {
            continue;
        }
        let subject = target.clone();
        let owner = viewer.clone();
        menu = menu.item(
            PopupMenuItem::new(command.label())
                .action(Box::new(commands::ExecuteCommand { command }))
                .disabled(command == CommandId::CopyChangeId && target.change.is_empty())
                .on_click(move |_, window, cx| {
                    let _ = owner.update(cx, |viewer, cx| {
                        viewer.execute_revision_action(command, &subject, window, cx)
                    });
                }),
        );
    }
    context_menu::command_entries(
        menu.separator(),
        &[
            (CommandId::AllChanges, false),
            (CommandId::CurrentWork, false),
            (CommandId::ChooseReview, false),
            (CommandId::ShowReferences, false),
            (CommandId::Refresh, false),
        ],
    )
}

pub(super) fn version_menu(
    mut menu: PopupMenu,
    target: VersionTarget,
    viewer: WeakEntity<Viewer>,
) -> PopupMenu {
    for command in [
        CommandId::VersionPatch,
        CommandId::SincePrevious,
        CommandId::CopyCommitId,
        CommandId::CopyChangeId,
        CommandId::CopyCommitMessage,
        CommandId::CopyOperationId,
        CommandId::ToggleEvolutionGroups,
        CommandId::ToggleUnchangedVersions,
    ] {
        let disabled = (command == CommandId::SincePrevious && !target.has_predecessors)
            || (command == CommandId::CopyOperationId && target.operation_id.is_none());
        let subject = target.clone();
        let owner = viewer.clone();
        menu = menu.item(
            PopupMenuItem::new(command.label())
                .action(Box::new(commands::ExecuteCommand { command }))
                .disabled(disabled)
                .on_click(move |_, window, cx| {
                    let _ = owner.update(cx, |viewer, cx| {
                        viewer.execute_version_action(command, &subject, window, cx)
                    });
                }),
        );
    }
    menu
}

impl Viewer {
    pub(super) fn menu_focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub(super) fn current_revision_target(&self) -> Option<RevisionTarget> {
        if matches!(
            self.repository.ready()?.scope,
            ComparisonScope::Evolution { .. }
        ) {
            return self.active_version_target().map(|target| target.revision);
        }
        let commit = self.selected_history_commit()?;
        let revision = self
            .repository
            .ready()?
            .history
            .as_ref()
            .ok()?
            .revisions
            .iter()
            .find(|revision| revision.commit_id == commit)?;
        Some(RevisionTarget::from_revision(revision))
    }

    pub(super) fn execute_revision_action(
        &mut self,
        command: CommandId,
        target: &RevisionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            command,
            CommandId::BranchChanges | CommandId::MergedBranch | CommandId::MergeResult
        ) {
            match command {
                CommandId::BranchChanges => {
                    self.all_review_changes(Some(target.commit.clone()), window, cx)
                }
                CommandId::MergedBranch => self.open_review_target(
                    jaywalk::branch_review::Target::MergedBranch {
                        merge: target.commit.clone(),
                    },
                    window,
                    cx,
                ),
                _ => self.open_review_target(
                    jaywalk::branch_review::Target::MergeResult {
                        merge: target.commit.clone(),
                    },
                    window,
                    cx,
                ),
            }
            return;
        }
        if let Some(text) = target.copy_text(command) {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
            return;
        }
        if matches!(
            command,
            CommandId::InspectRevision | CommandId::ShowChangeEvolution
        ) {
            // Only navigate if this exact commit is still available in the
            // current graph. A stale menu must never open a different row.
            if !self.repository.ready().is_some_and(|session| {
                session.history.as_ref().is_ok_and(|history| {
                    history
                        .revisions
                        .iter()
                        .any(|revision| revision.commit_id == target.commit)
                })
            }) {
                return;
            }
            self.select_revision(&target.commit, window, cx);
            if command == CommandId::ShowChangeEvolution && !self.evolution.visible {
                self.toggle_evolution(window, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RevisionTarget;
    use jaywalk::commands::CommandId;
    #[test]
    fn version_targets_reject_a_reused_row_after_operation_or_file_changes() {
        let target = super::VersionTarget {
            revision: RevisionTarget {
                commit: "abc".into(),
                change: "change".into(),
                message: String::new(),
                parents: Vec::new(),
            },
            operation_id: None,
            kind: super::evolution::EvolutionKind::File,
            operation: "operation".into(),
            file_path: Some("src/a.rs".into()),
            index: 2,
            has_predecessors: true,
        };
        assert!(target.matches("operation", Some("src/a.rs"), "abc"));
        assert!(!target.matches("operation", Some("src/b.rs"), "abc"));
        assert!(!target.matches("new-operation", Some("src/a.rs"), "abc"));
        assert!(!target.matches("operation", Some("src/a.rs"), "other"));
    }
    #[test]
    fn copy_keeps_full_row_metadata_including_multiline_messages() {
        let target = RevisionTarget {
            commit: "0123456789abcdef".into(),
            change: String::new(),
            message: "subject\n\nbody\n".into(),
            parents: Vec::new(),
        };
        assert_eq!(
            target.copy_text(CommandId::CopyCommitId),
            Some("0123456789abcdef")
        );
        assert_eq!(
            target.copy_text(CommandId::CopyCommitMessage),
            Some("subject\n\nbody\n")
        );
        assert_eq!(target.copy_text(CommandId::CopyChangeId), None);
    }
}
