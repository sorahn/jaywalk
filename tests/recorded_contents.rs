//! Operation-pinned jj content reads must stay bounded and avoid per-file processes.
use jaywalk::{
    repository::{Comparison, FileSide},
    text_content::TextLimit,
};
use std::{path::Path, process::Command};

fn jj(root: &Path, args: &[&str]) -> String {
    let output = Command::new("jj")
        .current_dir(root)
        .args([
            "--config",
            "user.name=Fixture",
            "--config",
            "user.email=fixture@example.invalid",
            "--config",
            "snapshot.max-new-file-size=10485760",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn overview_batch_child() {
    let Some(root) = std::env::var_os("JAYWALK_BATCH_TEST_ROOT") else {
        return;
    };
    let comparison = Comparison::load(Path::new(&root), Some("baseline"), "@").unwrap();
    let overview = comparison.overview();
    assert_eq!(overview.files.len(), 32);
    for file in overview.files {
        assert!(file.notice.is_none(), "{:?}", file.notice);
        assert!(file.patch.contains("-before\n+after\n"));
    }
}

#[cfg(unix)]
#[test]
fn overview_reads_many_files_with_one_git_stream_and_two_tree_queries() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    // Deliberately non-colocated: Git must use jj's captured object directory.
    jj(&root, &["git", "init", "--no-colocate"]);
    for i in 0..32 {
        std::fs::write(root.join(format!("file-{i}.txt")), "before\n").unwrap();
    }
    jj(&root, &["bookmark", "create", "baseline"]);
    jj(&root, &["new", "baseline"]);
    for i in 0..32 {
        std::fs::write(root.join(format!("file-{i}.txt")), "after\n").unwrap();
    }
    jj(&root, &["status"]);
    let tools = fixture.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    let log = fixture.path().join("commands.log");
    for program in ["jj", "git"] {
        let real = Command::new("which").arg(program).output().unwrap();
        assert!(real.status.success());
        let real = String::from_utf8(real.stdout).unwrap();
        let quoted = format!("'{}'", real.trim().replace('\'', "'\\''"));
        let script = tools.join(program);
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
case " $* " in
 *" file list "*) printf 'tree\n' >> "$JAYWALK_BATCH_TEST_LOG" ;;
 *" git root "*) printf 'git-root\n' >> "$JAYWALK_BATCH_TEST_LOG" ;;
 *" cat-file --batch "*) printf 'batch\n' >> "$JAYWALK_BATCH_TEST_LOG" ;;
 *" cat-file "*) printf 'unbatched\n' >> "$JAYWALK_BATCH_TEST_LOG" ;;
esac
exec {quoted} "$@"
"#
            ),
        )
        .unwrap();
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "overview_batch_child", "--nocapture"])
        .env("JAYWALK_BATCH_TEST_ROOT", &root)
        .env("JAYWALK_BATCH_TEST_LOG", &log)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let log = std::fs::read_to_string(log).unwrap();
    assert_eq!(
        log.lines().filter(|line| *line == "tree").count(),
        2,
        "{log}"
    );
    assert_eq!(
        log.lines().filter(|line| *line == "git-root").count(),
        1,
        "{log}"
    );
    assert_eq!(
        log.lines().filter(|line| *line == "batch").count(),
        1,
        "{log}"
    );
    assert!(!log.contains("unbatched"), "{log}");
}

#[test]
fn limited_jj_overview_recovers_after_large_and_binary_blobs_and_keeps_pinned_conflicts() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    jj(root, &["git", "init", "--no-colocate"]);
    std::fs::write(root.join("a-large.txt"), vec![b'x'; 1024 * 1024 + 1]).unwrap();
    std::fs::write(root.join("b-binary.txt"), [0, 255]).unwrap();
    std::fs::write(root.join("c-valid 漢\n.txt"), "before\n").unwrap();
    std::fs::write(root.join("conflict.txt"), "base\n").unwrap();
    jj(root, &["bookmark", "create", "baseline"]);
    jj(root, &["new", "baseline"]);
    for path in ["a-large.txt", "b-binary.txt", "c-valid 漢\n.txt"] {
        std::fs::write(root.join(path), "after\n").unwrap();
    }
    jj(root, &["status"]);
    let comparison =
        Comparison::load_with_limit(root, Some("baseline"), "@", TextLimit::from_mib(1).unwrap())
            .unwrap();
    let overview = comparison.overview();
    let large_notice = overview
        .files
        .iter()
        .find(|f| f.file.after_path() == "a-large.txt")
        .unwrap()
        .notice
        .as_ref()
        .unwrap();
    assert!(large_notice.contains("text limit"), "{large_notice}");
    assert!(
        overview
            .files
            .iter()
            .find(|f| f.file.after_path() == "b-binary.txt")
            .unwrap()
            .notice
            .as_ref()
            .unwrap()
            .contains("Binary")
    );
    let valid = overview
        .files
        .iter()
        .find(|f| f.file.after_path() == "c-valid 漢\n.txt")
        .unwrap();
    assert!(valid.notice.is_none());
    assert!(valid.patch.contains("-before\n+after\n"));
    let original = comparison.clone();
    jj(root, &["new", "baseline"]);
    std::fs::write(root.join("conflict.txt"), "left\n").unwrap();
    jj(root, &["bookmark", "create", "left"]);
    jj(root, &["new", "baseline"]);
    std::fs::write(root.join("conflict.txt"), "right\n").unwrap();
    jj(root, &["bookmark", "create", "right"]);
    jj(root, &["new", "left", "right"]);
    let conflict = Comparison::load(root, Some("baseline"), "@").unwrap();
    let markers = conflict.read_path("conflict.txt", FileSide::After).unwrap();
    assert!(markers.contains("left") && markers.contains("right") && markers.contains("<<<<<<<"));
    std::fs::write(root.join("conflict.txt"), "resolved later\n").unwrap();
    jj(root, &["status"]);
    assert_eq!(
        conflict.read_path("conflict.txt", FileSide::After).unwrap(),
        markers
    );
    assert_eq!(
        original
            .read_path("c-valid 漢\n.txt", FileSide::After)
            .unwrap(),
        "after\n"
    );
}
