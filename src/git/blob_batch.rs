//! One read-only object stream per capture, instead of two Git processes per blob.
use crate::text_content::TextLimit;
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Stdio},
    thread::JoinHandle,
};

struct Process {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    errors: Option<JoinHandle<std::io::Result<Vec<u8>>>>,
}
impl Process {
    fn stop(&mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.errors
            .take()
            .and_then(|errors| errors.join().ok())
            .and_then(Result::ok)
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
            .unwrap_or_default()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}

enum State {
    NotStarted,
    Unavailable,
    Running(Process),
}

pub(crate) struct BlobBatch {
    root: PathBuf,
    git_directory: Option<PathBuf>,
    limit: TextLimit,
    state: State,
}
impl BlobBatch {
    pub(crate) fn new(root: &Path, limit: TextLimit) -> Self {
        Self {
            root: root.to_owned(),
            git_directory: None,
            limit,
            state: State::NotStarted,
        }
    }
    pub(crate) fn with_git_directory(root: &Path, directory: &Path, limit: TextLimit) -> Self {
        Self {
            git_directory: Some(directory.to_owned()),
            ..Self::new(root, limit)
        }
    }
    /// Only a missing executable selects the optional-dependency fallback.
    /// Permission errors and a reader that starts but fails remain real errors.
    pub(crate) fn start_if_available(&mut self) -> Result<bool, String> {
        match self.state {
            State::Unavailable => return Ok(false),
            State::Running(_) => return Ok(true),
            State::NotStarted => {}
        }
        let mut command = crate::tools::command("git")?;
        command
            .current_dir(&self.root)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["--no-pager", "--literal-pathspecs"]);
        if let Some(directory) = &self.git_directory {
            command.arg("--git-dir").arg(directory);
        }
        command.args(["cat-file", "--batch"]);
        self.start_command(command)
    }
    fn start_command(&mut self, mut command: std::process::Command) -> Result<bool, String> {
        let mut child = match command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.state = State::Unavailable;
                return Ok(false);
            }
            Err(error) => return Err(format!("Could not start Git object reader: {error}")),
        };
        let input = child.stdin.take().expect("object reader stdin is piped");
        let output = BufReader::new(child.stdout.take().expect("object reader stdout is piped"));
        let errors = child.stderr.take().expect("object reader stderr is piped");
        let errors = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            errors.take(8192).read_to_end(&mut bytes)?;
            Ok(bytes)
        });
        self.state = State::Running(Process {
            child,
            input,
            output,
            errors: Some(errors),
        });
        Ok(true)
    }
    fn start(&mut self) -> Result<&mut Process, String> {
        if !self.start_if_available()? {
            return Err("Git executable is not available".into());
        }
        match &mut self.state {
            State::Running(process) => Ok(process),
            _ => unreachable!("successful reader startup installs the running process"),
        }
    }
    #[cfg(test)]
    pub(crate) fn missing_for_test(root: &Path, directory: &Path, limit: TextLimit) -> Self {
        let mut reader = Self::with_git_directory(root, directory, limit);
        assert!(
            !reader
                .start_command(std::process::Command::new(
                    root.join("missing-git-executable")
                ))
                .unwrap()
        );
        reader
    }
    pub(crate) fn read(&mut self, object: &str) -> Result<String, String> {
        // Only captured object IDs enter this protocol, never a path or user expression.
        if !matches!(object.len(), 40 | 64) || !object.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid captured Git object ID".into());
        }
        let result = self.read_inner(object);
        if let Err(error) = result {
            // Oversized/invalid replies may leave a body pending. Restart for the
            // next file instead of allocating/draining that body or desynchronizing.
            let detail = match std::mem::replace(&mut self.state, State::NotStarted) {
                State::Running(mut process) => process.stop(),
                State::Unavailable => {
                    self.state = State::Unavailable;
                    String::new()
                }
                State::NotStarted => String::new(),
            };
            return Err(if detail.is_empty() {
                error
            } else {
                format!("{error}: {detail}")
            });
        }
        result
    }
    fn read_inner(&mut self, object: &str) -> Result<String, String> {
        let limit = self.limit;
        let process = self.start()?;
        writeln!(process.input, "{object}")
            .and_then(|()| process.input.flush())
            .map_err(|e| format!("Could not request Git object {object}: {e}"))?;
        let mut header = String::new();
        (&mut process.output)
            .take(1024)
            .read_line(&mut header)
            .map_err(|e| format!("Could not read Git object header: {e}"))?;
        let fields: Vec<_> = header.split_whitespace().collect();
        if !header.ends_with('\n')
            || fields.len() != 3
            || fields[0] != object
            || fields[1] != "blob"
        {
            return Err(format!("Invalid or unavailable Git blob {object}"));
        }
        let size = fields[2]
            .parse::<u64>()
            .map_err(|e| format!("Invalid Git object size: {e}"))?;
        // Git sends its size before the body. Check before allocating any text.
        limit.check(size)?;
        let bytes = limit.read((&mut process.output).take(size))?;
        if bytes.len() as u64 != size {
            return Err(format!("Truncated Git blob {object}"));
        }
        let mut separator = [0];
        process
            .output
            .read_exact(&mut separator)
            .map_err(|e| format!("Missing Git object terminator: {e}"))?;
        if separator != *b"\n" {
            return Err("Invalid Git object terminator".into());
        }
        limit.text(bytes)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn permission_errors_do_not_mark_git_as_missing() {
        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("git-denied");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut reader = BlobBatch::new(fixture.path(), TextLimit::from_mib(1).unwrap());
        assert!(
            reader
                .start_command(std::process::Command::new(executable))
                .is_err()
        );
        assert!(matches!(reader.state, State::NotStarted));
    }
}
