//! Recorded revisions, typed references and explicit history query scope.
const DEFAULT_HISTORY_LIMIT: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TrackingState {
    Tracked,
    Untracked,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReferenceKind {
    Local,
    Remote {
        remote: String,
        tracking: Option<TrackingState>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Reference {
    pub name: String,
    pub target: String,
    pub kind: ReferenceKind,
    pub tracked_by: Vec<String>,
    pub conflicted: bool,
}
/// Defaults cover newly discovered remotes. Overrides survive removal/re-addition.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteVisibility {
    pub default_visible: bool,
    pub overrides: std::collections::BTreeMap<String, bool>,
}
impl Default for RemoteVisibility {
    fn default() -> Self {
        Self {
            default_visible: true,
            overrides: Default::default(),
        }
    }
}
impl RemoteVisibility {
    pub fn includes(&self, remote: &str) -> bool {
        self.overrides
            .get(remote)
            .copied()
            .unwrap_or(self.default_visible)
    }
    pub fn set(&mut self, remote: String, visible: bool) {
        self.overrides.insert(remote, visible);
    }
    pub fn all(visible: bool) -> Self {
        Self {
            default_visible: visible,
            overrides: Default::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryScope {
    Repository,
    Ancestors(Vec<String>),
    Sources {
        visibility: RemoteVisibility,
        reveal: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryQuery {
    scope: HistoryScope,
    limit: usize,
}
impl Default for HistoryQuery {
    fn default() -> Self {
        Self {
            scope: HistoryScope::Repository,
            limit: DEFAULT_HISTORY_LIMIT,
        }
    }
}
impl HistoryQuery {
    pub fn sources(visibility: RemoteVisibility, reveal: Option<String>) -> Result<Self, String> {
        if reveal
            .as_ref()
            .is_some_and(|id| id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("Reveal target must be a resolved commit ID".into());
        }
        Ok(Self {
            scope: HistoryScope::Sources { visibility, reveal },
            ..Self::default()
        })
    }
    pub(crate) fn includes_working_copy(&self) -> bool {
        !matches!(self.scope, HistoryScope::Ancestors(_))
    }

    pub fn roots(&self, references: &crate::references::ReferenceSnapshot) -> Vec<String> {
        if let HistoryScope::Ancestors(commits) = &self.scope {
            return commits.clone();
        }
        let mut roots = references.local_heads.clone();
        for reference in &references.references {
            let visible = match (&self.scope, reference.remote()) {
                (HistoryScope::Sources { visibility, .. }, Some(remote)) => {
                    visibility.includes(remote)
                }
                _ => true,
            };
            if visible {
                roots.push(reference.target.clone());
            }
        }
        if let HistoryScope::Sources {
            reveal: Some(commit),
            ..
        } = &self.scope
        {
            roots.push(commit.clone());
        }
        roots.sort();
        roots.dedup();
        roots
    }
    pub fn ancestors(commits: Vec<String>) -> Result<Self, String> {
        if commits.is_empty()
            || commits
                .iter()
                .any(|id| id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("History roots must be resolved commit IDs".into());
        }
        Ok(Self {
            scope: HistoryScope::Ancestors(commits),
            ..Self::default()
        })
    }
    pub fn with_limit(mut self, limit: usize) -> Result<Self, String> {
        if limit == 0 {
            return Err("History limit must be positive".into());
        }
        self.limit = limit;
        Ok(self)
    }
    pub fn scope(&self) -> &HistoryScope {
        &self.scope
    }
    pub fn limit(&self) -> usize {
        self.limit
    }
    pub(crate) fn jj_revset(
        &self,
        tip: &str,
        references: &crate::references::ReferenceSnapshot,
    ) -> String {
        let roots = match &self.scope {
            HistoryScope::Repository => format!("visible_heads() | {tip}"),
            HistoryScope::Ancestors(commits) => commits.join(" | "),
            HistoryScope::Sources { .. } => {
                let roots = self.roots(references);
                if roots.is_empty() {
                    "none()".into()
                } else {
                    roots.join(" | ")
                }
            }
        };
        format!("ancestors({roots}, {})", self.limit)
    }
}

pub(crate) const TEMPLATE: &str = r#""[" ++ json(commit_id) ++ "," ++ json(change_id) ++ "," ++ json(description.first_line()) ++ "," ++ json(author.name()) ++ ",[" ++ local_bookmarks.map(|b| json(b.name())).join(",") ++ "],[" ++ parents.map(|p| json(p.commit_id())).join(",") ++ "]," ++ json(current_working_copy) ++ "," ++ json(empty) ++ "]\n""#;
// Recorded revision metadata and topological graph lanes, independent of UI.

#[derive(Clone, Debug)]
pub struct Revision {
    pub commit_id: String,
    pub change_id: String,
    pub description: String,
    pub author: String,
    pub references: Vec<Reference>,
    pub parents: Vec<String>,
    pub working_copy: bool,
    pub empty: bool,
}

impl Revision {
    /// Git's virtual working-tree row has no commit or jj change identity.
    pub fn is_virtual_working_copy(&self) -> bool {
        self.working_copy && self.change_id.is_empty()
    }

    pub fn summary(&self) -> String {
        let description = if self.description.is_empty() {
            if self.working_copy {
                "working copy"
            } else {
                "(no description)"
            }
        } else {
            &self.description
        };
        if self.working_copy {
            let state = if self.empty { "no changes" } else { "changes" };
            if self.description.is_empty() {
                format!("{description} · {state}")
            } else {
                format!("{description} · working copy · {state}")
            }
        } else {
            description.to_owned()
        }
    }
}

#[derive(serde::Deserialize)]
struct Entry(
    String,
    String,
    String,
    String,
    Vec<String>,
    Vec<String>,
    #[serde(default)] bool,
    #[serde(default)] bool,
);

pub use crate::graph::GraphRow;

#[derive(Debug)]
pub struct History {
    pub revisions: Vec<Revision>,
    pub graph: Vec<GraphRow>,
    pub lane_count: usize,
    pub references: std::sync::Arc<crate::references::ReferenceSnapshot>,
}

impl History {
    pub fn parse(output: &str) -> Result<Self, String> {
        let mut revisions = Vec::new();
        for line in output.lines() {
            let Entry(
                commit_id,
                change_id,
                description,
                author,
                bookmarks,
                parents,
                working_copy,
                empty,
            ) = serde_json::from_str(line)
                .map_err(|error| format!("Invalid jj history entry: {error}"))?;
            for id in std::iter::once(&commit_id).chain(parents.iter()) {
                if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err("Invalid commit ID in jj history.".into());
                }
            }
            revisions.push(Revision {
                commit_id: commit_id.clone(),
                change_id,
                description,
                author,
                references: bookmarks
                    .into_iter()
                    .map(|name| Reference {
                        name,
                        target: commit_id.clone(),
                        kind: ReferenceKind::Local,
                        tracked_by: Vec::new(),
                        conflicted: false,
                    })
                    .collect(),
                parents,
                working_copy,
                empty,
            });
        }
        Ok(Self::new(revisions))
    }

    pub(crate) fn with_references(
        mut self,
        references: crate::references::ReferenceSnapshot,
    ) -> Self {
        for revision in &mut self.revisions {
            revision.references = references
                .references
                .iter()
                .filter(|reference| reference.target == revision.commit_id)
                .cloned()
                .collect();
        }
        self.references = std::sync::Arc::new(references);
        self
    }

    /// Space only for lanes drawn in the current visible row range. Edges
    /// count as well as nodes so partially visible branch sprawl is retained.
    pub fn visible_lane_count(&self, rows: std::ops::Range<usize>) -> usize {
        let end = rows.end.min(self.graph.len());
        let start = rows.start.min(end);
        self.graph[start..end]
            .iter()
            .flat_map(|row| {
                std::iter::once(row.lane).chain(
                    row.strokes
                        .iter()
                        .flat_map(|stroke| [stroke.from, stroke.to]),
                )
            })
            .max()
            .map_or(1, |lane| lane + 1)
    }

    pub(crate) fn new(revisions: Vec<Revision>) -> Self {
        let graph = crate::graph::layout(&revisions);
        let lane_count = graph
            .iter()
            .flat_map(|row| {
                std::iter::once(row.lane).chain(
                    row.strokes
                        .iter()
                        .flat_map(|stroke| [stroke.from, stroke.to]),
                )
            })
            .max()
            .map_or(1, |lane| lane + 1);
        Self {
            revisions,
            graph,
            lane_count,
            references: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::History;

    #[test]
    fn merges_and_shared_parents_keep_continuous_lanes() {
        let history = History::parse(concat!(
            "[\"a\",\"change-a\",\"merge\",\"Test\",[],[\"b\",\"c\"]]\n",
            "[\"b\",\"change-b\",\"left\",\"Test\",[],[\"d\"]]\n",
            "[\"c\",\"change-c\",\"right\",\"Test\",[],[\"d\"]]\n",
            "[\"d\",\"change-d\",\"base\",\"Test\",[],[]]\n"
        ))
        .unwrap();
        assert_eq!(history.lane_count, 2);
        assert_eq!(history.visible_lane_count(0..1), 2);
        assert_eq!(history.visible_lane_count(3..4), 2);
        assert_eq!(history.visible_lane_count(20..30), 1);
        assert_eq!(
            history.graph.iter().map(|row| row.lane).collect::<Vec<_>>(),
            [0, 0, 1, 0]
        );
        let joins = &history.graph[3].strokes;
        assert_eq!(
            joins.iter().map(|s| (s.from, s.to)).collect::<Vec<_>>(),
            [(0, 0), (1, 0)]
        );
        assert!(
            joins
                .iter()
                .all(|stroke| stroke.half == crate::graph::GraphHalf::Incoming)
        );
    }

    #[test]
    fn working_copy_state_is_separate_from_description_and_graph_selection() {
        let history = History::parse(concat!(
            "[\"a\",\"change-a\",\"\",\"Test\",[],[\"b\"],true,true]\n",
            "[\"b\",\"change-b\",\"saved change\",\"Test\",[],[],false,false]\n"
        ))
        .unwrap();
        assert_eq!(history.revisions[0].summary(), "working copy · no changes");
        assert!(history.revisions[0].description.is_empty());
        assert_eq!(history.revisions[1].summary(), "saved change");
        let mut revision = history.revisions[0].clone();
        revision.empty = false;
        assert_eq!(revision.summary(), "working copy · changes");
        revision.description = "in progress".into();
        assert_eq!(revision.summary(), "in progress · working copy · changes");
        revision.working_copy = false;
        revision.description.clear();
        assert_eq!(revision.summary(), "(no description)");
    }

    #[test]
    fn malformed_history_ids_are_not_used_as_revsets() {
        assert!(History::parse("[\"all()\",\"id\",\"title\",\"author\",[],[]]").is_err());
    }
}

#[cfg(test)]
mod query_tests {
    use super::{History, HistoryQuery, ReferenceKind};
    #[test]
    fn query_scope_and_reference_identity_are_independent_of_graph_geometry() {
        let query = HistoryQuery::default();
        assert_eq!(query.limit(), 200);
        assert_eq!(
            query.jj_revset("abc", &Default::default()),
            "ancestors(visible_heads() | abc, 200)"
        );
        assert_eq!(
            HistoryQuery::ancestors(vec!["abc".into(), "def".into()])
                .unwrap()
                .with_limit(12)
                .unwrap()
                .jj_revset("unused", &Default::default()),
            "ancestors(abc | def, 12)"
        );
        assert!(query.with_limit(0).is_err());
        assert!(HistoryQuery::ancestors(vec![]).is_err());
        assert!(HistoryQuery::ancestors(vec!["all()".into()]).is_err());
        let history =
            History::parse(r#"["abc","change","title","author",["main","feature"],[]]"#).unwrap();
        let references = &history.revisions[0].references;
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].name, "main");
        assert_eq!(references[0].target, "abc");
        assert_eq!(references[0].kind, ReferenceKind::Local);
    }
}
