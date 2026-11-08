use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ChangedFile {
    pub status: String,
    pub before_path: String,
    pub after_path: String,
}

#[derive(Clone, Debug)]
pub struct Comparison {
    pub root: PathBuf,
    pub operation: String,
    pub base: String,
    pub tip: String,
    pub files: Arc<[ChangedFile]>,
    pub(crate) content: ContentSource,
}

#[derive(Clone, Debug)]
pub(crate) enum ContentSource {
    Recorded,
    Git(Arc<crate::git::GitContents>),
    External(Arc<crate::external::ExternalContents>),
    Evolution(crate::evolution::EvolutionContents),
}

pub(crate) fn query(root: &Path, operation: &str, arguments: &[&str]) -> Result<String, String> {
    let result = Command::new("jj")
        .current_dir(root)
        .args([
            "--ignore-working-copy",
            "--no-pager",
            "--color",
            "never",
            "--at-op",
            operation,
        ])
        .args(arguments)
        .output()
        .map_err(|error| format!("Could not execute jj: {error}"))?;

    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_owned());
    }

    String::from_utf8(result.stdout).map_err(|_| "This file contains non-UTF-8 data.".to_owned())
}

fn single_revision(root: &Path, operation: &str, revset: &str) -> Result<String, String> {
    let result = query(
        root,
        operation,
        &[
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            "commit_id ++ \"\\n\"",
        ],
    )?;
    let commits: Vec<_> = result.lines().collect();

    if commits.len() != 1 {
        return Err(format!(
            "Comparison endpoint {revset:?} resolved to {} revisions; supply --base and --tip explicitly.",
            commits.len()
        ));
    }

    Ok(commits[0].to_owned())
}

pub fn parse_changed_files(output: &str) -> Result<Vec<ChangedFile>, String> {
    output
        .lines()
        .map(|line| {
            let fields: [String; 3] = serde_json::from_str(line)
                .map_err(|error| format!("Invalid jj diff entry: {error}"))?;
            let [status, before_path, after_path] = fields;
            Ok(ChangedFile {
                status,
                before_path,
                after_path,
            })
        })
        .collect()
}

impl Comparison {
    pub fn current_operation(directory: &Path) -> Result<String, String> {
        query(
            directory,
            "@",
            &["op", "log", "--no-graph", "-n", "1", "-T", "id"],
        )
        .map(|id| id.trim().to_owned())
    }

    /// Resolve visible versions only. Ambiguity is an error, never a newest-time guess.
    pub fn revision(&self, expression: &str) -> Result<crate::history::Revision, String> {
        let commit = single_revision(&self.root, &self.operation, expression)?;
        let output = query(
            &self.root,
            &self.operation,
            &[
                "log",
                "--no-graph",
                "-r",
                &commit,
                "-T",
                crate::history::TEMPLATE,
            ],
        )?;
        crate::history::History::parse(&output)?
            .revisions
            .into_iter()
            .next()
            .ok_or_else(|| "Selected change is unavailable".into())
    }

    pub fn load(directory: &Path, base: Option<&str>, tip: &str) -> Result<Self, String> {
        // Prefer jj in colocated repositories. Inspect metadata rather than
        // treating an arbitrary jj error as permission to change backends.
        let directory = std::fs::canonicalize(directory)
            .map_err(|error| format!("Could not open {}: {error}", directory.display()))?;
        if !directory.ancestors().any(|path| path.join(".jj").is_dir()) {
            return crate::git::load(&directory, base, tip);
        }
        let root = PathBuf::from(query(&directory, "@", &["root"])?.trim());
        let operation = query(
            &root,
            "@",
            &["op", "log", "--no-graph", "-n", "1", "-T", "id"],
        )?;
        let operation = operation.trim().to_owned();
        let tip = single_revision(&root, &operation, tip)?;
        let default_base = format!("heads(::{tip} & immutable())");
        let base = single_revision(&root, &operation, base.unwrap_or(&default_base))?;
        Self::between(root, operation, base, tip)
    }

