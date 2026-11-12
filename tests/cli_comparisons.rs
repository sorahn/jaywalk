use jaywalk::external::ExternalContents;
use jaywalk::repository::Comparison;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "jaywalk-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
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
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn content(comparison: &Comparison, name: &str) -> (String, String) {
    comparison
        .contents(
            comparison
                .files
                .iter()
                .find(|file| file.after_path == name)
                .unwrap(),
        )
        .unwrap()
}

#[test]
fn directory_diff_keeps_paths_empty_additions_and_deletions() {
    let fixture = Fixture::new();
    for (path, text) in [
        ("left/src/a.ts", "old\n"),
        ("right/src/a.ts", "new\n"),
        ("left/gone", "gone"),
        ("right/empty", ""),
        ("left/same", "same"),
        ("right/same", "same"),
    ] {
        fixture.write(path, text);
    }
    let comparison =
        ExternalContents::comparison(&fixture.0.join("left"), &fixture.0.join("right")).unwrap();
    assert!(!comparison.supports_evolution());
    assert_eq!(comparison.files.len(), 3);
    assert_eq!(
        content(&comparison, "src/a.ts"),
        ("old\n".into(), "new\n".into())
    );
    assert_eq!(
        content(&comparison, "empty"),
        (String::new(), String::new())
    );
    assert_eq!(content(&comparison, "gone"), ("gone".into(), String::new()));
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("left/src/a.ts")).unwrap(),
        "old\n"
    );
}
#[test]
fn file_diff_handles_different_names_unicode_and_binary_errors() {
    let fixture = Fixture::new();
    fixture.write("before.txt", "\té\n");
    fixture.write("after.txt", "\t漢\n");
    let comparison =
        ExternalContents::comparison(&fixture.0.join("before.txt"), &fixture.0.join("after.txt"))
            .unwrap();
    assert_eq!(
        content(&comparison, "after.txt"),
        ("\té\n".into(), "\t漢\n".into())
    );
    std::fs::write(fixture.0.join("after.txt"), [0, 1]).unwrap();
    assert!(
        comparison
            .contents(&comparison.files[0])
            .unwrap_err()
            .contains("Binary")
    );
    assert!(ExternalContents::comparison(&fixture.0, &fixture.0.join("after.txt")).is_err());
}
#[test]
fn git_worktree_and_history_are_read_only_and_use_pinned_contents() {
    let fixture = Fixture::new();
    fixture.git(&["init", "-b", "main"]);
    fixture.write("a file.txt", "original\n");
    fixture.write("deleted.txt", "deleted\n");
    fixture.write("unchanged.txt", "stable\n");
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "initial"]);
    let initial = fixture.git(&["rev-parse", "HEAD"]);
    fixture.write("a file.txt", "staged\n");
    fixture.git(&["add", "."]);
    fixture.write("a file.txt", "unstaged\n");
    fixture.write("new file.txt", "new\n");
    std::fs::remove_file(fixture.0.join("deleted.txt")).unwrap();
    let status = fixture.git(&["status", "--porcelain=v1"]);
    let comparison = Comparison::load(&fixture.0, None, "@").unwrap();
    assert!(!comparison.supports_evolution());
    assert_eq!(comparison.files.len(), 3);
    assert_eq!(comparison.identity(), comparison.clone().identity());
    let next_capture = Comparison::load(&fixture.0, None, "@").unwrap();
    assert_ne!(comparison.identity(), next_capture.identity());
    // Unchanged paths use their pinned base object, not later filesystem edits.
    fixture.write("unchanged.txt", "changed after capture\n");
    let unchanged = comparison.catalog.find("unchanged.txt").unwrap();
    assert!(unchanged.kind().is_none());
    assert_eq!(
        comparison.read_file(unchanged).unwrap(),
        ("stable\n".into(), "stable\n".into())
    );
    assert!(
        comparison
            .catalog
            .find("deleted.txt")
            .unwrap()
            .kind()
            .is_some()
    );
    assert!(
        comparison
            .read_path("never existed", jaywalk::repository::FileSide::After)
            .is_err()
    );
    fixture.write("unchanged.txt", "stable\n");
    fixture.write("a file.txt", "later\n");
    assert_eq!(
        content(&comparison, "a file.txt"),
        ("original\n".into(), "unstaged\n".into())
    );
    fixture.write("a file.txt", "unstaged\n");
    assert_eq!(fixture.git(&["status", "--porcelain=v1"]), status);
    assert_eq!(fixture.git(&["rev-parse", "HEAD"]), initial);
    let history = comparison.history(&Default::default()).unwrap();
    assert_eq!(history.revisions.len(), 1);
    let roots = jaywalk::history::HistoryQuery::ancestors(vec![initial.clone()]).unwrap();
    assert_eq!(comparison.history(&roots).unwrap().revisions.len(), 1);
    let patch = comparison.for_revision(&history.revisions[0]).unwrap();
    assert_eq!(
        content(&patch, "a file.txt"),
        (String::new(), "original\n".into())
    );
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "second"]);
    let committed = Comparison::load(&fixture.0, Some(&initial), "HEAD").unwrap();
    assert_eq!(
        committed.identity(),
        Comparison::load(&fixture.0, Some(&initial), "HEAD")
            .unwrap()
            .identity()
    );
    assert_eq!(
        content(&committed, "a file.txt"),
        ("original\n".into(), "unstaged\n".into())
    );
    assert_eq!(
        committed
            .history(&Default::default())
            .unwrap()
            .revisions
            .len(),
        2
    );
    assert_eq!(committed.history(&roots).unwrap().revisions.len(), 1);
    assert_eq!(
        committed
            .history(
                &jaywalk::history::HistoryQuery::default()
                    .with_limit(1)
                    .unwrap()
            )
            .unwrap()
            .revisions
            .len(),
        1
    );
    assert!(!Path::new(&fixture.0.join(".jj")).exists());
}

