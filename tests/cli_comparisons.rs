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
                .files()
                .iter()
                .find(|file| file.after_path() == name)
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
    assert_eq!(comparison.files().len(), 3);
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
    assert_eq!(content(&comparison, "after.txt").1, "\t漢\n");
    let binary =
        ExternalContents::comparison(&fixture.0.join("before.txt"), &fixture.0.join("after.txt"))
            .unwrap();
    assert!(
        binary
            .contents(&binary.files()[0])
            .unwrap_err()
            .contains("Binary")
    );
    fixture.write("after.txt", "later text");
    assert!(
        binary
            .contents(&binary.files()[0])
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
    fixture.git(&["commit", "-m", "initial\n\nDetailed message body."]);
    let initial = fixture.git(&["rev-parse", "HEAD"]);
    let metadata = Comparison::load(&fixture.0, None, "@")
        .unwrap()
        .history(&Default::default())
        .unwrap();
    let commit = &metadata.revisions[1];
    assert_eq!(commit.summary(), "initial");
    assert!(commit.description.contains("\n\nDetailed message body."));
    assert!(!commit.author.is_empty());
    assert!(!commit.timestamp.is_empty());
    fixture.write("a file.txt", "staged\n");
    fixture.git(&["add", "."]);
    fixture.write("a file.txt", "unstaged\n");
    fixture.write("new file.txt", "new\n");
    std::fs::remove_file(fixture.0.join("deleted.txt")).unwrap();
    let status = fixture.git(&["status", "--porcelain=v1"]);
    let comparison = Comparison::load(&fixture.0, None, "@").unwrap();
    assert!(!comparison.supports_evolution());
    assert_eq!(comparison.files().len(), 3);
    assert_eq!(comparison.identity(), comparison.clone().identity());
    let next_capture = Comparison::load(&fixture.0, None, "@").unwrap();
    assert_ne!(comparison.identity(), next_capture.identity());
    // Unchanged paths use their pinned base object, not later filesystem edits.
    fixture.write("unchanged.txt", "changed after capture\n");
    let unchanged = comparison.catalog().find("unchanged.txt").unwrap();
    assert!(unchanged.kind().is_none());
    assert_eq!(
        comparison.read_file(unchanged).unwrap(),
        ("stable\n".into(), "stable\n".into())
    );
    assert!(
        comparison
            .catalog()
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
    assert_eq!(history.revisions.len(), 2);
    let working = &history.revisions[0];
    assert!(working.is_virtual_working_copy());
    assert_eq!(working.parents.as_slice(), std::slice::from_ref(&initial));
    assert!(!working.empty);
    let current = comparison.for_revision(working).unwrap();
    assert_eq!(
        content(&current, "a file.txt"),
        ("original\n".into(), "unstaged\n".into())
    );
    assert_eq!(
        content(&current, "new file.txt"),
        ("".into(), "new\n".into())
    );
    assert_eq!(
        content(&current, "deleted.txt"),
        ("deleted\n".into(), "".into())
    );
    let roots = jaywalk::history::HistoryQuery::ancestors(vec![initial.clone()]).unwrap();
    assert_eq!(comparison.history(&roots).unwrap().revisions.len(), 1);
    let patch = comparison.for_revision(&history.revisions[1]).unwrap();
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
        3
    );
    let clean = committed.history(&Default::default()).unwrap();
    assert!(clean.revisions[0].is_virtual_working_copy());
    assert!(clean.revisions[0].empty);
    assert!(
        committed
            .for_revision(&clean.revisions[0])
            .unwrap()
            .files()
            .is_empty()
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
    use jaywalk::repository_file::ChangeKind;
    for text in ["", "content\n"] {
        fixture.write("file.txt", text);
        for (left, right, expected) in [
            (
                Path::new("/dev/null").to_path_buf(),
                fixture.0.join("file.txt"),
                ChangeKind::Added,
            ),
            (
                fixture.0.join("file.txt"),
                Path::new("/dev/null").to_path_buf(),
                ChangeKind::Removed,
            ),
        ] {
            let comparison = ExternalContents::comparison(&left, &right).unwrap();
            assert_eq!(comparison.files().len(), 1);
            assert_eq!(comparison.files()[0].kind(), &expected);
            let (before, after) = content(&comparison, "file.txt");
            assert_eq!(before + &after, text);
            let reverse = comparison.swapped().unwrap();
            assert_eq!(
                reverse.files()[0].kind(),
                if expected == ChangeKind::Added {
                    &ChangeKind::Removed
                } else {
                    &ChangeKind::Added
                }
            );
            assert_eq!(comparison.overview().files.len(), 1);
        }
    }
    let absent =
        ExternalContents::comparison(Path::new("/dev/null"), Path::new("/dev/null")).unwrap();
    assert!(absent.files().is_empty());
    assert_eq!(absent.catalog().entries().len(), 0);
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
    assert_eq!(comparison.files()[0].status(), "modified");
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
    assert_eq!(comparison.root().file_name().unwrap(), "right-link");
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
    assert_eq!(comparison.files().len(), 2);
    for path in ["link-to-file", "file-to-link"] {
        let entry = comparison.catalog().find(path).unwrap();
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

#[test]
fn external_limit_retains_oversized_files_and_streams_equality() {
    use jaywalk::{repository::FileSide, text_content::TextLimit};
    let f = Fixture::new();
    let mib = 1_048_576;
    for (name, size) in [("below", mib - 1), ("exact", mib), ("above", mib + 1)] {
        f.write(&format!("left/{name}"), "old");
        f.write(&format!("right/{name}"), &"x".repeat(size));
    }
    // Sparse, oversized equal files are discovered without allocating their contents.
    for side in ["left", "right"] {
        std::fs::File::create(f.0.join(side).join("large-equal"))
            .unwrap()
            .set_len(64 * 1024 * 1024)
            .unwrap();
    }
    let comparison = ExternalContents::comparison_with_limit(
        &f.0.join("left"),
        &f.0.join("right"),
        TextLimit::from_mib(1).unwrap(),
    )
    .unwrap();
    assert_eq!(comparison.files().len(), 3);
    for name in ["below", "exact"] {
        assert!(comparison.read_path(name, FileSide::After).is_ok());
    }
    let notice = comparison.read_path("above", FileSide::After).unwrap_err();
    assert!(notice.contains("1048577 bytes"), "{notice}");
    assert!(notice.contains("1 MiB"), "{notice}");
    let overview = comparison.overview();
    assert!(
        overview
            .files
            .iter()
            .find(|file| file.file.after_path() == "above")
            .unwrap()
            .notice
            .as_deref()
            .unwrap()
            .contains("text limit")
    );
}

#[test]
fn git_limits_worktree_and_recorded_blobs_and_refreshes_policy() {
    use jaywalk::{repository::FileSide, text_content::TextLimit};
    let f = Fixture::new();
    f.git(&["init", "-b", "main"]);
    f.write("file.txt", "old");
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "base"]);
    let base = f.git(&["rev-parse", "HEAD"]);
    f.write("file.txt", &"x".repeat(1_048_577));
    let small = TextLimit::from_mib(1).unwrap();
    let work = Comparison::load_with_limit(&f.0, Some(&base), "@", small).unwrap();
    assert!(
        work.read_path("file.txt", FileSide::After)
            .unwrap_err()
            .contains("1048577 bytes")
    );
    assert_eq!(work.files().len(), 1);
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "large"]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    let limited = Comparison::load_with_limit(&f.0, Some(&base), &tip, small).unwrap();
    let raised =
        Comparison::load_with_limit(&f.0, Some(&base), &tip, TextLimit::from_mib(2).unwrap())
            .unwrap();
    assert_ne!(limited.identity(), raised.identity());
    assert!(
        limited
            .read_path("file.txt", FileSide::After)
            .unwrap_err()
            .contains("text limit")
    );
    assert_eq!(
        raised.read_path("file.txt", FileSide::After).unwrap().len(),
        1_048_577
    );
    assert!(limited.read_path("file.txt", FileSide::After).is_err());
    #[cfg(unix)]
    {
        // A refused before-side text read must not hide the captured new kind.
        std::fs::remove_file(f.0.join("file.txt")).unwrap();
        std::os::unix::fs::symlink("../target", f.0.join("file.txt")).unwrap();
        let comparison = Comparison::load_with_limit(&f.0, Some(&tip), "@", small).unwrap();
        let entry = comparison.catalog().find("file.txt").unwrap();
        assert_eq!(
            entry.content_kind(),
            &jaywalk::repository_file::FileKind::Symlink
        );
        assert_eq!(
            entry.type_change_notice().as_deref(),
            Some("File type changed: regular file → symlink")
        );
        assert!(
            comparison
                .read_file(entry)
                .unwrap_err()
                .contains("text limit")
        );
    }
}