    pub(crate) fn between(
        root: PathBuf,
        operation: String,
        base: String,
        tip: String,
    ) -> Result<Self, String> {
        let template = "\"[\" ++ json(self.status()) ++ \",\" ++ json(self.source().path()) ++ \",\" ++ json(self.target().path()) ++ \"]\\n\"";
        let output = query(
            &root,
            &operation,
            &["diff", "--from", &base, "--to", &tip, "-T", template],
        )?;
        let files = parse_changed_files(&output)
            .map_err(|error| format!("Could not read changed files: {error}"))?;

        Ok(Self {
            root,
            operation,
            base,
            tip,
            files: files.into(),
            content: ContentSource::Recorded,
        })
    }

    /// Compare a recorded revision against its first parent. Merge revisions
    /// deliberately use the first parent, and the UI labels this choice.
    pub fn for_revision(&self, revision: &crate::history::Revision) -> Result<Self, String> {
        if matches!(self.content, ContentSource::Git(_)) {
            return crate::git::for_revision(self, revision);
        }
        let base = revision
            .parents
            .first()
            .unwrap_or(&revision.commit_id)
            .clone();
        Self::between(
            self.root.clone(),
            self.operation.clone(),
            base,
            revision.commit_id.clone(),
        )
    }

    pub fn supports_evolution(&self) -> bool {
        matches!(
            self.content,
            ContentSource::Recorded | ContentSource::Evolution(_)
        )
    }

    pub fn history(&self) -> Result<crate::history::History, String> {
        if matches!(self.content, ContentSource::Git(_)) {
            return crate::git::history(&self.root);
        }
        let revset = format!("ancestors(visible_heads() | {}, 200)", self.tip);
        let output = query(
            &self.root,
            &self.operation,
            &[
                "log",
                "--no-graph",
                "-n",
                "200",
                "-r",
                &revset,
                "-T",
                crate::history::TEMPLATE,
            ],
        )?;
        crate::history::History::parse(&output)
    }

    pub fn contents(&self, file: &ChangedFile) -> Result<(String, String), String> {
        if let ContentSource::Git(contents) = &self.content {
            return contents.read(file);
        }
        if let ContentSource::External(contents) = &self.content {
            return contents.read(file);
        }
        if let ContentSource::Evolution(contents) = &self.content {
            return contents.read(file);
        }
        let before = if file.status == "added" {
            String::new()
        } else {
            self.file_contents(&self.base, &file.before_path)?
        };
        let after = if file.status == "removed" {
            String::new()
        } else {
            self.file_contents(&self.tip, &file.after_path)?
        };

        Ok((before, after))
    }

