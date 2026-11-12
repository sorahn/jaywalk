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
    assert!(rewritten.working_copy);
    assert!(!rewritten.empty);
    assert!(!latest.revision(&latest.tip).unwrap().working_copy);
    let original = recorded.revision(&recorded.tip).unwrap();
    assert!(
        original.working_copy,
        "working-copy state belongs to the pinned operation"
    );
    assert_eq!(original.description, "first change\n");
    assert_eq!(original.summary(), "first change · working copy · changes");
    assert_ne!(
        latest.tip, rewritten.commit_id,
        "feature tip must not become the edited ancestor"
    );
    let refreshed = jaywalk::live_refresh::RecordedRefresh::load(
        &root,
        Some("main"),
        "feature",
        Some(&jaywalk::live_refresh::FollowedChange {
            change_id: change.clone(),
            working_copy: true,
        }),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(refreshed.branch.tip, latest.tip);
    assert_eq!(
        refreshed.revision.as_ref().unwrap().commit_id,
        rewritten.commit_id
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

#[test]
fn workspace_following_survives_an_abandoned_working_copy_and_keeps_explicit_targets_explicit() {
    use jaywalk::live_refresh::{FollowedChange, RecordedRefresh};
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "viewer-replaced-workspace-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    jj(&root, &["git", "init"]);
    std::fs::write(root.join("file.ts"), "const value = 1;\n").unwrap();
    jj(&root, &["describe", "-m", "baseline"]);
    jj(&root, &["bookmark", "create", "main"]);
    jj(&root, &["new", "-m", "feature"]);
    std::fs::write(root.join("file.ts"), "const value = 2;\n").unwrap();
    jj(&root, &["describe", "-m", "feature"]);
    jj(&root, &["bookmark", "create", "feature"]);
    jj(&root, &["new"]);
    let initial =
        RecordedRefresh::load(&root, Some("main"), "@", None, &Default::default()).unwrap();
    let old = initial.branch.revision(&initial.branch.tip).unwrap();
    assert!(old.working_copy);
    let selected = FollowedChange {
        change_id: old.change_id.clone(),
        working_copy: true,
    };
    std::fs::write(root.join("file.ts"), "const value = 3;\n").unwrap();
    jj(
        &root,
        &["squash", "--into", "@-", "--use-destination-message"],
    );
    let operation = Comparison::current_operation(&root).unwrap();
    let latest = RecordedRefresh::load(
        &root,
        Some("main"),
        "@",
        Some(&selected),
        &Default::default(),
    )
    .unwrap();
    let followed = latest.revision.as_ref().unwrap();
    assert_ne!(latest.branch.tip, initial.branch.tip);
    assert_eq!(followed.commit_id, latest.branch.tip);
    assert!(followed.working_copy);
    assert_ne!(followed.change_id, old.change_id);
    assert_eq!(latest.branch.operation, operation);
    assert_eq!(
        latest.branch.contents(&latest.branch.files[0]).unwrap().1,
        "const value = 3;\n"
    );
    let explicit =
        RecordedRefresh::load(&root, Some("main"), "feature", None, &Default::default()).unwrap();
    assert_ne!(explicit.branch.tip, latest.branch.tip);
    let failure = match RecordedRefresh::load(
        &root,
        Some("main"),
        &old.change_id,
        None,
        &Default::default(),
    ) {
        Err(failure) => failure,
        Ok(_) => panic!("an explicit abandoned target must not silently follow @"),
    };
    assert_eq!(failure.operation.as_deref(), Some(operation.as_str()));
    let missing_selection = FollowedChange {
        change_id: old.change_id.clone(),
        working_copy: false,
    };
    let failure = match RecordedRefresh::load(
        &root,
        Some("main"),
        "@",
        Some(&missing_selection),
        &Default::default(),
    ) {
        Err(failure) => failure,
        Ok(_) => {
            panic!("an abandoned historical selection must not silently become another commit")
        }
    };
    assert!(
        failure
            .message
            .contains("selected change is no longer visible")
    );
    std::fs::write(root.join("file.ts"), "unsnapshotted\n").unwrap();
    let read_only =
        RecordedRefresh::load(&root, Some("main"), "@", None, &Default::default()).unwrap();
    assert_eq!(
        read_only
            .branch
            .contents(&read_only.branch.files[0])
            .unwrap()
            .1,
        "const value = 3;\n"
    );
    assert_eq!(Comparison::current_operation(&root).unwrap(), operation);
    std::fs::remove_dir_all(root).unwrap();
}
