//! Immutable jj tree metadata with a batch reader scoped to each content request.
use super::{FileSide, query};
use crate::{git::blob_batch::BlobBatch, repository_file::FileKind, text_content::TextLimit};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug)]
struct Value {
    kind: FileKind,
    object: Option<String>,
}
#[derive(Debug)]
pub(crate) struct RecordedContents {
    root: PathBuf,
    operation: String,
    revisions: [String; 2],
    trees: [BTreeMap<String, Value>; 2],
    git_directory: Result<PathBuf, String>,
    limit: TextLimit,
}
impl RecordedContents {
    pub(super) fn load(
        root: &Path,
        operation: &str,
        base: &str,
        tip: &str,
        limit: TextLimit,
    ) -> Result<Self, String> {
        let tree = |revision: &str| {
            let template = r#""[" ++ json(path) ++ "," ++ json(file_type) ++ "," ++ if(normal_value, json(normal_value.object_id()), "null") ++ "]\n""#;
            let output = query(
                root,
                operation,
                &["file", "list", "-r", revision, "-T", template],
            )?;
            output
                .lines()
                .map(|line| {
                    let (path, kind, object): (String, String, Option<String>) =
                        serde_json::from_str(line)
                            .map_err(|error| format!("Invalid revision file entry: {error}"))?;
                    if object.as_ref().is_some_and(|id| {
                        !matches!(id.len(), 40 | 64) || !id.bytes().all(|b| b.is_ascii_hexdigit())
                    }) {
                        return Err("Invalid recorded object ID".into());
                    }
                    Ok((
                        path,
                        Value {
                            kind: FileKind::from(kind.as_str()),
                            object,
                        },
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, String>>()
        };
        let trees = [tree(base)?, tree(tip)?];
        // Keep discovery failures as content errors, rather than hiding the file
        // catalog. Conflicts can still be materialized without a normal Git blob.
        let git_directory =
            query(root, operation, &["git", "root"]).map(|path| PathBuf::from(path.trim()));
        Ok(Self {
            root: root.to_owned(),
            operation: operation.to_owned(),
            revisions: [base.to_owned(), tip.to_owned()],
            trees,
            git_directory,
            limit,
        })
    }
    fn index(side: FileSide) -> usize {
        match side {
            FileSide::Before => 0,
            FileSide::After => 1,
        }
    }
    pub(super) fn paths(&self, side: FileSide) -> BTreeMap<String, FileKind> {
        self.trees[Self::index(side)]
            .iter()
            .map(|(path, value)| (path.clone(), value.kind.clone()))
            .collect()
    }
    pub(super) fn read_path(
        &self,
        path: &str,
        side: FileSide,
        blobs: &mut Option<BlobBatch>,
    ) -> Result<String, String> {
        let index = Self::index(side);
        let value = self.trees[index]
            .get(path)
            .ok_or_else(|| format!("Path {path:?} is absent from the recorded revision"))?;
        if !matches!(
            value.kind,
            FileKind::Regular | FileKind::Symlink | FileKind::Conflict
        ) {
            return Err(format!(
                "Text view unavailable for {:?} entries",
                value.kind
            ));
        }
        if let Some(object) = &value.object {
            let directory = self.git_directory.as_ref().map_err(Clone::clone)?;
            let blobs = blobs.get_or_insert_with(|| {
                BlobBatch::with_git_directory(&self.root, directory, self.limit)
            });
            if blobs.start_if_available()? {
                return blobs.read(object);
            }
        }
        // Missing Git and conflicted/materialized content use jj directly.
        // Retain the bounded stream, pinned to the same operation and revision.
        let fileset = format!(
            "root-file:{}",
            serde_json::to_string(path).map_err(|e| e.to_string())?
        );
        let mut command = crate::tools::command("jj")?;
        command.current_dir(&self.root).args([
            "--ignore-working-copy",
            "--no-pager",
            "--color",
            "never",
            "--at-op",
            &self.operation,
            "file",
            "show",
            "-r",
            &self.revisions[index],
            "--",
            &fileset,
        ]);
        self.limit.text(self.limit.command(&mut command)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::Comparison;
    use std::process::Command;

    fn jj(root: &Path, arguments: &[&str]) -> String {
        let output = Command::new("jj")
            .current_dir(root)
            .args([
                "--config",
                "user.name=Fixture",
                "--config",
                "user.email=fixture@example.invalid",
                "--config",
                "snapshot.max-new-file-size=10485760",
            ])
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    fn fixture() -> (tempfile::TempDir, Comparison) {
        let root = tempfile::tempdir().unwrap();
        jj(root.path(), &["git", "init", "--no-colocate"]);
        std::fs::write(root.path().join("valid 漢\n.txt"), "before\n").unwrap();
        std::fs::write(root.path().join("large.txt"), vec![b'x'; 1048577]).unwrap();
        std::fs::write(root.path().join("binary.txt"), [0, 255]).unwrap();
        jj(root.path(), &["bookmark", "create", "baseline"]);
        jj(root.path(), &["new", "baseline"]);
        for path in ["valid 漢\n.txt", "large.txt", "binary.txt"] {
            std::fs::write(root.path().join(path), "after\n").unwrap();
        }
        jj(root.path(), &["status"]);
        let comparison = Comparison::load_with_limit(
            root.path(),
            Some("baseline"),
            "@",
            TextLimit::from_mib(1).unwrap(),
        )
        .unwrap();
        (root, comparison)
    }
    #[test]
    fn missing_git_falls_back_without_losing_limits_or_snapshot_identity() {
        let (root, comparison) = fixture();
        // Recorded history and reference discovery use jj too, not Git's CLI.
        jj(
            root.path(),
            &[
                "git",
                "remote",
                "add",
                "office",
                "https://example.invalid/repository.git",
            ],
        );
        let refreshed = Comparison::load(root.path(), Some("baseline"), "@").unwrap();
        let history = refreshed.history(&Default::default()).unwrap();
        assert_eq!(history.references.remotes, ["office"]);
        let mut reader = comparison.file_reader();
        reader.blobs = Some(BlobBatch::missing_for_test(
            root.path(),
            root.path(),
            comparison.text_limit(),
        ));
        let valid = comparison.catalog().find("valid 漢\n.txt").unwrap();
        assert_eq!(
            reader.read_file(valid).unwrap(),
            ("before\n".into(), "after\n".into())
        );
        assert!(
            reader
                .read_file(comparison.catalog().find("large.txt").unwrap())
                .unwrap_err()
                .contains("text limit")
        );
        assert!(
            reader
                .read_file(comparison.catalog().find("binary.txt").unwrap())
                .unwrap_err()
                .contains("Binary")
        );
        std::fs::write(root.path().join("valid 漢\n.txt"), "later snapshot\n").unwrap();
        jj(root.path(), &["status"]);
        let operation = query(
            root.path(),
            "@",
            &["op", "log", "-n", "1", "--no-graph", "-T", "id"],
        )
        .unwrap();
        assert_eq!(
            reader.read_file(valid).unwrap(),
            ("before\n".into(), "after\n".into())
        );
        assert!(!reader.blobs.as_mut().unwrap().start_if_available().unwrap());
        assert_eq!(
            query(
                root.path(),
                "@",
                &["op", "log", "-n", "1", "--no-graph", "-T", "id"]
            )
            .unwrap(),
            operation
        );

        // Verify jj itself can read the pinned blob with no Git on its PATH.
        let executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|directory| directory.join(if cfg!(windows) { "jj.exe" } else { "jj" }))
            .find(|path| path.is_file())
            .unwrap();
        let no_tools = tempfile::tempdir().unwrap();
        let output = Command::new(executable)
            .current_dir(root.path())
            .env("PATH", no_tools.path())
            .args([
                "--ignore-working-copy",
                "--at-op",
                comparison.operation(),
                "file",
                "show",
                "-r",
                comparison.tip(),
                "--",
                "root-file:\"valid 漢\\n.txt\"",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"after\n");
    }
    #[test]
    fn git_reader_failures_do_not_silently_use_jj() {
        let (root, comparison) = fixture();
        let mut contents = RecordedContents::load(
            root.path(),
            comparison.operation(),
            comparison.base(),
            comparison.tip(),
            comparison.text_limit(),
        )
        .unwrap();
        contents.git_directory = Ok(root.path().join("absent-object-store"));
        let mut reader = None;
        // jj could read this file. A broken object reader must still be reported.
        assert!(
            contents
                .read_path("valid 漢\n.txt", FileSide::After, &mut reader)
                .is_err()
        );
    }
}
