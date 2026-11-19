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

fn move_file(r: &Repo, from: &str, to: &str) {
    fs::rename(r.path().join(from), r.path().join(to)).unwrap();
}
fn rename_paths(c: &Comparison) -> Vec<(&str, &str)> {
    c.files()
        .iter()
        .filter(|f| f.status() == "renamed")
        .map(|f| (f.before_path(), f.after_path()))
        .collect()
}
#[test]
fn git_unstaged_renames_keep_both_captured_sides_and_do_not_write_the_index() {
    let r = Repo::new();
    r.write("old 漢字.txt", "one\ntwo\nthree\n");
    r.commit("file to move");
    move_file(&r, "old 漢字.txt", "new\tname.txt");
    let index = fs::read(r.path().join(".git/index")).unwrap();
    let c = r.open();
    assert_eq!(rename_paths(&c), [("old 漢字.txt", "new\tname.txt")]);
    assert_eq!(c.files().len(), 1);
    assert_eq!(
        c.contents(&c.files()[0]).unwrap(),
        ("one\ntwo\nthree\n".into(), "one\ntwo\nthree\n".into())
    );
    assert!(c.catalog().find("old 漢字.txt").is_some()); // old paths transfer selection to the rename
    assert_eq!(fs::read(r.path().join(".git/index")).unwrap(), index);
    r.write("new\tname.txt", "later content\n");
    assert_eq!(c.contents(&c.files()[0]).unwrap().1, "one\ntwo\nthree\n");
    // Refreshed contents still follow the actual working file, rather than the
    // deleted path or an index blob.
    let refreshed = r.open();
    assert_eq!(refreshed.files().len(), 2);
}
#[test]
fn git_staged_and_committed_renames_honor_git_detection_and_distinct_paths() {
    let r = Repo::new();
    r.write("old.rs", "fn preserved() {}\n");
    r.commit("add file");
    let base = r.git(&["rev-parse", "HEAD"]);
    r.git(&["mv", "old.rs", "new.rs"]);
    let staged = r.open();
    assert_eq!(rename_paths(&staged), [("old.rs", "new.rs")]);
    assert_eq!(staged.catalog().find("new.rs").unwrap().path(), "new.rs");
    r.commit("move file");
    let recorded = Comparison::load(r.path(), Some(&base), "HEAD").unwrap();
    assert_eq!(rename_paths(&recorded), [("old.rs", "new.rs")]);
    assert_eq!(
        recorded.contents(&recorded.files()[0]).unwrap(),
        ("fn preserved() {}\n".into(), "fn preserved() {}\n".into())
    );
    let reversed = recorded.swapped().unwrap();
    assert_eq!(rename_paths(&reversed), [("new.rs", "old.rs")]);
}
#[test]
fn git_unstaged_renamed_and_edited_files_use_bounded_line_similarity() {
    let r = Repo::new();
    let before = (0..30)
        .map(|n| format!("line {n}: preserved content 漢字\n"))
        .collect::<String>();
    r.write("before.ts", &before);
    r.commit("source");
    move_file(&r, "before.ts", "after.ts");
    let after = before.replace(
        "line 12: preserved content 漢字",
        "line 12: edited content 🌳",
    );
    r.write("after.ts", &after);
    let c = r.open();
    assert_eq!(rename_paths(&c), [("before.ts", "after.ts")]);
    assert_eq!(c.contents(&c.files()[0]).unwrap(), (before, after));
}
#[test]
fn git_ambiguous_duplicate_and_split_candidates_stay_as_additions_and_deletions() {
    let r = Repo::new();
    r.write("a.txt", "identical contents\n");
    r.write("b.txt", "identical contents\n");
    r.commit("duplicates");
    move_file(&r, "a.txt", "c.txt");
    move_file(&r, "b.txt", "d.txt");
    let c = r.open();
    assert!(rename_paths(&c).is_empty());
    assert_eq!(c.files().len(), 4);
    let split = Repo::new();
    let lines: Vec<_> = (0..10)
        .map(|n| format!("{n:02}: unique content line\n"))
        .collect();
    split.write("whole.txt", &lines.concat());
    split.commit("whole");
    fs::remove_file(split.path().join("whole.txt")).unwrap();
    split.write("part-one.txt", &lines[..6].concat());
    split.write("part-two.txt", &lines[6..].concat());
    let c = split.open();
    assert!(rename_paths(&c).is_empty());
    assert_eq!(c.files().len(), 3);
}
#[test]
fn git_does_not_infer_copies_or_empty_or_oversized_or_type_changed_renames() {
    let r = Repo::new();
    r.write("empty.txt", "");
    r.write("large.txt", &"a".repeat(2 * 1024 * 1024));
    r.commit("sources");
    move_file(&r, "empty.txt", "moved-empty.txt");
    move_file(&r, "large.txt", "moved-large.txt");
    r.write("copy.txt", "baseline\n");
    let limit = jaywalk::text_content::TextLimit::from_mib(5).unwrap();
    let c = Comparison::load_with_limit(r.path(), None, "@", limit).unwrap();
    // The large file is eligible for exact matching, while an unchanged source
    // is never consumed to infer a copy/rename.
    assert_eq!(rename_paths(&c), [("large.txt", "moved-large.txt")]);
    assert!(
        c.files()
            .iter()
            .any(|f| f.after_path() == "copy.txt" && f.status() == "added")
    );
    let limited = Comparison::load_with_limit(
        r.path(),
        None,
        "@",
        jaywalk::text_content::TextLimit::from_mib(1).unwrap(),
    )
    .unwrap();
    assert!(rename_paths(&limited).is_empty());
}
#[cfg(unix)]
#[test]
fn git_symlink_rename_uses_the_link_text_and_never_the_target_contents() {
    let r = Repo::new();
    std::os::unix::fs::symlink("missing-target", r.path().join("old-link")).unwrap();
    r.commit("link");
    move_file(&r, "old-link", "new-link");
    let c = r.open();
    assert_eq!(rename_paths(&c), [("old-link", "new-link")]);
    assert_eq!(
        c.contents(&c.files()[0]).unwrap(),
        ("missing-target".into(), "missing-target".into())
    );
    fs::remove_file(r.path().join("new-link")).unwrap();
    r.write("new-link", "missing-target");
    assert!(rename_paths(&r.open()).is_empty());
}

