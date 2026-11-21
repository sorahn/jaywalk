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
            return blobs.read(object);
        }
        // Conflicted/materialized jj content has no single object size. Retain
        // the bounded stream and pin it to the same operation and revision.
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
