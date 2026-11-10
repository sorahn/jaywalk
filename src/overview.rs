//! Operation-pinned overview data, prepared without touching the working copy.
use crate::repository::{ChangedFile, Comparison, ContentSource, query};
#[derive(Clone, Debug, serde::Deserialize)]
pub struct CommitDetails {
    pub id: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub working_copy: bool,
}
/// One entry in the exact base-exclusive, tip-inclusive review series.
#[derive(Clone, Debug)]
pub struct SeriesCommit {
    pub details: CommitDetails,
    pub revision: Option<crate::history::Revision>,
}
#[derive(Clone, Debug, Default)]
pub struct OverviewSeries {
    pub commits: Vec<SeriesCommit>,
}
impl OverviewSeries {
    pub fn commit_count(&self) -> usize {
        self.commits
            .iter()
            .filter(|commit| !commit.details.working_copy)
            .count()
    }
    pub fn has_working_changes(&self) -> bool {
        self.commits
            .iter()
            .any(|commit| commit.details.working_copy)
    }
    pub fn caption(&self) -> String {
        let count = self.commit_count();
        format!(
            "{count} commit{}{}",
            if count == 1 { "" } else { "s" },
            if self.has_working_changes() {
                " · working copy changes"
            } else {
                ""
            }
        )
    }
}
#[derive(serde::Deserialize)]
struct SeriesEntry {
    #[serde(flatten)]
    details: CommitDetails,
    change_id: String,
    parents: Vec<String>,
    empty: bool,
}
impl SeriesEntry {
    fn into_commit(self) -> Option<SeriesCommit> {
        if self.details.working_copy && self.empty {
            return None;
        }
        let revision = crate::history::Revision {
            commit_id: self.details.id.clone(),
            change_id: self.change_id,
            description: self
                .details
                .message
                .lines()
                .next()
                .unwrap_or_default()
                .into(),
            author: self.details.author.clone(),
            references: vec![],
            parents: self.parents,
            working_copy: self.details.working_copy,
            empty: self.empty,
        };
        Some(SeriesCommit {
            details: self.details,
            revision: Some(revision),
        })
    }
}
pub struct OverviewFile {
    pub file: ChangedFile,
    pub patch: String,
    pub notice: Option<String>,
}
pub struct OverviewSnapshot {
    pub commit: Option<CommitDetails>,
    pub metadata_error: Option<String>,
    pub files: Vec<OverviewFile>,
}
impl Comparison {
    /// Use immutable endpoints and the recorded jj operation, independently of
    /// history limits and remote visibility. Parents precede descendants.
    pub fn overview_series(&self) -> Result<Option<OverviewSeries>, String> {
        let mut commits = match &self.content {
            ContentSource::External(_) | ContentSource::Evolution(_) => return Ok(None),
            ContentSource::Git(contents) => {
                let tip = if self.tip == "working tree" {
                    contents.recorded_tip.as_str()
                } else {
                    &self.tip
                };
                let range = format!("{}..{tip}", self.base);
                let output = crate::git::output(
                    &self.root,
                    &[
                        "log",
                        "--reverse",
                        "--topo-order",
                        "-z",
                        "--format=%H%x00%P%x00%B%x00%an%x00%aI",
                        &range,
                        "--",
                    ],
                )?;
                let fields: Vec<_> = output.split_terminator('\0').collect();
                if !fields.len().is_multiple_of(5) {
                    return Err("Incomplete Git series metadata".into());
                }
                let mut commits = Vec::with_capacity(fields.len() / 5);
                for fields in fields.as_chunks::<5>().0 {
                    let entry = SeriesEntry {
                        details: CommitDetails {
                            id: fields[0].into(),
                            message: fields[2].trim_end().into(),
                            author: fields[3].into(),
                            timestamp: fields[4].into(),
                            working_copy: false,
                        },
                        change_id: fields[0].into(),
                        parents: fields[1].split_whitespace().map(str::to_owned).collect(),
                        empty: false,
                    };
                    if let Some(commit) = entry.into_commit() {
                        commits.push(commit);
                    }
                }
                if self.tip == "working tree" && contents.working_changes {
                    commits.push(SeriesCommit {
                        details: CommitDetails {
                            id: String::new(),
                            message: "Working copy changes".into(),
                            author: String::new(),
                            timestamp: String::new(),
                            working_copy: true,
                        },
                        revision: None,
                    });
                }
                return Ok(Some(OverviewSeries { commits }));
            }
            ContentSource::Recorded => {
                let range = format!("{}..{}", self.base, self.tip);
                let template = r#""{" ++ '"id":' ++ json(commit_id) ++ ',"message":' ++ json(description) ++ ',"author":' ++ json(author.name()) ++ ',"timestamp":' ++ json(author.timestamp().local().format("%Y-%m-%d %H:%M:%S")) ++ ',"working_copy":' ++ json(current_working_copy) ++ ',"change_id":' ++ json(change_id) ++ ',"parents":[' ++ parents.map(|p| json(p.commit_id())).join(",") ++ '],"empty":' ++ json(empty) ++ "}\n""#;
                let output = query(
                    &self.root,
                    &self.operation,
                    &["log", "-G", "-r", &range, "-T", template],
                )?;
                let mut commits = Vec::new();
                for line in output.lines() {
                    let entry: SeriesEntry = serde_json::from_str(line)
                        .map_err(|error| format!("Invalid jj series metadata: {error}"))?;
                    if let Some(commit) = entry.into_commit() {
                        commits.push(commit);
                    }
                }
                commits
            }
        };
        commits.reverse();
        Ok(Some(OverviewSeries { commits }))
    }
    pub fn commit_details(&self) -> Result<Option<CommitDetails>, String> {
        match &self.content {
            ContentSource::External(_) => Ok(None),
            ContentSource::Git(_) if self.tip == "working tree" => Ok(None),
            ContentSource::Git(_) => {
                let output = crate::git::output(
                    &self.root,
                    &[
                        "show",
                        "-s",
                        "--format=%H%x00%B%x00%an%x00%aI",
                        &self.tip,
                        "--",
                    ],
                )?;
                let fields: Vec<_> = output.splitn(4, '\0').collect();
                if fields.len() != 4 {
                    return Err("Incomplete Git commit metadata".into());
                }
                Ok(Some(CommitDetails {
                    id: fields[0].into(),
                    message: fields[1].trim_end().into(),
                    author: fields[2].into(),
                    timestamp: fields[3].trim().into(),
                    working_copy: false,
                }))
            }
            _ => {
                let template = r#""{" ++ '"id":' ++ json(commit_id) ++ ',"message":' ++ json(description) ++ ',"author":' ++ json(author.name()) ++ ',"timestamp":' ++ json(author.timestamp().local().format("%Y-%m-%d %H:%M:%S")) ++ ',"working_copy":' ++ json(current_working_copy) ++ "}""#;
                let output = query(
                    &self.root,
                    &self.operation,
                    &["log", "-G", "-r", &self.tip, "-T", template],
                )?;
                serde_json::from_str(&output)
                    .map(Some)
                    .map_err(|error| format!("Invalid commit details: {error}"))
            }
        }
    }
    pub fn overview(&self) -> OverviewSnapshot {
        let (commit, metadata_error) = match self.commit_details() {
            Ok(commit) => (commit, None),
            Err(error) => (None, Some(error)),
        };
        let mut files: Vec<_> = self
            .files
            .iter()
            .map(|file| {
                let (patch, notice) = match self
                    .catalog
                    .find(&file.after_path)
                    .map_or_else(|| self.contents(file), |entry| self.read_file(entry))
                {
                    Ok((before, after)) => {
                        let notice = if before == after {
                            Some(
                                match file.status.as_str() {
                                    "renamed" => "Renamed · contents unchanged",
                                    "copied" => "Copied · contents unchanged",
                                    "added" => "Added empty file",
                                    "removed" => "Deleted empty file",
                                    _ => "File metadata changed",
                                }
                                .to_owned(),
                            )
                        } else {
                            None
                        };
                        (overview_patch(file, Some((&before, &after))), notice)
                    }
                    Err(error) => (
                        overview_patch(file, None),
                        Some(format!("Text preview unavailable: {error}")),
                    ),
                };
                OverviewFile {
                    file: file.clone(),
                    patch,
                    notice,
                }
            })
            .collect();
        files.sort_by(|a, b| {
            crate::file_browser::compare_paths(&a.file.after_path, &b.file.after_path)
        });
        OverviewSnapshot {
            commit,
            metadata_error,
            files,
        }
    }
}
fn quoted_path(path: &str) -> String {
    let mut result = String::from("\"");
    for ch in path.chars() {
        match ch {
            '\"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ if ch.is_ascii_control() => result.push_str(&format!("\\{:03o}", ch as u32)),
            _ => result.push(ch),
        }
    }
    result.push('\"');
    result
}
/// Supply full source context so disclosures can reveal real revision contents.
/// Folding is a presentation choice, not an omission from the immutable patch.
pub fn overview_patch(file: &ChangedFile, contents: Option<(&str, &str)>) -> String {
    let old = quoted_path(&format!("a/{}", file.before_path));
    let new = quoted_path(&format!("b/{}", file.after_path));
    let mut patch = format!("diff --git {old} {new}\n");
    match file.status.as_str() {
        "added" => patch.push_str("new file mode 100644\n"),
        "removed" => patch.push_str("deleted file mode 100644\n"),
        "renamed" => patch.push_str(&format!(
            "rename from {}\nrename to {}\n",
            quoted_path(&file.before_path),
            quoted_path(&file.after_path)
        )),
        "copied" => patch.push_str(&format!(
            "copy from {}\ncopy to {}\n",
            quoted_path(&file.before_path),
            quoted_path(&file.after_path)
        )),
        _ => {}
    }
    if let Some((before, after)) = contents {
        let before_path = if file.status == "added" {
            "/dev/null"
        } else {
            &old
        };
        let after_path = if file.status == "removed" {
            "/dev/null"
        } else {
            &new
        };
        if before == after && !before.is_empty() {
            let count = before.lines().count();
            patch.push_str(&format!(
                "--- {before_path}\n+++ {after_path}\n@@ -1,{count} +1,{count} @@\n"
            ));
            for line in before.split_inclusive('\n') {
                patch.push(' ');
                patch.push_str(line);
            }
            if !before.ends_with('\n') {
                patch.push_str("\n\\ No newline at end of file\n");
            }
        } else {
            patch.push_str(
                &similar::TextDiff::from_lines(before, after)
                    .unified_diff()
                    .context_radius(before.lines().count().max(after.lines().count()))
                    .header(before_path, after_path)
                    .to_string(),
            );
        }
        if before == after && file.status != "renamed" && file.status != "copied" {
            patch.push_str("metadata changed\n");
        }
    } else {
        patch.push_str("text preview unavailable\n");
    }
    patch
}
