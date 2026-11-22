//! Persistent, explicit branch switches; temporary review leases remain separate.
use super::*;
use crate::branch_review::RefName;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    name: String,
    reference: Option<String>,
    remote: Option<String>,
    commit: String,
    current: bool,
}
impl Branch {
    pub fn label(&self) -> String {
        self.remote.as_ref().map_or_else(
            || self.name.clone(),
            |remote| format!("{remote}/{}", self.name),
        )
    }
    pub fn remote(&self) -> Option<&str> {
        self.remote.as_deref()
    }
    pub fn current(&self) -> bool {
        self.current
    }
}
#[derive(Clone, Debug)]
pub struct Catalog {
    snapshot: Snapshot,
    pub branches: Vec<Branch>,
}
impl Catalog {
    pub fn capture(root: &Path, control: &Control) -> Result<Self, String> {
        if root.ancestors().any(|path| path.join(".jj").exists()) {
            return Err("Branch checkout is plain Git only; jj checkout is deferred".into());
        }
        let snapshot = snapshot(root, control, false)?;
        let remotes = text(&snapshot.root, &["remote"], control)?;
        let output = text(
            &snapshot.root,
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00%(symref)",
                "refs/heads",
                "refs/remotes",
            ],
            control,
        )?;
        let mut branches = Vec::new();
        for line in output.lines() {
            let fields: Vec<_> = line.split('\0').collect();
            if fields.len() != 3 {
                return Err("Invalid branch metadata".into());
            }
            if !fields[2].is_empty() {
                continue;
            } // Remote HEAD aliases are not branches.
            let reference = fields[0];
            let (name, remote) = if let Some(name) = reference.strip_prefix("refs/heads/") {
                (name, None)
            } else if let Some(qualified) = reference.strip_prefix("refs/remotes/") {
                let Some(remote) = remotes
                    .lines()
                    .filter(|remote| qualified.starts_with(&format!("{remote}/")))
                    .max_by_key(|remote| remote.len())
                else {
                    continue;
                };
                (&qualified[remote.len() + 1..], Some(remote.to_owned()))
            } else {
                continue;
            };
            validate_commit(fields[1])?;
            branches.push(Branch {
                name: name.into(),
                reference: Some(reference.into()),
                remote,
                commit: fields[1].into(),
                current: snapshot.reference.as_deref() == Some(reference),
            });
        }
        branches
            .sort_by_cached_key(|branch| (branch.remote.is_some(), branch.label().to_lowercase()));
        Ok(Self { snapshot, branches })
    }
    pub fn reviewed(&self, reference: &RefName, commit: &str) -> Result<Branch, String> {
        self.branches.iter().find(|branch| branch.name==reference.name && branch.remote==reference.remote && branch.commit==commit)
            .cloned().ok_or_else(|| "Reviewed branch moved or is no longer available; refresh the review or use Checkout branch…".into())
    }
    pub fn current_branch(&self) -> Option<Branch> {
        self.branches.iter().find(|branch| branch.current).cloned()
    }
    pub fn current_branch_at(&self, commit: &str) -> Result<Branch, String> {
        if self.snapshot.head != commit {
            return Err(
                "Checkout changed since this review opened; refresh or choose a branch explicitly"
                    .into(),
            );
        }
        self.current_branch()
            .ok_or_else(|| "The checkout is detached; choose a branch explicitly".into())
    }
    pub fn root(&self) -> &Path {
        &self.snapshot.root
    }
}
fn same_checkout(a: &Snapshot, b: &Snapshot) -> bool {
    a.root == b.root
        && a.git_dir == b.git_dir
        && a.git_identity == b.git_identity
        && a.head == b.head
        && a.reference == b.reference
}
fn ensure_unowned(catalog: &Catalog, receipts: &Path, slots: &Leases) -> Result<(), String> {
    let root = &catalog.snapshot.root;
    if slots.get(root).is_some_and(|slot| {
        slot.receipt.exists() || slot.lease.upgrade().is_some_and(|lease| lease.active())
    }) {
        return Err("Restore the temporary review checkout before switching branches".into());
    }
    let key = format!(
        "{:x}",
        Sha256::digest(catalog.snapshot.git_dir.to_string_lossy().as_bytes())
    );
    match std::fs::read_dir(receipts) {
        Ok(entries) => {
            for entry in entries {
                let name = entry
                    .map_err(|_| "Could not inspect checkout ownership")?
                    .file_name();
                let name = name.to_string_lossy();
                if name.starts_with(&format!("{key}-")) && name.ends_with(".json") {
                    return Err(
                        "A previous review checkout needs recovery; use Restore review checkout"
                            .into(),
                    );
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not inspect checkout ownership".into()),
    }
    Ok(())
}
fn perform(
    catalog: &Catalog,
    branch: &Branch,
    receipts: &Path,
    control: &Control,
) -> Result<String, String> {
    if QUITTING.load(Ordering::Acquire) {
        return Err("Application is closing; checkout refused".into());
    }
    validate_commit(&branch.commit)?;
    let root = catalog.root();
    let checked = text(
        root,
        &["check-ref-format", "--branch", &branch.name],
        control,
    )?;
    if checked != branch.name {
        return Err("Invalid branch name".into());
    }
    let original = preflight(root, control)?;
    if !same_checkout(&original, &catalog.snapshot) {
        return Err("Repository changed while choosing a branch; open the picker again".into());
    }
    let slots = leases()
        .lock()
        .map_err(|_| "Checkout lease lock poisoned")?;
    ensure_unowned(catalog, receipts, &slots)?;
    if let Some(reference) = &branch.reference
        && ref_commit(root, reference, control)?.as_deref() != Some(&branch.commit)
    {
        return Err("Branch moved since it was selected; open the picker again".into());
    }
    let local = format!("refs/heads/{}", branch.name);
    let existing = ref_commit(root, &local, control)?;
    if existing
        .as_ref()
        .is_some_and(|commit| commit != &branch.commit)
    {
        return Err(
            "A local branch with this name points to another commit; it was left unchanged".into(),
        );
    }
    if original.reference.as_deref() == Some(&local) && original.head == branch.commit {
        return Ok(format!("Already on {}", branch.name));
    }
    let current = preflight(root, control)?;
    if !same_checkout(&original, &current) {
        return Err("Repository changed before checkout; nothing was switched".into());
    }
    control.check().map_err(|error| error.to_string())?;
    if QUITTING.load(Ordering::Acquire) {
        return Err("Application is closing; checkout refused".into());
    }
    // Once the switch starts, picker cancellation stops waiting, not the Git
    // operation. The app-owned checkout worker also drains this job on quit.
    control.commit().map_err(|error| error.to_string())?;
    let switching = Control::new(Duration::from_secs(30));
    if existing.is_some() {
        process::git(
            root,
            &["switch", "--no-overwrite-ignore", "--", &branch.name],
            &switching,
            None,
        )?;
    } else {
        process::git(
            root,
            &[
                "switch",
                "--no-overwrite-ignore",
                "--no-track",
                "-c",
                &branch.name,
                &branch.commit,
            ],
            &switching,
            None,
        )?;
    }
    if text(root, &["rev-parse", "HEAD"], &switching)? != branch.commit
        || symbolic(root, &switching)?.as_deref() != Some(&local)
    {
        return Err(
            "Checkout changed during the switch; inspect the repository before continuing".into(),
        );
    }
    Ok(format!("Checked out {} · stays checked out", branch.name))
}
fn queued(
    catalog: Catalog,
    branch: Branch,
    receipts: PathBuf,
    control: Control,
) -> Result<String, String> {
    struct Pending;
    impl Drop for Pending {
        fn drop(&mut self) {
            PERMANENT_JOBS.fetch_sub(1, Ordering::AcqRel);
        }
    }
    PERMANENT_JOBS.fetch_add(1, Ordering::AcqRel);
    let pending = Pending;
    let (sender, receiver) = mpsc::sync_channel(1);
    queue()?
        .send(Job::Permanent(Box::new(move || {
            let _pending = pending;
            let _ = sender.send(perform(&catalog, &branch, &receipts, &control));
        })))
        .map_err(|_| "Checkout worker is unavailable")?;
    receiver.recv().map_err(|_| "Checkout worker stopped")?
}
pub fn checkout(
    catalog: &Catalog,
    branch: &Branch,
    receipts: &Path,
    control: &Control,
) -> Result<String, String> {
    if !catalog.branches.contains(branch) {
        return Err("Branch does not belong to this picker".into());
    }
    queued(
        catalog.clone(),
        branch.clone(),
        receipts.to_owned(),
        control.clone(),
    )
}
pub fn checkout_published(
    root: &Path,
    metadata: &crate::hosting::review::Metadata,
    fetch_missing: bool,
    receipts: &Path,
    control: &Control,
) -> Result<String, String> {
    let catalog = Catalog::capture(root, control)?;
    let before = preflight(root, control)?;
    if !same_checkout(&before, &catalog.snapshot) {
        return Err("Repository changed before checkout".into());
    }
    {
        let slots = leases()
            .lock()
            .map_err(|_| "Checkout lease lock poisoned")?;
        ensure_unowned(&catalog, receipts, &slots)?;
    }
    let name = metadata
        .source_branch
        .as_deref()
        .ok_or("This review has no source branch name")?;
    if text(root, &["check-ref-format", "--branch", name], control)? != name {
        return Err("Invalid review branch name".into());
    }
    let remotes = crate::hosting::repository_remotes(root).map_err(|error| error.to_string())?;
    if !remotes.iter().any(|remote| {
        remote.provider == metadata.locator.provider
            && remote
                .repository
                .to_string()
                .eq_ignore_ascii_case(&metadata.locator.repository.to_string())
    }) {
        return Err("Local repository does not match this PR".into());
    }
    let target = &metadata.source.commit;
    validate_commit(target)?;
    if process::git(
        root,
        &["cat-file", "-e", &format!("{target}^{{commit}}")],
        control,
        None,
    )
    .is_err()
    {
        control.check().map_err(|error| error.to_string())?;
        if !fetch_missing {
            return Err("PR commit is missing locally and fetching is disabled".into());
        }
        let source = super::super::transport(
            root,
            metadata.locator.provider,
            &metadata.source.repository.name,
            control,
        )?;
        process::git(
            root,
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
                &source,
                target,
            ],
            control,
            None,
        )?;
    }
    process::git(
        root,
        &["rev-list", "--objects", "--quiet", target, "--not", "--all"],
        control,
        None,
    )?;
    let branch = Branch {
        name: name.into(),
        reference: None,
        remote: None,
        commit: target.clone(),
        current: false,
    };
    queued(catalog, branch, receipts.to_owned(), control.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Repo(tempfile::TempDir);
    impl Repo {
        fn new() -> Self {
            let repo = Self(tempfile::tempdir().unwrap());
            repo.git(&["init", "-b", "main"]);
            std::fs::write(repo.root().join("file.txt"), "main\n").unwrap();
            std::fs::write(repo.root().join(".gitignore"), "collision\n").unwrap();
            repo.git(&["add", "."]);
            repo.git(&["commit", "-m", "base"]);
            repo.git(&["switch", "-c", "local-topic"]);
            std::fs::write(repo.root().join("file.txt"), "topic\n").unwrap();
            repo.git(&["add", "file.txt"]);
            repo.git(&["commit", "-m", "topic"]);
            repo.git(&[
                "remote",
                "add",
                "origin",
                "https://github.com/example/project.git",
            ]);
            let tip = repo.git(&["rev-parse", "HEAD"]);
            repo.git(&["update-ref", "refs/remotes/origin/remote-topic", &tip]);
            repo.git(&[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/remote-topic",
            ]);
            repo.git(&["switch", "main"]);
            repo
        }
        fn root(&self) -> &Path {
            self.0.path()
        }
        fn git(&self, args: &[&str]) -> String {
            let output = std::process::Command::new("git")
                .current_dir(self.root())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().into()
        }
        fn catalog(&self) -> Catalog {
            Catalog::capture(self.root(), &control()).unwrap()
        }
        fn receipts(&self) -> PathBuf {
            self.root().join(".git/review-checkouts")
        }
        fn assert_main(&self) {
            assert_eq!(self.git(&["branch", "--show-current"]), "main");
        }
    }
    fn control() -> Control {
        Control::new(Duration::from_secs(30))
    }
    fn pick(catalog: &Catalog, label: &str) -> Branch {
        catalog
            .branches
            .iter()
            .find(|branch| branch.label() == label)
            .unwrap()
            .clone()
    }
    #[test]
    fn persistent_checkout_lists_local_first_and_creates_remote_without_prefix_or_hooks() {
        let repo = Repo::new();
        let catalog = repo.catalog();
        assert_eq!(
            catalog
                .branches
                .iter()
                .map(Branch::label)
                .collect::<Vec<_>>(),
            ["local-topic", "main", "origin/remote-topic"]
        );
        assert_eq!(catalog.current_branch().unwrap().label(), "main");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let hook = repo.root().join(".git/hooks/post-checkout");
            std::fs::write(&hook, "#!/bin/sh\ntouch hook-ran\n").unwrap();
            std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let request = control();
        let result = checkout(
            &catalog,
            &pick(&catalog, "origin/remote-topic"),
            &repo.receipts(),
            &request,
        )
        .unwrap();
        assert!(request.is_committing());
        assert!(
            !request.cancel(),
            "a started switch must keep the captured review pinned"
        );
        assert!(result.contains("stays checked out"));
        drop(catalog);
        assert_eq!(repo.git(&["branch", "--show-current"]), "remote-topic");
        assert_eq!(
            std::fs::read_to_string(repo.root().join("file.txt")).unwrap(),
            "topic\n"
        );
        assert!(!repo.receipts().exists());
        assert!(!repo.root().join("hook-ran").exists());
        let catalog = repo.catalog();
        checkout(
            &catalog,
            &pick(&catalog, "local-topic"),
            &repo.receipts(),
            &control(),
        )
        .unwrap();
        assert_eq!(repo.git(&["branch", "--show-current"]), "local-topic");
    }
    #[test]
    fn checkout_refuses_dirty_operations_colocation_and_cancellation_without_discarding() {
        for kind in ["unstaged", "staged", "untracked", "merge", "jj", "cancel"] {
            let repo = Repo::new();
            let catalog = repo.catalog();
            match kind {
                "unstaged" => std::fs::write(repo.root().join("file.txt"), "keep\n").unwrap(),
                "staged" => {
                    std::fs::write(repo.root().join("file.txt"), "keep\n").unwrap();
                    repo.git(&["add", "file.txt"]);
                }
                "untracked" => std::fs::write(repo.root().join("untracked"), "keep").unwrap(),
                "merge" => std::fs::write(
                    repo.root().join(".git/MERGE_HEAD"),
                    repo.git(&["rev-parse", "HEAD"]),
                )
                .unwrap(),
                "jj" => std::fs::create_dir(repo.root().join(".jj")).unwrap(),
                _ => {}
            }
            let request = control();
            if kind == "cancel" {
                request.cancel();
            }
            assert!(
                checkout(
                    &catalog,
                    &pick(&catalog, "local-topic"),
                    &repo.receipts(),
                    &request
                )
                .is_err(),
                "{kind}"
            );
            repo.assert_main();
            if matches!(kind, "unstaged" | "staged") {
                assert_eq!(
                    std::fs::read_to_string(repo.root().join("file.txt")).unwrap(),
                    "keep\n"
                );
            }
        }
    }
    #[test]
    fn checkout_protects_ignored_files_moved_refs_and_colliding_local_branches() {
        let repo = Repo::new();
        repo.git(&["switch", "local-topic"]);
        std::fs::write(repo.root().join("collision"), "tracked\n").unwrap();
        repo.git(&["add", "-f", "collision"]);
        repo.git(&["commit", "-m", "collision"]);
        repo.git(&["switch", "main"]);
        std::fs::write(repo.root().join("collision"), "ignored keep\n").unwrap();
        let catalog = repo.catalog();
        assert!(
            checkout(
                &catalog,
                &pick(&catalog, "local-topic"),
                &repo.receipts(),
                &control()
            )
            .is_err()
        );
        repo.assert_main();
        assert_eq!(
            std::fs::read_to_string(repo.root().join("collision")).unwrap(),
            "ignored keep\n"
        );
        let repo = Repo::new();
        repo.git(&["branch", "remote-topic", "main"]);
        let catalog = repo.catalog();
        assert!(
            checkout(
                &catalog,
                &pick(&catalog, "origin/remote-topic"),
                &repo.receipts(),
                &control()
            )
            .unwrap_err()
            .contains("another commit")
        );
        repo.assert_main();
        let repo = Repo::new();
        let catalog = repo.catalog();
        repo.git(&[
            "update-ref",
            "refs/heads/local-topic",
            &repo.git(&["rev-parse", "main"]),
        ]);
        assert!(
            checkout(
                &catalog,
                &pick(&catalog, "local-topic"),
                &repo.receipts(),
                &control()
            )
            .unwrap_err()
            .contains("moved")
        );
        repo.assert_main();
    }
    #[test]
    fn checkout_rechecks_repository_state_and_refuses_temporary_review_ownership() {
        let repo = Repo::new();
        let catalog = repo.catalog();
        repo.git(&["switch", "local-topic"]);
        assert!(
            checkout(
                &catalog,
                &pick(&catalog, "origin/remote-topic"),
                &repo.receipts(),
                &control()
            )
            .unwrap_err()
            .contains("changed")
        );
        let repo = Repo::new();
        let target = repo.git(&["rev-parse", "local-topic"]);
        let lease = start_at(repo.root(), "", &target, &repo.receipts(), None, &control()).unwrap();
        let catalog = repo.catalog();
        assert!(
            checkout(
                &catalog,
                &pick(&catalog, "main"),
                &repo.receipts(),
                &control()
            )
            .unwrap_err()
            .contains("review checkout")
        );
        restore(&lease.receipt).unwrap();
        repo.assert_main();
    }
    #[test]
    fn reviewed_branch_requires_the_named_ref_at_the_reviewed_commit() {
        let repo = Repo::new();
        let catalog = repo.catalog();
        let target = repo.git(&["rev-parse", "local-topic"]);
        assert!(
            catalog
                .reviewed(
                    &RefName {
                        name: "local-topic".into(),
                        remote: None
                    },
                    &target
                )
                .is_ok()
        );
        assert!(
            catalog
                .reviewed(
                    &RefName {
                        name: "main".into(),
                        remote: None
                    },
                    &target
                )
                .is_err()
        );
    }
    #[test]
    fn published_checkout_uses_the_pinned_source_and_never_resets_a_local_branch() {
        use crate::{
            accounts::Provider,
            hosting::{
                Locator, RepositoryName,
                review::{Endpoint, Metadata, Repository},
            },
        };
        let repo = Repo::new();
        let repository = Repository {
            name: RepositoryName::parse("example/project").unwrap(),
            id: "fixture".into(),
        };
        let source = repo.git(&["rev-parse", "local-topic"]);
        let base = repo.git(&["rev-parse", "main"]);
        let mut metadata = Metadata {
            locator: Locator {
                provider: Provider::Github,
                repository: repository.name.clone(),
                number: 1,
            },
            id: "PR_fixture".into(),
            title: "Review".into(),
            body: String::new(),
            source_branch: Some("review/topic".into()),
            target_branch: Some("main".into()),
            draft: Some(false),
            author: "Fixture".into(),
            state: "OPEN".into(),
            source: Endpoint {
                repository: repository.clone(),
                commit: source.clone(),
            },
            target: Endpoint {
                repository: repository.clone(),
                commit: base.clone(),
            },
            base: Endpoint {
                repository,
                commit: base,
            },
            history_partial: false,
        };
        checkout_published(repo.root(), &metadata, false, &repo.receipts(), &control()).unwrap();
        assert_eq!(repo.git(&["branch", "--show-current"]), "review/topic");
        assert_eq!(repo.git(&["rev-parse", "HEAD"]), source);
        repo.git(&["switch", "main"]);
        repo.git(&["branch", "collision", "main"]);
        metadata.source_branch = Some("collision".into());
        assert!(
            checkout_published(repo.root(), &metadata, false, &repo.receipts(), &control())
                .unwrap_err()
                .contains("another commit")
        );
        repo.assert_main();
        metadata.source_branch = Some("@{-1}".into());
        assert!(
            checkout_published(repo.root(), &metadata, false, &repo.receipts(), &control())
                .is_err()
        );
        repo.assert_main();
        metadata.source_branch = Some("missing".into());
        metadata.source.commit = "a".repeat(40);
        assert!(
            checkout_published(repo.root(), &metadata, false, &repo.receipts(), &control())
                .unwrap_err()
                .contains("fetching is disabled")
        );
        repo.assert_main();
    }
}
