use super::*;
impl Capture {
    pub fn bind(
        &self,
        comparison: crate::repository::Comparison,
    ) -> Result<crate::repository::Comparison, String> {
        let files = self
            .files
            .iter()
            .map(|f| {
                crate::repository::ChangedFile::new(
                    f.kind.as_str().into(),
                    f.before.clone(),
                    f.after.clone(),
                )
            })
            .collect();
        let comparison = comparison.bind_external(
            self.repository.clone(),
            None,
            self.working_head.clone(),
            files,
        )?;
        // Enrichment failure is deliberately nonfatal. The external capture is
        // authoritative even if a ref has moved or Git cannot resolve a name.
        let endpoints = if self.received == self.total {
            self.matched_endpoints(&comparison).ok().flatten()
        } else {
            None
        };
        comparison.bind_external(
            self.repository.clone(),
            endpoints,
            self.working_head.clone(),
            Vec::new(),
        )
    }
    fn matched_endpoints(
        &self,
        comparison: &crate::repository::Comparison,
    ) -> Result<Option<(String, String)>, String> {
        use crate::repository::FileSide;
        use std::collections::BTreeMap;
        let Some(intent) = &self.intent else {
            return Ok(None);
        };
        if intent.left == "index" || intent.right == "index" {
            return Ok(None);
        }
        let resolve = |name: &str| -> Result<String, String> {
            if name == "working tree" {
                return Ok(name.into());
            }
            if let Some(pair) = name.strip_prefix("merge-base:") {
                let (a, b) = pair.split_once(':').ok_or("Invalid merge-base intent")?;
                let output = crate::git::output(&self.repository, &["merge-base", "--all", a, b])?;
                if output.lines().count() != 1 {
                    return Err("Ambiguous merge base".into());
                }
                return Ok(output.trim().into());
            }
            Ok(crate::git::output(
                &self.repository,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{name}^{{commit}}"),
                ],
            )?
            .trim()
            .into())
        };
        let left = resolve(&intent.left)?;
        let right = resolve(&intent.right)?;
        let mut blobs =
            crate::git::blob_batch::BlobBatch::new(&self.repository, comparison.text_limit());
        for (endpoint, side) in [(&left, FileSide::Before), (&right, FileSide::After)] {
            if endpoint == "working tree" {
                continue;
            }
            let data = crate::git::output(&self.repository, &["ls-tree", "-r", "-z", endpoint])?;
            let tree: BTreeMap<_, _> = data
                .split_terminator('\0')
                .map(|entry| {
                    let (metadata, path) = entry.split_once('\t').ok_or("Invalid tree entry")?;
                    let object = metadata
                        .split_whitespace()
                        .nth(2)
                        .ok_or("Missing tree object")?;
                    Ok::<_, String>((path.to_owned(), object.to_owned()))
                })
                .collect::<Result<_, _>>()?;
            for entry in comparison.catalog().entries() {
                let file = entry.changed().unwrap_or_else(|| {
                    crate::repository::ChangedFile::new(
                        crate::repository_file::ChangeKind::Modified,
                        entry.path().into(),
                        entry.path().into(),
                    )
                });
                let path = match side {
                    FileSide::Before => file.before_path(),
                    FileSide::After => file.after_path(),
                };
                let absent = matches!(
                    (side, file.kind()),
                    (FileSide::Before, crate::repository_file::ChangeKind::Added)
                        | (FileSide::After, crate::repository_file::ChangeKind::Removed)
                );
                if absent {
                    if tree.contains_key(path) {
                        return Ok(None);
                    }
                    continue;
                }
                let Some(object) = tree.get(path) else {
                    return Ok(None);
                };
                let supplied_object = self
                    .files
                    .iter()
                    .find(|f| match side {
                        FileSide::Before => f.before == path,
                        FileSide::After => f.after == path,
                    })
                    .and_then(|f| match side {
                        FileSide::Before => f.before_object.as_ref(),
                        FileSide::After => f.after_object.as_ref(),
                    });
                if let Some(supplied) = supplied_object
                    && supplied != object
                {
                    return Ok(None);
                }
                if blobs.read(object)? != comparison.read_path(path, side)? {
                    return Ok(None);
                }
            }
        }
        Ok(Some((left, right)))
    }
}
