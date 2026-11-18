use super::*;
use crate::accounts::Provider;
use crate::repository::FileSide;
use serde_json::json;

fn sha(character: char) -> String {
    character.to_string().repeat(40)
}
fn locator(provider: Provider) -> Locator {
    Locator {
        provider,
        repository: RepositoryName::parse("team/project").unwrap(),
        number: 7,
    }
}
fn github_detail() -> Value {
    json!({"number":7,"id":7,"node_id":"PR_7","title":"Published review","body":"Description\nwith a second line","user":{"login":"author"},"state":"open","changed_files":1,"commits":1,"head":{"sha":sha('b'),"repo":{"full_name":"fork/project","node_id":"fork-id"}},"base":{"sha":sha('c'),"repo":{"full_name":"team/project","node_id":"target-id"}}})
}
fn github_response(url: &Url) -> Value {
    let p = url.path();
    if p.ends_with("/pulls/7") {
        github_detail()
    } else if p.contains("/compare/") {
        json!({"merge_base_commit":{"sha":sha('a')}})
    } else if p.ends_with("/files") {
        json!([{"filename":"new.rs","previous_filename":"old.rs","status":"renamed"}])
    } else if p.ends_with("/commits") {
        json!([{"sha":sha('b'),"parents":[{"sha":sha('a')}],"commit":{"message":"A change\n\nDetails","author":{"name":"Author","date":"2026-11-17T00:00:00Z"}}}])
    } else if p.contains("/git/trees/") {
        if p.ends_with(&sha('a')) {
            json!({"truncated":false,"tree":[{"path":"old.rs","type":"blob","mode":"100644","sha":sha('d')}]})
        } else {
            json!({"truncated":false,"tree":[{"path":"new.rs","type":"blob","mode":"100755","sha":sha('e')}]})
        }
    } else {
        panic!("Unexpected synthetic endpoint: {p}")
    }
}
#[test]
fn github_pins_merge_base_and_fork_blobs_instead_of_target_tip() {
    let mut requests = Vec::new();
    let data = resolve(
        &locator(Provider::Github),
        Options::default(),
        &control(),
        |r| {
            let Request::Json(url) = r else { panic!() };
            requests.push(url.clone());
            Ok(Reply::Json(github_response(&url)))
        },
    )
    .unwrap();
    assert_eq!(data.metadata.base.commit, sha('a'));
    assert_eq!(data.metadata.target.commit, sha('c'));
    assert_eq!(data.files[0].change.before_path(), "old.rs");
    assert_eq!(data.files[0].change.after_path(), "new.rs");
    assert!(data.files[0].after.as_ref().unwrap().executable);
    assert!(
        data.files[0]
            .after
            .as_ref()
            .unwrap()
            .url
            .path()
            .starts_with("/repos/fork/project/git/blobs/")
    );
    assert!(
        requests
            .iter()
            .any(|u| u.path().ends_with(&format!("/git/trees/{}", sha('a'))))
    );
    assert!(
        !requests
            .iter()
            .any(|u| u.path().ends_with(&format!("/git/trees/{}", sha('c'))))
    );
}
#[test]
fn github_rejects_a_push_during_snapshot_acquisition() {
    let mut reads = 0;
    let result = resolve(
        &locator(Provider::Github),
        Options::default(),
        &control(),
        |r| {
            let Request::Json(url) = r else { panic!() };
            let mut value = github_response(&url);
            if url.path().ends_with("/pulls/7") {
                reads += 1;
                if reads == 2 {
                    value["head"]["sha"] = json!(sha('f'));
                }
            }
            Ok(Reply::Json(value))
        },
    );
    assert!(matches!(result, Err(Error::Superseded)));
}
#[test]
fn limits_and_cancellation_stop_before_fetching_content() {
    let mut reads = 0;
    let result = resolve(
        &locator(Provider::Github),
        Options {
            max_files: 1,
            ..Default::default()
        },
        &control(),
        |r| {
            let Request::Json(url) = r else { panic!() };
            reads += 1;
            let mut value = github_response(&url);
            if url.path().ends_with("/pulls/7") {
                value["changed_files"] = json!(2);
            }
            Ok(Reply::Json(value))
        },
    );
    assert!(matches!(result, Err(Error::ContentLimit)));
    assert_eq!(reads, 2);
    let cancelled = control();
    cancelled.cancel();
    assert!(matches!(cancelled.check(), Err(Error::Cancelled)));
    for value in ["../secret", "/etc/passwd", "a/../b", "a//b", "a\n"] {
        assert!(path(value).is_err());
    }
    assert!(
        Options {
            max_content_mb: 0,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}
fn bb_detail() -> Value {
    json!({"id":7,"title":"Published review","description":"Description","author":{"display_name":"Author"},"state":"OPEN","source":{"commit":{"hash":sha('b')},"repository":{"full_name":"team/project","uuid":"repo-id"}},"destination":{"commit":{"hash":sha('c')},"repository":{"full_name":"team/project","uuid":"repo-id"}}})
}
fn bb_row() -> Value {
    json!({"status":"modified","old":{"path":"file.rs","links":{"self":{"href":format!("https://api.bitbucket.org/2.0/repositories/team/project/src/{}/file.rs",sha('a'))}}},"new":{"path":"file.rs","links":{"self":{"href":format!("https://api.bitbucket.org/2.0/repositories/team/project/src/{}/file.rs",sha('b'))}}}})
}
fn bb_response(url: &Url) -> Reply {
    let p = url.path();
    if p.ends_with("/pullrequests/7") {
        Reply::Json(bb_detail())
    } else if p.ends_with("/pullrequests/7/diffstat") {
        Reply::Redirect(
            Url::parse(
                "https://api.bitbucket.org/2.0/repositories/team/project/diffstat/published-spec",
            )
            .unwrap(),
        )
    } else if p.contains("/diffstat/") {
        Reply::Json(json!({"values":[bb_row()]}))
    } else if p.contains("/src/") {
        Reply::Json(json!({"attributes":["link"]}))
    } else if p.ends_with("/commits") {
        Reply::Json(
            json!({"values":[{"hash":sha('b'),"message":"Change","author":{"raw":"Author"},"date":"2026-11-17T00:00:00Z","parents":[{"hash":sha('a')}]}]}),
        )
    } else {
        panic!("Unexpected synthetic endpoint: {p}")
    }
}
#[test]
fn bitbucket_uses_authoritative_published_base_and_preserves_symlink_type() {
    let data = resolve(
        &locator(Provider::Bitbucket),
        Options::default(),
        &control(),
        |r| {
            let Request::Json(url) = r else { panic!() };
            Ok(bb_response(&url))
        },
    )
    .unwrap();
    assert_eq!(data.metadata.base.commit, sha('a'));
    assert_eq!(data.metadata.target.commit, sha('c'));
    assert_eq!(
        data.files[0].before.as_ref().unwrap().kind,
        FileKind::Symlink
    );
    assert_eq!(data.commits[0].commit_id, sha('b'));
}
#[test]
fn bitbucket_rejects_cross_host_and_cross_repository_redirects_before_following() {
    for redirect in [
        "https://evil.example/diffstat/anything",
        "https://api.bitbucket.org/2.0/repositories/other/project/diffstat/anything",
        "http://api.bitbucket.org/2.0/repositories/team/project/diffstat/anything",
    ] {
        let mut followed = false;
        let result = resolve(
            &locator(Provider::Bitbucket),
            Options::default(),
            &control(),
            |r| {
                let Request::Json(url) = r else { panic!() };
                if url.path().ends_with("/pullrequests/7/diffstat") {
                    return Ok(Reply::Redirect(Url::parse(redirect).unwrap()));
                }
                if url.path().contains("/diffstat/") {
                    followed = true;
                }
                Ok(bb_response(&url))
            },
        );
        assert!(matches!(result, Err(Error::InvalidResponse)));
        assert!(!followed);
    }
}
#[test]
fn bitbucket_rejects_untrusted_pagination_and_moving_file_links() {
    for hostile_page in [true, false] {
        let result = resolve(
            &locator(Provider::Bitbucket),
            Options::default(),
            &control(),
            |r| {
                let Request::Json(url) = r else { panic!() };
                if url.path().contains("/diffstat/") {
                    let mut row = bb_row();
                    if !hostile_page {
                        row["new"]["links"]["self"]["href"] = json!(format!(
                            "https://api.bitbucket.org/2.0/repositories/team/project/src/{}/file.rs",
                            sha('f')
                        ));
                    }
                    return Ok(Reply::Json(if hostile_page {
                        json!({"values":[row],"next":"https://evil.example/page2"})
                    } else {
                        json!({"values":[row]})
                    }));
                }
                Ok(bb_response(&url))
            },
        );
        assert!(matches!(
            result,
            Err(Error::InvalidResponse) | Err(Error::Superseded)
        ));
    }
}

struct Store;
impl crate::accounts::CredentialStore for Store {
    fn load(&self) -> Result<Vec<crate::accounts::StoredSession>, Error> {
        Ok(vec![serde_json::from_value(json!({"identity":{"key":{"provider":"github","subject":"test"},"label":"Test"},"client_id":"synthetic","tokens":{"access":"synthetic","refresh":null,"expires_at":null},"refresh_state":"Ready"})).unwrap()])
    }
    fn save(&self, _: &crate::accounts::StoredSession) -> Result<(), Error> {
        unreachable!()
    }
    fn remove(&self, _: &AccountKey) -> Result<(), Error> {
        Ok(())
    }
    fn registration(
        &self,
        _: Provider,
        _: &str,
    ) -> Result<crate::accounts::ClientRegistration, Error> {
        Err(Error::Registration)
    }
}
struct Transport {
    reads: std::sync::atomic::AtomicUsize,
    size: usize,
}
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
        _: Provider,
        _: &crate::accounts::Secret,
        _: &Control,
    ) -> Result<crate::accounts::Identity, Error> {
        Ok(crate::accounts::Identity {
            key: AccountKey {
                provider: Provider::Github,
                subject: "test".into(),
            },
            label: "Test".into(),
        })
    }
    fn review_request(
        &self,
        _: &crate::accounts::Secret,
        request: Request,
        control: &Control,
    ) -> Result<Reply, Error> {
        control.check()?;
        match request {
            Request::Json(url) => Ok(Reply::Json(github_response(&url))),
            Request::Bytes(_, limit) => {
                self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if self.size > limit {
                    Err(Error::ContentLimit)
                } else {
                    Ok(Reply::Bytes(vec![b'x'; self.size]))
                }
            }
        }
    }
}
fn service(size: usize) -> (Arc<Service>, Arc<Transport>, AccountKey) {
    let transport = Arc::new(Transport {
        reads: Default::default(),
        size,
    });
    let service = Service::new(Arc::new(Store), transport.clone());
    service.restore().unwrap();
    let key = service.accounts_snapshot().accounts[0].identity.key.clone();
    (service, transport, key)
}
#[test]
fn immutable_blobs_are_cached_and_clones_share_the_same_memory_budget() {
    let (service, transport, key) = service(600_000);
    let review = service
        .open_review(
            &key,
            &locator(Provider::Github),
            Options {
                max_content_mb: 1,
                ..Default::default()
            },
            TextLimit::from_mib(1).unwrap(),
            &control(),
        )
        .unwrap();
    let contents = review.comparison().hosted().unwrap();
    assert_eq!(
        contents
            .read_path("old.rs", FileSide::Before)
            .unwrap()
            .len(),
        600_000
    );
    assert_eq!(
        review
            .clone()
            .comparison
            .hosted()
            .unwrap()
            .read_path("old.rs", FileSide::Before)
            .unwrap()
            .len(),
        600_000
    );
    assert_eq!(transport.reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        contents.read_path("new.rs", FileSide::After).is_err(),
        "both sides count against the same review budget"
    );
    assert!(contents.read_path("unlisted.rs", FileSide::After).is_err());
}
#[test]
fn uncached_reads_reject_disconnected_accounts_and_per_file_limit_errors_are_retryable() {
    let (service, transport, key) = service(1_048_577);
    let review = service
        .open_review(
            &key,
            &locator(Provider::Github),
            Options::default(),
            TextLimit::from_mib(1).unwrap(),
            &control(),
        )
        .unwrap();
    let contents = review.comparison().hosted().unwrap();
    assert!(contents.read_path("old.rs", FileSide::Before).is_err());
    assert!(contents.read_path("old.rs", FileSide::Before).is_err());
    assert_eq!(
        transport.reads.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "failed reads do not poison the content cache"
    );
    service.disconnect(&key).unwrap();
    assert!(contents.read_path("new.rs", FileSide::After).is_err());
    assert_eq!(
        transport.reads.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "retired account generations cannot issue reads"
    );
}
#[test]
fn provider_host_must_match_the_authenticated_account_before_any_request() {
    let (service, transport, key) = service(4);
    let result = service.review_request(
        &key,
        service.accounts_snapshot().accounts[0].generation,
        Request::Bytes(
            Url::parse("https://api.bitbucket.org/2.0/repositories/team/project/src/branch/file")
                .unwrap(),
            8,
        ),
        &control(),
    );
    assert!(matches!(result, Err(Error::InvalidResponse)));
    assert_eq!(transport.reads.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn bitbucket_expands_abbreviations_in_each_forks_own_repository() {
    let mut requests = Vec::new();
    let data = resolve(
        &locator(Provider::Bitbucket),
        Options::default(),
        &control(),
        |r| {
            let Request::Json(url) = r else { panic!() };
            requests.push(url.clone());
            if url.path().contains("/commit/") {
                return Ok(Reply::Json(
                    json!({"hash":if url.path().contains("/fork/") {sha('b')} else {sha('c')}}),
                ));
            }
            if url.path().ends_with("/pullrequests/7") {
                let mut detail = bb_detail();
                detail["source"]["repository"] =
                    json!({"full_name":"fork/project","uuid":"fork-id"});
                detail["source"]["commit"]["hash"] = json!(&sha('b')[..12]);
                detail["destination"]["commit"]["hash"] = json!(&sha('c')[..12]);
                return Ok(Reply::Json(detail));
            }
            if url.path().contains("/diffstat/") {
                let mut row = bb_row();
                row["new"]["links"]["self"]["href"] = json!(format!(
                    "https://api.bitbucket.org/2.0/repositories/fork/project/src/{}/file.rs",
                    sha('b')
                ));
                return Ok(Reply::Json(json!({"values":[row]})));
            }
            Ok(bb_response(&url))
        },
    )
    .unwrap();
    assert_eq!(
        data.metadata.source.repository.name.to_string(),
        "fork/project"
    );
    assert_eq!(
        data.metadata.base.repository.name.to_string(),
        "team/project"
    );
    assert!(
        requests
            .iter()
            .any(|u| u.path().contains("/fork/project/commit/bbbbbbbbbbbb"))
    );
    assert!(
        requests
            .iter()
            .any(|u| u.path().contains("/team/project/commit/cccccccccccc"))
    );
}

#[test]
fn github_history_limit_keeps_the_newest_available_commit_without_changing_the_diff() {
    let data = resolve(
        &locator(Provider::Github),
        Options {
            history_limit: 1,
            ..Default::default()
        },
        &control(),
        |r| {
            let Request::Json(url) = r else { panic!() };
            let mut value = github_response(&url);
            if url.path().ends_with("/pulls/7") {
                value["commits"] = json!(3);
            }
            if url.path().ends_with("/commits") {
                let commit = value[0].clone();
                let mut first = commit.clone();
                first["sha"] = json!(sha('d'));
                let mut second = commit.clone();
                second["sha"] = json!(sha('e'));
                value = json!([first, second, commit]);
            }
            Ok(Reply::Json(value))
        },
    )
    .unwrap();
    assert_eq!(data.commits.len(), 1);
    assert_eq!(data.commits[0].commit_id, sha('b'));
    assert!(data.metadata.history_partial);
    assert_eq!(data.metadata.base.commit, sha('a'));
    assert_eq!(data.metadata.source.commit, sha('b'));
    assert_eq!(data.files.len(), 1);
}