#[test]
fn git_rename_candidate_limits_leave_the_original_file_changes_intact() {
    let r = Repo::new();
    for n in 0..201 {
        r.write(&format!("old-{n}.txt"), &format!("unique contents {n}\n"));
    }
    r.commit("many sources");
    for n in 0..201 {
        fs::remove_file(r.path().join(format!("old-{n}.txt"))).unwrap();
    }
    r.write("new.txt", "unique contents 0\n");
    let c = r.open();
    assert!(rename_paths(&c).is_empty());
    assert_eq!(c.files().len(), 202);
    assert_eq!(
        c.contents(
            c.files()
                .iter()
                .find(|f| f.after_path() == "new.txt")
                .unwrap()
        )
        .unwrap()
        .1,
        "unique contents 0\n"
    );
}
#[test]
fn git_similar_competing_sources_and_large_edited_files_are_not_guessed() {
    let r = Repo::new();
    let common = (0..30)
        .map(|n| format!("common source {n:02}\n"))
        .collect::<String>();
    r.write("a.txt", &format!("{common}old a\n"));
    r.write("b.txt", &format!("{common}old b\n"));
    r.commit("competing sources");
    fs::remove_file(r.path().join("a.txt")).unwrap();
    fs::remove_file(r.path().join("b.txt")).unwrap();
    r.write("new.txt", &format!("{common}new c\n"));
    assert!(rename_paths(&r.open()).is_empty());
    let large = Repo::new();
    let text = (0..5000)
        .map(|n| format!("large retained source line {n:05}\n"))
        .collect::<String>();
    large.write("before.txt", &text);
    large.commit("large source");
    move_file(&large, "before.txt", "after.txt");
    large.write("after.txt", &text.replace("00012", "edited"));
    let c = large.open();
    assert!(rename_paths(&c).is_empty());
    assert_eq!(c.files().len(), 2);
}
