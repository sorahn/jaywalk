pub(crate) const TEMPLATE: &str = r#""[" ++ json(commit_id) ++ "," ++ json(change_id) ++ "," ++ json(description.first_line()) ++ "," ++ json(author.name()) ++ ",[" ++ bookmarks.map(|b| json(b.name())).join(",") ++ "],[" ++ parents.map(|p| json(p.commit_id())).join(",") ++ "]," ++ json(current_working_copy) ++ "," ++ json(empty) ++ "]\n""#;
// Recorded revision metadata and topological graph lanes, independent of UI.

#[derive(Clone, Debug)]
pub struct Revision {
    pub commit_id: String,
    pub change_id: String,
    pub description: String,
    pub author: String,
    pub bookmarks: Vec<String>,
    pub parents: Vec<String>,
    pub working_copy: bool,
    pub empty: bool,
}

impl Revision {
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

#[derive(Clone, Debug)]
pub struct GraphRow {
    pub lane: usize,
    pub incoming: bool,
    pub edges: Vec<(usize, usize)>,
}

#[derive(Debug)]
pub struct History {
    pub revisions: Vec<Revision>,
    pub graph: Vec<GraphRow>,
    pub lane_count: usize,
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
                commit_id,
                change_id,
                description,
                author,
                bookmarks,
                parents,
                working_copy,
                empty,
            });
        }
        Ok(Self::new(revisions))
    }

    /// Space only for lanes drawn in the current visible row range. Edges
    /// count as well as nodes so partially visible branch sprawl is retained.
    pub fn visible_lane_count(&self, rows: std::ops::Range<usize>) -> usize {
        let end = rows.end.min(self.graph.len());
        let start = rows.start.min(end);
        self.graph[start..end]
            .iter()
            .flat_map(|row| {
                std::iter::once(row.lane)
                    .chain(row.edges.iter().flat_map(|(from, to)| [*from, *to]))
            })
            .max()
            .map_or(1, |lane| lane + 1)
    }

    pub(crate) fn new(revisions: Vec<Revision>) -> Self {
        let mut pending: Vec<String> = Vec::new();
        let mut graph = Vec::new();
        let mut lane_count = 1;
        for revision in &revisions {
            let incoming = pending.contains(&revision.commit_id);
            let lane = if let Some(lane) = pending.iter().position(|id| id == &revision.commit_id) {
                lane
            } else {
                pending.push(revision.commit_id.clone());
                pending.len() - 1
            };
            let before = pending.clone();
            pending.remove(lane);
            for (index, parent) in revision.parents.iter().enumerate() {
                if !pending.contains(parent) {
                    pending.insert((lane + index).min(pending.len()), parent.clone());
                }
            }
            let mut edges = Vec::new();
            for (from, id) in before.iter().enumerate() {
                let targets: &[String] = if id == &revision.commit_id {
                    &revision.parents
                } else {
                    std::slice::from_ref(id)
                };
                for target in targets {
                    if let Some(to) = pending.iter().position(|id| id == target) {
                        edges.push((from, to));
                    }
                }
            }
            lane_count = lane_count.max(before.len()).max(pending.len());
            graph.push(GraphRow {
                lane,
                incoming,
                edges,
            });
        }
        Self {
            revisions,
            graph,
            lane_count,
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
        assert_eq!(history.visible_lane_count(3..4), 1);
        assert_eq!(history.visible_lane_count(20..30), 1);
        assert_eq!(history.graph[0].edges, vec![(0, 0), (0, 1)]);
        assert_eq!(history.graph[1].edges, vec![(0, 0), (1, 1)]);
        assert_eq!(history.graph[2].edges, vec![(0, 0), (1, 0)]);
        assert!(history.graph[3].incoming);
        assert!(history.graph[3].edges.is_empty());
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
