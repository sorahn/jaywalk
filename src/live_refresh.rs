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

/// Read-only refresh of the original Git review plus its optional virtual row.
pub struct GitRefresh {
    pub home: Comparison,
    pub history: History,
    pub revision: Option<Revision>,
    pub comparison: Comparison,
}
impl GitRefresh {
    pub fn load(
        directory: &Path,
        base: Option<&str>,
        tip: &str,
        working_copy: bool,
        history_query: &HistoryQuery,
        limit: crate::text_content::TextLimit,
    ) -> Result<Self, String> {
        let home = Comparison::load_with_limit(directory, base, tip, limit)?;
        if home.supports_evolution() {
            return Err("Git refresh requires a plain Git repository".into());
        }
        let history = home.history(history_query)?;
        let revision = if working_copy {
            Some(
                history
                    .revisions
                    .iter()
                    .find(|revision| revision.is_virtual_working_copy())
                    .cloned()
                    .ok_or("Working copy is outside the current history query")?,
            )
        } else {
            None
        };
        let comparison = match &revision {
            Some(revision) => home.for_revision(revision)?,
            None => home.clone(),
        };
        Ok(Self {
            home,
            history,
            revision,
            comparison,
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