#[cfg(unix)]
#[test]
fn git_and_jj_catalogs_preserve_side_kinds_independently_of_text() {
    use jaywalk::repository_file::FileKind;
    let f = Fixture::new();
    f.git(&["init", "-b", "main"]);
    std::os::unix::fs::symlink("../target", f.0.join("to-file")).unwrap();
    std::os::unix::fs::symlink("../gone", f.0.join("deleted-link")).unwrap();
    f.write("to-link", "../target");
    f.write("empty-deleted", "");
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "initial"]);
    let object = f.git(&["rev-parse", "HEAD"]);
    f.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{object},deleted-module"),
    ]);
    f.git(&["commit", "-m", "module"]);
    let base = f.git(&["rev-parse", "HEAD"]);
    std::fs::remove_file(f.0.join("to-file")).unwrap();
    f.write("to-file", "../target");
    std::fs::remove_file(f.0.join("to-link")).unwrap();
    std::os::unix::fs::symlink("../target", f.0.join("to-link")).unwrap();
    std::fs::remove_file(f.0.join("deleted-link")).unwrap();
    std::fs::remove_file(f.0.join("empty-deleted")).unwrap();
    f.write("empty-added", "");
    let check = |comparison: Comparison| {
        for (name, kind, notice) in [
            (
                "to-file",
                FileKind::Regular,
                "File type changed: symlink → regular file",
            ),
            (
                "to-link",
                FileKind::Symlink,
                "File type changed: regular file → symlink",
            ),
        ] {
            let entry = comparison.catalog().find(name).unwrap();
            assert_eq!(entry.content_kind(), &kind);
            assert_eq!(entry.type_change_notice().as_deref(), Some(notice));
            assert_eq!(
                comparison.read_file(entry).unwrap(),
                ("../target".into(), "../target".into())
            );
        }
        let removed = comparison.catalog().find("deleted-link").unwrap();
        assert_eq!(removed.content_kind(), &FileKind::Symlink);
        assert!(removed.type_change_notice().is_none());
        assert_eq!(
            comparison.read_file(removed).unwrap(),
            ("../gone".into(), "".into())
        );
        let module = comparison.catalog().find("deleted-module").unwrap();
        assert_eq!(module.content_kind(), &FileKind::Submodule);
        assert!(
            comparison
                .read_file(module)
                .unwrap_err()
                .contains("Submodule")
        );
        let empty = comparison.catalog().find("empty-added").unwrap();
        if empty.status() == Some("renamed") {
            // Git can report a recorded empty-file rename. Preserve its result;
            // unstaged inference deliberately leaves empty candidates separate.
            assert_eq!(empty.before_path(), "empty-deleted");
            assert_eq!(comparison.read_file(empty).unwrap(), ("".into(), "".into()));
            assert!(empty.type_change_notice().is_none());
        } else {
            for (path, status) in [("empty-added", "added"), ("empty-deleted", "removed")] {
                let entry = comparison.catalog().find(path).unwrap();
                assert_eq!(entry.status(), Some(status));
                assert!(entry.type_change_notice().is_none());
                assert_eq!(comparison.read_file(entry).unwrap(), ("".into(), "".into()));
            }
        }
        let overview = comparison.overview();
        assert!(
            overview
                .files
                .iter()
                .find(|file| file.file.after_path() == "to-file")
                .unwrap()
                .notice
                .as_deref()
                .unwrap()
                .contains("symlink → regular file")
        );
    };
    check(Comparison::load(&f.0, Some(&base), "@").unwrap());
    f.git(&["add", "-A"]);
    f.git(&["commit", "-m", "changes"]);
    let tip = f.git(&["rev-parse", "HEAD"]);
    check(Comparison::load(&f.0, Some(&base), &tip).unwrap());
    let result = Command::new("jj")
        .current_dir(&f.0)
        .args(["git", "init", "--git-repo", "."])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    check(Comparison::load(&f.0, Some(&base), &tip).unwrap());
}

