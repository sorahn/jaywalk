//! Plain-Git checkout leases. Receipts survive failed restoration and crashes.
use super::{Workspace, process, validate_commit};
use crate::accounts::Control;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    root: PathBuf,
    git_dir: PathBuf,
    git_identity: (u64, u64),
    original_commit: String,
    original_ref: Option<String>,
    expected_commit: String,
    #[serde(default)]
    expected_ref: Option<String>,
    #[serde(default)]
    created_branch: bool,
    #[serde(default)]
    cleanup_created_branch: bool,
    #[serde(default)]
    creation_token: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    root: PathBuf,
    git_dir: PathBuf,
    git_identity: (u64, u64),
    head: String,
    reference: Option<String>,
}
fn identity(path: &Path) -> Result<(u64, u64), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = path
            .metadata()
            .map_err(|_| "Git directory identity is unavailable")?;
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err("Review checkout identity checks are unavailable on this platform".into())
    }
}
#[derive(Clone, Debug)]
pub struct Recovery {
    pub path: PathBuf,
    pub root: PathBuf,
}
#[derive(Debug)]
pub struct Lease {
    receipt: PathBuf,
    root: PathBuf,
    expected: String,
    expected_ref: Option<String>,
    active: Arc<AtomicBool>,
}
impl Lease {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn expected_commit(&self) -> &str {
        &self.expected
    }
    pub fn branch_name(&self) -> Option<&str> {
        self.expected_ref
            .as_deref()
            .and_then(|r| r.strip_prefix("refs/heads/"))
    }
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.active.swap(false, Ordering::AcqRel)
            && let Ok(queue) = queue()
        {
            let _ = queue.send(Job::Restore(self.receipt.clone(), None));
        }
    }
}
struct Slot {
    lease: Weak<Lease>,
    receipt: PathBuf,
}
type Leases = HashMap<PathBuf, Slot>;
static QUITTING: AtomicBool = AtomicBool::new(false);
fn leases() -> &'static Mutex<Leases> {
    static LEASES: OnceLock<Mutex<Leases>> = OnceLock::new();
    LEASES.get_or_init(Mutex::default)
}
enum Job {
    Restore(
        PathBuf,
        Option<async_channel::Sender<Result<String, String>>>,
    ),
    Quit(async_channel::Sender<()>),
}
static QUEUE: OnceLock<Result<mpsc::Sender<Job>, String>> = OnceLock::new();
fn queue() -> Result<&'static mpsc::Sender<Job>, String> {
    QUEUE
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("jaywalk-checkout-restoration".into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        match job {
                            Job::Restore(path, ack) => {
                                let result = restore(&path);
                                if let Some(ack) = ack {
                                    let _ = ack.try_send(result);
                                }
                            }
                            Job::Quit(ack) => {
                                // Wait for protected switches on the worker, not
                                // the UI thread; include receipts whose last lease
                                // is concurrently being released.
                                let paths: Vec<_> = leases()
                                    .lock()
                                    .map(|leases| {
                                        leases
                                            .values()
                                            .map(|slot| {
                                                if let Some(lease) = slot.lease.upgrade() {
                                                    lease.active.store(false, Ordering::Release);
                                                }
                                                slot.receipt.clone()
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                for path in paths {
                                    if path.exists() {
                                        let _ = restore(&path);
                                    }
                                }
                                let _ = ack.try_send(());
                            }
                        }
                    }
                })
                .map_err(|_| "Could not start checkout restoration worker".to_string())?;
            Ok(sender)
        })
        .as_ref()
        .map_err(Clone::clone)
}
fn text(root: &Path, args: &[&str], control: &Control) -> Result<String, String> {
    let bytes = process::git(root, args, control, None)?;
    String::from_utf8(bytes)
        .map(|text| text.strip_suffix('\n').unwrap_or(&text).to_owned())
        .map_err(|_| "Git metadata is not UTF-8".into())
}
fn clean(root: &Path, control: &Control) -> Result<(), String> {
    if root.ancestors().any(|path| path.join(".jj").exists()) {
        return Err("Local checkout is plain Git only; jj checkout is deferred".into());
    }
    if !process::git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
        control,
        None,
    )?
    .is_empty()
    {
        return Err("Checkout refused: staged, unstaged, untracked or submodule changes exist; nothing was stashed or discarded".into());
    }
    let git_dir = PathBuf::from(text(root, &["rev-parse", "--absolute-git-dir"], control)?);
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-apply",
        "rebase-merge",
        "sequencer",
    ] {
        if git_dir.join(marker).exists() {
            return Err("Checkout refused: another Git operation is in progress".into());
        }
    }
    Ok(())
}
fn symbolic(root: &Path, control: &Control) -> Result<Option<String>, String> {
    // Resolve symbolic state without converting an arbitrary Git error into detached HEAD.
    let value = text(
        root,
        &["rev-parse", "--symbolic-full-name", "HEAD"],
        control,
    )?;
    Ok((value != "HEAD").then_some(value))
}
fn directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|_| "Could not create checkout receipt directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Could not secure checkout receipts")?;
    }
    Ok(())
}
fn open_private(path: &Path, new: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).read(true);
    if new {
        options.create_new(true);
    } else {
        options.create(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| "Checkout ownership receipt already exists or is unavailable; use Restore review checkout")
        .map_err(str::to_owned)
}
fn lock(path: &Path) -> Result<File, String> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or("Invalid receipt path")?;
    let key = stem.split('-').next().ok_or("Invalid receipt path")?;
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid receipt lock identity".into());
    }
    let file = open_private(&path.with_file_name(format!("{key}.lock")), false)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => {
                return Err(format!(
                    "Checkout/restoration ownership lock is unavailable: {error}"
                ));
            }
        }
    }
}
fn save(path: &Path, receipt: &Receipt) -> Result<(), String> {
    use std::io::Write;
    let bytes = serde_json::to_vec(receipt).map_err(|_| "Could not encode checkout receipt")?;
    let mut file = open_private(path, true)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Could not persist checkout receipt before switching")?;
    File::open(path.parent().ok_or("Receipt directory is unavailable")?)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Could not persist the receipt directory before switching")?;
    Ok(())
}
fn read(path: &Path) -> Result<Receipt, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "Checkout receipt is unavailable")?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read checkout receipt")?;
    if bytes.len() > 65536 {
        return Err("Checkout receipt is oversized".into());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid checkout recovery receipt; left unchanged")?;
    validate_commit(&receipt.original_commit)?;
    validate_commit(&receipt.expected_commit)?;
    if !matches!(receipt.version, 1 | 2)
        || (receipt.version == 1
            && (receipt.expected_ref.is_some()
                || receipt.created_branch
                || receipt.cleanup_created_branch
                || receipt.creation_token.is_some()))
        || (receipt.created_branch && receipt.expected_ref.is_none())
        || receipt
            .expected_ref
            .as_ref()
            .is_some_and(|r| !r.starts_with("refs/heads/"))
        || !receipt.root.is_absolute()
        || !receipt.git_dir.is_absolute()
        || receipt
            .original_ref
            .as_ref()
            .is_some_and(|reference| !reference.starts_with("refs/heads/"))
    {
        return Err("Invalid checkout recovery identity".into());
    }
    Ok(receipt)
}
pub fn preflight(root: &Path, control: &Control) -> Result<Snapshot, String> {
    let root = PathBuf::from(text(root, &["rev-parse", "--show-toplevel"], control)?)
        .canonicalize()
        .map_err(|_| "Repository is unavailable")?;
    clean(&root, control)?;
    let git_dir = PathBuf::from(text(&root, &["rev-parse", "--absolute-git-dir"], control)?)
        .canonicalize()
        .map_err(|_| "Git directory is unavailable")?;
    let reference = symbolic(&root, control)?;
    if let Some(reference) = &reference {
        let branch = reference
            .strip_prefix("refs/heads/")
            .ok_or("Checkout requires a branch or detached HEAD")?;
        process::git(
            &root,
            &["check-ref-format", "--branch", branch],
            control,
            None,
        )?;
    }
    Ok(Snapshot {
        head: text(&root, &["rev-parse", "HEAD"], control)?,
        reference,
        git_identity: identity(&git_dir)?,
        root,
        git_dir,
    })
}
pub fn start(
    workspace: &Workspace,
    receipt_directory: &Path,
    snapshot: &Snapshot,
    control: &Control,
) -> Result<Arc<Lease>, String> {
    let object = format!("{}^{{commit}}", workspace.metadata.source.commit);
    let source = if process::git(
        workspace.known_repository(),
        &["cat-file", "-e", &object],
        control,
        None,
    )
    .is_ok()
    {
        String::new()
    } else {
        control.check().map_err(|error| error.to_string())?;
        if !workspace.fetch_missing {
            return Err(
                "PR objects are missing from the checkout and remote fetching is disabled".into(),
            );
        }
        super::transport(
            workspace.known_repository(),
            workspace.metadata.locator.provider,
            &workspace.metadata.source.repository.name,
            control,
        )?
    };
    start_checkout(
        workspace.known_repository(),
        &source,
        &workspace.metadata.source.commit,
        receipt_directory,
        Some(snapshot),
        control,
        Some(workspace),
    )
}

