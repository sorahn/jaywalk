//! Capture Git-supplied files before acknowledging a tool callback. Optional
//! command provenance never replaces these bytes with a reconstructed diff.
mod matching;
mod ownership;
mod process;
pub use ownership::OwnedInputs;
pub use process::Identity;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {
    pub identity: Identity,
    pub grouped: bool,
    pub directory: bool,
    pub repository: PathBuf,
    pub working_head: Option<String>,
    pub received: usize,
    pub total: usize,
    pub files: Vec<File>,
    pub intent: Option<Intent>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct File {
    pub before: String,
    pub after: String,
    pub kind: String,
    pub before_object: Option<String>,
    pub after_object: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub left: String,
    pub right: String,
}
impl Intent {
    /// Deliberately conservative: unsupported flags retain exact supplied inputs
    /// but do not get guessed endpoint markers. No reconstructed shell execution.
    fn parse(args: &[String]) -> Option<Self> {
        let start = process::command_index(args)?;
        if args[start] != "difftool" {
            return None;
        }
        let mut revisions = Vec::new();
        let mut cached = false;
        let mut reverse = false;
        let mut i = start + 1;
        while i < args.len() {
            let arg = &args[i];
            match arg.as_str() {
                "--" => break,
                "--cached" | "--staged" => cached = true,
                "-R" => reverse = !reverse,
                "-y" | "--no-prompt" | "-d" | "--dir-diff" | "--no-symlinks" | "--symlinks"
                | "--no-ext-diff" | "--no-textconv" => {}
                "-t" | "--tool" => {
                    i += 1;
                    if i >= args.len() {
                        return None;
                    }
                }
                a if a.starts_with("--tool=") => {}
                a if a.starts_with('-') => return None,
                a => revisions.push(a.to_owned()),
            }
            i += 1;
        }
        let (left, right) = match revisions.as_slice() {
            [] => (
                "HEAD".into(),
                if cached { "index" } else { "working tree" }.into(),
            ),
            [one] if one.contains("...") => {
                let (a, b) = one.split_once("...")?;
                (
                    format!(
                        "merge-base:{}:{}",
                        if a.is_empty() { "HEAD" } else { a },
                        if b.is_empty() { "HEAD" } else { b }
                    ),
                    if b.is_empty() { "HEAD" } else { b }.into(),
                )
            }
            [one] if one.contains("..") => {
                let (a, b) = one.split_once("..")?;
                (
                    if a.is_empty() { "HEAD" } else { a }.into(),
                    if b.is_empty() { "HEAD" } else { b }.into(),
                )
            }
            [one] => (
                one.clone(),
                if cached { "index" } else { "working tree" }.into(),
            ),
            [a, b] if !cached => (a.clone(), b.clone()),
            _ => return None,
        };
        // Without revisions and without --cached Git compares index to worktree.
        let left = if revisions.is_empty() && !cached {
            "index".into()
        } else {
            left
        };
        Some(if reverse {
            Self {
                left: right,
                right: left,
            }
        } else {
            Self { left, right }
        })
    }
}
fn valid_relative(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Unsafe difftool file path".into());
    }
    Ok(path)
}
fn object(value: &str) -> Option<String> {
    ((value.len() == 40 || value.len() == 64)
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value.bytes().any(|b| b != b'0'))
    .then(|| value.to_owned())
}
fn copy_entry(source: &Path, destination: &Path) -> Result<(), String> {
    if source == Path::new("/dev/null") {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(source)
        .map_err(|e| format!("Could not inspect {}: {e}", source.display()))?;
    if meta.is_dir() {
        std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            copy_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if meta.is_symlink() {
        // Git's directory-mode optimization links regular files back into the
        // worktree. Materialize those contents; preserve actual repository links
        // using the captured tree mode when available (handled by caller).
        let target = std::fs::canonicalize(source)
            .map_err(|e| format!("Could not capture {}: {e}", source.display()))?;
        if !target.is_file() {
            return Err("Difftool input link does not refer to a regular file".into());
        }
        return copy_entry(&target, destination);
    }
    if !meta.is_file() {
        return Err("Unsupported difftool input type".into());
    }
    use std::io::Write;
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| format!("Could not own {}: {e}", destination.display()))?;
    let mut input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())?;
    std::fs::set_permissions(destination, meta.permissions()).map_err(|e| e.to_string())
}
impl Capture {
    pub fn key(&self) -> String {
        format!(
            "{}-{}-{:016x}{:016x}",
            self.identity.pid, self.identity.started, self.identity.boot[0], self.identity.boot[1]
        )
    }
    pub fn paths(&self, runtime: &Path) -> [PathBuf; 2] {
        let root = runtime.join("difftool").join(self.key());
        [root.join("left"), root.join("right")]
    }
    pub fn progress_label(&self) -> String {
        if self.directory {
            "Difftool".into()
        } else {
            format!("Difftool · {} / {} files", self.received, self.total)
        }
    }
    pub fn label(&self) -> String {
        format!(
            "{}{}",
            self.progress_label(),
            self.intent
                .as_ref()
                .map(|i| format!(" · {} → {}", i.left, i.right))
                .unwrap_or_default()
        )
    }
    pub fn try_capture(inputs: &[PathBuf], runtime: &Path) -> Result<Option<Self>, String> {
        if inputs.len() != 2
            || (std::env::var_os("GIT_DIFF_TOOL").is_none()
                && std::env::var_os("GIT_DIFFTOOL_DIRDIFF").is_none()
                && std::env::var_os("GIT_DIFF_PATH_COUNTER").is_none())
        {
            return Ok(None);
        }
        let (invocation, helper_file) = process::capture();
        let grouped = invocation.is_some();
        let identity = invocation
            .as_ref()
            .map(|i| i.identity.clone())
            .or_else(|| process::read(std::process::id()).map(|p| p.identity))
            .unwrap_or_else(|| Identity {
                pid: std::process::id(),
                boot: [0, 0],
                started: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64,
            });
        let intent = invocation
            .as_ref()
            .and_then(|i| Intent::parse(&i.arguments));
        let helper_file = invocation.and_then(|i| i.file).or(helper_file);
        // Ask Git in its producer environment before isolating later app queries.
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Ok(None);
        }
        let repository = PathBuf::from(
            String::from_utf8(output.stdout)
                .map_err(|e| e.to_string())?
                .trim_end_matches('\n'),
        );
        let directory = std::env::var_os("GIT_DIFFTOOL_DIRDIFF").is_some();
        let number = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
        };
        let (counter, total) = if directory || !grouped {
            (1, 1)
        } else {
            match (
                number("GIT_DIFF_PATH_COUNTER"),
                number("GIT_DIFF_PATH_TOTAL"),
            ) {
                (Some(n), Some(t)) if n > 0 && n <= t => (n, t),
                _ => return Ok(None),
            }
        };
        if counter == 1 {
            ownership::reclaim(runtime);
        }
        let mut capture = Self {
            identity,
            grouped,
            directory,
            working_head: crate::git::output(&repository, &["rev-parse", "--verify", "HEAD"])
                .ok()
                .map(|id| id.trim().to_owned()),
            repository,
            received: counter,
            total,
            files: Vec::new(),
            intent,
        };
        let paths = capture.paths(runtime);
        let root = paths[0].parent().expect("owned side has a parent");
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(runtime, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("capture.lock"))
            .map_err(|e| e.to_string())?;
        lock.lock().map_err(|e| e.to_string())?;
        let manifest = root.join("capture.json");
        if manifest.exists() {
            let previous: Self =
                serde_json::from_slice(&std::fs::read(&manifest).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            if previous.total != total
                || previous.repository != capture.repository
                || previous.identity != capture.identity
            {
                return Err("Difftool invocation changed during capture".into());
            }
            capture.files = previous.files;
            capture.working_head = previous.working_head;
        }
        for side in &paths {
            std::fs::create_dir_all(side).map_err(|e| e.to_string())?;
        }
        if directory {
            for (input, destination) in inputs.iter().zip(&paths) {
                copy_entry(input, destination)?;
            }
        } else {
            let args = helper_file.unwrap_or_else(|| {
                let label = inputs
                    .iter()
                    .find(|p| p.as_path() != Path::new("/dev/null"))
                    .and_then(|p| p.file_name())
                    .and_then(|p| p.to_str())
                    .unwrap_or("supplied-file");
                vec![
                    label.into(),
                    String::new(),
                    ".".into(),
                    "100644".into(),
                    String::new(),
                    ".".into(),
                    "100644".into(),
                ]
            });
            let before = args[0].clone();
            let after = args.get(7).cloned().unwrap_or_else(|| before.clone());
            let names = [valid_relative(&before)?, valid_relative(&after)?];
            for (side, ((input, destination), name)) in
                inputs.iter().zip(&paths).zip(&names).enumerate()
            {
                if input == Path::new("/dev/null") {
                    continue;
                }
                let dest = destination.join(name);
                if args[if side == 0 { 3 } else { 6 }] == "120000" {
                    use std::io::Read;
                    let mut data = Vec::new();
                    std::fs::File::open(input)
                        .map_err(|e| e.to_string())?
                        .take(1_048_577)
                        .read_to_end(&mut data)
                        .map_err(|e| e.to_string())?;
                    if data.len() > 1_048_576 {
                        return Err("Difftool symlink target is too large".into());
                    }
                    let target = String::from_utf8(data).map_err(|e| e.to_string())?;
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(target, dest).map_err(|e| e.to_string())?;
                } else {
                    copy_entry(input, &dest)?;
                }
            }
            capture.files.push(File {
                before,
                after,
                kind: if inputs[0] == Path::new("/dev/null") {
                    "added"
                } else if inputs[1] == Path::new("/dev/null") {
                    "removed"
                } else if names[0] != names[1] {
                    "renamed"
                } else {
                    "modified"
                }
                .into(),
                before_object: object(&args[2]),
                after_object: object(&args[5]),
            });
        }
        let bytes = serde_json::to_vec(&capture).map_err(|e| e.to_string())?;
        std::fs::write(&manifest, bytes).map_err(|e| e.to_string())?;
        Ok(Some(capture))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intent_preserves_git_direction_and_rejects_unsupported_options() {
        let parse =
            |args: &[&str]| Intent::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            parse(&["git", "difftool", "main"]).unwrap(),
            Intent {
                left: "main".into(),
                right: "working tree".into()
            }
        );
        assert_eq!(parse(&["git", "difftool"]).unwrap().left, "index");
        assert_eq!(
            parse(&["git", "difftool", "--cached"]).unwrap().right,
            "index"
        );
        assert_eq!(
            parse(&["git", "difftool", "-R", "main", "HEAD"]).unwrap(),
            Intent {
                left: "HEAD".into(),
                right: "main".into()
            }
        );
        assert_eq!(
            parse(&[
                "git",
                "difftool",
                "-t",
                "jaywalk",
                "main...HEAD",
                "--",
                "a b"
            ])
            .unwrap()
            .left,
            "merge-base:main:HEAD"
        );
        assert!(parse(&["git", "difftool", "--word-diff", "main"]).is_none());
        assert!(valid_relative("../../escape").is_err());
        assert!(valid_relative("/absolute").is_err());
    }
}
