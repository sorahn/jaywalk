use gpui_kit::component::diff::DiffFile;
use jaywalk::overview::overview_patch;
use jaywalk::repository::{ChangedFile, Comparison};
use std::{path::PathBuf, process::Command};

fn file(status: &str, before: &str, after: &str) -> ChangedFile {
    ChangedFile {
        status: status.into(),
        before_path: before.into(),
        after_path: after.into(),
    }
}
fn parse(file: &ChangedFile, contents: Option<(&str, &str)>) -> DiffFile {
    let patch = overview_patch(file, contents);
    let mut documents = DiffFile::parse(&patch).unwrap_or_else(|error| panic!("{error}: {patch}"));
    assert_eq!(documents.len(), 1, "{patch}");
    let document = documents.remove(0);
    assert_eq!(document.path().as_ref(), file.after_path, "{patch}");
    document
}
#[test]
fn overview_preserves_path_identity_and_line_counts() {
    for path in [
        "src/main.rs",
        "a b/漢字.rs",
        "quote\".txt",
        "tab\tname",
        "line\nname",
        "back\\slash",
    ] {
        let document = parse(
            &file("modified", path, path),
            Some(("context\nold\n", "context\nnew")),
        );
        assert_eq!(document.additions(), 1);
        assert_eq!(document.deletions(), 1);
    }
    let added = parse(&file("added", "new", "new"), Some(("", "one\ntwo\n")));
    assert_eq!((added.additions(), added.deletions()), (2, 0));
    let removed = parse(&file("removed", "gone", "gone"), Some(("one", "")));
    assert_eq!((removed.additions(), removed.deletions()), (0, 1));
}
#[test]
fn metadata_only_files_remain_present_and_keep_expandable_context() {
    for status in ["renamed", "copied", "modified"] {
        for source in ["line\nlast", "line\nlast\n"] {
            let file = file(status, "before.txt", "after.txt");
            let patch = overview_patch(&file, Some((source, source)));
            assert!(patch.contains("@@ -1,2 +1,2 @@\n line\n last"));
            let document = parse(&file, Some((source, source)));
            assert_eq!((document.additions(), document.deletions()), (0, 0));
        }
    }
    for status in ["added", "removed", "modified", "renamed"] {
        parse(&file(status, "empty", "empty"), Some(("", "")));
        parse(&file(status, "binary", "binary"), None);
    }
}
#[test]
fn overview_patch_keeps_distant_context_for_expansion() {
    let before = (0..100).map(|i| format!("line {i}\n")).collect::<String>();
    let after = before.replace("line 50\n", "changed\n");
    let patch = overview_patch(&file("modified", "long", "long"), Some((&before, &after)));
    assert!(patch.contains(" line 0\n"));
    assert!(patch.contains(" line 99\n"));
    let document = parse(&file("modified", "long", "long"), Some((&before, &after)));
    assert_eq!((document.additions(), document.deletions()), (1, 1));
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "jaywalk-overview-{}-{unique}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn run(&self, program: &str, args: &[&str]) -> String {
        let output = Command::new(program)
            .current_dir(&self.0)
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
        String::from_utf8(output.stdout).unwrap().trim_end().into()
    }
    fn jj(&self, args: &[&str]) -> String {
        let mut config = vec![
            "--config",
            "user.name=Fixture",
            "--config",
            "user.email=fixture@example.invalid",
        ];
        config.extend_from_slice(args);
        self.run("jj", &config)
    }
    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.0.join(path), text).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn git_overview_has_full_message_and_keeps_nontext_entries() {
    let fixture = Fixture::new();
    fixture.run("git", &["init", "-b", "main"]);
    fixture.write("old.txt", "unchanged content\n");
    fixture.write("deleted.txt", "gone\n");
    fixture.run("git", &["add", "."]);
    fixture.run("git", &["commit", "-m", "baseline"]);
    let base = fixture.run("git", &["rev-parse", "HEAD"]);
    std::fs::rename(fixture.0.join("old.txt"), fixture.0.join("renamed.txt")).unwrap();
    std::fs::remove_file(fixture.0.join("deleted.txt")).unwrap();
    fixture.write("empty.txt", "");
    std::fs::write(fixture.0.join("binary.dat"), [0, 255, 1]).unwrap();
    fixture.run("git", &["add", "."]);
    fixture.run(
        "git",
        &[
            "commit",
            "-m",
            "overview subject\n\nFull body with 漢字.\nSecond paragraph.",
        ],
    );
    let comparison = Comparison::load(&fixture.0, Some(&base), "HEAD").unwrap();
    fixture.write("renamed.txt", "later working copy edits\n");
    let overview = comparison.overview();
    assert!(
        overview.metadata_error.is_none(),
        "{:?}",
        overview.metadata_error
    );
    let details = overview.commit.unwrap();
    assert_eq!(details.author, "Fixture");
    assert_eq!(
        details.message,
        "overview subject\n\nFull body with 漢字.\nSecond paragraph."
    );
    assert_eq!(overview.files.len(), 5);
    for entry in &overview.files {
        assert_eq!(DiffFile::parse(&entry.patch).unwrap().len(), 1);
    }
    let rename = overview
        .files
        .iter()
        .find(|entry| entry.file.after_path == "renamed.txt")
        .unwrap();
    // Ordinary Git currently represents renames as independent additions/deletions.
    assert_eq!(rename.file.status, "added");
    assert!(rename.patch.contains("+unchanged content\n"));
    assert!(!rename.patch.contains("later working"));
    let binary = overview
        .files
        .iter()
        .find(|entry| entry.file.after_path == "binary.dat")
        .unwrap();
    assert!(
        binary
            .notice
            .as_ref()
            .unwrap()
            .contains("Text preview unavailable")
    );
    let working = Comparison::load(&fixture.0, Some("HEAD"), "@").unwrap();
    assert!(working.commit_details().unwrap().is_none());
}
#[test]
fn jj_overview_is_operation_pinned_and_never_snapshots_unsaved_work() {
    let fixture = Fixture::new();
    fixture.jj(&["git", "init"]);
    fixture.write("file.rs", "old\n");
    fixture.write("before.txt", "rename context\n");
    fixture.jj(&["describe", "-m", "baseline"]);
    fixture.jj(&["bookmark", "create", "main"]);
    fixture.jj(&["new", "-m", "subject\n\nFull body."]);
    fixture.write("file.rs", "new\n");
    std::fs::rename(fixture.0.join("before.txt"), fixture.0.join("after.txt")).unwrap();
    fixture.jj(&["describe", "-m", "subject\n\nFull body."]);
    let comparison = Comparison::load(&fixture.0, Some("main"), "@").unwrap();
    fixture.jj(&["describe", "-m", "later description"]);
    fixture.write("file.rs", "unsnapshotted\n");
    let operation = Comparison::current_operation(&fixture.0).unwrap();
    let overview = comparison.overview();
    assert!(
        overview.metadata_error.is_none(),
        "{:?}",
        overview.metadata_error
    );
    let details = overview.commit.unwrap();
    assert_eq!(details.message.trim_end(), "subject\n\nFull body.");
    assert_eq!(details.id, comparison.tip);
    assert!(details.working_copy);
    let changed = overview
        .files
        .iter()
        .find(|entry| entry.file.after_path == "file.rs")
        .unwrap();
    assert!(changed.patch.contains("+new\n"));
    assert!(!changed.patch.contains("unsnapshotted"));
    let renamed = overview
        .files
        .iter()
        .find(|entry| entry.file.after_path == "after.txt")
        .unwrap();
    assert_eq!(
        renamed.notice.as_deref(),
        Some("Renamed · contents unchanged")
    );
    assert!(renamed.patch.contains(" rename context\n"));
    assert_eq!(DiffFile::parse(&renamed.patch).unwrap()[0].additions(), 0);
    assert_eq!(
        Comparison::current_operation(&fixture.0).unwrap(),
        operation
    );
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("file.rs")).unwrap(),
        "unsnapshotted\n"
    );
}
#[test]
fn overview_settings_validate_context_and_live_geometry() {
    use jaywalk::settings::Settings;
    let settings = Settings::parse("").unwrap();
    assert_eq!(settings.overview.context_lines, 3);
    assert_eq!(settings.overview.expansion_lines, 20);
    assert!(
        Settings::parse("[overview]\ncontext_lines=0\nexpansion_lines=1\nmessage_max_height=100")
            .is_ok()
    );
    for value in [
        "context_lines=101",
        "expansion_lines=0",
        "message_max_height=0",
        "unknown=true",
    ] {
        assert!(
            Settings::parse(&format!("[overview]\n{value}")).is_err(),
            "{value}"
        );
    }
}

