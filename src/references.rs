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
        self.references.sort_by(|left, right| {
            (
                left.remote().unwrap_or(""),
                left.name.as_str(),
                left.target.as_str(),
            )
                .cmp(&(
                    right.remote().unwrap_or(""),
                    right.name.as_str(),
                    right.target.as_str(),
                ))
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
        if matches!(self.content(), ContentSource::Git(_)) {
            crate::git::references(self.root())
        } else if self.supports_evolution() {
            recorded(self.root(), self.operation())
        } else {
            Ok(ReferenceSnapshot::default())
        }
    }
}

/// A shared badge identifies a local tip with one or more visible remote refs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceBadgeKind {
    Local,
    Remote,
    Shared,
}

/// Local labels precede remote labels; the overflow counter has its own slot.
#[derive(Debug, PartialEq, Eq)]
pub struct ReferenceBadge {
    pub kind: ReferenceBadgeKind,
    pub name: String,
    pub additional: usize,
}
pub fn compact_label(references: &[&Reference]) -> Option<ReferenceBadge> {
    let preferred = references
        .iter()
        .min_by_key(|reference| (reference.remote().is_some(), reference.display_name()))?;
    let has_local = references
        .iter()
        .any(|reference| reference.remote().is_none());
    let has_remote = references
        .iter()
        .any(|reference| reference.remote().is_some());
    Some(ReferenceBadge {
        kind: if has_local && has_remote {
            ReferenceBadgeKind::Shared
        } else if has_remote {
            ReferenceBadgeKind::Remote
        } else {
            ReferenceBadgeKind::Local
        },
        name: format!(
            "{}{}",
            preferred.display_name(),
            if preferred.conflicted { " !" } else { "" }
        ),
        additional: references.len() - 1,
    })
}
#[cfg(test)]
mod tests {
    use super::{Reference, ReferenceBadge, ReferenceBadgeKind, ReferenceKind, compact_label};
    #[test]
    fn badge_prefers_local_and_keeps_overflow_bounded_to_one_label() {
        let remote = Reference {
            name: "main".into(),
            target: "abc".into(),
            kind: ReferenceKind::Remote {
                remote: "origin".into(),
                tracking: None,
            },
            tracked_by: vec![],
            conflicted: false,
        };
        let local = Reference {
            kind: ReferenceKind::Local,
            ..remote.clone()
        };
        assert_eq!(
            compact_label(&[&remote, &local]),
            Some(ReferenceBadge {
                kind: ReferenceBadgeKind::Shared,
                name: "main".into(),
                additional: 1
            })
        );
        assert_eq!(
            compact_label(&[&local, &remote]),
            Some(ReferenceBadge {
                kind: ReferenceBadgeKind::Shared,
                name: "main".into(),
                additional: 1
            })
        );
        assert_eq!(compact_label(&[]), None);
    }
    #[test]
    fn badge_distinguishes_local_remote_and_shared_targets_without_name_guessing() {
        let local = Reference {
            name: "origin/a-local-name".into(),
            target: "abc".into(),
            kind: ReferenceKind::Local,
            tracked_by: vec![],
            conflicted: false,
        };
        let remote = Reference {
            name: "feature/日本語".into(),
            kind: ReferenceKind::Remote {
                remote: "upstream".into(),
                tracking: None,
            },
            ..local.clone()
        };
        let second_remote = Reference {
            kind: ReferenceKind::Remote {
                remote: "origin".into(),
                tracking: None,
            },
            conflicted: true,
            ..remote.clone()
        };
        assert_eq!(
            compact_label(&[&local]).unwrap().kind,
            ReferenceBadgeKind::Local
        );
        let only_remote = compact_label(&[&remote]).unwrap();
        assert_eq!(only_remote.kind, ReferenceBadgeKind::Remote);
        assert_eq!(only_remote.name, "upstream/feature/日本語");
        let remotes = compact_label(&[&remote, &second_remote]).unwrap();
        assert_eq!(remotes.kind, ReferenceBadgeKind::Remote);
        assert_eq!(remotes.name, "origin/feature/日本語 !");
        assert_eq!(remotes.additional, 1);
        let shared = compact_label(&[&second_remote, &local, &remote]).unwrap();
        assert_eq!(shared.kind, ReferenceBadgeKind::Shared);
        assert_eq!(shared.name, local.name);
        assert_eq!(shared.additional, 2);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceStamp {
    pub operation: Option<String>,
    remotes: Vec<String>,
    git_fingerprint: Option<u64>,
}
impl ReferenceStamp {
    pub fn from_snapshot(snapshot: &ReferenceSnapshot) -> Self {
        Self {
            operation: snapshot.operation.clone(),
            remotes: snapshot.remotes.clone(),
            git_fingerprint: snapshot.operation.is_none().then(|| snapshot.fingerprint()),
        }
    }
    pub fn read(root: &Path, recorded: bool) -> Result<Self, String> {
        if recorded {
            let mut remotes = configured_remotes(root)?;
            remotes.sort();
            Ok(Self {
                operation: Some(Comparison::current_operation(root)?),
                remotes,
                git_fingerprint: None,
            })
        } else {
            Ok(Self::from_snapshot(&crate::git::references(root)?))
        }
    }
}
