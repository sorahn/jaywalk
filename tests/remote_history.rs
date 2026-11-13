use jaywalk::{history::HistoryQuery, repository::Comparison};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "jaywalk-remotes-{}-{stamp}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        fixture.run("git", &["init", "-b", "main"]);
        fixture.run("git", &["config", "user.name", "Viewer Test"]);
        fixture.run("git", &["config", "user.email", "viewer@example.invalid"]);
        std::fs::write(fixture.0.join("file.rs"), "fn main() {}\n").unwrap();
        fixture.run("git", &["add", "."]);
        fixture.run("git", &["commit", "-m", "baseline"]);
        fixture
    }
    fn run(&self, program: &str, args: &[&str]) -> String {
        run(&self.0, program, args)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn run(root: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .current_dir(root)
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
#[test]
fn git_and_jj_enumerate_synced_untracked_and_same_named_remote_references() {
    let f = Fixture::new();
    let base = f.run("git", &["rev-parse", "HEAD"]);
    f.run("git", &["remote", "add", "origin", "."]);
    f.run("git", &["remote", "add", "upstream", "."]);
    f.run("git", &["remote", "add", "empty", "."]);
    for name in ["refs/remotes/origin/main", "refs/remotes/upstream/main"] {
        f.run("git", &["update-ref", name, &base]);
    }
    f.run(
        "git",
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    f.run("git", &["config", "branch.main.remote", "origin"]);
    f.run("git", &["config", "branch.main.merge", "refs/heads/main"]);
    let git = Comparison::load(&f.0, Some("HEAD"), "HEAD").unwrap();
    let refs = git.reference_snapshot().unwrap();
    assert_eq!(refs.remotes, ["empty", "origin", "upstream"]);
    assert_eq!(refs.references.len(), 3);
    assert_eq!(
        refs.references
            .iter()
            .find(|r| r.remote() == Some("origin"))
            .unwrap()
            .tracked_by,
        ["main"]
    );
    assert!(
        refs.references
            .iter()
            .any(|r| r.remote() == Some("upstream") && r.name == "main")
    );
    f.run("jj", &["git", "init", "--colocate"]);
    f.run("jj", &["bookmark", "track", "main@origin"]);
    let jj = Comparison::load(&f.0, Some("main"), "@").unwrap();
    let operation = Comparison::current_operation(&f.0).unwrap();
    let history = jj.history(&HistoryQuery::default()).unwrap();
    let imported = jj.evolution(&base, 100).unwrap();
    assert!(
        imported
            .versions
            .iter()
            .any(|version| version.operation_id.is_none())
    );
    let refs = &history.references;
    assert_eq!(refs.remotes, ["empty", "origin", "upstream"]);
    assert_eq!(refs.references.len(), 3);
    assert!(
        refs.references
            .iter()
            .any(|r| r.remote() == Some("origin") && r.tracked_by == ["main"])
    );
    assert!(
        refs.references
            .iter()
            .any(|r| r.remote() == Some("upstream") && r.tracked_by.is_empty())
    );
    assert_eq!(Comparison::current_operation(&f.0).unwrap(), operation);
}

#[test]
fn hiding_remote_roots_keeps_shared_local_history_and_can_reveal_a_pinned_selection() {
    use jaywalk::history::RemoteVisibility;
    let f = Fixture::new();
    let base = f.run("git", &["rev-parse", "HEAD"]);
    f.run("git", &["remote", "add", "origin", "."]);
    f.run("git", &["checkout", "-b", "temporary"]);
    f.run("git", &["commit", "--allow-empty", "-m", "remote only"]);
    let remote = f.run("git", &["rev-parse", "HEAD"]);
    f.run("git", &["checkout", "main"]);
    f.run("git", &["update-ref", "refs/remotes/origin/topic", &remote]);
    f.run("git", &["update-ref", "refs/remotes/origin/main", &base]);
    f.run("git", &["branch", "-D", "temporary"]);
    for recorded in [false, true] {
        if recorded {
            f.run("jj", &["git", "init", "--colocate"]);
        }
        let comparison =
            Comparison::load(&f.0, Some("main"), if recorded { "@" } else { "HEAD" }).unwrap();
        let visible = HistoryQuery::sources(RemoteVisibility::default(), None).unwrap();
        let hidden = HistoryQuery::sources(RemoteVisibility::all(false), None).unwrap();
        assert!(
            comparison
                .history(&visible)
                .unwrap()
                .revisions
                .iter()
                .any(|r| r.commit_id == remote)
        );
        let filtered = comparison.history(&hidden).unwrap();
        assert!(!filtered.revisions.iter().any(|r| r.commit_id == remote));
        assert!(filtered.revisions.iter().any(|r| r.commit_id == base));
        assert!(
            filtered
                .references
                .references
                .iter()
                .any(|r| r.target == remote),
            "hidden references remain discoverable"
        );
        let revealed = comparison
            .history(
                &HistoryQuery::sources(RemoteVisibility::all(false), Some(remote.clone())).unwrap(),
            )
            .unwrap();
        assert!(revealed.revisions.iter().any(|r| r.commit_id == remote));
    }
}

#[test]
fn external_metadata_refresh_does_not_repin_the_open_comparison() {
    use jaywalk::references::ReferenceStamp;
    let f = Fixture::new();
    f.run("git", &["remote", "add", "origin", "."]);
    let git = Comparison::load(&f.0, Some("main"), "HEAD").unwrap();
    let initial = ReferenceStamp::read(&f.0, false).unwrap();
    f.run("git", &["remote", "add", "another", "."]);
    assert_ne!(initial, ReferenceStamp::read(&f.0, false).unwrap());
    f.run("git", &["remote", "remove", "another"]);
    assert_eq!(initial, ReferenceStamp::read(&f.0, false).unwrap());
    let base = git.tip().to_owned();
    f.run("jj", &["git", "init", "--colocate"]);
    let pinned = Comparison::load(&f.0, Some("main"), "@").unwrap();
    let operation = pinned.operation().to_owned();
    let stamp = ReferenceStamp::read(&f.0, true).unwrap();
    let tree = f.run("git", &["rev-parse", &format!("{base}^{{tree}}")]);
    let remote = f.run(
        "git",
        &["commit-tree", &tree, "-p", &base, "-m", "fetched update"],
    );
    f.run(
        "git",
        &["update-ref", "refs/remotes/origin/update", &remote],
    );
    f.run("jj", &["git", "import"]);
    assert_ne!(stamp, ReferenceStamp::read(&f.0, true).unwrap());
    let current = pinned.current_history(&Default::default()).unwrap();
    let revision = current
        .revisions
        .iter()
        .find(|r| r.commit_id == remote)
        .unwrap();
    let source = current.references.operation.as_deref();
    assert_ne!(source, Some(operation.as_str()));
    let patch = pinned.for_history_revision(revision, source).unwrap();
    assert_eq!(patch.tip(), remote);
    assert_eq!(pinned.operation(), operation);
    assert!(
        pinned
            .reference_snapshot()
            .unwrap()
            .references
            .iter()
            .all(|r| r.target != remote)
    );
    assert!(
        pinned
            .evolution_at(&remote, 100, source.unwrap())
            .unwrap()
            .versions
            .iter()
            .any(|v| v.commit_id == remote)
    );
    assert_eq!(
        Comparison::current_operation(&f.0).unwrap(),
        source.unwrap(),
        "reads must not import or snapshot"
    );
}

#[test]
fn git_reference_indexes_preserve_slash_remotes_aliases_and_tracking() {
    let f = Fixture::new();
    let target = f.run("git", &["rev-parse", "HEAD"]);
    for remote in ["team", "team/origin"] {
        f.run("git", &["config", &format!("remote.{remote}.url"), "."]);
        f.run(
            "git",
            &[
                "config",
                &format!("remote.{remote}.fetch"),
                &format!("+refs/heads/*:refs/remotes/{remote}/*"),
            ],
        );
    }
    f.run(
        "git",
        &["update-ref", "refs/remotes/team/origin/main", &target],
    );
    for local in ["feature/one", "feature/two"] {
        f.run(
            "git",
            &["update-ref", &format!("refs/heads/{local}"), &target],
        );
        f.run(
            "git",
            &["config", &format!("branch.{local}.remote"), "team/origin"],
        );
        f.run(
            "git",
            &[
                "config",
                &format!("branch.{local}.merge"),
                "refs/heads/main",
            ],
        );
    }
    let comparison = Comparison::load(&f.0, Some("HEAD"), "HEAD").unwrap();
    let refs = comparison.reference_snapshot().unwrap();
    let remote = refs
        .references
        .iter()
        .find(|reference| reference.remote() == Some("team/origin"))
        .unwrap();
    assert_eq!(remote.name, "main");
    assert_eq!(remote.tracked_by, ["feature/one", "feature/two"]);
    assert_eq!(refs.remotes, ["team", "team/origin"]);
    let history = comparison.history(&HistoryQuery::default()).unwrap();
    assert_eq!(
        history
            .revisions
            .iter()
            .find(|revision| revision.commit_id == target)
            .unwrap()
            .references
            .len(),
        4
    );
}
