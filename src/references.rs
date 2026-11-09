//! Reference snapshots are read-only; remote configuration contains names only.
use crate::history::{Reference, ReferenceKind, TrackingState};
use crate::repository::{Comparison, ContentSource, query};
use std::{
    collections::BTreeSet,
    hash::{Hash, Hasher},
    path::Path,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ReferenceSnapshot {
    pub references: Vec<Reference>,
    pub remotes: Vec<String>,
    pub local_heads: Vec<String>,
    pub operation: Option<String>,
}
impl ReferenceSnapshot {
    pub fn fingerprint(&self) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut hash);
        hash.finish()
    }
    pub(crate) fn normalize(&mut self) {
        self.references.sort_by_key(|reference| {
            (
                reference.remote().unwrap_or("").to_owned(),
                reference.name.clone(),
                reference.target.clone(),
            )
        });
        self.references.dedup();
        self.remotes.sort();
        self.remotes.dedup();
        self.local_heads.sort();
        self.local_heads.dedup();
    }
}
impl Reference {
    pub fn remote(&self) -> Option<&str> {
        match &self.kind {
            ReferenceKind::Local => None,
            ReferenceKind::Remote { remote, .. } => Some(remote),
        }
    }
    pub fn display_name(&self) -> String {
        self.remote().map_or_else(
            || self.name.clone(),
            |remote| format!("{remote}/{}", self.name),
        )
    }
}
pub(crate) fn configured_remotes(root: &Path) -> Result<Vec<String>, String> {
    Ok(crate::git::output(root, &["remote"])?
        .lines()
        .map(str::to_owned)
        .collect())
}

pub(crate) fn recorded(root: &Path, operation: &str) -> Result<ReferenceSnapshot, String> {
    let template = r#""[" ++ json(name) ++ "," ++ json(if(remote, remote, "")) ++ "," ++ json(tracked) ++ "," ++ json(conflict) ++ ",[" ++ added_targets.map(|c| if(c, json(c.commit_id()), "null")).join(",") ++ "]]\n""#;
    let output = query(
        root,
        operation,
        &["bookmark", "list", "--all-remotes", "-T", template],
    )?;
    let mut snapshot = ReferenceSnapshot {
        remotes: configured_remotes(root)?,
        operation: Some(operation.into()),
        ..Default::default()
    };
    for line in output.lines() {
        let (name, remote, tracked, conflicted, targets): (
            String,
            String,
            bool,
            bool,
            Vec<Option<String>>,
        ) = serde_json::from_str(line)
            .map_err(|error| format!("Invalid jj reference entry: {error}"))?;
        if remote == "git" {
            continue;
        } // jj's local Git tracking view, not a configured network remote.
        for target in targets.into_iter().flatten() {
            if target.is_empty() || !target.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Invalid reference target commit ID".into());
            }
            let kind = if remote.is_empty() {
                ReferenceKind::Local
            } else {
                snapshot.remotes.push(remote.clone());
                ReferenceKind::Remote {
                    remote: remote.clone(),
                    tracking: Some(if tracked {
                        TrackingState::Tracked
                    } else {
                        TrackingState::Untracked
                    }),
                }
            };
            snapshot.references.push(Reference {
                name: name.clone(),
                target,
                kind,
                tracked_by: if tracked && !remote.is_empty() {
                    vec![name.clone()]
                } else {
                    Vec::new()
                },
                conflicted,
            });
        }
    }
    let remote_heads: BTreeSet<_> = snapshot
        .references
        .iter()
        .filter(|reference| reference.remote().is_some())
        .map(|reference| reference.target.as_str())
        .collect();
    let visible = if remote_heads.is_empty() {
        "visible_heads()".into()
    } else {
        format!(
            "(visible_heads() ~ ancestors({}))",
            remote_heads.into_iter().collect::<Vec<_>>().join(" | ")
        )
    };
    let roots = format!("{visible} | bookmarks() | tags() | working_copies()");
    snapshot.local_heads = query(
        root,
        operation,
        &[
            "log",
            "--no-graph",
            "-r",
            &roots,
            "-T",
            r#"commit_id ++ "\n""#,
        ],
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    snapshot.normalize();
    Ok(snapshot)
}
impl Comparison {
    pub fn reference_snapshot(&self) -> Result<ReferenceSnapshot, String> {
        if matches!(self.content, ContentSource::Git(_)) {
            crate::git::references(&self.root)
        } else if self.supports_evolution() {
            recorded(&self.root, &self.operation)
        } else {
            Ok(ReferenceSnapshot::default())
        }
    }
}
