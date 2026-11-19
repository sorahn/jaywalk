use crate::accounts::Control;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
pub(super) fn command(root: &Path) -> Result<Command, String> {
    let mut command = crate::tools::command("git")?;
    command
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "submodule.recurse=false",
            "-c",
            "protocol.ext.allow=never",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    Ok(command)
}
pub(super) fn git(
    root: &Path,
    args: &[&str],
    control: &Control,
    budget: Option<(&Path, u64)>,
) -> Result<Vec<u8>, String> {
    control.check().map_err(|e| e.to_string())?;
    if let Some((path, max)) = budget {
        check_budget(path, max)?;
    }
    let mut command = command(root)?;
    command.args(args);
    let mut child = command.spawn().map_err(|_| "Could not start Git")?;
    fn drain(mut stream: impl Read) -> std::io::Result<(Vec<u8>, bool)> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        let mut oversized = false;
        loop {
            let count = match stream.read(&mut buffer) {
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if count == 0 {
                break;
            }
            let keep = count.min((16 * 1024 * 1024usize).saturating_sub(bytes.len()));
            bytes.extend_from_slice(&buffer[..keep]);
            oversized |= keep < count;
        }
        Ok((bytes, oversized))
    }
    let stdout = child.stdout.take().expect("Git stdout is piped");
    let stderr = child.stderr.take().expect("Git stderr is piped");
    let out = std::thread::spawn(move || drain(stdout));
    let err = std::thread::spawn(move || drain(stderr));
    let mut failure = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(_) => {
                failure = Some("Could not wait for Git".to_string());
            }
        }
        if let Err(error) = control.check() {
            failure = Some(error.to_string());
        }
        if let Some((path, max)) = budget {
            match directory_bytes(path) {
                Ok(bytes) if bytes > max => {
                    failure = Some("Local review storage limit exceeded".into())
                }
                Err(error) => failure = Some(error),
                _ => {}
            }
        }
        if failure.is_some() {
            #[cfg(unix)]
            // SAFETY: this unreaped child was created in its own process group;
            // a negative PID targets that group, including Git's helper children.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            break Err(());
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    let output = out
        .join()
        .map_err(|_| "Git output worker failed")?
        .map_err(|_| "Could not read Git output")?;
    let errors = err
        .join()
        .map_err(|_| "Git error worker failed")?
        .map_err(|_| "Could not read Git errors")?;
    if let Some(error) = failure {
        return Err(error);
    }
    if !status.map_err(|_| "Git was interrupted")?.success() {
        #[cfg(test)]
        eprintln!(
            "synthetic local-action Git failure: {}",
            String::from_utf8_lossy(&errors.0)
                .chars()
                .take(800)
                .collect::<String>()
        );
        #[cfg(not(test))]
        let _ = errors;
        return Err("Git operation failed (authentication, missing revision or repository state); no force or cleanup was attempted".into());
    }
    if output.1 {
        return Err("Git metadata exceeds the 16 MiB limit".into());
    }
    if let Some((path, max)) = budget
        && directory_bytes(path)? > max
    {
        return Err("Local review storage limit exceeded".into());
    }
    Ok(output.0)
}
fn directory_bytes(root: &Path) -> Result<u64, String> {
    let mut pending = vec![root.to_owned()];
    let mut total = 0u64;
    while let Some(path) = pending.pop() {
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if path != root && error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("Could not inspect local review storage".into()),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("Could not inspect local review storage".into()),
            };
            // Git atomically renames/deletes temporary packs while fetch runs.
            // A vanished entry is absent, not a broken storage directory; the
            // post-process check measures the settled store again.
            let metadata = match entry.path().symlink_metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("Could not inspect local review storage".into()),
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

pub(super) fn check_budget(root: &Path, max: u64) -> Result<(), String> {
    if directory_bytes(root)? > max {
        Err("Local review storage limit exceeded".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_scans_tolerate_pack_renames_but_still_enforce_the_settled_size() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().to_owned();
        for index in 0..100 {
            std::fs::write(path.join(format!("pack-{index}")), [0u8; 128]).unwrap();
        }
        let changing = path.clone();
        let writer = std::thread::spawn(move || {
            for index in 0..1000 {
                let old = changing.join(format!("pack-{}", index % 100));
                let new = changing.join("temporary-pack");
                let _ = std::fs::rename(&old, &new);
                let _ = std::fs::rename(&new, &old);
            }
        });
        for _ in 0..100 {
            assert!(directory_bytes(&path).is_ok());
        }
        writer.join().unwrap();
        assert_eq!(directory_bytes(&path).unwrap(), 12800);
        assert!(check_budget(&path, 12799).is_err());
    }
}
