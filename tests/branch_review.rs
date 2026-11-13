use jaywalk::{
    branch_review::{Catalog, RefName, Target, Tip},
    repository::Comparison,
};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "jaywalk-branch-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let f = Self(root);
        f.run("git", &["init", "-b", "main"]);
        f.run("git", &["config", "user.name", "Fixture"]);
        f.run("git", &["config", "user.email", "fixture@example.invalid"]);
        f.write("base.txt", "base\n");
        f.commit("baseline");
        f.run("git", &["checkout", "-b", "feature"]);
        f.write("one.rs", "first\n");
        f.commit("first change");
        f.run("git", &["tag", "middle"]);
        f.write("two.rs", "second\n");
        f.commit("second change");
        f.run("git", &["checkout", "main"]);
        f.write("main-only", "advanced main\n");
        f.commit("advance target");
        f
    }
    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.0.join(path), text).unwrap();
    }
    fn commit(&self, message: &str) {
        self.run("git", &["add", "."]);
        self.run("git", &["commit", "-m", message]);
    }
    fn run(&self, program: &str, args: &[&str]) -> String {
        let output = Command::new(program)
            .current_dir(&self.0)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn catalog(&self, jj: bool) -> Catalog {
        if jj {
            self.run("jj", &["git", "init", "--colocate"]);
        }
        let home = Comparison::load(&self.0, Some("main"), "main").unwrap();
        Catalog::capture(&home).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn name(value: &str) -> RefName {
    RefName {
        name: value.into(),
        remote: None,
    }
}
#[test]
fn branch_review_uses_the_full_tip_and_common_base_on_git_and_jj() {
    for jj in [false, true] {
        let f = Fixture::new();
        let catalog = f.catalog(jj);
        assert_eq!(catalog.default_base(), Some(&name("main")));
        let review = catalog
            .resolve(&Target::Branch {
                tip: Tip::Reference(name("feature")),
                base: name("main"),
            })
            .unwrap();
        let paths: Vec<_> = review
            .files()
            .iter()
            .map(|file| file.after_path.as_str())
            .collect();
        assert_eq!(paths, ["one.rs", "two.rs"]);
        let middle = f.run("git", &["rev-parse", "middle"]);
        assert!(
            catalog
                .contains(review.base(), review.tip(), &middle)
                .unwrap()
        );
        assert!(
            !catalog
                .contains(review.base(), review.tip(), review.base())
                .unwrap()
        );
        assert_eq!(
            catalog.candidates(&middle).unwrap(),
            [Tip::Reference(name("feature"))]
        );
        let before = review.contents(&review.files()[1]).unwrap();
        assert_eq!(before, (String::new(), "second\n".into()));
        f.run("git", &["branch", "-f", "feature", "main"]);
        assert_eq!(
            catalog
                .resolve(&Target::Branch {
                    tip: Tip::Reference(name("feature")),
                    base: name("main")
                })
                .unwrap()
                .tip(),
            review.tip(),
            "catalog endpoints stay fixed after refs move"
        );
    }
}
#[test]
fn deleted_merged_branches_keep_the_original_range_separate_from_the_merge_result() {
    for jj in [false, true] {
        let f = Fixture::new();
        f.run(
            "git",
            &["merge", "--no-ff", "feature", "-m", "merge feature"],
        );
        let merge = f.run("git", &["rev-parse", "HEAD"]);
        f.run("git", &["branch", "-d", "feature"]);
        f.write("later", "later\n");
        f.commit("advance main again");
        let catalog = f.catalog(jj);
        let original = catalog
            .resolve(&Target::MergedBranch {
                merge: merge.clone(),
            })
            .unwrap();
        assert_eq!(
            original
                .files()
                .iter()
                .map(|file| file.after_path.as_str())
                .collect::<Vec<_>>(),
            ["one.rs", "two.rs"]
        );
        let landed = catalog
            .resolve(&Target::MergeResult {
                merge: merge.clone(),
            })
            .unwrap();
        assert_eq!(landed.tip(), merge);
        assert_ne!(landed.base(), original.base());
        assert!(!landed.files().iter().any(|file| file.after_path == "later"));
    }
}
#[test]
fn missing_names_and_nonmerge_commits_fail_without_substituting_the_workspace() {
    let f = Fixture::new();
    let catalog = f.catalog(false);
    assert!(
        catalog
            .resolve(&Target::Branch {
                tip: Tip::Reference(name("gone")),
                base: name("main")
            })
            .unwrap_err()
            .contains("unavailable")
    );
    let main = f.run("git", &["rev-parse", "main"]);
    assert!(
        catalog
            .resolve(&Target::MergedBranch { merge: main })
            .unwrap_err()
            .contains("two-parent")
    );
    assert!(catalog.candidates("--all").is_err());
}

#[test]
fn crisscross_histories_require_explicit_endpoints_on_git_and_jj() {
    for jj in [false, true] {
        let f = Fixture::new();
        let root = f.run("git", &["rev-list", "--max-parents=0", "main"]);
        let tree = f.run("git", &["rev-parse", "main^{tree}"]);
        let left = f.run("git", &["commit-tree", &tree, "-p", &root, "-m", "left"]);
        let right = f.run("git", &["commit-tree", &tree, "-p", &root, "-m", "right"]);
        let one = f.run(
            "git",
            &["commit-tree", &tree, "-p", &left, "-p", &right, "-m", "one"],
        );
        let two = f.run(
            "git",
            &["commit-tree", &tree, "-p", &right, "-p", &left, "-m", "two"],
        );
        f.run("git", &["update-ref", "refs/heads/feature", &one]);
        f.run("git", &["update-ref", "refs/heads/main", &two]);
        let catalog = f.catalog(jj);
        let error = catalog
            .resolve(&Target::Branch {
                tip: Tip::Reference(name("feature")),
                base: name("main"),
            })
            .unwrap_err();
        assert!(error.contains("2 merge bases"), "{error}");
    }
}

#[test]
fn named_merged_branches_use_the_historical_base_after_main_advances() {
    for jj in [false, true] {
        let f = Fixture::new();
        f.run(
            "git",
            &["merge", "--no-ff", "feature", "-m", "merge feature"],
        );
        f.write("later", "later\n");
        f.commit("advance main");
        let catalog = f.catalog(jj);
        let review = catalog
            .resolve(&Target::Branch {
                tip: Tip::Reference(name("feature")),
                base: name("main"),
            })
            .unwrap();
        assert_eq!(
            review
                .files()
                .iter()
                .map(|file| file.after_path.as_str())
                .collect::<Vec<_>>(),
            ["one.rs", "two.rs"]
        );
    }
}