#[cfg(test)]
fn start_at(
    root: &Path,
    object_source: &str,
    target: &str,
    receipt_directory: &Path,
    expected: Option<&Snapshot>,
    control: &Control,
) -> Result<Arc<Lease>, String> {
    start_checkout(
        root,
        object_source,
        target,
        receipt_directory,
        expected,
        control,
        None,
    )
}

/// Only exact refs count: a prefix match from for-each-ref must not select a
/// similarly named branch. Missing refs are distinct from command failures.
fn ref_commit(root: &Path, reference: &str, control: &Control) -> Result<Option<String>, String> {
    let refs = text(
        root,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "--",
            reference,
        ],
        control,
    )?;
    Ok(refs
        .lines()
        .filter_map(|line| line.split_once(' '))
        .find(|(_, name)| *name == reference)
        .map(|(commit, _)| commit.to_owned()))
}

enum BranchChoice {
    Detached,
    Existing(String),
    Create(String),
}
impl BranchChoice {
    fn reference(&self) -> Option<&str> {
        match self {
            Self::Detached => None,
            Self::Existing(reference) | Self::Create(reference) => Some(reference),
        }
    }
    fn creates_branch(&self) -> bool {
        matches!(self, Self::Create(_))
    }
}
fn choose_branch(
    root: &Path,
    target: &str,
    workspace: Option<&Workspace>,
    control: &Control,
) -> Result<BranchChoice, String> {
    let detached = || BranchChoice::Detached;
    let Some(workspace) = workspace else {
        return Ok(detached());
    };
    if workspace.checkout_branch == super::CheckoutBranch::Detached {
        return Ok(detached());
    }
    let Some(name) = workspace.metadata.source_branch.as_deref() else {
        return Ok(detached());
    };
    let reference = format!("refs/heads/{name}");
    if process::git(root, &["check-ref-format", &reference], control, None).is_err()
        || text(root, &["check-ref-format", "--branch", name], control)? != name
    {
        return Ok(detached());
    }
    if let Some(commit) = ref_commit(root, &reference, control)? {
        return Ok(if commit == target {
            BranchChoice::Existing(reference)
        } else {
            detached()
        });
    }
    if workspace.checkout_branch != super::CheckoutBranch::CreateFromRemote {
        return Ok(detached());
    }
    let remotes = text(
        root,
        &[
            "config",
            "--local",
            "--no-includes",
            "--get-regexp",
            r"^remote\..*\.url$",
        ],
        control,
    )?;
    for line in remotes.lines() {
        let Some((key, url)) = line.split_once(' ') else {
            continue;
        };
        let Some(remote) = key
            .strip_prefix("remote.")
            .and_then(|key| key.strip_suffix(".url"))
        else {
            continue;
        };
        let Some(identity) = crate::hosting::RemoteRepository::parse(url) else {
            continue;
        };
        if identity.provider != workspace.metadata.locator.provider
            || !identity
                .repository
                .to_string()
                .eq_ignore_ascii_case(&workspace.metadata.source.repository.name.to_string())
        {
            continue;
        }
        let tracking = format!("refs/remotes/{remote}/{name}");
        if ref_commit(root, &tracking, control)?.as_deref() == Some(target) {
            return Ok(BranchChoice::Create(reference));
        }
    }
    Ok(detached())
}