#[test]
fn virtual_git_node_loads_current_changes_from_a_pinned_commit_view() {
    let f = Fixture::new();
    f.git(&["init", "-b", "main"]);
    f.write("staged.txt", "original staged\n");
    f.write("unstaged.txt", "original unstaged\n");
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "initial"]);
    let initial = f.git(&["rev-parse", "HEAD"]);
    let pinned = Comparison::load(&f.0, Some(&initial), &initial).unwrap();
    f.write("staged.txt", "staged edit\n");
    f.git(&["add", "staged.txt"]);
    f.write("unstaged.txt", "unstaged edit\n");
    f.write("new.txt", "untracked\n");
    let status = f.git(&["status", "--porcelain=v1"]);
    let history = pinned.history(&Default::default()).unwrap();
    let working = &history.revisions[0];
    assert!(working.is_virtual_working_copy());
    assert_eq!(history.graph.len(), history.revisions.len());
    let comparison = pinned.for_revision(working).unwrap();
    assert_eq!(comparison.files().len(), 3);
    assert_eq!(
        content(&comparison, "staged.txt"),
        ("original staged\n".into(), "staged edit\n".into())
    );
    assert_eq!(
        content(&comparison, "unstaged.txt"),
        ("original unstaged\n".into(), "unstaged edit\n".into())
    );
    assert_eq!(
        content(&comparison, "new.txt"),
        ("".into(), "untracked\n".into())
    );
    assert_eq!(f.git(&["status", "--porcelain=v1"]), status);
    assert_eq!(f.git(&["rev-parse", "HEAD"]), initial);
    // A virtual node follows current HEAD, rather than treating its old parent
    // or the pinned review base as the base of a future working-tree capture.
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "next"]);
    f.write("staged.txt", "later edit\n");
    let refreshed = pinned.for_revision(working).unwrap();
    assert_eq!(
        content(&refreshed, "staged.txt"),
        ("staged edit\n".into(), "later edit\n".into())
    );
    assert_eq!(refreshed.files().len(), 1);
}

