use super::*;
/// A window holds a shared lease on its owned inputs. Last-window cleanup waits
/// for the producer to finish and takes an exclusive lease before removing data.
pub struct OwnedInputs {
    file: Option<std::fs::File>,
    root: PathBuf,
    identity: Identity,
}
impl Capture {
    pub fn own(&self, runtime: &Path) -> Result<OwnedInputs, String> {
        let root = self.paths(runtime)[0]
            .parent()
            .expect("side parent")
            .to_owned();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("viewer.lock"))
            .map_err(|e| e.to_string())?;
        file.lock_shared().map_err(|e| e.to_string())?;
        Ok(OwnedInputs {
            file: Some(file),
            root,
            identity: self.identity.clone(),
        })
    }
}
impl Drop for OwnedInputs {
    fn drop(&mut self) {
        drop(self.file.take());
        let root = self.root.clone();
        let identity = self.identity.clone();
        std::thread::spawn(move || {
            for _ in 0..240 {
                if process::alive(&identity) == Some(true) {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    continue;
                }
                if process::alive(&identity).is_none() {
                    return;
                }
                let Ok(file) = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(root.join("viewer.lock"))
                else {
                    return;
                };
                if file.try_lock().is_ok() {
                    let _ = std::fs::remove_dir_all(root);
                }
                return;
            }
        });
    }
}

/// Also recover captures left by app termination before a cleanup thread ran.
/// Recent or permission-indeterminate producers are never removed here.
pub(super) fn reclaim(runtime: &Path) {
    use std::io::Read;
    let Ok(entries) = std::fs::read_dir(runtime.join("difftool")) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if !entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(86400))
        {
            continue;
        }
        let root = entry.path();
        let Ok(file) = std::fs::File::open(root.join("capture.json")) else {
            continue;
        };
        let mut data = Vec::new();
        if file.take(1_048_577).read_to_end(&mut data).is_err() || data.len() > 1_048_576 {
            continue;
        }
        let Ok(capture) = serde_json::from_slice::<Capture>(&data) else {
            continue;
        };
        if root.file_name().and_then(|s| s.to_str()) != Some(capture.key().as_str())
            || process::alive(&capture.identity) != Some(false)
        {
            continue;
        }
        let Ok(lease) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("viewer.lock"))
        else {
            continue;
        };
        if lease.try_lock().is_ok() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
