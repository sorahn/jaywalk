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