fn start_checkout(
    root: &Path,
    object_source: &str,
    target: &str,
    receipt_directory: &Path,
    expected: Option<&Snapshot>,
    control: &Control,
    workspace: Option<&Workspace>,
) -> Result<Arc<Lease>, String> {
    if QUITTING.load(Ordering::Acquire) {
        return Err("Application is closing; checkout refused".into());
    }
    validate_commit(target)?;
    let root = PathBuf::from(text(root, &["rev-parse", "--show-toplevel"], control)?)
        .canonicalize()
        .map_err(|_| "Local repository is unavailable")?;
    clean(&root, control)?;
    if let Some(expected) = expected {
        let current = preflight(&root, control)?;
        if current.root != expected.root
            || current.git_dir != expected.git_dir
            || current.git_identity != expected.git_identity
            || current.head != expected.head
            || current.reference != expected.reference
        {
            return Err("Repository changed during PR acquisition; checkout refused".into());
        }
    }
    let mut leases = leases()
        .lock()
        .map_err(|_| "Checkout lease lock poisoned")?;
    if let Some(lease) = leases
        .get(&root)
        .and_then(|slot| slot.lease.upgrade())
        .filter(|lease| lease.active())
    {
        if lease.expected == target
            && text(&root, &["rev-parse", "HEAD"], control)? == target
            && symbolic(&root, control)? == lease.expected_ref
        {
            return Ok(lease);
        }
        return Err("Repository already has a review checkout; restore it before opening another PR locally".into());
    }
    queue()?;
    directory(receipt_directory)?;
    let git_dir = PathBuf::from(text(&root, &["rev-parse", "--absolute-git-dir"], control)?)
        .canonicalize()
        .map_err(|_| "Git directory is unavailable")?;
    let key = format!("{:x}", Sha256::digest(git_dir.to_string_lossy().as_bytes()));
    let path = receipt_directory.join(format!("{key}-{:016x}.json", rand::random::<u64>()));
    let _lock = lock(&path)?;
    let mut existing = false;
    for entry in
        std::fs::read_dir(receipt_directory).map_err(|_| "Could not inspect checkout ownership")?
    {
        let entry = entry.map_err(|_| "Could not inspect checkout ownership")?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        existing |= name.starts_with(&format!("{key}-")) && name.ends_with(".json");
    }
    if existing {
        return Err(
            "A previous review checkout needs recovery; use Restore review checkout".into(),
        );
    }
    if let Some(expected) = expected {
        let current = preflight(&root, control)?;
        if current.root != expected.root
            || current.git_dir != expected.git_dir
            || current.git_identity != expected.git_identity
            || current.head != expected.head
            || current.reference != expected.reference
        {
            return Err(
                "Repository changed while waiting for checkout ownership; nothing was switched"
                    .into(),
            );
        }
    }
    let original_commit = text(&root, &["rev-parse", "HEAD"], control)?;
    let original_ref = symbolic(&root, control)?;
    if let Some(reference) = &original_ref {
        let branch = reference
            .strip_prefix("refs/heads/")
            .ok_or("Checkout requires a branch or detached HEAD")?;
        process::git(
            &root,
            &["check-ref-format", "--branch", branch],
            control,
            None,
        )?;
    }

    if original_commit == target {
        return Ok(Arc::new(Lease {
            receipt: path,
            root,
            expected: target.into(),
            expected_ref: original_ref.clone(),
            active: Arc::new(AtomicBool::new(false)),
        }));
    }
    let branch = choose_branch(&root, target, workspace, control)?;
    let receipt = Receipt {
        version: 2,
        root: root.clone(),
        git_identity: identity(&git_dir)?,
        git_dir,
        original_commit,
        original_ref,
        expected_commit: target.into(),
        expected_ref: branch.reference().map(str::to_owned),
        created_branch: branch.creates_branch(),
        cleanup_created_branch: branch.creates_branch()
            && workspace.is_some_and(|w| w.cleanup_created_branch),
        creation_token: branch
            .creates_branch()
            .then(|| format!("{:032x}", rand::random::<u128>())),
    };
    // Import a connected graph without moving refs/FETCH_HEAD or changing repository depth.
    let object = format!("{target}^{{commit}}");
    if process::git(&root, &["cat-file", "-e", &object], control, None).is_err() {
        // A shallow scratch server may omit intermediate parents even when
        // fetch returns success. Import from the full provider transport, never
        // from that scratch store; existing user refs/FETCH_HEAD stay unchanged.
        if Path::new(object_source).is_dir()
            && text(
                Path::new(object_source),
                &["rev-parse", "--is-shallow-repository"],
                control,
            )? == "true"
        {
            return Err("Cannot import a checkout from a shallow scratch repository".into());
        }
        let source = object_source;
        process::git(
            &root,
            &[
                "-c",
                "protocol.file.allow=always",
                "fetch",
                "--no-tags",
                "--no-prune",
                "--no-prune-tags",
                "--recurse-submodules=no",
                "--no-write-fetch-head",
                "--no-auto-maintenance",
                source,
                target,
            ],
            control,
            None,
        )?;
    }
    process::git(
        &root,
        &["rev-list", "--objects", "--quiet", target, "--not", "--all"],
        control,
        None,
    )
    .map_err(|_| {
        "PR history is not connected to the known repository; checkout refused before switching"
            .to_string()
    })?;
    clean(&root, control)?;
    if text(&root, &["rev-parse", "HEAD"], control)? != receipt.original_commit
        || symbolic(&root, control)? != receipt.original_ref
    {
        return Err("Repository changed before checkout; nothing was switched".into());
    }
    if QUITTING.load(Ordering::Acquire) {
        return Err("Application is closing; checkout refused".into());
    }
    save(&path, &receipt)?;
    // Once the receipt is durable, cancellation cannot strand an unrecorded switch.
    let switching = Control::new(Duration::from_secs(30));
    let switched = if let Some(reference) = branch.reference() {
        let name = reference
            .strip_prefix("refs/heads/")
            .expect("a local branch ref was constructed");
        if branch.creates_branch() {
            let marker = format!(
                "jaywalk review {}",
                receipt
                    .creation_token
                    .as_deref()
                    .expect("new branches have an ownership token")
            );
            // Compare-and-create cannot reset an existing branch. The reflog marker
            // proves ownership if Git completes but the app exits before recording a lease.
            process::git(
                &root,
                &[
                    "update-ref",
                    "--create-reflog",
                    "-m",
                    &marker,
                    reference,
                    target,
                    &"0".repeat(target.len()),
                ],
                &switching,
                None,
            )
            .and_then(|_| {
                process::git(
                    &root,
                    &["switch", "--no-overwrite-ignore", "--", name],
                    &switching,
                    None,
                )
            })
        } else if ref_commit(&root, reference, &switching)?.as_deref() != Some(target) {
            Err("Review branch moved before checkout".into())
        } else {
            process::git(
                &root,
                &["switch", "--no-overwrite-ignore", "--", name],
                &switching,
                None,
            )
        }
    } else {
        process::git(
            &root,
            &[
                "checkout",
                "--detach",
                "--no-overwrite-ignore",
                target,
                "--",
            ],
            &switching,
            None,
        )
    };
    if switched.is_err()
        || text(&root, &["rev-parse", "HEAD"], &switching)? != target
        || symbolic(&root, &switching)? != receipt.expected_ref
    {
        let _ = restore_locked(&path, &receipt);
        return Err("Checkout refused or failed; no force/stash/reset was used. Check Restore review checkout if recovery is needed".into());
    }
    let lease = Arc::new(Lease {
        receipt: path.clone(),
        root: root.clone(),
        expected: target.into(),
        expected_ref: receipt.expected_ref.clone(),
        active: Arc::new(AtomicBool::new(true)),
    });
    leases.insert(
        root,
        Slot {
            lease: Arc::downgrade(&lease),
            receipt: path,
        },
    );
    Ok(lease)
}
fn restore_locked(path: &Path, receipt: &Receipt) -> Result<String, String> {
    let control = Control::new(Duration::from_secs(30));
    let root = receipt
        .root
        .canonicalize()
        .map_err(|_| "Original repository is unavailable; receipt retained")?;
    let git_dir = PathBuf::from(text(&root, &["rev-parse", "--absolute-git-dir"], &control)?)
        .canonicalize()
        .map_err(|_| "Original Git directory is unavailable")?;
    if root != receipt.root
        || git_dir != receipt.git_dir
        || identity(&git_dir)? != receipt.git_identity
    {
        return Err("Repository identity changed; restoration skipped and receipt retained".into());
    }
    clean(&root, &control)?;
    let head = text(&root, &["rev-parse", "HEAD"], &control)?;
    let reference = symbolic(&root, &control)?;
    let already_original = head == receipt.original_commit && reference == receipt.original_ref;
    if !already_original {
        if head != receipt.expected_commit || reference != receipt.expected_ref {
            return Err(
                "Checkout was changed externally; restoration skipped and receipt retained".into(),
            );
        }
        if let Some(reference) = &receipt.original_ref
            && text(&root, &["rev-parse", "--verify", reference], &control)?
                != receipt.original_commit
        {
            return Err("Original branch moved; restoration skipped and receipt retained".into());
        }
        if let Some(reference) = &receipt.original_ref {
            let branch = reference
                .strip_prefix("refs/heads/")
                .expect("validated branch ref");
            process::git(
                &root,
                &["check-ref-format", "--branch", branch],
                &control,
                None,
            )?;
            process::git(
                &root,
                &["switch", "--no-overwrite-ignore", "--", branch],
                &control,
                None,
            )?;
        } else {
            process::git(
                &root,
                &[
                    "checkout",
                    "--detach",
                    "--no-overwrite-ignore",
                    &receipt.original_commit,
                    "--",
                ],
                &control,
                None,
            )?;
        }
    }
    if text(&root, &["rev-parse", "HEAD"], &control)? != receipt.original_commit
        || symbolic(&root, &control)? != receipt.original_ref
    {
        return Err("Original checkout could not be verified; receipt retained".into());
    }
    if receipt.cleanup_created_branch && receipt.created_branch {
        let reference = receipt
            .expected_ref
            .as_ref()
            .expect("created branches have a validated ref");
        if let Some(commit) = ref_commit(&root, reference, &control)? {
            if commit != receipt.expected_commit {
                return Err("Original checkout restored; created review branch moved, so cleanup was skipped and receipt retained".into());
            }
            let marker = receipt
                .creation_token
                .as_ref()
                .map(|token| format!("jaywalk review {token}"));
            let last_creation = text(
                &root,
                &["reflog", "show", "--format=%gs", "-1", reference],
                &control,
            )?;
            if marker.as_deref() != Some(last_creation.as_str()) {
                return Err("Original checkout restored; review branch ownership changed, so cleanup was skipped and receipt retained".into());
            }
            let worktrees = text(&root, &["worktree", "list", "--porcelain", "-z"], &control)?;
            if worktrees
                .split('\0')
                .any(|field| field.strip_prefix("branch ") == Some(reference.as_str()))
            {
                return Err("Original checkout restored; review branch is in use by a worktree, so cleanup was skipped and receipt retained".into());
            }
            // Compare-and-delete prevents deleting a branch that moved after the check.
            process::git(
                &root,
                &["update-ref", "-d", reference, &receipt.expected_commit],
                &control,
                None,
            )?;
        }
    }
    std::fs::remove_file(path).map_err(|_| "Checkout restored, but receipt cleanup failed")?;
    Ok(format!("Restored checkout in {}", root.display()))
}
pub fn restore(path: &Path) -> Result<String, String> {
    let _lock = lock(path)?;
    let receipt = read(path)?;
    let result = restore_locked(path, &receipt);
    if let Ok(mut leases) = leases().lock()
        && let Some(slot) = leases
            .get(&receipt.root)
            .filter(|slot| slot.receipt == path)
    {
        if let Some(lease) = slot.lease.upgrade() {
            lease.active.store(false, Ordering::Release);
        }
        if result.is_ok() {
            leases.remove(&receipt.root);
        }
    }
    result
}
/// Serialize requested and automatic restoration so quit can drain both.
pub async fn restore_requested(path: PathBuf) -> Result<String, String> {
    let (sender, receiver) = async_channel::bounded(1);
    queue()?
        .send(Job::Restore(path, Some(sender)))
        .map_err(|_| "Checkout restoration worker is unavailable")?;
    receiver
        .recv()
        .await
        .map_err(|_| "Checkout restoration result is unavailable")?
}
pub fn recoveries(directory: &Path) -> Result<Vec<Recovery>, String> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Could not read checkout recovery receipts".into()),
    };
    let mut result = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|_| "Could not inspect checkout receipt")?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let receipt = read(&path)?;
            result.push(Recovery {
                path,
                root: receipt.root,
            });
            if result.len() > 50 {
                return Err(
                    "More than 50 checkout receipts; inspect the recovery directory".into(),
                );
            }
        }
    }
    result.sort_by(|a, b| a.root.cmp(&b.root));
    Ok(result)
}
/// Explicit quit waits for best-effort guarded restoration. Failed receipts stay.
pub fn has_active_checkouts() -> bool {
    leases()
        .try_lock()
        .map_or(true, |leases| !leases.is_empty())
}
pub async fn restore_on_quit() {
    QUITTING.store(true, Ordering::Release);
    if QUEUE.get().is_none() {
        return;
    }
    if let Ok(queue) = queue() {
        let (sender, receiver) = async_channel::bounded(1);
        if queue.send(Job::Quit(sender)).is_ok() {
            let _ = receiver.recv().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_review::tests::{Fixture, control, git};
    fn start(fixture: &Fixture, receipts: &Path) -> Result<Arc<Lease>, String> {
        start_at(
            fixture.directory.path(),
            fixture.directory.path().to_str().unwrap(),
            &fixture.head,
            receipts,
            None,
            &control(),
        )
    }
    #[test]
    fn checkout_is_detached_and_guarded_restoration_returns_to_the_original_branch() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        let refs = git(root, &["show-ref"]);
        let index = git(root, &["ls-files", "--stage"]);
        let lease = start(&fixture, receipts.path()).unwrap();
        assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.head);
        assert_eq!(symbolic(root, &control()).unwrap(), None);
        assert_eq!(recoveries(receipts.path()).unwrap().len(), 1);
        assert!(Arc::ptr_eq(
            &lease,
            &start(&fixture, receipts.path()).unwrap()
        ));
        restore(&lease.receipt).unwrap();
        assert!(!lease.active());
        assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert_eq!(git(root, &["show-ref"]), refs);
        assert_eq!(git(root, &["ls-files", "--stage"]), index);
        assert!(recoveries(receipts.path()).unwrap().is_empty());
    }
    #[test]
    fn dirty_and_untracked_files_and_jj_are_refused_without_mutation() {
        for case in ["tracked", "staged", "untracked", "jj", "merge"] {
            let fixture = Fixture::new();
            let root = fixture.directory.path();
            let receipts = tempfile::tempdir().unwrap();
            match case {
                "tracked" | "staged" => {
                    std::fs::write(root.join("changed.rs"), "unreviewed\n").unwrap();
                    if case == "staged" {
                        git(root, &["add", "changed.rs"]);
                    }
                }
                "untracked" => std::fs::write(root.join("untracked"), "preserve me").unwrap(),
                "jj" => std::fs::create_dir(root.join(".jj")).unwrap(),
                _ => std::fs::write(root.join(".git/MERGE_HEAD"), &fixture.head).unwrap(),
            }
            let status = git(root, &["status", "--porcelain=v1"]);
            assert!(start(&fixture, receipts.path()).is_err(), "{case}");
            assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.base);
            assert_eq!(git(root, &["status", "--porcelain=v1"]), status);
            assert!(recoveries(receipts.path()).unwrap().is_empty());
        }
    }
    #[test]
    fn ignored_checkout_collisions_are_never_overwritten() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        std::fs::write(root.join(".git/info/exclude"), "new.rs\n").unwrap();
        std::fs::write(root.join("new.rs"), "private ignored data").unwrap();
        assert!(start(&fixture, receipts.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("new.rs")).unwrap(),
            "private ignored data"
        );
        assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert!(recoveries(receipts.path()).unwrap().is_empty());
    }
    #[test]
    fn edits_or_external_switches_prevent_restoration_and_keep_the_receipt() {
        for external in [false, true] {
            let fixture = Fixture::new();
            let receipts = tempfile::tempdir().unwrap();
            let root = fixture.directory.path();
            let lease = start(&fixture, receipts.path()).unwrap();
            if external {
                git(root, &["checkout", "main"]);
                git(root, &["checkout", "--detach", &fixture.base]);
            } else {
                std::fs::write(root.join("changed.rs"), "new user edits").unwrap();
            }
            assert!(restore(&lease.receipt).is_err());
            assert!(lease.receipt.exists());
            assert!(!lease.active());
            if !external {
                assert_eq!(
                    std::fs::read_to_string(root.join("changed.rs")).unwrap(),
                    "new user edits"
                );
            }
        }
    }
    #[test]
    fn moved_original_branches_block_restore_until_explicitly_repaired() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        let lease = start(&fixture, receipts.path()).unwrap();
        git(root, &["branch", "-f", "main", &fixture.head]);
        assert!(restore(&lease.receipt).is_err());
        assert!(lease.receipt.exists());
        git(root, &["branch", "-f", "main", &fixture.base]);
        restore(&lease.receipt).unwrap();
        assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.base);
    }
    #[test]
    fn an_already_matching_checkout_does_not_detach_or_create_a_receipt() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        git(root, &["checkout", "-b", "feature", &fixture.head]);
        let lease = start(&fixture, receipts.path()).unwrap();
        assert!(!lease.active());
        assert!(recoveries(receipts.path()).unwrap().is_empty());
        assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/feature");
    }
}

