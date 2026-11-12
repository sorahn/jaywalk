//! Resolve a review from local references and immutable endpoints. Never checkout
//! or fetch; a jj catalog is pinned to one operation and Git refs to captured IDs.
use crate::{
    history::Reference,
    repository::{Comparison, ContentSource, query},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefName {
    pub name: String,
    pub remote: Option<String>,
}
impl RefName {
    pub fn from_reference(reference: &Reference) -> Self {
        Self {
            name: reference.name.clone(),
            remote: reference.remote().map(str::to_owned),
        }
    }
    pub fn display(&self) -> String {
        self.remote.as_ref().map_or_else(
            || self.name.clone(),
            |remote| format!("{remote}/{}", self.name),
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tip {
    Reference(RefName),
    Commit(String),
}
impl Tip {
    pub fn label(&self) -> String {
        match self {
            Self::Reference(reference) => reference.display(),
            Self::Commit(id) => format!("Revision {}", &id[..id.len().min(8)]),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Branch { tip: Tip, base: RefName },
    MergedBranch { merge: String },
    MergeResult { merge: String },
}
impl Target {
    pub fn label(&self) -> String {
        match self {
            Self::Branch { tip, base } => format!("{} · based on {}", tip.label(), base.display()),
            Self::MergedBranch { merge } => {
                format!("Merged branch · {}", &merge[..merge.len().min(8)])
            }
            Self::MergeResult { merge } => {
                format!("Merge result · {}", &merge[..merge.len().min(8)])
            }
        }
    }
}
#[derive(Clone)]
pub struct Catalog {
    root: PathBuf,
    operation: Option<String>,
    pub references: Vec<Reference>,
    heads: Vec<String>,
    default_base: Option<RefName>,
}
impl Catalog {
    pub fn capture(comparison: &Comparison) -> Result<Self, String> {
        let operation = if comparison.supports_evolution() {
            Some(Comparison::current_operation(&comparison.root)?)
        } else if matches!(comparison.content, ContentSource::Git(_)) {
            None
        } else {
            return Err("Branch review requires a Git or jj repository".into());
        };
        let snapshot = match &operation {
            Some(operation) => crate::references::recorded(&comparison.root, operation)?,
            None => crate::git::references(&comparison.root)?,
        };
        let heads = match &operation {
            Some(operation) => query(
                &comparison.root,
                operation,
                &[
                    "log",
                    "--no-graph",
                    "-r",
                    "visible_heads()",
                    "-T",
                    r#"commit_id ++ "\n""#,
                ],
            )?
            .lines()
            .map(str::to_owned)
            .collect(),
            None => vec![
                crate::git::output(&comparison.root, &["rev-parse", "HEAD"])?
                    .trim()
                    .to_owned(),
            ],
        };
        let mut catalog = Self {
            root: comparison.root.clone(),
            operation,
            references: snapshot.references,
            heads,
            default_base: None,
        };
        // Remote HEAD gives a repository default, not the branch's tracking ref.
        let symbolic = crate::git::output(
            &catalog.root,
            &["for-each-ref", "--format=%(symref)", "refs/remotes"],
        )?;
        let mut defaults: Vec<_> = catalog
            .references
            .iter()
            .filter(|reference| {
                reference.remote().is_some_and(|remote| {
                    symbolic
                        .lines()
                        .any(|line| line == format!("refs/remotes/{remote}/{}", reference.name))
                })
            })
            .map(RefName::from_reference)
            .collect();
        if defaults.is_empty() {
            defaults = catalog
                .references
                .iter()
                .filter(|reference| {
                    reference.remote().is_none()
                        && matches!(reference.name.as_str(), "main" | "master" | "trunk")
                })
                .map(RefName::from_reference)
                .collect();
        }
        defaults.dedup();
        if defaults.len() == 1 {
            catalog.default_base = defaults.pop();
        }
        Ok(catalog)
    }
    pub fn default_base(&self) -> Option<&RefName> {
        self.default_base.as_ref()
    }
    pub fn operation(&self) -> Option<&str> {
        self.operation.as_deref()
    }
    fn reference(&self, name: &RefName) -> Result<&Reference, String> {
        let mut matches = self
            .references
            .iter()
            .filter(|reference| RefName::from_reference(reference) == *name);
        let reference = matches
            .next()
            .ok_or_else(|| format!("Reference {} is unavailable", name.display()))?;
        if reference.conflicted || matches.next().is_some() {
            return Err(format!(
                "Reference {} has multiple/conflicted targets; choose an explicit revision",
                name.display()
            ));
        }
        Ok(reference)
    }
    fn tip(&self, tip: &Tip) -> Result<String, String> {
        match tip {
            Tip::Reference(name) => Ok(self.reference(name)?.target.clone()),
            Tip::Commit(id) => {
                validate_id(id)?;
                Ok(id.clone())
            }
        }
    }
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, String> {
        validate_id(ancestor)?;
        validate_id(descendant)?;
        if let Some(operation) = &self.operation {
            return Ok(!query(
                &self.root,
                operation,
                &[
                    "log",
                    "--no-graph",
                    "-r",
                    &format!("{ancestor} & ::{descendant}"),
                    "-T",
                    "commit_id",
                ],
            )?
            .trim()
            .is_empty());
        }
        let output = crate::tools::command("git")?
            .current_dir(&self.root)
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .output()
            .map_err(|error| format!("Could not inspect ancestry: {error}"))?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(String::from_utf8_lossy(&output.stderr).trim().into()),
        }
    }
    pub fn contains(&self, base: &str, tip: &str, commit: &str) -> Result<bool, String> {
        Ok(self.is_ancestor(commit, tip)? && !self.is_ancestor(commit, base)?)
    }
    pub fn candidates(&self, commit: &str) -> Result<Vec<Tip>, String> {
        validate_id(commit)?;
        let mut endpoints: Vec<_> = self
            .references
            .iter()
            .map(|reference| reference.target.clone())
            .chain(self.heads.iter().cloned())
            .collect();
        endpoints.sort();
        endpoints.dedup();
        for endpoint in &endpoints {
            validate_id(endpoint)?;
        }
        let output = if endpoints.is_empty() {
            String::new()
        } else if let Some(operation) = &self.operation {
            query(
                &self.root,
                operation,
                &[
                    "log",
                    "--no-graph",
                    "-r",
                    &format!("({}) & descendants({commit})", endpoints.join(" | ")),
                    "-T",
                    r#"commit_id ++ "\n""#,
                ],
            )?
        } else {
            let excluded = format!("^{commit}");
            let mut arguments = vec!["rev-list", "--ancestry-path"];
            arguments.extend(endpoints.iter().map(String::as_str));
            arguments.push(&excluded);
            crate::git::output(&self.root, &arguments)?
        };
        let descendants: BTreeSet<_> = output.lines().chain(std::iter::once(commit)).collect();
        let mut tips = BTreeMap::new();
        // Prefer local names only as labels for aliases at the same endpoint.
        let mut references: Vec<_> = self.references.iter().collect();
        references
            .sort_by_key(|reference| (reference.remote().is_some(), reference.display_name()));
        for reference in references {
            if !reference.conflicted && descendants.contains(reference.target.as_str()) {
                tips.entry(reference.target.clone())
                    .or_insert_with(|| Tip::Reference(RefName::from_reference(reference)));
            }
        }
        for head in &self.heads {
            if !tips.contains_key(head) && descendants.contains(head.as_str()) {
                tips.insert(head.clone(), Tip::Commit(head.clone()));
            }
        }
        Ok(tips.into_values().collect())
    }
    fn merge_base(&self, left: &str, right: &str) -> Result<String, String> {
        validate_id(left)?;
        validate_id(right)?;
        let output = match &self.operation {
            Some(operation) => query(
                &self.root,
                operation,
                &[
                    "log",
                    "--no-graph",
                    "-r",
                    &format!("heads(::{left} & ::{right})"),
                    "-T",
                    r#"commit_id ++ "\n""#,
                ],
            )?,
            None => crate::git::output(&self.root, &["merge-base", "--all", left, right]).map_err(|error| format!("Could not determine a common comparison base; choose explicit endpoints: {error}"))?,
        };
        let values: Vec<_> = output.lines().filter(|line| !line.is_empty()).collect();
        if values.len() != 1 {
            return Err(format!(
                "Comparison has {} merge bases; choose explicit endpoints",
                values.len()
            ));
        }
        Ok(values[0].into())
    }
    fn merge_parents(&self, merge: &str) -> Result<Vec<String>, String> {
        validate_id(merge)?;
        let output = match &self.operation {
            Some(operation) => query(
                &self.root,
                operation,
                &[
                    "log",
                    "--no-graph",
                    "-r",
                    merge,
                    "-T",
                    "parents.map(|p| p.commit_id()).join(\"\\n\")",
                ],
            )?,
            None => crate::git::output(&self.root, &["show", "-s", "--format=%P", merge])?,
        };
        let parents: Vec<_> = output.split_whitespace().map(str::to_owned).collect();
        if parents.len() != 2 {
            return Err(
                "Choose a normal two-parent merge to review its side branch or merge result".into(),
            );
        }
        Ok(parents)
    }
    fn original_merged_base(&self, tip: &str, receiving_tip: &str) -> Result<String, String> {
        let output = match &self.operation {
            Some(operation) => query(
                &self.root,
                operation,
                &[
                    "log",
                    "--no-graph",
                    "-r",
                    &format!("merges() & descendants({tip}) & ::{receiving_tip}"),
                    "-T",
                    r#"commit_id ++ " " ++ parents.map(|p| p.commit_id()).join(" ") ++ "\n""#,
                ],
            )?,
            None => crate::git::output(
                &self.root,
                &[
                    "rev-list",
                    "--parents",
                    "--merges",
                    "--ancestry-path",
                    &format!("{tip}..{receiving_tip}"),
                ],
            )?,
        };
        let matches: Vec<Vec<_>> = output
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>())
            .filter(|parts| parts.len() == 3 && parts[2] == tip)
            .collect();
        if matches.len() != 1 {
            return Err(format!(
                "Branch tip is already in the comparison base; found {} matching merges. Choose an explicit base or a merge in history",
                matches.len()
            ));
        }
        self.merge_base(matches[0][1], tip)
    }

    pub fn resolve(&self, target: &Target) -> Result<Comparison, String> {
        self.resolve_with_limit(target, crate::text_content::TextLimit::default())
    }
    pub fn resolve_with_limit(
        &self,
        target: &Target,
        limit: crate::text_content::TextLimit,
    ) -> Result<Comparison, String> {
        let (base, tip) = match target {
            Target::Branch { tip, base } => {
                let tip = self.tip(tip)?;
                let receiving_tip = &self.reference(base)?.target;
                let mut base = self.merge_base(receiving_tip, &tip)?;
                if base == tip && tip != *receiving_tip {
                    base = self.original_merged_base(&tip, receiving_tip)?;
                }
                (base, tip)
            }
            Target::MergedBranch { merge } => {
                let parents = self.merge_parents(merge)?;
                (
                    self.merge_base(&parents[0], &parents[1])?,
                    parents[1].clone(),
                )
            }
            Target::MergeResult { merge } => (self.merge_parents(merge)?[0].clone(), merge.clone()),
        };
        match &self.operation {
            Some(operation) => Comparison::between(self.root.clone(), operation.clone(), base, tip)
                .map(|comparison| comparison.with_text_limit(limit)),
            None => crate::git::between(self.root.clone(), base, tip, limit),
        }
    }
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
}
fn validate_id(id: &str) -> Result<(), String> {
    if id.len() < 8 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Err("Review endpoints must be resolved commit IDs".into())
    } else {
        Ok(())
    }
}
