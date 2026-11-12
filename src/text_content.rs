//! Bounded acquisition of text bytes, before decoding, diffing or parsing.
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

const MIB: u64 = 1_048_576;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextLimit(usize);
impl Default for TextLimit {
    fn default() -> Self {
        Self::from_mib(
            crate::settings::Config::default()
                .diff
                .max_text_file_size_mb,
        )
        .expect("shipped text limit fits the address space")
    }
}
impl TextLimit {
    pub fn from_mib(mib: u64) -> Result<Self, String> {
        let bytes = mib
            .checked_mul(MIB)
            .and_then(|bytes| usize::try_from(bytes).ok())
            .filter(|bytes| *bytes > 0 && *bytes < usize::MAX)
            .ok_or_else(|| {
                "text limit must be positive integer MiB with a representable byte count plus one"
                    .to_owned()
            })?;
        Ok(Self(bytes))
    }
    pub fn bytes(self) -> usize {
        self.0
    }
    fn too_large(self, size: u64) -> String {
        format!(
            "File size is at least {size} bytes; exceeds the configured text limit of {} MiB ({} bytes) per side",
            self.0 as u64 / MIB,
            self.0
        )
    }
    pub(crate) fn check(self, size: u64) -> Result<(), String> {
        if size > self.0 as u64 {
            Err(self.too_large(size))
        } else {
            Ok(())
        }
    }
    pub(crate) fn read(self, reader: impl Read) -> Result<Vec<u8>, String> {
        // No capacity is reserved from untrusted metadata. Read at most one
        // byte over the cap, including when a file grows after preflight.
        let mut bytes = Vec::new();
        reader
            .take(self.0 as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        self.check(bytes.len() as u64)?;
        Ok(bytes)
    }
    pub(crate) fn file(self, path: &Path) -> Result<Vec<u8>, String> {
        let file = std::fs::File::open(path)
            .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
        let size = file.metadata().map_err(|error| error.to_string())?.len();
        self.check(size)?;
        self.read(file)
            .map_err(|error| format!("{}: {error}", path.display()))
    }
    pub(crate) fn text(self, bytes: Vec<u8>) -> Result<String, String> {
        self.check(bytes.len() as u64)?;
        if bytes.contains(&0) {
            return Err("Binary file: text comparison is unavailable.".into());
        }
        String::from_utf8(bytes)
            .map_err(|_| "Non-UTF-8 content: text comparison is unavailable.".into())
    }
    pub(crate) fn command(self, command: &mut Command) -> Result<Vec<u8>, String> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("Could not start content reader: {error}"))?;
        let mut stderr = child.stderr.take().expect("stderr was piped");
        let errors = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            let mut buffer = [0; 8192];
            // Keep bounded diagnostics, but drain the rest without closing the
            // pipe early or making the content reader fail with a broken pipe.
            loop {
                let count = stderr.read(&mut buffer)?;
                if count == 0 {
                    return Ok(bytes);
                }
                let keep = count.min(65_536 - bytes.len());
                bytes.extend_from_slice(&buffer[..keep]);
            }
        });
        let bytes = self.read(child.stdout.take().expect("stdout was piped"));
        if bytes.is_err() {
            let _ = child.kill();
        }
        let status = child.wait();
        if status.is_err() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let stderr = errors
            .join()
            .map_err(|_| "Content diagnostic reader panicked".to_owned())?
            .map_err(|error| format!("Could not read content diagnostics: {error}"))?;
        let bytes = bytes?;
        let status = status.map_err(|error| format!("Could not reap content reader: {error}"))?;
        if !status.success() {
            return Err(format!(
                "Content reader failed: {}",
                String::from_utf8_lossy(&stderr).trim()
            ));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::TextLimit;
    use std::io::{Cursor, Read};
    #[test]
    fn bounded_reads_accept_the_boundary_and_stop_growing_inputs() {
        let limit = TextLimit(8);
        assert_eq!(limit.read(Cursor::new(vec![b'x'; 8])).unwrap().len(), 8);
        let mut input = Cursor::new(vec![b'x'; 100]);
        assert!(limit.read(&mut input).unwrap_err().contains("text limit"));
        assert_eq!(input.position(), 9);
        // Unknown/growing input lengths cannot bypass the post-read guard.
        assert!(limit.read(std::io::repeat(b'x').take(100)).is_err());
        assert!(TextLimit::from_mib(0).is_err());
        assert!(TextLimit::from_mib(u64::MAX).is_err());
    }
    #[test]
    fn regular_file_growth_after_size_preflight_is_still_bounded() {
        let path =
            std::env::temp_dir().join(format!("jaywalk-growing-read-{}", std::process::id()));
        std::fs::write(&path, b"12345678").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let limit = TextLimit(8);
        limit.check(file.metadata().unwrap().len()).unwrap();
        std::fs::write(&path, [b'x'; 100]).unwrap();
        assert!(limit.read(file).unwrap_err().contains("text limit"));
        std::fs::remove_file(path).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn subprocess_diagnostics_are_drained_without_blocking_content() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "head -c 131072 /dev/zero >&2; printf ok"]);
        assert_eq!(TextLimit(8).command(&mut command).unwrap(), b"ok");
    }
    #[cfg(unix)]
    #[test]
    fn oversized_subprocess_is_stopped_and_reaped_with_bounded_stderr() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "while :; do printf 'xxxxxxxxxxxxxxxx'; done"]);
        assert!(
            TextLimit(64)
                .command(&mut command)
                .unwrap_err()
                .contains("text limit")
        );
    }
}