#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use crate::local_review::tests::{Fixture, control, git};
    #[test]
    fn switching_during_acquisition_and_replacing_the_git_directory_are_refused() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        let snapshot = preflight(root, &control()).unwrap();
        git(root, &["checkout", "--detach", &fixture.head]);
        assert!(
            start_at(
                root,
                root.to_str().unwrap(),
                &fixture.head,
                receipts.path(),
                Some(&snapshot),
                &control()
            )
            .is_err()
        );
        assert_eq!(symbolic(root, &control()).unwrap(), None);
        git(root, &["checkout", "main"]);
        let lease = start_at(
            root,
            root.to_str().unwrap(),
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        std::fs::rename(root.join(".git"), root.join("original-git-dir")).unwrap();
        git(root, &["init"]);
        assert!(restore(&lease.receipt).is_err());
        assert!(lease.receipt.exists());
    }
    #[test]
    fn ignored_files_created_during_review_block_restoration_without_overwrite() {
        let mut fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        std::fs::write(root.join("old-only"), "original tracked text").unwrap();
        git(root, &["add", "old-only"]);
        git(root, &["commit", "-m", "base with old file"]);
        fixture.base = git(root, &["rev-parse", "HEAD"]);
        git(root, &["rm", "old-only"]);
        git(root, &["commit", "-m", "remove old file"]);
        fixture.head = git(root, &["rev-parse", "HEAD"]);
        git(root, &["checkout", "--detach", &fixture.base]);
        git(root, &["branch", "-f", "main", &fixture.base]);
        git(root, &["checkout", "main"]);
        let lease = start_at(
            root,
            root.to_str().unwrap(),
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        std::fs::write(root.join(".git/info/exclude"), "old-only\n").unwrap();
        std::fs::write(root.join("old-only"), "new ignored user data").unwrap();
        assert!(restore(&lease.receipt).is_err());
        assert!(lease.receipt.exists());
        assert_eq!(
            std::fs::read_to_string(root.join("old-only")).unwrap(),
            "new ignored user data"
        );
        assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.head);
    }
    #[test]
    fn non_branch_head_and_option_shaped_refs_are_not_restoration_commands() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        git(root, &["update-ref", "refs/heads/-f", &fixture.base]);
        git(root, &["symbolic-ref", "HEAD", "refs/heads/-f"]);
        assert!(
            start_at(
                root,
                root.to_str().unwrap(),
                &fixture.head,
                receipts.path(),
                None,
                &control()
            )
            .is_err()
        );
        assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/-f");
        assert!(recoveries(receipts.path()).unwrap().is_empty());
    }
}