#[test]
fn jj_series_is_ordered_and_operation_pinned_and_skips_empty_working_copies() {
    let fixture = Fixture::new();
    fixture.jj(&["git", "init"]);
    fixture.write("file", "base\n");
    fixture.jj(&["commit", "-m", "baseline"]);
    let base = fixture.jj(&["log", "-G", "-r", "@-", "-T", "commit_id"]);
    fixture.write("file", "first\n");
    fixture.jj(&["commit", "-m", "first subject\n\nDetailed 漢字 body."]);
    fixture.write("file", "second\n");
    fixture.jj(&["commit", "-m", "second subject"]);
    let comparison = Comparison::load(&fixture.0, Some(&base), "@").unwrap();
    let series = comparison.overview_series().unwrap().unwrap();
    assert_eq!(series.commit_count(), 2);
    assert!(!series.has_working_changes());
    assert_eq!(
        series.commits[0].details.message.trim_end(),
        "first subject\n\nDetailed 漢字 body."
    );
    assert_eq!(
        series.commits[1].revision.as_ref().unwrap().description,
        "second subject"
    );
    let operation = Comparison::current_operation(&fixture.0).unwrap();
    fixture.write("file", "unsnapshotted edit\n");
    assert!(
        !comparison
            .overview_series()
            .unwrap()
            .unwrap()
            .has_working_changes()
    );
    assert_eq!(
        Comparison::current_operation(&fixture.0).unwrap(),
        operation
    );
    fixture.jj(&["status"]);
    let changed = Comparison::load(&fixture.0, Some(&base), "@").unwrap();
    let series = changed.overview_series().unwrap().unwrap();
    assert_eq!(series.commit_count(), 2);
    assert!(series.has_working_changes());
    assert!(series.commits.last().unwrap().details.working_copy);
    assert!(series.commits.last().unwrap().revision.is_some());
}

