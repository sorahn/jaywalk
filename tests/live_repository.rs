use jaywalk::repository::Comparison;
use std::{path::Path, process::Command};
fn jj(root: &Path, args: &[&str]) -> String {
    let result = Command::new("jj")
        .current_dir(root)
        .args([
            "--config",
            "user.name=Viewer Test",
            "--config",
            "user.email=viewer@example.invalid",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}
#[test]
fn follows_stable_changes_without_following_the_checkout_or_snapshotting_files() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("viewer-live-{}-{unique}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    jj(&root, &["git", "init"]);
    std::fs::write(root.join("file.ts"), "const value = 1;\n").unwrap();
    jj(&root, &["describe", "-m", "baseline"]);
    jj(&root, &["bookmark", "create", "main"]);
    jj(&root, &["new", "-m", "first change"]);
    std::fs::write(root.join("file.ts"), "const value = 2;\n").unwrap();
    jj(&root, &["describe", "-m", "first change"]);
    let recorded = Comparison::load(&root, Some("main"), "@").unwrap();
    let change = recorded.revision(&recorded.tip).unwrap().change_id;
    jj(&root, &["bookmark", "create", "feature"]);
    jj(&root, &["new", "-m", "tip"]);
    jj(&root, &["bookmark", "set", "feature"]);
    jj(&root, &["edit", &change]);
    std::fs::write(root.join("file.ts"), "inserted\nconst value = 3;\n").unwrap();
    jj(&root, &["describe", "-m", "first change revised"]);
    let operation = Comparison::current_operation(&root).unwrap();
    let latest = Comparison::load(&root, Some("main"), "feature").unwrap();
    let rewritten = latest.revision(&change).unwrap();
    assert_ne!(rewritten.commit_id, recorded.tip);
    assert_ne!(
        latest.tip, rewritten.commit_id,
        "feature tip must not become the edited ancestor"
    );
    let comparison = latest.for_revision(&rewritten).unwrap();
    assert_eq!(
        comparison.contents(&comparison.files[0]).unwrap().1,
        "inserted\nconst value = 3;\n"
    );
    assert_eq!(
        recorded.contents(&recorded.files[0]).unwrap().1,
        "const value = 2;\n"
    );
    std::fs::write(root.join("file.ts"), "unsnapshotted\n").unwrap();
    assert_eq!(Comparison::current_operation(&root).unwrap(), operation);
    assert!(latest.revision("none()").is_err());
    assert!(latest.revision("all()").is_err());
    assert_eq!(Comparison::current_operation(&root).unwrap(), operation);
    std::fs::remove_dir_all(root).unwrap();
}