#[cfg(test)]
mod receipt_lifetime_tests {
    use super::*;
    use crate::local_review::tests::{Fixture, control, git};
    #[test]
    fn an_old_receipt_cannot_restore_a_later_checkout() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        let old = start_at(
            root,
            root.to_str().unwrap(),
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        let old_path = old.receipt.clone();
        restore(&old_path).unwrap();
        let new = start_at(
            root,
            root.to_str().unwrap(),
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        assert_ne!(old_path, new.receipt);
        assert!(restore(&old_path).is_err());
        assert!(new.active());
        assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.head);
        restore(&new.receipt).unwrap();
    }
    #[test]
    fn releasing_the_last_view_restores_through_the_owned_worker() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let root = fixture.directory.path();
        let lease = start_at(
            root,
            root.to_str().unwrap(),
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        let path = lease.receipt.clone();
        drop(lease);
        // The queued acknowledgement follows the automatic drop job. It may
        // find the receipt already removed; it must never affect another lease.
        let (sender, receiver) = async_channel::bounded(1);
        queue()
            .unwrap()
            .send(Job::Restore(path, Some(sender)))
            .unwrap();
        let acknowledgement = receiver.recv_blocking().unwrap();
        assert_eq!(
            symbolic(root, &control()).unwrap(),
            Some("refs/heads/main".into()),
            "restoration acknowledgement: {acknowledgement:?}; receipts: {:?}",
            recoveries(receipts.path())
        );
        assert!(recoveries(receipts.path()).unwrap().is_empty());
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use crate::local_review::{
        Mode, Options, Workspace,
        tests::{Fixture, control, git},
    };
    #[test]
    fn checkout_can_import_a_missing_multi_commit_pr_without_making_the_known_repo_shallow() {
        let mut fixture = Fixture::new();
        let source = fixture.directory.path();
        git(source, &["checkout", "--detach", &fixture.head]);
        std::fs::write(source.join("second.rs"), "fn second() {}\n").unwrap();
        git(source, &["add", "second.rs"]);
        git(source, &["commit", "-m", "second PR commit"]);
        fixture.head = git(source, &["rev-parse", "HEAD"]);
        let user = tempfile::tempdir().unwrap();
        let user_path = user.path().join("known");
        git(
            user.path(),
            &[
                "clone",
                "--no-local",
                "--single-branch",
                "--branch",
                "main",
                source.to_str().unwrap(),
                user_path.to_str().unwrap(),
            ],
        );
        let workspace = Workspace::prepare(
            source,
            &fixture.metadata(),
            Options {
                mode: Mode::Isolated,
                max_repository_mb: 16,
                fetch_missing: false,
                ..Default::default()
            },
            &control(),
        )
        .unwrap();
        let receipts = tempfile::tempdir().unwrap();
        assert!(
            start_at(
                &user_path,
                workspace.directory().to_str().unwrap(),
                &fixture.head,
                receipts.path(),
                None,
                &control()
            )
            .is_err(),
            "shallow scratch sources must not populate a user checkout"
        );
        assert_eq!(git(&user_path, &["rev-parse", "HEAD"]), fixture.base);
        let lease = start_at(
            &user_path,
            source.to_str().unwrap(),
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        assert!(!user_path.join(".git/shallow").exists());
        let history = git(&user_path, &["rev-list", "--count", "HEAD"]);
        assert_eq!(history, "3");
        restore(&lease.receipt).unwrap();
    }
}

#[cfg(test)]
mod existing_depth_tests {
    use super::*;
    use crate::local_review::tests::{Fixture, control, git};
    #[test]
    fn import_preserves_existing_shallow_boundaries_tags_refs_and_fetch_head() {
        let fixture = Fixture::new();
        let source = fixture.directory.path();
        let user = tempfile::tempdir().unwrap();
        let root = user.path().join("known");
        let url = format!("file://{}", source.display());
        git(
            user.path(),
            &[
                "clone",
                "--depth=1",
                "--single-branch",
                "--branch",
                "main",
                &url,
                root.to_str().unwrap(),
            ],
        );
        git(&root, &["tag", "keep-local", &fixture.base]);
        git(&root, &["config", "fetch.prune", "true"]);
        git(&root, &["config", "fetch.pruneTags", "true"]);
        let shallow = std::fs::read(root.join(".git/shallow")).unwrap();
        let refs = git(&root, &["show-ref"]);
        let fetch_head = std::fs::read(root.join(".git/FETCH_HEAD")).ok();
        let receipts = tempfile::tempdir().unwrap();
        let lease = start_at(
            &root,
            source.to_str().unwrap(),
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        assert_eq!(std::fs::read(root.join(".git/shallow")).unwrap(), shallow);
        assert_eq!(git(&root, &["show-ref"]), refs);
        assert_eq!(std::fs::read(root.join(".git/FETCH_HEAD")).ok(), fetch_head);
        assert_eq!(git(&root, &["rev-list", "--count", "HEAD"]), "2");
        restore(&lease.receipt).unwrap();
    }
}

#[cfg(test)]
mod named_branch_tests {
    use super::*;
    use crate::local_review::{
        CheckoutBranch, Options,
        tests::{Fixture, control, git},
    };

    fn workspace(fixture: &Fixture, branch: CheckoutBranch, cleanup: bool) -> Arc<Workspace> {
        let mut metadata = fixture.metadata();
        metadata.source_branch = Some("feature/review".into());
        Workspace::prepare(
            fixture.directory.path(),
            &metadata,
            Options {
                checkout_branch: branch,
                cleanup_created_branch: cleanup,
                fetch_missing: false,
                ..Default::default()
            },
            &control(),
        )
        .unwrap()
    }
    fn begin(workspace: &Workspace, receipts: &Path) -> Arc<Lease> {
        let snapshot = preflight(workspace.known_repository(), &control()).unwrap();
        super::start(workspace, receipts, &snapshot, &control()).unwrap()
    }
    fn tracking(fixture: &Fixture) {
        git(
            fixture.directory.path(),
            &[
                "update-ref",
                "refs/remotes/origin/feature/review",
                &fixture.head,
            ],
        );
    }
    #[test]
    fn existing_exact_branch_is_shared_restored_and_never_cleaned_up() {
        let fixture = Fixture::new();
        let root = fixture.directory.path();
        git(root, &["branch", "feature/review", &fixture.head]);
        let workspace = workspace(&fixture, CheckoutBranch::CreateFromRemote, true);
        let receipts = tempfile::tempdir().unwrap();
        let lease = begin(&workspace, receipts.path());
        assert_eq!(
            symbolic(root, &control()).unwrap().as_deref(),
            Some("refs/heads/feature/review")
        );
        assert!(Arc::ptr_eq(&lease, &begin(&workspace, receipts.path())));
        restore(&lease.receipt).unwrap();
        assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert_eq!(
            ref_commit(root, "refs/heads/feature/review", &control()).unwrap(),
            Some(fixture.head)
        );
    }
    #[test]
    fn remote_branch_creates_the_local_name_and_cleanup_is_optional() {
        for cleanup in [false, true] {
            let fixture = Fixture::new();
            let root = fixture.directory.path();
            tracking(&fixture);
            let workspace = workspace(&fixture, CheckoutBranch::CreateFromRemote, cleanup);
            let receipts = tempfile::tempdir().unwrap();
            let lease = begin(&workspace, receipts.path());
            assert_eq!(
                git(root, &["symbolic-ref", "HEAD"]),
                "refs/heads/feature/review"
            );
            assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.head);
            assert!(read(&lease.receipt).unwrap().created_branch);
            restore(&lease.receipt).unwrap();
            assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
            assert_eq!(
                ref_commit(root, "refs/heads/feature/review", &control()).unwrap(),
                (!cleanup).then_some(fixture.head.clone())
            );
            assert_eq!(
                ref_commit(root, "refs/remotes/origin/feature/review", &control()).unwrap(),
                Some(fixture.head)
            );
        }
    }
    #[test]
    fn detached_and_existing_modes_never_create_a_missing_branch() {
        for mode in [CheckoutBranch::Detached, CheckoutBranch::Existing] {
            let fixture = Fixture::new();
            tracking(&fixture);
            let workspace = workspace(&fixture, mode, true);
            let receipts = tempfile::tempdir().unwrap();
            let lease = begin(&workspace, receipts.path());
            assert!(
                symbolic(fixture.directory.path(), &control())
                    .unwrap()
                    .is_none()
            );
            assert!(
                ref_commit(
                    fixture.directory.path(),
                    "refs/heads/feature/review",
                    &control()
                )
                .unwrap()
                .is_none()
            );
            restore(&lease.receipt).unwrap();
        }
    }
    #[test]
    fn an_existing_branch_at_another_commit_is_neither_moved_nor_replaced() {
        let fixture = Fixture::new();
        let root = fixture.directory.path();
        tracking(&fixture);
        git(root, &["branch", "feature/review", &fixture.base]);
        let workspace = workspace(&fixture, CheckoutBranch::CreateFromRemote, true);
        let receipts = tempfile::tempdir().unwrap();
        let lease = begin(&workspace, receipts.path());
        assert!(symbolic(root, &control()).unwrap().is_none());
        restore(&lease.receipt).unwrap();
        assert_eq!(
            ref_commit(root, "refs/heads/feature/review", &control()).unwrap(),
            Some(fixture.base)
        );
    }
    #[test]
    fn remote_creation_requires_the_exact_commit_and_the_source_repository_identity() {
        let fixture = Fixture::new();
        let root = fixture.directory.path();
        git(
            root,
            &[
                "update-ref",
                "refs/remotes/origin/feature/review",
                &fixture.base,
            ],
        );
        let mut workspace = workspace(&fixture, CheckoutBranch::CreateFromRemote, true);
        assert!(
            choose_branch(root, &fixture.head, Some(&workspace), &control())
                .unwrap()
                .reference()
                .is_none()
        );
        tracking(&fixture);
        Arc::get_mut(&mut workspace)
            .unwrap()
            .metadata
            .source
            .repository
            .name = crate::hosting::RepositoryName::parse("another/review").unwrap();
        assert!(
            choose_branch(root, &fixture.head, Some(&workspace), &control())
                .unwrap()
                .reference()
                .is_none()
        );
    }
    #[test]
    fn cleanup_retains_moved_branches_and_branches_used_by_another_worktree() {
        for case in ["moved", "worktree", "recreated"] {
            let fixture = Fixture::new();
            let root = fixture.directory.path();
            tracking(&fixture);
            let workspace = workspace(&fixture, CheckoutBranch::CreateFromRemote, true);
            let receipts = tempfile::tempdir().unwrap();
            let lease = begin(&workspace, receipts.path());
            git(root, &["switch", "main"]);
            let other = tempfile::tempdir().unwrap();
            let worktree = other.path().join("review");
            if case == "moved" {
                git(
                    root,
                    &["update-ref", "refs/heads/feature/review", &fixture.base],
                );
            } else if case == "recreated" {
                git(root, &["branch", "-D", "feature/review"]);
                git(root, &["branch", "feature/review", &fixture.head]);
            } else {
                git(
                    root,
                    &[
                        "worktree",
                        "add",
                        worktree.to_str().unwrap(),
                        "feature/review",
                    ],
                );
            }
            assert!(restore(&lease.receipt).is_err());
            assert!(lease.receipt.exists());
            assert!(
                ref_commit(root, "refs/heads/feature/review", &control())
                    .unwrap()
                    .is_some()
            );
            if case != "worktree" {
                git(root, &["branch", "-D", "feature/review"]);
            } else {
                git(root, &["worktree", "remove", worktree.to_str().unwrap()]);
            }
            restore(&lease.receipt).unwrap();
        }
    }
    #[test]
    fn old_detached_receipts_remain_restorable() {
        let fixture = Fixture::new();
        let receipts = tempfile::tempdir().unwrap();
        let lease = start_at(
            fixture.directory.path(),
            "",
            &fixture.head,
            receipts.path(),
            None,
            &control(),
        )
        .unwrap();
        let mut value = serde_json::to_value(read(&lease.receipt).unwrap()).unwrap();
        value["version"] = 1.into();
        for key in [
            "expected_ref",
            "created_branch",
            "cleanup_created_branch",
            "creation_token",
        ] {
            value.as_object_mut().unwrap().remove(key);
        }
        std::fs::write(&lease.receipt, serde_json::to_vec(&value).unwrap()).unwrap();
        restore(&lease.receipt).unwrap();
        assert_eq!(
            git(fixture.directory.path(), &["symbolic-ref", "HEAD"]),
            "refs/heads/main"
        );
    }
}