    fn file_contents(&self, revision: &str, path: &str) -> Result<String, String> {
        let fileset = format!(
            "root-file:{}",
            serde_json::to_string(path).map_err(|error| error.to_string())?
        );
        let text = query(
            &self.root,
            &self.operation,
            &["file", "show", "-r", revision, "--", &fileset],
        )
        .map_err(|error| {
            format!(
                "Could not read {path:?} at {}: {error}",
                &revision[..revision.len().min(8)]
            )
        })?;

        if text.contains('\0') {
            return Err("Binary file: text comparison is unavailable.".into());
        }
        if text.len() > 2_000_000 {
            return Err("File exceeds the prototype's 2 MB text-view limit.".into());
        }

        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_renames_and_escaped_paths() {
        let output = "[\"renamed\",\"old file.ts\",\"new\\tfile.ts\"]\n[\"removed\",\"gone.ts\",\"gone.ts\"]\n";

        let files = parse_changed_files(output).unwrap();

        assert_eq!(files[0].before_path, "old file.ts");
        assert_eq!(files[0].after_path, "new\tfile.ts");
        assert_eq!(files[1].status, "removed");
    }

    #[test]
    fn rejects_incomplete_diff_entries() {
        assert!(parse_changed_files("[\"added\",\"only-one-path\"]").is_err());
    }

    #[test]
    fn reads_pinned_contents_without_snapshotting_working_files() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("jaywalk-test-{}-{unique}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let run = |arguments: &[&str]| {
            let result = Command::new("jj")
                .current_dir(&root)
                .args([
                    "--config",
                    "user.name=Viewer Test",
                    "--config",
                    "user.email=viewer@example.invalid",
                ])
                .args(arguments)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        run(&["git", "init"]);
        std::fs::write(root.join("old file.ts"), "const value = 1;\n").unwrap();
        std::fs::write(root.join("removed.ts"), "removed\n").unwrap();
        run(&["describe", "-m", "baseline"]);
        run(&["bookmark", "create", "main"]);
        run(&["new", "main", "-m", "change files"]);
        std::fs::rename(root.join("old file.ts"), root.join("new file.ts")).unwrap();
        std::fs::remove_file(root.join("removed.ts")).unwrap();
        std::fs::write(root.join("added.ts"), "added\n").unwrap();
        run(&["status"]);

        let comparison = Comparison::load(&root, Some("main"), "@").unwrap();
        let operation_before = comparison.operation.clone();
        let history = comparison.history().unwrap();
        let selected = history
            .revisions
            .iter()
            .find(|revision| revision.commit_id == comparison.tip)
            .unwrap();
        let revision_comparison = comparison.for_revision(selected).unwrap();
        assert_eq!(revision_comparison.base, comparison.base);
        assert_eq!(revision_comparison.operation, comparison.operation);
        std::fs::write(root.join("added.ts"), "unsnapshotted\n").unwrap();
        let added = comparison
            .files
            .iter()
            .find(|file| file.status == "added")
            .unwrap();
        let renamed = comparison
            .files
            .iter()
            .find(|file| file.status == "renamed")
            .unwrap();
        let removed = comparison
            .files
            .iter()
            .find(|file| file.status == "removed")
            .unwrap();

        assert_eq!(
            comparison.contents(added).unwrap(),
            (String::new(), "added\n".into())
        );
        assert_eq!(
            comparison.contents(renamed).unwrap(),
            ("const value = 1;\n".into(), "const value = 1;\n".into())
        );
        assert_eq!(
            comparison.contents(removed).unwrap(),
            ("removed\n".into(), String::new())
        );
        assert_eq!(
            Comparison::load(&root, Some("main"), "@")
                .unwrap()
                .operation,
            operation_before
        );
        // A merge comparison follows first-parent review semantics, while
        // history still exposes both edges of the recorded DAG.
        run(&["new", &comparison.tip, "-m", "left branch"]);
        std::fs::write(root.join("left.rs"), "left\n").unwrap();
        run(&["bookmark", "create", "left"]);
        run(&["new", &comparison.tip, "-m", "right branch"]);
        std::fs::write(root.join("right.rs"), "right\n").unwrap();
        run(&["bookmark", "create", "right"]);
        run(&["new", "left", "right", "-m", "merge branches"]);
        let merge = Comparison::load(&root, Some(&comparison.tip), "@").unwrap();
        let history = merge.history().unwrap();
        let revision = history
            .revisions
            .iter()
            .find(|revision| revision.commit_id == merge.tip)
            .unwrap();
        assert_eq!(revision.parents.len(), 2);
        let reviewed = merge.for_revision(revision).unwrap();
        assert_eq!(reviewed.base, revision.parents[0]);
        assert!(
            !reviewed
                .files
                .iter()
                .any(|file| file.after_path == "left.rs")
        );
        let added = reviewed
            .files
            .iter()
            .find(|file| file.after_path == "right.rs")
            .unwrap();
        assert_eq!(
            reviewed.contents(added).unwrap(),
            (String::new(), "right\n".into())
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
