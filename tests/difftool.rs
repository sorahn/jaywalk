//! Run real Git callbacks against the production CLI; no native window opens.
use std::{
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "jw-difftool-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = Self(root);
        fixture.git(&["init", "-b", "main"]);
        for name in ["one.txt", "nested/two space.txt", "quote'λ.txt"] {
            let path = fixture.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "before\n").unwrap();
        }
        fixture.git(&["add", "."]);
        fixture.git(&["commit", "-m", "base"]);
        let executable = env!("CARGO_BIN_EXE_jaywalk").replace('\'', "'\\''");
        fixture.git(&[
            "config",
            "difftool.probe.cmd",
            &format!("'{executable}' --inspect --diff \"$LOCAL\" \"$REMOTE\""),
        ]);
        for name in ["one.txt", "nested/two space.txt", "quote'λ.txt"] {
            std::fs::write(fixture.0.join(name), "after\n").unwrap();
        }
        fixture
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new("git");
        c.current_dir(&self.0)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid");
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        let output = self.command(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    fn git(&self, args: &[&str]) -> String {
        String::from_utf8(self.run(args).stdout)
            .unwrap()
            .trim()
            .into()
    }
    fn captures(&self) -> Vec<jaywalk::difftool::Capture> {
        std::fs::read_dir(self.0.join("config/jaywalk/runtime/difftool"))
            .unwrap()
            .map(|e| {
                serde_json::from_slice(
                    &std::fs::read(e.unwrap().path().join("capture.json")).unwrap(),
                )
                .unwrap()
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn production_callbacks_group_own_and_match_the_supplied_inputs() {
    let fixture = Fixture::new();
    let index = std::fs::read(fixture.0.join(".git/index")).unwrap();
    let output = fixture.git(&["difftool", "-y", "-t", "probe", "HEAD"]);
    assert!(output.contains("3 changed files"), "{output}");
    let captures = fixture.captures();
    assert_eq!(captures.len(), 1);
    let capture = &captures[0];
    assert_eq!((capture.received, capture.total), (3, 3));
    let paths = capture.paths(&fixture.0.join("config/jaywalk/runtime"));
    let comparison = capture
        .bind(jaywalk::external::ExternalContents::comparison(&paths[0], &paths[1]).unwrap())
        .unwrap();
    assert_eq!(comparison.base(), fixture.git(&["rev-parse", "HEAD"]));
    assert_eq!(comparison.tip(), "working tree");
    assert_eq!(
        comparison
            .history(&Default::default())
            .unwrap()
            .revisions
            .len(),
        2
    );
    std::fs::write(fixture.0.join("one.txt"), "later\n").unwrap();
    assert_eq!(
        comparison
            .read_path("one.txt", jaywalk::repository::FileSide::After)
            .unwrap(),
        "after\n"
    );
    assert_eq!(std::fs::read(fixture.0.join(".git/index")).unwrap(), index);
}
#[test]
fn concurrent_commands_have_distinct_captures_and_directory_mode_owns_symlink_optimization() {
    let fixture = Fixture::new();
    let mut a = fixture
        .command(&["difftool", "-y", "-t", "probe", "HEAD"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut b = fixture
        .command(&["difftool", "-y", "-t", "probe", "--dir-diff", "HEAD"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    let captures = fixture.captures();
    assert_eq!(captures.len(), 2);
    assert_ne!(captures[0].identity, captures[1].identity);
    for capture in captures {
        let paths = capture.paths(&fixture.0.join("config/jaywalk/runtime"));
        assert_eq!(
            std::fs::read_to_string(paths[1].join("one.txt")).unwrap(),
            "after\n"
        );
        assert!(
            !std::fs::symlink_metadata(paths[1].join("one.txt"))
                .unwrap()
                .is_symlink()
        );
    }
}

#[test]
fn moved_refs_leave_supplied_contents_available_without_false_endpoint_markers() {
    let fixture = Fixture::new();
    fixture.git(&["difftool", "-y", "-t", "probe", "HEAD"]);
    let capture = fixture.captures().remove(0);
    let paths = capture.paths(&fixture.0.join("config/jaywalk/runtime"));
    fixture.git(&["add", "one.txt", "nested/two space.txt", "quote'λ.txt"]);
    fixture.git(&["commit", "-m", "move head after capture"]);
    let comparison = capture
        .bind(jaywalk::external::ExternalContents::comparison(&paths[0], &paths[1]).unwrap())
        .unwrap();
    assert_ne!(comparison.base(), fixture.git(&["rev-parse", "HEAD"]));
    assert_eq!(
        comparison
            .read_path("one.txt", jaywalk::repository::FileSide::Before)
            .unwrap(),
        "before\n"
    );
    assert!(comparison.has_git_context());
}
#[test]
fn renamed_removed_empty_and_link_inputs_survive_the_standard_helper() {
    let fixture = Fixture::new();
    fixture.git(&["reset", "--hard", "HEAD"]);
    fixture.git(&["mv", "nested/two space.txt", "renamed.txt"]);
    std::fs::remove_file(fixture.0.join("quote'λ.txt")).unwrap();
    std::fs::write(fixture.0.join("empty.txt"), "").unwrap();
    fixture.git(&["add", "empty.txt"]);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("one.txt", fixture.0.join("link")).unwrap();
        fixture.git(&["add", "link"]);
    }
    fixture.git(&["difftool", "-y", "-t", "probe", "-M", "HEAD"]);
    let capture = fixture.captures().remove(0);
    let paths = capture.paths(&fixture.0.join("config/jaywalk/runtime"));
    let comparison = capture
        .bind(jaywalk::external::ExternalContents::comparison(&paths[0], &paths[1]).unwrap())
        .unwrap();
    let renamed = comparison
        .files()
        .iter()
        .find(|f| f.after_path() == "renamed.txt")
        .unwrap();
    assert_eq!(renamed.before_path(), "nested/two space.txt");
    assert_eq!(renamed.status(), "renamed");
    let empty = comparison
        .files()
        .iter()
        .find(|f| f.after_path() == "empty.txt")
        .unwrap();
    assert_eq!(empty.status(), "added");
    assert_eq!(
        comparison.contents(empty).unwrap(),
        (String::new(), String::new())
    );
    assert!(comparison.files().iter().any(|f| f.status() == "removed"));
    #[cfg(unix)]
    assert_eq!(
        comparison.catalog().find("link").unwrap().content_kind(),
        &jaywalk::repository_file::FileKind::Symlink
    );
}
#[test]
fn input_leases_keep_captures_until_the_last_window_releases_them() {
    let fixture = Fixture::new();
    fixture.git(&["difftool", "-y", "-t", "probe", "--dir-diff", "HEAD"]);
    let capture = fixture.captures().remove(0);
    let runtime = fixture.0.join("config/jaywalk/runtime");
    let first = capture.own(&runtime).unwrap();
    let second = capture.own(&runtime).unwrap();
    let paths = capture.paths(&runtime);
    drop(first);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(paths[0].exists());
    drop(second);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while paths[0].exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!paths[0].exists());
}

#[cfg(unix)]
#[test]
fn unavailable_difftool_parent_owns_inputs_independently_without_guessing_a_batch() {
    let fixture = Fixture::new();
    let script = fixture.0.join("external.sh");
    let executable = env!("CARGO_BIN_EXE_jaywalk").replace('\'', "'\\''");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nexec '{executable}' --inspect --diff \"$2\" \"$5\"\n"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = fixture
        .command(&["diff", "--ext-diff"])
        .env("GIT_EXTERNAL_DIFF", script)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let captures = fixture.captures();
    assert_eq!(captures.len(), 3);
    for capture in captures {
        assert!(!capture.grouped);
        assert!(capture.intent.is_none());
        let paths = capture.paths(&fixture.0.join("config/jaywalk/runtime"));
        let comparison = capture
            .bind(jaywalk::external::ExternalContents::comparison(&paths[0], &paths[1]).unwrap())
            .unwrap();
        assert_eq!(comparison.files().len(), 1);
        assert_eq!(
            comparison.contents(&comparison.files()[0]).unwrap(),
            ("before\n".into(), "after\n".into())
        );
    }
}