#[cfg(unix)]
#[test]
fn git_null_device_inputs_represent_additions_and_deletions() {
    let fixture = Fixture::new();
    fixture.write("file.txt", "content\n");
    for (left, right) in [
        (
            Path::new("/dev/null").to_path_buf(),
            fixture.0.join("file.txt"),
        ),
        (
            fixture.0.join("file.txt"),
            Path::new("/dev/null").to_path_buf(),
        ),
    ] {
        let comparison = ExternalContents::comparison(&left, &right).unwrap();
        let (before, after) = content(&comparison, "file.txt");
        assert_eq!(before + &after, "content\n");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn finder_environment_can_read_a_jj_repository_without_launching_a_window() {
    let fixture = Fixture::new();
    fixture.git(&["init", "-b", "main"]);
    fixture.write("file.txt", "recorded\n");
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "baseline"]);
    let result = Command::new("jj")
        .current_dir(&fixture.0)
        .args(["git", "init", "--colocate"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(env!("CARGO_BIN_EXE_jaywalk"))
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .args(["--inspect", fixture.0.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        String::from_utf8(result.stdout)
            .unwrap()
            .contains("changed files")
    );
}

#[test]
fn untracked_paths_keep_contents_from_the_selected_base() {
    let fixture = Fixture::new();
    fixture.git(&["init", "-b", "main"]);
    fixture.write("file.txt", "original\n");
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "initial"]);
    let base = fixture.git(&["rev-parse", "HEAD"]);
    fixture.git(&["rm", "--cached", "file.txt"]);
    fixture.write("file.txt", "edited\n");
    let comparison = Comparison::load(&fixture.0, None, "@").unwrap();
    assert_eq!(comparison.files[0].status, "modified");
    assert_eq!(
        content(&comparison, "file.txt"),
        ("original\n".into(), "edited\n".into())
    );
    fixture.git(&["commit", "-m", "remove from index"]);
    let comparison = Comparison::load(&fixture.0, Some(&base), "@").unwrap();
    assert_eq!(
        content(&comparison, "file.txt"),
        ("original\n".into(), "edited\n".into())
    );
    assert!(
        fixture
            .git(&["ls-files", "--others", "--exclude-standard"])
            .contains("file.txt")
    );
}

#[cfg(unix)]
#[test]
fn standalone_links_compare_targets_including_dangling_and_directory_links() {
    let fixture = Fixture::new();
    fixture.write("target-a", "same");
    fixture.write("target-b", "same");
    let left = fixture.0.join("left-link");
    let right = fixture.0.join("right-link");
    std::os::unix::fs::symlink("target-a", &left).unwrap();
    std::os::unix::fs::symlink("target-b", &right).unwrap();
    let comparison = ExternalContents::comparison(&left, &right).unwrap();
    assert_eq!(comparison.root.file_name().unwrap(), "right-link");
    assert_eq!(
        content(&comparison, "right-link"),
        ("target-a".into(), "target-b".into())
    );
    std::fs::remove_file(&right).unwrap();
    std::os::unix::fs::symlink("missing-target", &right).unwrap();
    let comparison = ExternalContents::comparison(&left, &right).unwrap();
    assert_eq!(content(&comparison, "right-link").1, "missing-target");
    std::fs::remove_file(&left).unwrap();
    std::fs::create_dir(fixture.0.join("directory-target")).unwrap();
    std::os::unix::fs::symlink("directory-target", &left).unwrap();
    let comparison = ExternalContents::comparison(&left, &right).unwrap();
    assert_eq!(
        content(&comparison, "right-link"),
        ("directory-target".into(), "missing-target".into())
    );
    assert_eq!(
        jaywalk::external::normalize_input(&right).unwrap(),
        std::fs::canonicalize(&fixture.0)
            .unwrap()
            .join("right-link")
    );
}

#[cfg(unix)]
#[test]
fn external_type_changes_remain_visible_with_identical_display_text() {
    let fixture = Fixture::new();
    fixture.write("left/link-to-file", "../target");
    fixture.write("right/file-to-link", "../target");
    std::os::unix::fs::symlink("../target", fixture.0.join("right/link-to-file")).unwrap();
    std::os::unix::fs::symlink("../target", fixture.0.join("left/file-to-link")).unwrap();
    let comparison =
        ExternalContents::comparison(&fixture.0.join("left"), &fixture.0.join("right")).unwrap();
    assert_eq!(comparison.files.len(), 2);
    for path in ["link-to-file", "file-to-link"] {
        let entry = comparison.catalog.find(path).unwrap();
        let (before, after) = comparison.read_file(entry).unwrap();
        assert_eq!(before, after);
        assert!(entry.type_change_notice().unwrap().contains("symlink"));
    }
    let overview = comparison.overview();
    assert!(overview.files.iter().all(|file| {
        file.notice
            .as_deref()
            .unwrap()
            .starts_with("File type changed:")
    }));
}
