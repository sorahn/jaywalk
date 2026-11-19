//! Local review acquisition is separate from mutations of a user's repository.
pub mod checkout;
mod process;
use crate::{
    accounts::Control,
    hosting::{
        RemoteRepository,
        review::{Endpoint, Metadata},
    },
    repository_file::FileKind,
    text_content::TextLimit,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Hosted,
    Isolated,
    CheckoutIfClean,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub mode: Mode,
    pub max_repository_mb: u64,
    pub fetch_missing: bool,
}
impl Default for Options {
    fn default() -> Self {
        crate::settings::Config::default().local_review
    }
}
impl Options {
    pub fn validate(self) -> Result<(), String> {
        if !(16..=8192).contains(&self.max_repository_mb) {
            return Err("local_review.max_repository_mb must be 16..8192".into());
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct Workspace {
    directory: tempfile::TempDir,
    known: PathBuf,
    metadata: Metadata,
    max_bytes: u64,
    fetch_missing: bool,
    trees: Mutex<BTreeMap<String, BTreeMap<String, FileKind>>>,
}
impl Workspace {
    pub fn prepare(
        known: &Path,
        metadata: &Metadata,
        options: Options,
        control: &Control,
    ) -> Result<Arc<Self>, String> {
        options.validate()?;
        let root = process::git(known, &["rev-parse", "--show-toplevel"], control, None)?;
        let known = PathBuf::from(
            std::str::from_utf8(&root)
                .map_err(|_| "Repository path is not UTF-8")?
                .trim_end_matches('\n'),
        )
        .canonicalize()
        .map_err(|_| "Known repository is unavailable")?;
        let remotes = crate::hosting::repository_remotes(&known).map_err(|e| e.to_string())?;
        if !remotes.iter().any(|remote| {
            remote.provider == metadata.locator.provider
                && remote
                    .repository
                    .to_string()
                    .eq_ignore_ascii_case(&metadata.locator.repository.to_string())
        }) {
            return Err("Local repository does not match this PR's provider/repository".into());
        }
        Self::create(known, metadata.clone(), options, control)
    }
    fn create(
        known: PathBuf,
        metadata: Metadata,
        options: Options,
        control: &Control,
    ) -> Result<Arc<Self>, String> {
        let directory = tempfile::Builder::new()
            .prefix("jaywalk-review-")
            .tempdir()
            .map_err(|e| format!("Could not create private review storage: {e}"))?;
        validate_commit(&metadata.source.commit)?;
        validate_commit(&metadata.base.commit)?;
        let format = if metadata.source.commit.len() == 64 {
            "--object-format=sha256"
        } else {
            "--object-format=sha1"
        };
        process::git(
            directory.path(),
            &["init", "--bare", "--template=", format],
            control,
            None,
        )?;
        let workspace = Arc::new(Self {
            directory,
            known,
            metadata,
            max_bytes: options.max_repository_mb * 1024 * 1024,
            fetch_missing: options.fetch_missing,
            trees: Mutex::default(),
        });
        workspace.ensure(&workspace.metadata.base, control)?;
        workspace.ensure(&workspace.metadata.source, control)?;
        control.check().map_err(|e| e.to_string())?;
        Ok(workspace)
    }
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
    pub fn known_repository(&self) -> &Path {
        &self.known
    }
    pub fn ensure(&self, endpoint: &Endpoint, control: &Control) -> Result<(), String> {
        validate_commit(&endpoint.commit)?;
        let object = format!("{}^{{commit}}", endpoint.commit);
        if process::git(
            self.directory(),
            &["cat-file", "-e", &object],
            control,
            None,
        )
        .is_ok()
        {
            return Ok(());
        }
        control.check().map_err(|e| e.to_string())?;
        let budget = Some((self.directory(), self.max_bytes));
        // Read the known repository as a transport source; never create refs,
        // FETCH_HEAD or worktrees there, and never borrow its object lifetime.
        let local = process::git(&self.known, &["cat-file", "-e", &object], control, None);
        if local.is_ok() {
            let path = self.known.to_str().ok_or("Repository path is not UTF-8")?;
            if process::git(
                self.directory(),
                &[
                    "-c",
                    "protocol.file.allow=always",
                    "fetch",
                    "--depth=1",
                    "--no-tags",
                    "--no-prune",
                    "--no-prune-tags",
                    "--recurse-submodules=no",
                    "--no-write-fetch-head",
                    "--no-auto-maintenance",
                    path,
                    &endpoint.commit,
                ],
                control,
                budget,
            )
            .is_ok()
            {
                return Ok(());
            }
        }
        control.check().map_err(|e| e.to_string())?;
        process::check_budget(self.directory(), self.max_bytes)?;
        if !self.fetch_missing {
            return Err("Pinned revision could not be copied from the known repository; fetching missing remote objects is disabled".into());
        }
        let remote = transport(
            &self.known,
            self.metadata.locator.provider,
            &endpoint.repository.name,
            control,
        )?;
        process::git(self.directory(), &["fetch", "--depth=1", "--no-tags", "--no-prune", "--no-prune-tags", "--recurse-submodules=no", "--no-write-fetch-head", "--no-auto-maintenance", &remote, &endpoint.commit], control, budget)
            .map_err(|error| format!("Could not fetch the pinned PR revision: {error}. Check Git transport credentials; hosted review is still available"))?;
        process::git(
            self.directory(),
            &["cat-file", "-e", &object],
            control,
            None,
        )?;
        Ok(())
    }
    pub fn paths(&self, commit: &str) -> Result<BTreeMap<String, FileKind>, String> {
        validate_commit(commit)?;
        if let Some(paths) = self
            .trees
            .lock()
            .map_err(|_| "Local tree index lock poisoned")?
            .get(commit)
        {
            return Ok(paths.clone());
        }
        let control = Control::new(Duration::from_secs(30));
        let bytes = process::git(
            self.directory(),
            &["ls-tree", "-r", "-z", commit],
            &control,
            None,
        )?;
        let mut paths = BTreeMap::new();
        for entry in bytes
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let tab = entry
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or("Malformed Git tree entry")?;
            let metadata =
                std::str::from_utf8(&entry[..tab]).map_err(|_| "Invalid Git tree metadata")?;
            let path = std::str::from_utf8(&entry[tab + 1..])
                .map_err(|_| "Non-UTF-8 paths are unavailable in local review")?;
            let mode = metadata
                .split_whitespace()
                .next()
                .ok_or("Missing Git tree mode")?;
            paths.insert(path.to_owned(), FileKind::from(mode));
        }
        let mut trees = self
            .trees
            .lock()
            .map_err(|_| "Local tree index lock poisoned")?;
        if trees.len() >= 8 {
            trees.clear();
        }
        trees.insert(commit.into(), paths.clone());
        Ok(paths)
    }
    pub fn read(&self, commit: &str, path: &str, limit: TextLimit) -> Result<String, String> {
        validate_commit(commit)?;
        let mut command = process::command(self.directory())?;
        command.args(["show", &format!("{commit}:{path}")]);
        limit.text(limit.command(&mut command)?)
    }
}
pub(crate) fn validate_commit(value: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Expected a complete immutable Git commit ID".into());
    }
    Ok(())
}
fn transport(
    root: &Path,
    provider: crate::accounts::Provider,
    repository: &crate::hosting::RepositoryName,
    control: &Control,
) -> Result<String, String> {
    let urls = process::git(
        root,
        &[
            "config",
            "--local",
            "--no-includes",
            "--get-regexp",
            r"^remote\..*\.url$",
        ],
        control,
        None,
    )?;
    let urls = std::str::from_utf8(&urls).map_err(|_| "Invalid remote configuration")?;
    let mut ssh = false;
    for line in urls.lines() {
        if let Some((_, url)) = line.split_once(' ')
            && let Some(remote) = RemoteRepository::parse(url)
        {
            ssh |= url.starts_with("git@") || url.starts_with("ssh://");
            if remote.provider == provider
                && remote
                    .repository
                    .to_string()
                    .eq_ignore_ascii_case(&repository.to_string())
            {
                return Ok(url.into());
            }
        }
    }
    let host = match provider {
        crate::accounts::Provider::Github => "github.com",
        crate::accounts::Provider::Bitbucket => "bitbucket.org",
    };
    Ok(if ssh {
        format!("git@{host}:{repository}.git")
    } else {
        format!("https://{host}/{repository}.git")
    })
}

#[cfg(test)]
pub(crate) mod tests;

/// Policy application is worker-only. Failure is visible without losing the PR.
pub fn apply(
    review: crate::hosting::review::Review,
    candidates: &[PathBuf],
    options: Options,
    receipts: &Path,
    control: &Control,
) -> Result<crate::hosting::review::Review, String> {
    if options.mode == Mode::Hosted {
        return Ok(review);
    }
    let result = crate::hosting::associate(&review.metadata().locator, candidates, control)
        .map_err(|error| error.to_string())?;
    control.check().map_err(|error| error.to_string())?;
    let local = if result.matches.len() == 1 && !result.truncated {
        review.locally(&result.matches[0], options, receipts, control)
    } else {
        Err(if result.truncated {
            "Repository search is limited to 50 paths; use Review PR locally to choose a match"
        } else if result.matches.is_empty() {
            "No matching known repository; using hosted review"
        } else {
            "Multiple matching repositories; use Review PR locally to choose one"
        }
        .into())
    };
    control.check().map_err(|error| error.to_string())?;
    Ok(match local {
        Ok(local) => local,
        Err(error) => review.with_notice(format!(
            "Local review unavailable: {error} · Hosted snapshot retained"
        )),
    })
}
