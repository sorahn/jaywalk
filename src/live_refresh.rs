//! Follow recorded repository intent and suppress retries of unchanged failures.
use crate::{
    history::{History, HistoryQuery, Revision},
    repository::{Comparison, query},
};
use std::path::{Path, PathBuf};

pub struct FollowedChange {
    pub change_id: String,
    pub working_copy: bool,
}

pub struct RecordedRefresh {
    pub branch: Comparison,
    pub history: History,
    pub revision: Option<Revision>,
    pub comparison: Comparison,
}

#[derive(Debug)]
pub struct RefreshFailure {
    pub operation: Option<String>,
    pub message: String,
}

impl RecordedRefresh {
    pub fn load(
        directory: &Path,
        base: Option<&str>,
        tip: &str,
        selected: Option<&FollowedChange>,
        history_query: &HistoryQuery,
    ) -> Result<Self, RefreshFailure> {
        Self::load_with_limit(
            directory,
            base,
            tip,
            selected,
            history_query,
            crate::text_content::TextLimit::default(),
        )
    }
    pub fn load_with_limit(
        directory: &Path,
        base: Option<&str>,
        tip: &str,
        selected: Option<&FollowedChange>,
        history_query: &HistoryQuery,
        limit: crate::text_content::TextLimit,
    ) -> Result<Self, RefreshFailure> {
        let operation =
            Comparison::current_operation(directory).map_err(|message| RefreshFailure {
                operation: None,
                message,
            })?;
        let read = || -> Result<Self, String> {
            let root = PathBuf::from(query(directory, &operation, &["root"])?.trim());
            let branch =
                Comparison::recorded_at(root, operation.clone(), base, tip)?.with_text_limit(limit);
            let history = branch.history(history_query)?;
            let revision = match selected {
                None => None,
                Some(selected) => Some(match branch.visible_change(&selected.change_id)? {
                    Some(revision) => revision,
                    None if selected.working_copy && tip == "@" => branch.revision("@")?,
                    None => return Err("The selected change is no longer visible. Choose Whole branch, then Refresh to continue.".into()),
                }),
            };
            let comparison = match &revision {
                Some(revision) => branch.for_revision(revision)?,
                None => branch.clone(),
            };
            Ok(Self {
                branch,
                history,
                revision,
                comparison,
            })
        };
        read().map_err(|message| RefreshFailure {
            operation: Some(operation),
            message,
        })
    }
}

#[derive(Default)]
pub struct RefreshGate {
    failed_operation: Option<String>,
}
impl RefreshGate {
    pub fn allows(&self, recorded: &str, displayed: &str) -> bool {
        recorded != displayed && self.failed_operation.as_deref() != Some(recorded)
    }
    pub fn failed(&mut self, operation: Option<String>) {
        self.failed_operation = operation;
    }
    pub fn clear(&mut self) {
        self.failed_operation = None;
    }
}

#[cfg(test)]
mod tests {
    use super::RefreshGate;
    #[test]
    fn unchanged_failure_waits_for_a_new_operation_or_explicit_retry() {
        let mut gate = RefreshGate::default();
        assert!(gate.allows("new", "old"));
        gate.failed(Some("new".into()));
        assert!(!gate.allows("new", "old"));
        assert!(gate.allows("newer", "old"));
        gate.clear();
        assert!(gate.allows("new", "old"));
        assert!(!gate.allows("new", "new"));
    }
}
