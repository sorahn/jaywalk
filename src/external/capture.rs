//! Private, disk-backed contents owned by one external comparison. The unnamed
//! file is reclaimed when its last handle closes, including on process exit.
use crate::{repository_file::FileKind, text_content::TextLimit};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata, OpenOptions},
    io::{BufRead, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::Mutex,
};

#[derive(Clone, Debug)]
enum Contents {
    Stored { offset: u64, length: u64 },
    TooLarge { length: u64 },
}
impl Contents {
    fn len(&self) -> u64 {
        match self {
            Self::Stored { length, .. } | Self::TooLarge { length } => *length,
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub kind: FileKind,
    pub executable: bool,
    digest: Option<[u8; 32]>,
    contents: Contents,
}
impl Entry {
    pub fn equal(&self, other: &Self) -> Option<bool> {
        if self.kind != other.kind
            || self.executable != other.executable
            || self.contents.len() != other.contents.len()
        {
            return Some(false);
        }
        self.digest
            .zip(other.digest)
            .map(|(left, right)| left == right)
    }
}
pub(super) struct Source {
    path: PathBuf,
    metadata: Metadata,
}
impl Source {
    pub fn new(path: PathBuf) -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
        if !metadata.is_file() && !metadata.is_symlink() {
            return Err(format!("Unsupported file type: {}", path.display()));
        }
        Ok(Self { path, metadata })
    }
    pub fn equal_oversized(&self, other: &Self) -> Result<bool, String> {
        let mut left = std::io::BufReader::new(open_regular(self)?);
        let mut right = std::io::BufReader::new(open_regular(other)?);
        // Bounded reads also reject growth rather than chasing an appending file.
        let mut remaining = self.metadata.len();
        let equal = loop {
            let a = left.fill_buf().map_err(|error| error.to_string())?;
            let b = right.fill_buf().map_err(|error| error.to_string())?;
            if remaining == 0 {
                break true;
            }
            if a.is_empty() || b.is_empty() {
                break false;
            }
            let count = ((a.len().min(b.len()) as u64).min(remaining)) as usize;
            if a[..count] != b[..count] {
                break false;
            }
            left.consume(count);
            right.consume(count);
            remaining -= count as u64;
        };
        stable(
            &self.metadata,
            &left
                .get_ref()
                .metadata()
                .map_err(|error| error.to_string())?,
        )?;
        stable(
            &other.metadata,
            &right
                .get_ref()
                .metadata()
                .map_err(|error| error.to_string())?,
        )?;
        Ok(equal)
    }
}
fn executable(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o100 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

#[derive(Debug)]
pub(super) struct Capture {
    file: Mutex<File>,
}
impl Capture {
    pub fn read(&self, entry: &Entry, limit: TextLimit) -> Result<String, String> {
        limit.check(entry.contents.len())?;
        let Contents::Stored { offset, length } = entry.contents else {
            return Err("Captured file exceeds the text limit".into());
        };
        let mut file = self
            .file
            .lock()
            .map_err(|_| "External capture lock poisoned")?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| format!("Could not seek external capture: {error}"))?;
        let bytes = limit.read((&mut *file).take(length))?;
        if bytes.len() as u64 != length {
            return Err("External capture is incomplete".into());
        }
        limit.text(bytes)
    }
}
pub(super) struct Builder {
    file: File,
    limit: TextLimit,
}
impl Builder {
    pub fn new(limit: TextLimit) -> Result<Self, String> {
        Ok(Self {
            file: tempfile::tempfile()
                .map_err(|error| format!("Could not create private external capture: {error}"))?,
            limit,
        })
    }
    pub fn finish(self) -> Capture {
        Capture {
            file: Mutex::new(self.file),
        }
    }
    pub fn entry(&mut self, source: &Source) -> Result<Entry, String> {
        self.capture_entry(source)
            .map_err(|error| format!("Could not capture {}: {error}", source.path.display()))
    }
    fn capture_entry(&mut self, source: &Source) -> Result<Entry, String> {
        let path = &source.path;
        let metadata = &source.metadata;
        if metadata.is_symlink() {
            let target = std::fs::read_link(path).map_err(|error| error.to_string())?;
            // Link targets are data, never followed into a different file/tree.
            #[cfg(unix)]
            let data = {
                use std::os::unix::ffi::OsStrExt;
                target.as_os_str().as_bytes()
            };
            #[cfg(not(unix))]
            let data = target.to_str().ok_or("Non-UTF-8 link target")?.as_bytes();
            let (contents, digest) = self.stream(std::io::Cursor::new(data))?;
            let after = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
            stable(metadata, &after)?;
            return Ok(Entry {
                kind: FileKind::Symlink,
                executable: false,
                digest: Some(digest),
                contents,
            });
        }
        let mut file = open_regular(source)?;
        let before = file.metadata().map_err(|error| error.to_string())?;
        let (contents, digest) = if before.len() > self.limit.bytes() as u64 {
            (
                Contents::TooLarge {
                    length: before.len(),
                },
                None,
            )
        } else {
            let (contents, digest) = self.stream(&mut file)?;
            (contents, Some(digest))
        };
        let after = file.metadata().map_err(|error| error.to_string())?;
        stable(&before, &after)?;
        if contents.len() != after.len() {
            return Err("Input changed during capture; reopen the comparison".into());
        }
        Ok(Entry {
            kind: FileKind::Regular,
            executable: executable(&before),
            digest,
            contents,
        })
    }
    fn stream(&mut self, input: impl Read) -> Result<(Contents, [u8; 32]), String> {
        let mut input = input.take(self.limit.bytes() as u64 + 1);
        let offset = self
            .file
            .stream_position()
            .map_err(|error| error.to_string())?;
        let mut length = 0_u64;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65_536];
        loop {
            let count = input.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
            length = length
                .checked_add(count as u64)
                .ok_or("External input size overflow")?;
            if length <= self.limit.bytes() as u64 {
                self.file
                    .write_all(&buffer[..count])
                    .map_err(|error| error.to_string())?;
            }
        }
        let contents = if length > self.limit.bytes() as u64 {
            // Discard a growing input's bounded prefix. Normal oversized files
            // never enter this copy path; assembly compares those separately.
            self.file
                .set_len(offset)
                .map_err(|error| error.to_string())?;
            self.file
                .seek(SeekFrom::Start(offset))
                .map_err(|error| error.to_string())?;
            Contents::TooLarge { length }
        } else {
            Contents::Stored { offset, length }
        };
        Ok((contents, hash.finalize().into()))
    }
}
fn open_regular(source: &Source) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(&source.path)
        .map_err(|error| format!("Could not open {}: {error}", source.path.display()))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("Input is not a regular file or symbolic link".into());
    }
    stable(&source.metadata, &metadata)?;
    Ok(file)
}

