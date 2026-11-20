use super::*;
use crate::local_review::{
    self,
    tests::{Fixture, git},
};
use crate::repository_file::ChangeKind;
use std::{cell::Cell, path::Path};

struct Harness {
    fixture: Fixture,
    rows: Vec<(ChangeKind, String, String)>,
    trees: BTreeMap<String, Value>,
    provider: crate::accounts::Provider,
    requests: Cell<usize>,
    metadata_requests: Cell<usize>,
    detail_reads: Cell<usize>,
    move_pr: bool,
    latency: std::time::Duration,
}
impl Harness {
    fn new(provider: crate::accounts::Provider) -> Self {
        let mut fixture = Fixture::new();
        let root = fixture.directory.path();
        git(root, &["checkout", "--detach", &fixture.head]);
        git(root, &["mv", "stable.rs", "renamed.rs"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                root.join("changed.rs"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            std::os::unix::fs::symlink("changed.rs", root.join("link.rs")).unwrap();
        }
        let mut rows = vec![
            (
                ChangeKind::Modified,
                "changed.rs".into(),
                "changed.rs".into(),
            ),
            (ChangeKind::Renamed, "stable.rs".into(), "renamed.rs".into()),
            (ChangeKind::Added, "new.rs".into(), "new.rs".into()),
        ];
        #[cfg(unix)]
        rows.push((ChangeKind::Added, "link.rs".into(), "link.rs".into()));
        for i in 0..40 {
            let path = format!("added-{i}.rs");
            std::fs::write(root.join(&path), format!("fn added_{i}() {{}}\n")).unwrap();
            rows.push((ChangeKind::Added, path.clone(), path));
        }
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "modes and renamed files"]);
        fixture.head = git(root, &["rev-parse", "HEAD"]);
        git(root, &["checkout", "main"]);
        let host = match provider {
            crate::accounts::Provider::Github => "github.com",
            crate::accounts::Provider::Bitbucket => "ssh.bitbucket.org",
        };
        git(
            root,
            &[
                "remote",
                "set-url",
                "origin",
                &format!("git@{host}:fixture/review.git"),
            ],
        );
        let trees = [&fixture.base, &fixture.head].into_iter().map(|commit| {
            let rows = git(root, &["ls-tree", commit]).lines().map(|row| {
                let (metadata, path) = row.split_once('\t').unwrap(); let parts = metadata.split_whitespace().collect::<Vec<_>>();
                serde_json::json!({"path":path,"type":parts[1],"mode":parts[0],"sha":parts[2]})
            }).collect::<Vec<_>>();
            (commit.clone(), serde_json::json!({"truncated":false,"tree":rows}))
        }).collect();
        Self {
            fixture,
            rows,
            trees,
            provider,
            requests: Cell::new(0),
            metadata_requests: Cell::new(0),
            detail_reads: Cell::new(0),
            move_pr: false,
            latency: std::time::Duration::ZERO,
        }
    }
    fn locator(&self) -> Locator {
        Locator {
            provider: self.provider,
            repository: RepositoryName::parse("fixture/review").unwrap(),
            number: 1,
        }
    }
    fn detail(&self) -> Value {
        let head = if self.move_pr && self.detail_reads.get() > 1 {
            "f".repeat(40)
        } else {
            self.fixture.head.clone()
        };
        match self.provider {
            crate::accounts::Provider::Github => {
                serde_json::json!({"number":1,"id":1,"node_id":"PR","title":"Synthetic review","body":"","user":{"login":"Fixture"},"state":"open","changed_files":self.rows.len(),"commits":1,"head":{"sha":head,"ref":"feature","repo":{"full_name":"fixture/review","node_id":"R"}},"base":{"sha":"c".repeat(40),"ref":"main","repo":{"full_name":"fixture/review","node_id":"R"}}})
            }
            crate::accounts::Provider::Bitbucket => {
                serde_json::json!({"id":1,"title":"Synthetic review","description":"","author":{"display_name":"Fixture"},"state":"OPEN","source":{"commit":{"hash":head},"branch":{"name":"feature"},"repository":{"full_name":"fixture/review","uuid":"R"}},"destination":{"commit":{"hash":"c".repeat(40)},"branch":{"name":"main"},"repository":{"full_name":"fixture/review","uuid":"R"}}})
            }
        }
    }
    fn file_rows(&self) -> Vec<Value> {
        self.rows.iter().map(|(kind, before, after)| {
            let status = status(kind);
            if self.provider == crate::accounts::Provider::Github {
                serde_json::json!({"filename":after,"previous_filename":before,"status":status})
            } else {
                let entry = |path: &str, commit: &str| serde_json::json!({
                    "path": path,
                    "links": {"self": {"href": format!("https://api.bitbucket.org/2.0/repositories/fixture/review/src/{}/{path}", &commit[..12])}}
                });
                let old = (kind != &ChangeKind::Added).then(|| entry(before, &self.fixture.base));
                let new = (kind != &ChangeKind::Removed).then(|| entry(after, &self.fixture.head));
                serde_json::json!({"status":status,"old":old,"new":new})
            }
        }).collect()
    }
    fn request(&self, r: Request) -> Result<Reply, Error> {
        std::thread::sleep(self.latency);
        self.requests.set(self.requests.get() + 1);
        let Request::Json(url) = r else {
            panic!("file bytes should be read locally")
        };
        let p = url.path();
        let value = if p.ends_with("/pulls/1") || p.ends_with("/pullrequests/1") {
            self.detail_reads.set(self.detail_reads.get() + 1);
            self.detail()
        } else if p.contains("/compare/") {
            serde_json::json!({"merge_base_commit":{"sha":self.fixture.base}})
        } else if p.ends_with("/files") {
            Value::Array(self.file_rows())
        } else if p.ends_with("/diffstat") {
            serde_json::json!({"values":self.file_rows()})
        } else if p.contains("/commit/") {
            let short = p.rsplit('/').next().unwrap();
            let full = [&self.fixture.base, &self.fixture.head]
                .into_iter()
                .find(|commit| commit.starts_with(short))
                .unwrap();
            serde_json::json!({"hash":full})
        } else if p.ends_with("/commits") {
            if self.provider == crate::accounts::Provider::Github {
                serde_json::json!([{"sha":self.fixture.head,"parents":[{"sha":self.fixture.base}],"commit":{"message":"Review","author":{"name":"Fixture","date":"2026-11-20T00:00:00Z"}}}])
            } else {
                serde_json::json!({"values":[{"hash":self.fixture.head,"parents":[{"hash":self.fixture.base}],"message":"Review","author":{"raw":"Fixture"},"date":"2026-11-20T00:00:00Z"}]})
            }
        } else if p.contains("/git/trees/") {
            self.metadata_requests.set(self.metadata_requests.get() + 1);
            self.trees[p.rsplit('/').next().unwrap()].clone()
        } else if p.contains("/src/") {
            self.metadata_requests.set(self.metadata_requests.get() + 1);
            let path = p.rsplit('/').next().unwrap();
            let commit = if p.contains(&self.fixture.base) {
                &self.fixture.base
            } else {
                &self.fixture.head
            };
            let entry = self.trees[commit]["tree"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["path"] == path)
                .unwrap();
            let attrs = match entry["mode"].as_str().unwrap() {
                "100755" => vec!["executable"],
                "120000" => vec!["link"],
                _ => vec![],
            };
            serde_json::json!({"attributes":attrs})
        } else {
            panic!("Unexpected fixture endpoint {p}")
        };
        Ok(Reply::Json(value))
    }
    fn open(
        &self,
        mode: local_review::Mode,
        candidates: &[std::path::PathBuf],
        receipts: &Path,
        control: &Control,
        progress: &dyn Fn(OpenStage),
    ) -> Result<Review, Error> {
        open::open_with_request(
            Access {
                service: Weak::new(),
                account: AccountKey {
                    provider: self.provider,
                    subject: "fixture".into(),
                },
                generation: 0,
            },
            &self.locator(),
            OpenOptions {
                hosting: Options::default(),
                limit: TextLimit::default(),
                local: Some(LocalRequest {
                    candidates,
                    options: local_review::Options {
                        mode,
                        fetch_missing: false,
                        ..Default::default()
                    },
                    receipts,
                }),
                progress,
            },
            control,
            |r| self.request(r),
        )
    }
}
fn status(kind: &ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Renamed => "renamed",
        _ => "modified",
    }
}
#[test]
fn local_open_preserves_published_paths_modes_and_history_without_file_metadata_requests() {
    for provider in [
        crate::accounts::Provider::Github,
        crate::accounts::Provider::Bitbucket,
    ] {
        let h = Harness::new(provider);
        let receipts = tempfile::tempdir().unwrap();
        let candidates = vec![h.fixture.directory.path().to_owned()];
        let review = h
            .open(
                local_review::Mode::Isolated,
                &candidates,
                receipts.path(),
                &control(),
                &|_| {},
            )
            .unwrap();
        assert_eq!(h.metadata_requests.get(), 0);
        assert!(
            h.requests.get() <= 7,
            "request count must not grow with file count: {}",
            h.requests.get()
        );
        assert_eq!(review.metadata().base.commit, h.fixture.base);
        assert_ne!(
            review.metadata().base.commit,
            review.metadata().target.commit
        );
        let contents = review.comparison().hosted().unwrap();
        assert!(contents.is_local());
        assert_eq!(contents.data.files.len(), h.rows.len());
        assert_eq!(contents.data.commits.len(), 1);
        let changed = contents
            .data
            .files
            .iter()
            .find(|f| f.change.after_path() == "changed.rs")
            .unwrap();
        #[cfg(unix)]
        {
            assert!(!changed.before.as_ref().unwrap().executable);
            assert!(changed.after.as_ref().unwrap().executable);
            assert_eq!(
                contents
                    .data
                    .files
                    .iter()
                    .find(|f| f.change.after_path() == "link.rs")
                    .unwrap()
                    .after
                    .as_ref()
                    .unwrap()
                    .kind,
                FileKind::Symlink
            );
        }
        assert_eq!(
            contents
                .read_path("changed.rs", crate::repository::FileSide::After)
                .unwrap(),
            "fn after() {}\n"
        );
        assert_eq!(
            git(h.fixture.directory.path(), &["symbolic-ref", "HEAD"]),
            "refs/heads/main"
        );
        let hosted = h
            .open(
                local_review::Mode::Hosted,
                &candidates,
                receipts.path(),
                &control(),
                &|_| {},
            )
            .unwrap();
        assert!(!hosted.comparison().hosted().unwrap().is_local());
        assert!(h.metadata_requests.get() > 0);
        for (local, web) in contents
            .data
            .files
            .iter()
            .zip(&hosted.comparison().hosted().unwrap().data.files)
        {
            assert_eq!(local.change.before_path(), web.change.before_path());
            assert_eq!(local.change.after_path(), web.change.after_path());
            assert_eq!(local.change.kind(), web.change.kind());
            for (a, b) in [(&local.before, &web.before), (&local.after, &web.after)] {
                assert_eq!(
                    a.as_ref().map(|a| (&a.kind, a.executable)),
                    b.as_ref().map(|b| (&b.kind, b.executable))
                );
            }
        }
    }
}
#[test]
fn dirty_checkout_falls_back_to_hosted_metadata_without_changing_the_checkout() {
    let h = Harness::new(crate::accounts::Provider::Bitbucket);
    let root = h.fixture.directory.path();
    std::fs::write(root.join("changed.rs"), "user edits\n").unwrap();
    let receipts = tempfile::tempdir().unwrap();
    let review = h
        .open(
            local_review::Mode::CheckoutIfClean,
            &[root.to_owned()],
            receipts.path(),
            &control(),
            &|_| {},
        )
        .unwrap();
    assert!(!review.comparison().hosted().unwrap().is_local());
    assert!(review.local_notice().unwrap().contains("refused"));
    assert_eq!(h.metadata_requests.get(), 46);
    assert_eq!(
        std::fs::read_to_string(root.join("changed.rs")).unwrap(),
        "user edits\n"
    );
    assert!(
        local_review::checkout::recoveries(receipts.path())
            .unwrap()
            .is_empty()
    );
}
#[test]
fn a_push_during_local_acquisition_is_rejected_before_checkout() {
    let mut h = Harness::new(crate::accounts::Provider::Bitbucket);
    h.move_pr = true;
    let root = h.fixture.directory.path();
    let receipts = tempfile::tempdir().unwrap();
    assert!(matches!(
        h.open(
            local_review::Mode::CheckoutIfClean,
            &[root.to_owned()],
            receipts.path(),
            &control(),
            &|_| {}
        ),
        Err(Error::Superseded)
    ));
    assert_eq!(h.metadata_requests.get(), 0);
    assert_eq!(git(root, &["rev-parse", "HEAD"]), h.fixture.base);
    assert!(
        local_review::checkout::recoveries(receipts.path())
            .unwrap()
            .is_empty()
    );
}
#[test]
fn cancellation_before_checkout_does_not_hydrate_hosted_files_or_switch_branches() {
    for stage in [
        OpenStage::Objects,
        OpenStage::FileMetadata,
        OpenStage::Checkout,
    ] {
        let h = Harness::new(crate::accounts::Provider::Github);
        let root = h.fixture.directory.path();
        let receipts = tempfile::tempdir().unwrap();
        let control = control();
        assert!(matches!(
            h.open(
                local_review::Mode::CheckoutIfClean,
                &[root.to_owned()],
                receipts.path(),
                &control,
                &|current| {
                    if current == stage {
                        control.cancel();
                    }
                }
            ),
            Err(Error::Cancelled)
        ));
        assert_eq!(h.metadata_requests.get(), 0);
        assert_eq!(git(root, &["rev-parse", "HEAD"]), h.fixture.base);
        assert!(
            local_review::checkout::recoveries(receipts.path())
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
#[ignore = "synthetic API latency profile; run explicitly with --ignored --nocapture"]
fn profile_bitbucket_open_with_simulated_network_latency() {
    let mut harness = Harness::new(crate::accounts::Provider::Bitbucket);
    harness.latency = std::time::Duration::from_millis(100);
    let receipts = tempfile::tempdir().unwrap();
    for mode in [local_review::Mode::Hosted, local_review::Mode::Isolated] {
        harness.requests.set(0);
        harness.metadata_requests.set(0);
        harness.detail_reads.set(0);
        let start = std::time::Instant::now();
        let review = harness
            .open(
                mode,
                &[harness.fixture.directory.path().to_owned()],
                receipts.path(),
                &control(),
                &|_| {},
            )
            .unwrap();
        eprintln!(
            "Synthetic 44-file PR, 100ms/API request: {mode:?} elapsed_ms={} api_requests={} file_metadata_requests={}",
            start.elapsed().as_millis(),
            harness.requests.get(),
            harness.metadata_requests.get()
        );
        if mode == local_review::Mode::Isolated {
            assert_eq!(harness.metadata_requests.get(), 0);
            assert!(review.comparison().hosted().unwrap().is_local());
        }
    }
}

#[test]
fn missing_local_objects_fall_back_without_fetching_when_disabled() {
    let h = Harness::new(crate::accounts::Provider::Bitbucket);
    let directory = tempfile::tempdir().unwrap();
    let clone = directory.path().join("known");
    git(
        directory.path(),
        &[
            "clone",
            "--no-local",
            "--single-branch",
            "--branch",
            "main",
            h.fixture.directory.path().to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    git(
        &clone,
        &[
            "remote",
            "set-url",
            "origin",
            "git@ssh.bitbucket.org:fixture/review.git",
        ],
    );
    let receipts = tempfile::tempdir().unwrap();
    let review = h
        .open(
            local_review::Mode::CheckoutIfClean,
            std::slice::from_ref(&clone),
            receipts.path(),
            &control(),
            &|_| {},
        )
        .unwrap();
    assert!(!review.comparison().hosted().unwrap().is_local());
    assert!(
        review
            .local_notice()
            .unwrap()
            .contains("fetching missing remote objects is disabled")
    );
    assert_eq!(git(&clone, &["rev-parse", "HEAD"]), h.fixture.base);
    assert_eq!(git(&clone, &["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert!(
        local_review::checkout::recoveries(receipts.path())
            .unwrap()
            .is_empty()
    );
}

struct Store(crate::accounts::Provider);
impl crate::accounts::CredentialStore for Store {
    fn load(&self) -> Result<Vec<crate::accounts::StoredSession>, Error> {
        Ok(vec![serde_json::from_value(serde_json::json!({"identity":{"key":{"provider":self.0,"subject":"fixture"},"label":"Fixture"},"client_id":"fixture","tokens":{"access":"fixture","refresh":null,"expires_at":null},"refresh_state":"Ready"})).unwrap()])
    }
    fn save(&self, _: &crate::accounts::StoredSession) -> Result<(), Error> {
        unreachable!()
    }
    fn remove(&self, _: &AccountKey) -> Result<(), Error> {
        Ok(())
    }
    fn registration(
        &self,
        _: crate::accounts::Provider,
        _: &str,
    ) -> Result<crate::accounts::ClientRegistration, Error> {
        Err(Error::Registration)
    }
}
struct Transport(Arc<std::sync::Mutex<Harness>>);
impl crate::accounts::HostingProvider for Transport {
    fn exchange(
        &self,
        _: &crate::accounts::ClientRegistration,
        _: &str,
        _: &str,
        _: &Control,
    ) -> Result<crate::accounts::Tokens, Error> {
        unreachable!()
    }
    fn refresh(
        &self,
        _: &crate::accounts::ClientRegistration,
        _: &crate::accounts::Secret,
        _: &Control,
    ) -> Result<crate::accounts::Tokens, Error> {
        unreachable!()
    }
    fn identity(
        &self,
        provider: crate::accounts::Provider,
        _: &crate::accounts::Secret,
        _: &Control,
    ) -> Result<crate::accounts::Identity, Error> {
        Ok(crate::accounts::Identity {
            key: AccountKey {
                provider,
                subject: "fixture".into(),
            },
            label: "Fixture".into(),
        })
    }
    fn review_request(
        &self,
        _: &crate::accounts::Secret,
        request: Request,
        control: &Control,
    ) -> Result<Reply, Error> {
        control.check()?;
        self.0.lock().unwrap().request(request)
    }
}
#[test]
fn service_refresh_stays_local_without_repeating_file_requests_or_switching_checkout() {
    let h = Arc::new(std::sync::Mutex::new(Harness::new(
        crate::accounts::Provider::Bitbucket,
    )));
    let service = Service::new(
        Arc::new(Store(crate::accounts::Provider::Bitbucket)),
        Arc::new(Transport(h.clone())),
    );
    service.restore().unwrap();
    let key = service.accounts_snapshot().accounts[0].identity.key.clone();
    let (root, locator, base, head) = {
        let h = h.lock().unwrap();
        (
            h.fixture.directory.path().to_owned(),
            h.locator(),
            h.fixture.base.clone(),
            h.fixture.head.clone(),
        )
    };
    let receipts = tempfile::tempdir().unwrap();
    let stages = std::cell::RefCell::new(Vec::new());
    let review = service
        .open_review_with_options(
            &key,
            &locator,
            OpenOptions {
                hosting: Options::default(),
                limit: TextLimit::default(),
                local: Some(LocalRequest {
                    candidates: std::slice::from_ref(&root),
                    options: local_review::Options {
                        mode: local_review::Mode::CheckoutIfClean,
                        fetch_missing: false,
                        ..Default::default()
                    },
                    receipts: receipts.path(),
                }),
                progress: &|stage| stages.borrow_mut().push(stage),
            },
            &control(),
        )
        .unwrap();
    assert_eq!(stages.borrow().first(), Some(&OpenStage::Account));
    assert!(stages.borrow().contains(&OpenStage::Objects));
    assert!(stages.borrow().contains(&OpenStage::Checkout));
    assert_eq!(stages.borrow().last(), Some(&OpenStage::Ready));
    assert!(review.checkout().unwrap().active());
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    let updated = review
        .comparison()
        .hosted()
        .unwrap()
        .newer(Options::default(), TextLimit::default(), &control())
        .unwrap();
    assert!(updated.comparison().hosted().unwrap().is_local());
    assert!(updated.checkout().is_none());
    assert_eq!(h.lock().unwrap().metadata_requests.get(), 0);
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        local_review::checkout::recoveries(receipts.path())
            .unwrap()
            .len(),
        1
    );
    let receipt = local_review::checkout::recoveries(receipts.path())
        .unwrap()
        .remove(0);
    local_review::checkout::restore(&receipt.path).unwrap();
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), base);
}
