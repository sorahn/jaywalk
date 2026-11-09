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
        let files = self
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
