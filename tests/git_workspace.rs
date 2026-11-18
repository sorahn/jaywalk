use jaywalk::{history::HistoryQuery, repository::Comparison};
use std::{fs, path::Path, process::Command};
struct Repo(tempfile::TempDir);
impl Repo {
    fn new() -> Self {
        let r = Self(tempfile::tempdir().unwrap());
        r.git(&["init", "-b", "main"]);
        r.write("base.txt", "baseline\n");
        r.commit("baseline");
        r
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("git")
            .current_dir(self.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim_end().into()
    }
    fn write(&self, path: &str, text: &str) {
        fs::write(self.path().join(path), text).unwrap();
    }
    fn commit(&self, message: &str) {
        self.git(&["add", "."]);
        self.git(&["commit", "-m", message]);
    }
    fn open(&self) -> Comparison {
        Comparison::load(self.path(), None, "@").unwrap()
    }
}
#[test]
fn git_startup_includes_branch_commits_and_staged_unstaged_and_untracked_work() {
    let r = Repo::new();
    let base = r.git(&["rev-parse", "HEAD"]);
    r.git(&["switch", "-c", "feature"]);
    r.write("feature.txt", "committed\n");
    r.commit("feature commit");
    r.write("staged.txt", "staged\n");
    r.git(&["add", "staged.txt"]);
    r.write("feature.txt", "committed plus unstaged\n");
    r.write("new.txt", "untracked\n");
    let index = fs::read(r.path().join(".git/index")).unwrap();
    let c = r.open();
    assert_eq!(c.base(), base);
    assert_eq!(c.recorded_tip(), r.git(&["rev-parse", "HEAD"]));
    assert_eq!(c.workspace_label(), Some("Current work · based on main"));
    let paths: Vec<_> = c.files().iter().map(|f| f.after_path()).collect();
    assert_eq!(paths, ["feature.txt", "new.txt", "staged.txt"]);
    let series = c.overview_series().unwrap().unwrap();
    assert_eq!(series.commit_count(), 1);
    assert!(series.has_working_changes());
    assert_eq!(fs::read(r.path().join(".git/index")).unwrap(), index);
    let h = c
        .history(&HistoryQuery::sources(Default::default(), None).unwrap())
        .unwrap();
    let w = c
        .for_revision(
            h.revisions
                .iter()
                .find(|r| r.is_virtual_working_copy())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(w.base(), c.recorded_tip());
    assert_eq!(
        w.contents(
            w.files()
                .iter()
                .find(|f| f.after_path() == "feature.txt")
                .unwrap()
        )
        .unwrap()
        .0,
        "committed\n"
    );
    r.write("new.txt", "later edit\n");
    assert_eq!(
        c.contents(
            c.files()
                .iter()
                .find(|f| f.after_path() == "new.txt")
                .unwrap()
        )
        .unwrap()
        .1,
        "untracked\n"
    );
}
#[test]
fn git_startup_uses_the_common_ancestor_not_an_advanced_target_or_feature_upstream() {
    let r = Repo::new();
    let base = r.git(&["rev-parse", "HEAD"]);
    r.git(&["switch", "-c", "feature"]);
    r.write("feature.txt", "feature\n");
    r.commit("feature");
    let tip = r.git(&["rev-parse", "HEAD"]);
    r.git(&["update-ref", "refs/remotes/origin/feature", &tip]);
    r.git(&["config", "branch.feature.remote", "origin"]);
    r.git(&["config", "branch.feature.merge", "refs/heads/feature"]);
    r.git(&["switch", "main"]);
    r.write("target-only.txt", "advanced\n");
    r.commit("target advances");
    let target = r.git(&["rev-parse", "HEAD"]);
    r.git(&["update-ref", "refs/remotes/origin/main", &target]);
    r.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    r.git(&["switch", "feature"]);
    let c = r.open();
    assert_eq!(c.base(), base);
    assert_eq!(
        c.workspace_label(),
        Some("Current work · based on origin/main")
    );
    assert_eq!(c.files().len(), 1);
    assert_eq!(c.files()[0].after_path(), "feature.txt");
    // Explicit endpoints retain their literal semantics, including target-only changes.
    let explicit = Comparison::load(r.path(), Some("main"), "HEAD").unwrap();
    assert_eq!(explicit.base(), target);
    assert!(explicit.workspace_label().is_none());
    assert_eq!(explicit.files().len(), 2);
    let implicit_recorded = Comparison::load(r.path(), None, "HEAD").unwrap();
    assert!(implicit_recorded.files().is_empty());
    // Detached HEAD is still a captured endpoint, independent of a branch name.
    r.git(&["switch", "--detach", &tip]);
    assert_eq!(r.open().base(), base);
}
#[test]
fn git_startup_falls_back_visibly_for_missing_ambiguous_and_unrelated_integration_bases() {
    let r = Repo::new();
    r.git(&["branch", "-m", "topic"]);
    r.write("new.txt", "new\n");
    let c = r.open();
    assert_eq!(c.base(), c.recorded_tip());
    assert!(c.workspace_label().unwrap().contains("HEAD only"));
    r.git(&["branch", "main"]);
    r.git(&["branch", "master"]);
    assert!(
        r.open()
            .workspace_label()
            .unwrap()
            .contains("no unambiguous integration branch")
    );
    r.git(&["branch", "-D", "master"]);
    r.git(&["switch", "--orphan", "unrelated"]);
    r.write("other.txt", "other\n");
    r.commit("unrelated");
    assert!(
        r.open()
            .workspace_label()
            .unwrap()
            .contains("no common integration ancestor")
    );
}
#[test]
fn git_startup_does_not_choose_one_of_multiple_merge_bases() {
    let r = Repo::new();
    let root = r.git(&["rev-parse", "HEAD"]);
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    let a = r.git(&["commit-tree", &tree, "-p", &root, "-m", "a"]);
    let b = r.git(&["commit-tree", &tree, "-p", &root, "-m", "b"]);
    let left = r.git(&["commit-tree", &tree, "-p", &a, "-p", &b, "-m", "left"]);
    let right = r.git(&["commit-tree", &tree, "-p", &b, "-p", &a, "-m", "right"]);
    r.git(&["branch", "feature", &left]);
    r.git(&["switch", "feature"]);
    r.git(&["update-ref", "refs/heads/main", &right]);
    let c = r.open();
    assert_eq!(c.base(), left);
    assert!(
        c.workspace_label()
            .unwrap()
            .contains("multiple integration ancestors")
    );
}
