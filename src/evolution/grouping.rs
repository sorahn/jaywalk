//! Presentation-only grouping. Every member keeps its original version index.
use super::{Evolution, EvolutionView, ReviewMode};
use crate::history::History;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub enum TimelineRow {
    Version(usize),
    ExpandedHead(Vec<usize>),
    Collapsed(Vec<usize>),
}

impl TimelineRow {
    pub fn representative(&self) -> usize {
        match self {
            Self::Version(index) => *index,
            Self::ExpandedHead(members) | Self::Collapsed(members) => members[0],
        }
    }

    pub fn members(&self) -> &[usize] {
        match self {
            Self::Version(index) => std::slice::from_ref(index),
            Self::ExpandedHead(members) | Self::Collapsed(members) => members,
        }
    }

    pub fn contains_version(&self, selected: usize) -> bool {
        match self {
            Self::Collapsed(members) => members.contains(&selected),
            _ => self.representative() == selected,
        }
    }
}

pub struct GroupedEvolutionView {
    pub rows: Vec<TimelineRow>,
    pub graph: History,
}

impl EvolutionView {
    pub fn grouped(
        &self,
        timeline: &Evolution,
        path: Option<&str>,
        mode: ReviewMode,
        collapse_repeats: bool,
        expanded: &HashSet<String>,
    ) -> GroupedEvolutionView {
        let mut incoming = HashMap::<&str, usize>::new();
        for revision in &self.graph.revisions {
            for parent in &revision.parents {
                *incoming.entry(parent).or_default() += 1;
            }
        }
        let can_join = |left: usize, right: usize| {
            let a = &timeline.versions[self.indices[left]];
            let b = &timeline.versions[self.indices[right]];
            if a.change_id != b.change_id
                || a.divergent
                || b.divergent
                || a.conflict
                || b.conflict
                || self.graph.revisions[left].parents.len() != 1
                || self.graph.revisions[left].parents[0] != b.commit_id
                || self.graph.revisions[right].parents.len() > 1
                || incoming
                    .get(a.commit_id.as_str())
                    .is_some_and(|count| *count > 1)
                || incoming.get(b.commit_id.as_str()) != Some(&1)
            {
                return false;
            }
            match path {
                Some(path) => a.file_effect(path, mode) == b.file_effect(path, mode),
                None => {
                    a.evolution_files == b.evolution_files
                        && (!a.evolution_files.is_empty()
                            || timeline.event(self.indices[left])
                                == timeline.event(self.indices[right]))
                }
            }
        };
        let mut rows = Vec::new();
        let mut start = 0;
        while start < self.indices.len() {
            let mut end = start + 1;
            if collapse_repeats {
                while end < self.indices.len() && can_join(end - 1, end) {
                    end += 1;
                }
            }
            let members = self.indices[start..end].to_vec();
            if members.len() == 1 {
                rows.push(TimelineRow::Version(members[0]));
            } else if expanded.contains(&timeline.versions[members[0]].commit_id) {
                rows.push(TimelineRow::ExpandedHead(members.clone()));
                rows.extend(members[1..].iter().copied().map(TimelineRow::Version));
            } else {
                rows.push(TimelineRow::Collapsed(members));
            }
            start = end;
        }
        // Map collapsed members to their representative node, and use the
        // oldest member's outgoing edges. Expansion restores original edges.
        let mut representatives = HashMap::<&str, &str>::new();
        for row in &rows {
            let id = timeline.versions[row.representative()].commit_id.as_str();
            match row {
                TimelineRow::Collapsed(members) => {
                    for &index in members {
                        representatives.insert(&timeline.versions[index].commit_id, id);
                    }
                }
                _ => {
                    representatives.insert(id, id);
                }
            }
        }
        let positions: HashMap<_, _> = self
            .indices
            .iter()
            .enumerate()
            .map(|(position, &index)| (index, position))
            .collect();
        let revisions = rows
            .iter()
            .map(|row| {
                let mut revision = timeline.versions[row.representative()].revision();
                let outgoing = match row {
                    TimelineRow::Collapsed(members) => {
                        *members.last().expect("a group has at least two members")
                    }
                    _ => row.representative(),
                };
                revision.parents = self.graph.revisions[positions[&outgoing]]
                    .parents
                    .iter()
                    .map(|id| {
                        representatives
                            .get(id.as_str())
                            .copied()
                            .unwrap_or(id)
                            .to_owned()
                    })
                    .collect();
                revision
            })
            .collect();
        GroupedEvolutionView {
            rows,
            graph: History::new(revisions),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::{FileDelta, Version};

    fn timeline(edges: &[(&str, &[&str])]) -> Evolution {
        let versions: Vec<_> = edges
            .iter()
            .map(|(id, predecessors)| Version {
                commit_id: (*id).into(),
                change_id: "same-change".into(),
                description: "same description".into(),
                parents: vec!["f".into()],
                predecessors: predecessors.iter().map(|id| (*id).into()).collect(),
                timestamp: "2026-11-07 12:00:00".into(),
                operation_id: "ff".into(),
                operation: "rewrite".into(),
                hidden: *id != "a",
                divergent: false,
                conflict: false,
                patch_changed: true,
                patch_files: vec![FileDelta {
                    status: "renamed".into(),
                    before_path: "old.rs".into(),
                    after_path: "file.rs".into(),
                    unchanged: true,
                    conflict: false,
                }],
                evolution_files: vec![FileDelta {
                    status: "modified".into(),
                    before_path: "file.rs".into(),
                    after_path: "file.rs".into(),
                    unchanged: false,
                    conflict: false,
                }],
            })
            .collect();
        let graph = History::new(
            versions
                .iter()
                .map(|version| {
                    let mut revision = version.revision();
                    revision.parents = version.predecessors.clone();
                    revision
                })
                .collect(),
        );
        Evolution {
            versions,
            graph,
            truncated: false,
        }
    }

    #[test]
    fn collapse_and_expand_preserve_selection_and_restore_original_edges() {
        let timeline = timeline(&[("a", &["b"]), ("b", &["c"]), ("c", &["d"]), ("d", &[])]);
        let view = timeline.view(true, 2);
        let collapsed = view.grouped(
            &timeline,
            Some("file.rs"),
            ReviewMode::Patch,
            true,
            &HashSet::new(),
        );
        assert_eq!(collapsed.rows.len(), 1);
        assert_eq!(collapsed.rows[0].members(), &[0, 1, 2, 3]);
        assert!(collapsed.rows[0].contains_version(2));
        assert_eq!(collapsed.graph.revisions[0].commit_id, "a");
        assert!(collapsed.graph.revisions[0].parents.is_empty());
        let expanded = view.grouped(
            &timeline,
            Some("file.rs"),
            ReviewMode::Patch,
            true,
            &HashSet::from(["a".into()]),
        );
        assert_eq!(expanded.rows.len(), 4);
        assert!(matches!(expanded.rows[0], TimelineRow::ExpandedHead(_)));
        assert!(!expanded.rows[0].contains_version(2));
        assert!(expanded.rows[2].contains_version(2));
        for (actual, original) in expanded.graph.revisions.iter().zip(&view.graph.revisions) {
            assert_eq!(actual.commit_id, original.commit_id);
            assert_eq!(actual.parents, original.parents);
        }
        let disabled = view.grouped(
            &timeline,
            Some("file.rs"),
            ReviewMode::Patch,
            false,
            &HashSet::new(),
        );
        assert!(
            disabled
                .rows
                .iter()
                .all(|row| matches!(row, TimelineRow::Version(_)))
        );
    }

    #[test]
    fn grouping_stops_at_forks_joins_divergence_and_conflicts() {
        let branch = timeline(&[("a", &["b", "c"]), ("b", &["d"]), ("c", &["d"]), ("d", &[])]);
        let grouped = branch.view(true, 0).grouped(
            &branch,
            Some("file.rs"),
            ReviewMode::Patch,
            true,
            &HashSet::new(),
        );
        assert_eq!(grouped.rows.len(), 4);
        assert_eq!(grouped.graph.lane_count, 2);
        let mut linear = timeline(&[("a", &["b"]), ("b", &["c"]), ("c", &[])]);
        for version in &mut linear.versions {
            version.divergent = true;
        }
        assert_eq!(
            linear
                .view(true, 0)
                .grouped(
                    &linear,
                    Some("file.rs"),
                    ReviewMode::Patch,
                    true,
                    &HashSet::new()
                )
                .rows
                .len(),
            3
        );
        for version in &mut linear.versions {
            version.divergent = false;
        }
        linear.versions[1].conflict = true;
        assert_eq!(
            linear
                .view(true, 0)
                .grouped(
                    &linear,
                    Some("file.rs"),
                    ReviewMode::Patch,
                    true,
                    &HashSet::new()
                )
                .rows
                .len(),
            3
        );
    }

    #[test]
    fn same_caption_does_not_merge_different_files_in_rewrite_overview() {
        let mut timeline = timeline(&[("a", &["b"]), ("b", &[])]);
        timeline.versions[0].evolution_files[0].before_path = "left/file.rs".into();
        timeline.versions[0].evolution_files[0].after_path = "left/file.rs".into();
        timeline.versions[1].evolution_files[0].before_path = "right/file.rs".into();
        timeline.versions[1].evolution_files[0].after_path = "right/file.rs".into();
        assert_eq!(
            timeline.versions[0].rewrite_summary(timeline.event(0)),
            timeline.versions[1].rewrite_summary(timeline.event(1))
        );
        assert_eq!(
            timeline
                .view(true, 0)
                .grouped(&timeline, None, ReviewMode::Patch, true, &HashSet::new())
                .rows
                .len(),
            2
        );
    }

    #[test]
    fn active_comparison_and_file_focus_define_matching_groups() {
        let mut timeline = timeline(&[("a", &["b"]), ("b", &["c"]), ("c", &["d"]), ("d", &[])]);
        timeline.versions[1].evolution_files.clear();
        timeline.versions[2].evolution_files.clear();
        timeline.versions[3].evolution_files[0].status = "added".into();
        let view = timeline.view(true, 0);
        assert_eq!(
            view.grouped(
                &timeline,
                Some("file.rs"),
                ReviewMode::Patch,
                true,
                &HashSet::new()
            )
            .rows
            .len(),
            1
        );
        let interdiff = view.grouped(
            &timeline,
            Some("file.rs"),
            ReviewMode::SincePrevious,
            true,
            &HashSet::new(),
        );
        assert_eq!(interdiff.rows.len(), 3);
        assert_eq!(interdiff.rows[1].members(), &[1, 2]);
        assert_eq!(interdiff.graph.revisions[0].parents, vec!["b"]);
        assert_eq!(interdiff.graph.revisions[1].parents, vec!["d"]);
        assert_eq!(
            view.grouped(
                &timeline,
                Some("another.rs"),
                ReviewMode::SincePrevious,
                true,
                &HashSet::new()
            )
            .rows
            .len(),
            1
        );
    }

    #[test]
    fn grouping_composes_with_filtered_rebases_without_inventing_members() {
        let mut timeline = timeline(&[("a", &["b"]), ("b", &["c"]), ("c", &["d"]), ("d", &[])]);
        timeline.versions[1].patch_changed = false;
        timeline.versions[1].parents = vec!["e".into()];
        timeline.versions[2].patch_changed = false;
        timeline.versions[3].parents = vec!["0".into()];
        let view = timeline.view(false, 0);
        assert_eq!(view.indices, vec![0, 3]);
        let grouped = view.grouped(
            &timeline,
            Some("file.rs"),
            ReviewMode::Patch,
            true,
            &HashSet::new(),
        );
        assert_eq!(grouped.rows.len(), 1);
        assert_eq!(grouped.rows[0].members(), &[0, 3]);
        assert!(grouped.graph.revisions[0].parents.is_empty());
    }
}
