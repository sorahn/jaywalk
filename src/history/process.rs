//! Bounded streaming reads. Cancellation stops the child and its helper group.
use super::Revision;
use crate::accounts::Control;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

#[derive(Clone, Copy)]
pub(super) enum Format {
    Git,
    Jj,
}
const MAX_FIELD: u64 = 1024 * 1024;

fn field(reader: &mut impl BufRead, delimiter: u8) -> Result<Option<String>, String> {
    let mut bytes = Vec::new();
    let count = reader
        .take(MAX_FIELD + 1)
        .read_until(delimiter, &mut bytes)
        .map_err(|e| format!("Reading history: {e}"))?;
    if count == 0 {
        return Ok(None);
    }
    if count as u64 > MAX_FIELD {
        return Err("History metadata field exceeds 1 MiB".into());
    }
    if bytes.pop() != Some(delimiter) {
        return Err("Incomplete history metadata record".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| format!("Non-UTF-8 history metadata: {e}"))
}
fn record(reader: &mut impl BufRead, format: Format) -> Result<Option<Revision>, String> {
    match format {
        Format::Git => {
            let Some(id) = field(reader, 0)? else {
                return Ok(None);
            };
            let mut next =
                || field(reader, 0)?.ok_or_else(|| "Incomplete Git history record".to_owned());
            Ok(Some(Revision {
                commit_id: id.clone(),
                change_id: id,
                parents: next()?.split_whitespace().map(str::to_owned).collect(),
                description: next()?,
                author: next()?,
                timestamp: next()?,
                references: vec![],
                working_copy: false,
                empty: false,
            }))
        }
        Format::Jj => {
            let Some(line) = field(reader, b'\n')? else {
                return Ok(None);
            };
            Ok(super::History::parse(&line)?.revisions.pop())
        }
    }
}
fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    // SAFETY: the unreaped child owns this process group; kill its helpers too.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

pub(super) fn scan(
    mut command: Command,
    format: Format,
    control: &Control,
    mut consume: impl FnMut(Revision) -> bool,
) -> Result<(), String> {
    control.check().map_err(|e| e.to_string())?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Starting history query: {e}"))?;
    let stdout = child.stdout.take().expect("history stdout is piped");
    let stderr = child.stderr.take().expect("history stderr is piped");
    let (tx, rx) = mpsc::sync_channel(16);
    let output = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let entry = record(&mut reader, format);
            if matches!(entry, Ok(None)) {
                break;
            }
            let failed = entry.is_err();
            if tx.send(entry).is_err() || failed {
                break;
            }
        }
    });
    let errors = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut reader = BufReader::new(stderr);
        reader.by_ref().take(64 * 1024).read_to_end(&mut kept)?;
        std::io::copy(&mut reader, &mut std::io::sink())?;
        Ok::<_, std::io::Error>(kept)
    });
    let mut failure = None;
    let mut stopped = false;
    loop {
        if let Err(error) = control.check() {
            failure = Some(error.to_string());
            break;
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(Some(revision))) => {
                if !consume(revision) {
                    stopped = true;
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(error)) => {
                failure = Some(error);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(rx);
    if stopped || failure.is_some() {
        terminate(&mut child);
    }
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => {
                terminate(&mut child);
                let _ = child.wait();
                break Err(format!("Waiting for history query: {error}"));
            }
            Ok(None) => {
                if let Err(error) = control.check() {
                    failure = Some(error.to_string());
                    terminate(&mut child);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    output.join().map_err(|_| "History output worker failed")?;
    let errors = errors
        .join()
        .map_err(|_| "History error worker failed")?
        .map_err(|e| format!("Reading history errors: {e}"))?;
    if let Some(error) = failure {
        return Err(error);
    }
    if !stopped && !status?.success() {
        return Err(format!(
            "History query failed: {}",
            String::from_utf8_lossy(&errors).trim()
        ));
    }
    Ok(())
}