#[test]
fn git_blob_stream_recovers_after_rejected_files_and_preserves_pinned_data() {
    use jaywalk::{repository::FileSide, text_content::TextLimit};
    let f = Fixture::new();
    f.git(&["init", "-b", "main"]);
    f.write("a-large.txt", &"x".repeat(1_048_577));
    std::fs::write(f.0.join("b-binary.txt"), [0xff, 0xfe]).unwrap();
    f.write("c-valid.txt", "before λ\n");
    f.write("d-empty.txt", "");
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "baseline"]);
    let base = f.git(&["rev-parse", "HEAD"]);
    f.write("a-large.txt", "small now\n");
    f.write("b-binary.txt", "text now\n");
    f.write("c-valid.txt", "after λ\n");
    f.write("d-empty.txt", "added\n");
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "changed"]);
    let comparison =
        Comparison::load_with_limit(&f.0, Some(&base), "HEAD", TextLimit::from_mib(1).unwrap())
            .unwrap();
    assert!(
        comparison
            .read_path("a-large.txt", FileSide::Before)
            .unwrap_err()
            .contains("text limit")
    );
    assert!(
        comparison
            .read_path("b-binary.txt", FileSide::Before)
            .is_err()
    );
    assert_eq!(
        content(&comparison, "c-valid.txt"),
        ("before λ\n".into(), "after λ\n".into())
    );
    assert_eq!(
        content(&comparison, "d-empty.txt"),
        ("".into(), "added\n".into())
    );
    f.write("c-valid.txt", "later edit\n");
    assert_eq!(content(&comparison, "c-valid.txt").1, "after λ\n");
}

