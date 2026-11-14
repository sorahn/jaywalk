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

pub(crate) struct BlobBatch {
    root: PathBuf,
    limit: TextLimit,
    process: Option<Process>,
}
impl BlobBatch {
    pub(crate) fn new(root: &Path, limit: TextLimit) -> Self {
        Self {
            root: root.to_owned(),
            limit,
            process: None,
        }
    }
    fn start(&mut self) -> Result<&mut Process, String> {
        if self.process.is_none() {
            let mut child = crate::tools::command("git")?
                .current_dir(&self.root)
                .env("GIT_OPTIONAL_LOCKS", "0")
                .args(["--no-pager", "--literal-pathspecs", "cat-file", "--batch"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("Could not start Git object reader: {e}"))?;
            let input = child.stdin.take().expect("object reader stdin is piped");
            let output =
                BufReader::new(child.stdout.take().expect("object reader stdout is piped"));
            let errors = child.stderr.take().expect("object reader stderr is piped");
            let errors = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                errors.take(8192).read_to_end(&mut bytes)?;
                Ok(bytes)
            });
            self.process = Some(Process {
                child,
                input,
                output,
                errors: Some(errors),
            });
        }
        Ok(self.process.as_mut().expect("object reader was started"))
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
            let detail = self
                .process
                .take()
                .map(|mut process| process.stop())
                .unwrap_or_default();
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
