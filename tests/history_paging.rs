//! Only disposable synthetic repositories are mutated by these tests.
use jaywalk::{
    accounts::Control,
    history::{HistoryQuery, HistorySource, SearchQuery},
};
use std::{path::Path, process::Command, sync::Arc, time::Duration};
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture Author")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture Author")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-11-12T12:00:00+0000")
        .env("GIT_COMMITTER_DATE", "2026-11-12T12:00:00+0000")
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
fn fixture() -> (tempfile::TempDir, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    let tree = git(root.path(), &["mktree"]);
    let mut commits: Vec<String> = vec![];
    for n in 0..12 {
        let message = if n == 0 {
            "oldest café needle".to_owned()
        } else {
            format!("ordinary {n}")
        };
        let mut args = vec!["commit-tree", &tree, "-m", &message];
        if let Some(parent) = commits.last() {
            args.extend(["-p", parent]);
        }
        commits.push(git(root.path(), &args));
    }
    git(
        root.path(),
        &["update-ref", "refs/heads/main", commits.last().unwrap()],
    );
    (root, commits)
}
fn control() -> Control {
    Control::new(Duration::from_secs(20))
}
#[test]
fn pages_keep_pinned_roots_graph_prefix_and_working_row_without_duplicates() {
    let (root, commits) = fixture();
    let source = Arc::new(HistorySource::git(root.path(), &HistoryQuery::default()).unwrap());
    let first = source.first_page(3, &control()).unwrap();
    assert_eq!(first.revisions.len(), 3);
    assert!(first.revisions[0].is_virtual_working_copy());
    assert!(first.has_more());
    let tree = git(root.path(), &["mktree"]);
    let new = git(
        root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            commits.last().unwrap(),
            "-m",
            "new after capture",
        ],
    );
    git(root.path(), &["update-ref", "refs/heads/main", &new]);
    let second = first.load_more(3, &control()).unwrap();
    assert_eq!(second.revisions.len(), 6);
    for (old, new) in first.revisions.iter().zip(&second.revisions) {
        assert_eq!(old.commit_id, new.commit_id);
    }
    for (old, new) in first.graph.iter().zip(&second.graph) {
        assert_eq!((old.lane, old.color), (new.lane, new.color));
    }
    let mut history = second;
    while history.has_more() {
        history = history.load_more(3, &control()).unwrap();
    }
    assert_eq!(history.revisions.len(), 13);
    assert_eq!(
        history
            .revisions
            .iter()
            .filter(|r| r.is_virtual_working_copy())
            .count(),
        1
    );
    assert!(!history.revisions.iter().any(|r| r.commit_id == new));
    assert_eq!(history.revisions.last().unwrap().commit_id, commits[0]);
    let unique: std::collections::HashSet<_> =
        history.revisions.iter().map(|r| &r.commit_id).collect();
    assert_eq!(unique.len(), history.revisions.len());
}
#[test]
fn search_reaches_older_history_pages_filters_and_direct_context() {
    let (root, commits) = fixture();
    let source = Arc::new(HistorySource::git(root.path(), &HistoryQuery::default()).unwrap());
    let first = source.first_page(3, &control()).unwrap();
    assert!(!first.revisions.iter().any(|r| r.commit_id == commits[0]));
    for text in [
        "café",
        &commits[0][..10],
        "author:\"Fixture Author\" after:2026-11-12 before:2026-11-13 needle",
    ] {
        let page = source
            .search(&SearchQuery::parse(text).unwrap(), 0, 2, &control())
            .unwrap();
        assert_eq!(page.matches.len(), 1);
        assert_eq!(page.matches[0].commit_id, commits[0]);
    }
    let query = SearchQuery::parse("ordinary").unwrap();
    let a = source.search(&query, 0, 2, &control()).unwrap();
    let b = source
        .search(&query, a.next_offset.unwrap(), 2, &control())
        .unwrap();
    assert_eq!(a.matches.len(), 2);
    assert_eq!(b.matches.len(), 2);
    assert!(
        a.matches
            .iter()
            .all(|a| b.matches.iter().all(|b| a.commit_id != b.commit_id))
    );
    let context = source.around(&commits[2], 2, &control()).unwrap();
    assert_eq!(context.revisions[0].commit_id, commits[2]);
    assert_eq!(
        context.source().unwrap().context(),
        Some(commits[2].as_str())
    );
    let cancelled = control();
    cancelled.cancel();
    assert!(source.search(&query, 0, 2, &cancelled).is_err());
}
#[test]
fn jj_pages_remain_at_the_captured_operation() {
    let (root, _) = fixture();
    let run = |args: &[&str]| {
        let output = Command::new("jj")
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["git", "init", "--colocate"]);
    let comparison = jaywalk::repository::Comparison::load(root.path(), Some("main"), "@").unwrap();
    let before = comparison
        .history(&HistoryQuery::default().with_limit(3).unwrap())
        .unwrap();
    run(&["describe", "-m", "new operation description"]);
    let after = before.load_more(3, &control()).unwrap();
    assert_eq!(before.revisions[0].commit_id, after.revisions[0].commit_id);
    assert!(
        after
            .revisions
            .iter()
            .all(|r| r.description != "new operation description\n")
    );
}

#[test]
fn graph_lanes_and_completed_edges_are_stable_across_merge_pages() {
    let (root, commits) = fixture();
    let tree = git(root.path(), &["mktree"]);
    let side = git(
        root.path(),
        &["commit-tree", &tree, "-p", &commits[4], "-m", "side branch"],
    );
    let merge = git(
        root.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            commits.last().unwrap(),
            "-p",
            &side,
            "-m",
            "merge",
        ],
    );
    git(root.path(), &["update-ref", "refs/heads/main", &merge]);
    let source = Arc::new(HistorySource::git(root.path(), &HistoryQuery::default()).unwrap());
    let first = source.first_page(4, &control()).unwrap();
    let next = first.load_more(4, &control()).unwrap();
    let strokes = |row: &jaywalk::graph::GraphRow| {
        row.strokes
            .iter()
            .map(|s| (s.from, s.to, s.color, s.half, s.layer()))
            .collect::<Vec<_>>()
    };
    for (old, new) in first.graph.iter().zip(&next.graph) {
        assert_eq!((old.lane, old.color), (new.lane, new.color));
    }
    for (old, new) in first.graph[..first.graph.len() - 1].iter().zip(&next.graph) {
        assert_eq!(strokes(old), strokes(new));
    }
}
