use super::*;
use crate::hosting::{Locator, RepositoryName, review::Repository};
use std::process::Command;
pub(crate) struct Fixture {
    pub directory: tempfile::TempDir,
    pub base: String,
    pub head: String,
}
impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), &["init", "-b", "main"]);
        std::fs::write(directory.path().join("changed.rs"), "fn before() {}\n").unwrap();
        std::fs::write(directory.path().join("stable.rs"), "fn stable() {}\n").unwrap();
        git(directory.path(), &["add", "."]);
        git(directory.path(), &["commit", "-m", "base"]);
        let base = git(directory.path(), &["rev-parse", "HEAD"]);
        std::fs::write(directory.path().join("changed.rs"), "fn after() {}\n").unwrap();
        std::fs::write(directory.path().join("new.rs"), "fn added() {}\n").unwrap();
        git(directory.path(), &["add", "."]);
        git(directory.path(), &["commit", "-m", "head"]);
        let head = git(directory.path(), &["rev-parse", "HEAD"]);
        git(directory.path(), &["checkout", "--detach", &base]);
        git(directory.path(), &["branch", "-f", "main", &base]);
        git(directory.path(), &["checkout", "main"]);
        git(
            directory.path(),
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:fixture/review.git",
            ],
        );
        Self {
            directory,
            base,
            head,
        }
    }
    pub fn metadata(&self) -> Metadata {
        let repository = Repository {
            name: RepositoryName::parse("fixture/review").unwrap(),
            id: "fixture-id".into(),
        };
        Metadata {
            locator: Locator {
                provider: crate::accounts::Provider::Github,
                repository: RepositoryName::parse("fixture/review").unwrap(),
                number: 1,
            },
            id: "1".into(),
            title: "Fixture".into(),
            body: String::new(),
            source_branch: Some("feature".into()),
            target_branch: Some("main".into()),
            draft: Some(false),
            author: "Fixture".into(),
            state: "open".into(),
            source: Endpoint {
                repository: repository.clone(),
                commit: self.head.clone(),
            },
            target: Endpoint {
                repository: repository.clone(),
                commit: self.base.clone(),
            },
            base: Endpoint {
                repository,
                commit: self.base.clone(),
            },
            history_partial: false,
        }
    }
}
pub(crate) fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
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
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}
pub(crate) fn control() -> Control {
    Control::new(Duration::from_secs(20))
}
#[test]
fn isolated_objects_keep_full_trees_and_survive_the_source_repository_disappearing() {
    let fixture = Fixture::new();
    let metadata = fixture.metadata();
    let root = fixture.directory.path();
    let before = [
        git(root, &["rev-parse", "HEAD"]),
        git(root, &["show-ref"]),
        git(root, &["status", "--porcelain=v1"]),
        std::fs::read_to_string(root.join(".git/config")).unwrap(),
    ];
    let workspace = Workspace::prepare(
        root,
        &metadata,
        Options {
            mode: Mode::Isolated,
            max_repository_mb: 16,
            fetch_missing: false,
            ..Default::default()
        },
        &control(),
    )
    .unwrap();
    assert!(
        workspace
            .paths(&metadata.source.commit)
            .unwrap()
            .contains_key("stable.rs")
    );
    assert_eq!(
        workspace
            .read(&metadata.base.commit, "changed.rs", TextLimit::default())
            .unwrap(),
        "fn before() {}\n"
    );
    assert_eq!(
        workspace
            .read(&metadata.source.commit, "changed.rs", TextLimit::default())
            .unwrap(),
        "fn after() {}\n"
    );
    let after = [
        git(root, &["rev-parse", "HEAD"]),
        git(root, &["show-ref"]),
        git(root, &["status", "--porcelain=v1"]),
        std::fs::read_to_string(root.join(".git/config")).unwrap(),
    ];
    assert_eq!(before, after);
    assert!(!root.join(".git/FETCH_HEAD").exists());
    drop(fixture);
    assert_eq!(
        workspace
            .read(&metadata.source.commit, "stable.rs", TextLimit::default())
            .unwrap(),
        "fn stable() {}\n"
    );
    assert!(
        !workspace
            .directory()
            .join("objects/info/alternates")
            .exists()
    );
}
#[test]
fn local_acquisition_rejects_mismatched_identities_and_cancelled_requests() {
    let fixture = Fixture::new();
    let mut metadata = fixture.metadata();
    metadata.locator = Locator {
        provider: crate::accounts::Provider::Github,
        repository: RepositoryName::parse("other/repo").unwrap(),
        number: 1,
    };
    assert!(
        Workspace::prepare(
            fixture.directory.path(),
            &metadata,
            Options::default(),
            &control()
        )
        .is_err()
    );
    let cancelled = control();
    cancelled.cancel();
    assert!(
        Workspace::prepare(
            fixture.directory.path(),
            &fixture.metadata(),
            Options::default(),
            &cancelled
        )
        .is_err()
    );
    assert!(validate_commit("main").is_err());
}
#[test]
fn git_worker_stops_a_synthetic_child_on_deadline() {
    let fixture = Fixture::new();
    let started = std::time::Instant::now();
    let result = process::git(
        fixture.directory.path(),
        &["-c", "alias.pause=!sleep 10", "pause"],
        &Control::new(Duration::from_millis(100)),
        None,
    );
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn local_review_policy_is_opt_in_and_settings_are_validated() {
    assert_eq!(
        crate::settings::Config::default().local_review.mode,
        Mode::Hosted
    );
    assert_eq!(
        crate::settings::Settings::parse(
            "[local_review]\nmode='checkout_if_clean'\nmax_repository_mb=64"
        )
        .unwrap()
        .local_review
        .mode,
        Mode::CheckoutIfClean
    );
    assert!(crate::settings::Settings::parse("[local_review]\nmode='force'").is_err());
    assert!(crate::settings::Settings::parse("[local_review]\nmax_repository_mb=0").is_err());
}

#[test]
fn bitbucket_ssh_clones_match_acquire_checkout_and_restore_without_remote_access() {
    for remote in [
        "git@ssh.bitbucket.org:fixture/review.git",
        "ssh://git@altssh.bitbucket.org:443/fixture/review.git",
    ] {
        let fixture = Fixture::new();
        let root = fixture.directory.path();
        git(root, &["remote", "set-url", "origin", remote]);
        let mut metadata = fixture.metadata();
        metadata.locator.provider = crate::accounts::Provider::Bitbucket;
        let association =
            crate::hosting::associate(&metadata.locator, &[root.to_owned()], &control()).unwrap();
        assert_eq!(association.matches, vec![root.to_owned()]);
        assert_eq!(
            transport(
                root,
                metadata.locator.provider,
                &metadata.source.repository.name,
                &control()
            )
            .unwrap(),
            remote
        );
        assert_eq!(
            transport(
                root,
                metadata.locator.provider,
                &RepositoryName::parse("fork/review").unwrap(),
                &control()
            )
            .unwrap(),
            "git@ssh.bitbucket.org:fork/review.git"
        );
        let snapshot = checkout::preflight(root, &control()).unwrap();
        let workspace = Workspace::prepare(
            root,
            &metadata,
            Options {
                mode: Mode::CheckoutIfClean,
                fetch_missing: false,
                ..Default::default()
            },
            &control(),
        )
        .unwrap();
        let receipts = tempfile::tempdir().unwrap();
        let lease = checkout::start(&workspace, receipts.path(), &snapshot, &control()).unwrap();
        assert!(lease.active());
        assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.head);
        assert_eq!(git(root, &["rev-parse", "--abbrev-ref", "HEAD"]), "HEAD");
        let recovery = checkout::recoveries(receipts.path()).unwrap().remove(0);
        checkout::restore(&recovery.path).unwrap();
        assert_eq!(git(root, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert_eq!(git(root, &["rev-parse", "HEAD"]), fixture.base);
        assert_eq!(git(root, &["remote", "get-url", "origin"]), remote);
    }
}

#[test]
fn checkout_branch_settings_validate_and_preserve_default_policy() {
    let defaults = crate::settings::Config::default().local_review;
    assert_eq!(defaults.checkout_branch, CheckoutBranch::CreateFromRemote);
    assert!(!defaults.cleanup_created_branch);
    for (name, expected) in [
        ("detached", CheckoutBranch::Detached),
        ("existing", CheckoutBranch::Existing),
        ("create_from_remote", CheckoutBranch::CreateFromRemote),
    ] {
        let settings = crate::settings::Settings::parse(&format!(
            "[local_review]\ncheckout_branch='{name}'\ncleanup_created_branch=true"
        ))
        .unwrap();
        assert_eq!(settings.local_review.checkout_branch, expected);
        assert!(settings.local_review.cleanup_created_branch);
    }
    assert!(crate::settings::Settings::parse("[local_review]\ncheckout_branch='force'").is_err());
    assert!(
        crate::settings::Settings::parse("[local_review]\ncleanup_created_branch='true'").is_err()
    );
}