#[test]
fn git_series_includes_merged_commits_and_retains_the_captured_head() {
    let fixture = Fixture::new();
    fixture.run("git", &["init", "-b", "main"]);
    fixture.write("base", "base\n");
    fixture.run("git", &["add", "."]);
    fixture.run("git", &["commit", "-m", "baseline"]);
    let base = fixture.run("git", &["rev-parse", "HEAD"]);
    fixture.write("first", "first\n");
    fixture.run("git", &["add", "."]);
    fixture.run("git", &["commit", "-m", "first subject\n\nDetailed body."]);
    fixture.run("git", &["checkout", "-b", "side", &base]);
    fixture.write("side", "side\n");
    fixture.run("git", &["add", "."]);
    fixture.run("git", &["commit", "-m", "side subject"]);
    fixture.run("git", &["checkout", "main"]);
    fixture.run("git", &["merge", "--no-ff", "side", "-m", "merge subject"]);
    let comparison = Comparison::load(&fixture.0, Some(&base), "@").unwrap();
    let series = comparison.overview_series().unwrap().unwrap();
    assert_eq!(series.commit_count(), 3);
    assert!(!series.has_working_changes());
    assert_eq!(
        series.commits.last().unwrap().details.message,
        "merge subject"
    );
    assert!(
        series
            .commits
            .iter()
            .any(|commit| commit.details.message.contains("Detailed body."))
    );
    assert_eq!(
        series
            .commits
            .last()
            .unwrap()
            .revision
            .as_ref()
            .unwrap()
            .parents
            .len(),
        2
    );
    fixture.write("base", "dirty\n");
    let dirty = Comparison::load(&fixture.0, Some(&base), "@").unwrap();
    assert!(
        dirty
            .overview_series()
            .unwrap()
            .unwrap()
            .has_working_changes()
    );
    fixture.run("git", &["add", "."]);
    fixture.run("git", &["commit", "-m", "later edit"]);
    assert_eq!(
        comparison
            .overview_series()
            .unwrap()
            .unwrap()
            .commit_count(),
        3
    );
    assert!(
        !comparison
            .overview_series()
            .unwrap()
            .unwrap()
            .has_working_changes()
    );
    assert_eq!(dirty.overview_series().unwrap().unwrap().commit_count(), 3);
    assert!(
        dirty
            .overview_series()
            .unwrap()
            .unwrap()
            .has_working_changes()
    );
    let single = comparison
        .for_revision(series.commits[0].revision.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        single.commit_details().unwrap().unwrap().message,
        series.commits[0].details.message
    );
}
