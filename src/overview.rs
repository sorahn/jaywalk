//! Operation-pinned overview data, prepared without touching the working copy.
use crate::repository::{ChangedFile, Comparison, ContentSource, query};
use crate::repository_file::ChangeKind;
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
            timestamp: self.details.timestamp.clone(),
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
        let mut commits = match self.content() {
            ContentSource::Evolution(_) => return Ok(None),
            ContentSource::External(contents)
                if self.has_git_context()
                    && [self.base(), self.tip()].iter().all(|id| {
                        *id == "working tree"
                            || (id.len() >= 40 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                    }) =>
            {
                let Some(head) = contents.recorded_working.as_deref() else {
                    return Ok(None);
                };
                return git_series(self, head, true);
            }
            ContentSource::External(_) => return Ok(None),
            ContentSource::Git(contents) => {
                return git_series(self, &contents.recorded_tip, contents.working_changes);
            }
            ContentSource::Recorded => {
                let range = format!("{}..{}", self.base(), self.tip());
                let template = r#""{" ++ '"id":' ++ json(commit_id) ++ ',"message":' ++ json(description) ++ ',"author":' ++ json(author.name()) ++ ',"timestamp":' ++ json(author.timestamp().local().format("%Y-%m-%d %H:%M:%S")) ++ ',"working_copy":' ++ json(current_working_copy) ++ ',"change_id":' ++ json(change_id) ++ ',"parents":[' ++ parents.map(|p| json(p.commit_id())).join(",") ++ '],"empty":' ++ json(empty) ++ "}\n""#;
                let output = query(
                    self.root(),
                    self.operation(),
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
                let reverse_range = format!("{}..{}", self.tip(), self.base());
                let removed = query(
                    self.root(),
                    self.operation(),
                    &["log", "-G", "-r", &reverse_range, "-T", template],
                )?;
                let mut left = Vec::new();
                for line in removed.lines() {
                    let entry: SeriesEntry = serde_json::from_str(line)
                        .map_err(|e| format!("Invalid jj series metadata: {e}"))?;
                    if let Some(mut commit) = entry.into_commit() {
                        commit.details.message = format!("Left only · {}", commit.details.message);
                        left.push(commit);
                    }
                }
                if !left.is_empty() {
                    for commit in &mut commits {
                        commit.details.message = format!("Right only · {}", commit.details.message);
                    }
                    left.reverse();
                    commits.reverse();
                    left.extend(commits);
                    return Ok(Some(OverviewSeries { commits: left }));
                }
                commits
            }
        };
        commits.reverse();
        Ok(Some(OverviewSeries { commits }))
    }
    pub fn commit_details(&self) -> Result<Option<CommitDetails>, String> {
        match self.content() {
            ContentSource::External(_) => Ok(None),
            ContentSource::Git(_) if self.tip() == "working tree" => Ok(None),
            ContentSource::Git(_) => {
                let output = crate::git::output(
                    self.root(),
                    &[
                        "show",
                        "-s",
                        "--format=%H%x00%B%x00%an%x00%aI",
                        self.tip(),
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
                    self.root(),
                    self.operation(),
                    &["log", "-G", "-r", self.tip(), "-T", template],
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
            .files()
            .iter()
            .map(|file| {
                let type_notice = self
                    .catalog()
                    .find(file.after_path())
                    .and_then(|entry| entry.type_change_notice());
                let (patch, notice) = match self
                    .catalog()
                    .find(file.after_path())
                    .map_or_else(|| self.contents(file), |entry| self.read_file(entry))
                {
                    Ok((before, after)) => {
                        let notice = if before == after {
                            Some(type_notice.clone().unwrap_or_else(|| {
                                match file.kind() {
                                    ChangeKind::Renamed => "Renamed · contents unchanged",
                                    ChangeKind::Copied => "Copied · contents unchanged",
                                    ChangeKind::Added => "Added empty file",
                                    ChangeKind::Removed => "Deleted empty file",
                                    _ => "File metadata changed",
                                }
                                .to_owned()
                            }))
                        } else {
                            type_notice
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
            crate::file_browser::compare_paths(a.file.after_path(), b.file.after_path())
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
    let old = quoted_path(&format!("a/{}", file.before_path()));
    let new = quoted_path(&format!("b/{}", file.after_path()));
    let mut patch = format!("diff --git {old} {new}\n");
    match file.kind() {
        ChangeKind::Added => patch.push_str("new file mode 100644\n"),
        ChangeKind::Removed => patch.push_str("deleted file mode 100644\n"),
        ChangeKind::Renamed => patch.push_str(&format!(
            "rename from {}\nrename to {}\n",
            quoted_path(file.before_path()),
            quoted_path(file.after_path())
        )),
        ChangeKind::Copied => patch.push_str(&format!(
            "copy from {}\ncopy to {}\n",
            quoted_path(file.before_path()),
            quoted_path(file.after_path())
        )),
        _ => {}
    }
    if let Some((before, after)) = contents {
        let before_path = if file.kind() == &ChangeKind::Added {
            "/dev/null"
        } else {
            &old
        };
        let after_path = if file.kind() == &ChangeKind::Removed {
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
        if before == after
            && file.kind() != &ChangeKind::Renamed
            && file.kind() != &ChangeKind::Copied
        {
            patch.push_str("metadata changed\n");
        }
    } else {
        patch.push_str("text preview unavailable\n");
    }
    patch
}

/// Symmetric ancestry keeps divergent and reversed pairs honest. Presentation
/// labels do not modify revision metadata used by inspection and copy actions.
fn git_series(
    comparison: &Comparison,
    recorded_working: &str,
    working_changes: bool,
) -> Result<Option<OverviewSeries>, String> {
    let left = if comparison.base() == "working tree" {
        recorded_working
    } else {
        comparison.base()
    };
    let right = if comparison.tip() == "working tree" {
        recorded_working
    } else {
        comparison.tip()
    };
    let range = format!("{left}...{right}");
    let output = crate::git::output(
        comparison.root(),
        &[
            "log",
            "--left-right",
            "--reverse",
            "--topo-order",
            "-z",
            "--format=%m%x00%H%x00%P%x00%B%x00%an%x00%aI",
            &range,
            "--",
        ],
    )?;
    let fields: Vec<_> = output.split_terminator('\0').collect();
    if !fields.len().is_multiple_of(6) {
        return Err("Incomplete Git series metadata".into());
    }
    let mut left_commits = Vec::new();
    let mut right_commits = Vec::new();
    for entry in fields.as_chunks::<6>().0 {
        let parsed = SeriesEntry {
            details: CommitDetails {
                id: entry[1].into(),
                message: entry[3].trim_end().into(),
                author: entry[4].into(),
                timestamp: entry[5].into(),
                working_copy: false,
            },
            change_id: entry[1].into(),
            parents: entry[2].split_whitespace().map(str::to_owned).collect(),
            empty: false,
        };
        if let Some(commit) = parsed.into_commit() {
            if entry[0] == "<" {
                left_commits.push(commit);
            } else {
                right_commits.push(commit);
            }
        }
    }
    if !left_commits.is_empty() {
        for commit in &mut left_commits {
            commit.details.message = format!("Left only · {}", commit.details.message);
        }
        for commit in &mut right_commits {
            commit.details.message = format!("Right only · {}", commit.details.message);
        }
    }
    left_commits.extend(right_commits);
    if working_changes
        && (comparison.base() == "working tree" || comparison.tip() == "working tree")
    {
        let commit = SeriesCommit {
            details: CommitDetails {
                id: String::new(),
                message: if comparison.base() == "working tree" {
                    "Working copy changes · left endpoint"
                } else {
                    "Working copy changes"
                }
                .into(),
                author: String::new(),
                timestamp: String::new(),
                working_copy: true,
            },
            revision: None,
        };
        if comparison.base() == "working tree" {
            left_commits.insert(0, commit);
        } else {
            left_commits.push(commit);
        }
    }
    Ok(Some(OverviewSeries {
        commits: left_commits,
    }))
}