fn stable(before: &Metadata, after: &Metadata) -> Result<(), String> {
    let changed = before.len() != after.len()
        || before.modified().map_err(|error| error.to_string())?
            != after.modified().map_err(|error| error.to_string())?;
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        changed
            || before.dev() != after.dev()
            || before.ino() != after.ino()
            || before.ctime() != after.ctime()
            || before.ctime_nsec() != after.ctime_nsec()
            || before.mode() != after.mode()
    };
    if changed {
        Err("Input changed during capture; reopen the comparison".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn growing_reads_are_bounded_and_partial_copies_are_discarded() {
        let limit = TextLimit::from_mib(1).unwrap();
        let mut builder = Builder::new(limit).unwrap();
        let mut input = std::io::Cursor::new(vec![b'x'; 2 * 1024 * 1024]);
        let (large, _) = builder.stream(&mut input).unwrap();
        assert!(matches!(large, Contents::TooLarge { .. }));
        assert_eq!(input.position(), limit.bytes() as u64 + 1);
        assert_eq!(builder.file.metadata().unwrap().len(), 0);
        let (contents, digest) = builder.stream(&b"captured"[..]).unwrap();
        let entry = Entry {
            kind: FileKind::Regular,
            executable: false,
            contents,
            digest: Some(digest),
        };
        let captured = builder.finish();
        for _ in 0..2 {
            assert_eq!(captured.read(&entry, limit).unwrap(), "captured");
        }
        let too_large = Entry {
            contents: large,
            ..entry
        };
        assert!(
            captured
                .read(&too_large, limit)
                .unwrap_err()
                .contains("text limit")
        );
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;
    #[test]
    fn metadata_only_oversized_capture_avoids_retaining_or_hashing_the_file() {
        let source_file = tempfile::NamedTempFile::new().unwrap();
        source_file
            .as_file()
            .set_len(4 * 1024 * 1024 * 1024)
            .unwrap();
        let source = Source::new(source_file.path().into()).unwrap();
        let mut builder = Builder::new(TextLimit::from_mib(1).unwrap()).unwrap();
        let entry = builder.entry(&source).unwrap();
        assert!(entry.digest.is_none());
        assert_eq!(entry.contents.len(), 4 * 1024 * 1024 * 1024);
        assert_eq!(builder.file.metadata().unwrap().len(), 0);
    }
    #[test]
    fn changing_a_planned_source_fails_instead_of_mixing_metadata_and_bytes() {
        let source_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source_file.path(), b"before").unwrap();
        let source = Source::new(source_file.path().into()).unwrap();
        std::fs::write(source_file.path(), b"a different length").unwrap();
        let mut builder = Builder::new(TextLimit::from_mib(1).unwrap()).unwrap();
        assert!(
            builder
                .entry(&source)
                .unwrap_err()
                .contains("changed during capture")
        );
    }
}
