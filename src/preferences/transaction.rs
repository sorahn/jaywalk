//! Best-effort rollback across configuration and profile files. Each replacement
//! is atomic; all validation/staging happens first. External edits always win.
use std::{io::Write, path::PathBuf};
pub(super) fn replace(documents: &[(PathBuf, String, String)]) -> Result<(), String> {
    let mut staged = Vec::new();
    let mut backups = Vec::new();
    for (path, original, updated) in documents {
        let metadata = std::fs::metadata(path)
            .map_err(|e| format!("Could not inspect {}: {e}", path.display()))?;
        if metadata.permissions().readonly() {
            return Err(format!("File is read-only: {}", path.display()));
        }
        let stage = |text: &str| -> Result<tempfile::NamedTempFile, String> {
            let mut file =
                tempfile::NamedTempFile::new_in(path.parent().ok_or("File has no parent")?)
                    .map_err(|e| e.to_string())?;
            file.as_file()
                .set_permissions(metadata.permissions())
                .map_err(|e| e.to_string())?;
            file.write_all(text.as_bytes())
                .and_then(|_| file.as_file().sync_all())
                .map_err(|e| e.to_string())?;
            Ok(file)
        };
        staged.push(stage(updated)?);
        backups.push(stage(original)?);
    }
    let unchanged = |path: &PathBuf, expected: &str| -> Result<bool, String> {
        Ok(std::fs::read_to_string(path)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?
            == expected)
    };
    for (path, original, _) in documents {
        if !unchanged(path, original)? {
            return Err(format!(
                "{} changed elsewhere. Discard or reload your draft before saving.",
                path.display()
            ));
        }
    }
    for (index, file) in staged.into_iter().enumerate() {
        let (path, original, _) = &documents[index];
        let result = unchanged(path, original).and_then(|same| {
            if !same {
                return Err(format!(
                    "{} changed while saving; external edit preserved.",
                    path.display()
                ));
            }
            file.persist(path)
                .map(|_| ())
                .map_err(|e| format!("Could not save {}: {e}", path.display()))
        });
        if let Err(error) = result {
            let mut rollback_errors = Vec::new();
            for ((path, _, updated), backup) in documents.iter().zip(backups).take(index) {
                match unchanged(path, updated) {
                    Ok(true) => {
                        if let Err(e) = backup.persist(path) {
                            rollback_errors.push(format!("{}: {e}", path.display()));
                        }
                    }
                    Ok(false) => rollback_errors
                        .push(format!("{} changed externally; preserved", path.display())),
                    Err(e) => rollback_errors.push(e),
                }
            }
            return Err(if rollback_errors.is_empty() {
                error
            } else {
                format!(
                    "{error} Some prior writes could not be rolled back: {}",
                    rollback_errors.join("; ")
                )
            });
        }
    }
    Ok(())
}