#[cfg(unix)]
#[test]
fn external_executable_changes_are_visible_without_a_text_change() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    for (side, mode) in [("left", 0o644), ("right", 0o755)] {
        fixture.write(&format!("{side}/script.sh"), "#!/bin/sh\nexit 0\n");
        std::fs::set_permissions(
            fixture.0.join(side).join("script.sh"),
            std::fs::Permissions::from_mode(mode),
        )
        .unwrap();
    }
    let comparison =
        ExternalContents::comparison(&fixture.0.join("left"), &fixture.0.join("right")).unwrap();
    assert_eq!(comparison.files().len(), 1);
    assert_eq!(comparison.files()[0].status(), "modified");
    let file = comparison.catalog().find("script.sh").unwrap();
    assert_eq!(
        file.metadata_change_notice().as_deref(),
        Some("File mode changed: 100644 → 100755")
    );
    let (before, after) = comparison.read_file(file).unwrap();
    assert_eq!(before, after);
    assert_eq!(
        comparison.overview().files[0].notice,
        file.metadata_change_notice()
    );
    let swapped = comparison.swapped().unwrap();
    assert_eq!(
        swapped
            .catalog()
            .find("script.sh")
            .unwrap()
            .metadata_change_notice()
            .as_deref(),
        Some("File mode changed: 100755 → 100644")
    );
    std::fs::set_permissions(
        fixture.0.join("right/script.sh"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let same_mode =
        ExternalContents::comparison(&fixture.0.join("left"), &fixture.0.join("right")).unwrap();
    assert!(same_mode.files().is_empty());
}

#[test]
fn external_captures_survive_input_mutation_deletion_swap_and_navigation() {
    use jaywalk::{navigation::History, repository::FileSide};
    let fixture = Fixture::new();
    for (path, text) in [
        ("left/file.txt", "before\n"),
        ("right/file.txt", "after\n"),
        ("left/same.txt", "unchanged\n"),
        ("right/same.txt", "unchanged\n"),
    ] {
        fixture.write(path, text);
    }
    let comparison =
        ExternalContents::comparison(&fixture.0.join("left"), &fixture.0.join("right")).unwrap();
    let original_identity = comparison.identity();
    let patch = comparison.overview().files[0].patch.clone();
    let clone = comparison.clone();
    let mut navigation = History::new(4);
    navigation.observe(comparison.identity(), comparison.clone());
    fixture.write("left/file.txt", "overwritten");
    fixture.write("right/same.txt", "was unchanged");
    std::fs::remove_dir_all(fixture.0.join("left")).unwrap();
    std::fs::remove_dir_all(fixture.0.join("right")).unwrap();
    fixture.write("other-before", "other before");
    fixture.write("other-after", "other after");
    let other = ExternalContents::comparison(
        &fixture.0.join("other-before"),
        &fixture.0.join("other-after"),
    )
    .unwrap();
    navigation.observe(other.identity(), other);
    drop(comparison);
    assert_eq!(clone.identity(), original_identity);
    assert_eq!(
        content(&clone, "file.txt"),
        ("before\n".into(), "after\n".into())
    );
    assert_eq!(
        clone.read_path("same.txt", FileSide::After).unwrap(),
        "unchanged\n"
    );
    assert_eq!(clone.overview().files[0].patch, patch);
    let swapped = clone.swapped().unwrap();
    assert_eq!(
        content(&swapped, "file.txt"),
        ("after\n".into(), "before\n".into())
    );
    assert_eq!(swapped.catalog().entries().len(), 2);
    assert_eq!(swapped.swapped().unwrap().identity(), original_identity);
    drop(clone);
    let back = navigation.step(false).unwrap().value;
    assert_eq!(back.overview().files[0].patch, patch);
    assert_eq!(content(&back, "file.txt").1, "after\n");
    assert!(navigation.step(true).is_some());
}

#[cfg(unix)]
#[test]
fn external_capture_keeps_symlink_targets_and_modes_after_sources_disappear() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::new();
    fixture.write("left/script", "same");
    fixture.write("right/script", "same");
    symlink("missing-before", fixture.0.join("left/link")).unwrap();
    symlink("missing-after", fixture.0.join("right/link")).unwrap();
    std::fs::set_permissions(
        fixture.0.join("left/script"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    std::fs::set_permissions(
        fixture.0.join("right/script"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let comparison =
        ExternalContents::comparison(&fixture.0.join("left"), &fixture.0.join("right")).unwrap();
    std::fs::remove_dir_all(fixture.0.join("left")).unwrap();
    std::fs::remove_dir_all(fixture.0.join("right")).unwrap();
    assert_eq!(
        content(&comparison, "link"),
        ("missing-before".into(), "missing-after".into())
    );
    assert!(
        comparison
            .catalog()
            .find("script")
            .unwrap()
            .metadata_change_notice()
            .unwrap()
            .contains("100644 → 100755")
    );
    let swapped = comparison.swapped().unwrap();
    assert_eq!(
        content(&swapped, "link"),
        ("missing-after".into(), "missing-before".into())
    );
    assert!(
        swapped
            .catalog()
            .find("script")
            .unwrap()
            .metadata_change_notice()
            .unwrap()
            .contains("100755 → 100644")
    );
}

#[test]
fn oversized_external_snapshots_keep_equality_and_notices_after_inputs_change() {
    use jaywalk::{repository::FileSide, text_content::TextLimit};
    use std::io::{Seek, SeekFrom, Write};
    let fixture = Fixture::new();
    for side in ["left", "right"] {
        fixture.write(&format!("{side}/large"), "");
        std::fs::File::options()
            .write(true)
            .open(fixture.0.join(side).join("large"))
            .unwrap()
            .set_len(2 * 1_048_576)
            .unwrap();
    }
    let mut after = std::fs::File::options()
        .write(true)
        .open(fixture.0.join("right/large"))
        .unwrap();
    after.seek(SeekFrom::End(-1)).unwrap();
    after.write_all(b"x").unwrap();
    drop(after);
    let comparison = ExternalContents::comparison_with_limit(
        &fixture.0.join("left"),
        &fixture.0.join("right"),
        TextLimit::from_mib(1).unwrap(),
    )
    .unwrap();
    assert_eq!(comparison.files().len(), 1);
    let notice = comparison.read_path("large", FileSide::After).unwrap_err();
    fixture.write("right/large", "small now");
    std::fs::remove_file(fixture.0.join("left/large")).unwrap();
    assert_eq!(
        comparison.read_path("large", FileSide::After).unwrap_err(),
        notice
    );
    assert_eq!(
        comparison
            .swapped()
            .unwrap()
            .read_path("large", FileSide::Before)
            .unwrap_err(),
        notice
    );
    assert!(
        comparison.overview().files[0]
            .notice
            .as_ref()
            .unwrap()
            .contains("2097152 bytes")
    );
}
