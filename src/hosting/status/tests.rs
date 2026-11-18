use super::*;
use crate::hosting::{
    Locator, RepositoryName,
    review::{Endpoint, Repository},
};
use serde_json::json;
use std::time::Duration;
fn sha(c: char) -> String {
    c.to_string().repeat(40)
}
fn control() -> Control {
    Control::new(Duration::from_secs(30))
}
fn metadata(provider: Provider) -> Metadata {
    let repository = Repository {
        name: RepositoryName::parse("team/project").unwrap(),
        id: "repo-id".into(),
    };
    Metadata {
        locator: Locator {
            provider,
            repository: repository.name.clone(),
            number: 7,
        },
        id: if provider == Provider::Github {
            "PR_7".into()
        } else {
            "repo-id:7".into()
        },
        title: "Synthetic review".into(),
        body: String::new(),
        source_branch: Some("topic".into()),
        target_branch: Some("main".into()),
        draft: Some(false),
        author: "author".into(),
        state: "OPEN".into(),
        source: Endpoint {
            repository: repository.clone(),
            commit: sha('a'),
        },
        target: Endpoint {
            repository: repository.clone(),
            commit: sha('b'),
        },
        base: Endpoint {
            repository,
            commit: sha('c'),
        },
        history_partial: false,
    }
}
fn gh_detail() -> Value {
    json!({"node_id":"PR_7","head":{"sha":sha('a'),"repo":{"node_id":"repo-id"}},"base":{"sha":sha('b'),"repo":{"node_id":"repo-id"}},"requested_reviewers":[{"node_id":"user-3","login":"waiting"}],"requested_teams":[]})
}
fn gh_response(url: &Url) -> Value {
    let path = url.path();
    if path.ends_with("/pulls/7") {
        gh_detail()
    } else if path.ends_with("/check-runs") {
        json!({"total_count":1,"check_runs":[{"id":1,"head_sha":sha('a'),"name":"CI","status":"completed","conclusion":"success","details_url":"https://ci.example.invalid/run/1"}]})
    } else if path.ends_with("/status") {
        json!({"sha":sha('a'),"total_count":0,"statuses":[]})
    } else if path.ends_with("/reviews") {
        json!([
            {"id":1,"user":{"node_id":"user-1","login":"alice"},"state":"APPROVED","commit_id":sha('a')},
            {"id":2,"user":{"node_id":"user-1","login":"alice"},"state":"COMMENTED","commit_id":sha('a')},
            {"id":3,"user":{"node_id":"user-2","login":"bob"},"state":"CHANGES_REQUESTED","commit_id":sha('c')},
            {"id":4,"user":{"node_id":"user-2","login":"bob"},"state":"DISMISSED","commit_id":sha('c')}
        ])
    } else {
        panic!("unexpected synthetic route: {path}")
    }
}
fn json_request(r: Request) -> Url {
    let Request::Json(url) = r else {
        panic!("status reads must use JSON GET requests")
    };
    url
}
#[test]
fn github_comment_only_reviews_do_not_revoke_approval_and_dismissal_is_not_a_change_request() {
    let snapshot = load(&metadata(Provider::Github), &control(), |r| {
        Ok(Reply::Json(gh_response(&json_request(r))))
    })
    .unwrap();
    let Availability::Known(reviews) = &snapshot.reviews else {
        panic!()
    };
    assert_eq!(
        reviews
            .items
            .iter()
            .find(|r| r.name == "alice")
            .unwrap()
            .decision,
        Decision::Approved
    );
    assert_eq!(
        reviews
            .items
            .iter()
            .find(|r| r.name == "bob")
            .unwrap()
            .decision,
        Decision::Dismissed
    );
    assert_eq!(
        reviews
            .items
            .iter()
            .find(|r| r.name == "waiting")
            .unwrap()
            .decision,
        Decision::Awaiting
    );
    assert_eq!(snapshot.freshness, Freshness::Current);
    assert!(snapshot.review_summary().contains("1 approval"));
    assert!(
        snapshot
            .review_summary()
            .contains("merge requirements not evaluated")
    );
}
#[test]
fn empty_permission_denied_and_unknown_check_states_remain_distinct() {
    let snapshot = load(&metadata(Provider::Github), &control(), |r| {
        let url = json_request(r);
        if url.path().ends_with("/check-runs") {
            return Err(Error::NoAccess);
        }
        Ok(Reply::Json(gh_response(&url)))
    })
    .unwrap();
    assert!(matches!(
        snapshot.checks[0].checks,
        Availability::Unavailable(Issue::Provider(Error::NoAccess))
    ));
    assert!(
        matches!(&snapshot.checks[1].checks,Availability::Known(list) if list.items.is_empty())
    );
    assert!(!snapshot.checks[0].summary().contains("None reported"));
    let snapshot = load(&metadata(Provider::Github), &control(), |r| {
        let url = json_request(r);
        let mut value = gh_response(&url);
        if url.path().ends_with("/check-runs") {
            value["check_runs"][0]["conclusion"] = json!("future-result");
        }
        Ok(Reply::Json(value))
    })
    .unwrap();
    assert!(
        matches!(&snapshot.checks[0].checks,Availability::Known(list) if matches!(list.items[0].state,CheckState::Unknown(_)))
    );
}
#[test]
fn a_push_during_status_read_does_not_attach_live_reviews_to_the_pinned_diff() {
    let mut reads = 0;
    let snapshot = load(&metadata(Provider::Github), &control(), |r| {
        let url = json_request(r);
        let mut value = gh_response(&url);
        if url.path().ends_with("/pulls/7") {
            reads += 1;
            if reads == 2 {
                value["head"]["sha"] = json!(sha('d'));
            }
        }
        Ok(Reply::Json(value))
    })
    .unwrap();
    assert_eq!(snapshot.freshness, Freshness::NewVersion);
    assert!(matches!(
        snapshot.reviews,
        Availability::Unavailable(Issue::NewVersion)
    ));
    assert_eq!(snapshot.head, sha('a'));
    assert!(
        matches!(&snapshot.checks[0].checks,Availability::Known(list) if list.items[0].state==CheckState::Passed)
    );
}
#[test]
fn check_rows_for_another_head_are_rejected_and_unsafe_browser_links_are_removed() {
    let snapshot = load(&metadata(Provider::Github), &control(), |r| {
        let url = json_request(r);
        let mut value = gh_response(&url);
        if url.path().ends_with("/check-runs") {
            value["check_runs"][0]["head_sha"] = json!(sha('d'));
        }
        Ok(Reply::Json(value))
    })
    .unwrap();
    assert!(matches!(
        snapshot.checks[0].checks,
        Availability::Unavailable(Issue::Provider(Error::InvalidResponse))
    ));
    for link in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "https://user:secret@example.invalid/",
        "http://example.invalid/",
    ] {
        assert!(safe_url(Some(link)).is_none());
    }
    assert!(safe_url(Some("https://ci.example.invalid/job/1#log")).is_some());
    assert!(
        safe_url(Some(&format!(
            "https://ci.example.invalid/{}",
            "x".repeat(9000)
        )))
        .is_none()
    );
}
#[test]
fn fork_checks_query_both_repositories_with_the_same_immutable_head() {
    let mut metadata = metadata(Provider::Github);
    metadata.source.repository = Repository {
        name: RepositoryName::parse("fork/project").unwrap(),
        id: "fork-id".into(),
    };
    let mut paths = Vec::new();
    let snapshot = load(&metadata, &control(), |r| {
        let url = json_request(r);
        paths.push(url.path().to_owned());
        let mut value = gh_response(&url);
        if url.path().ends_with("/pulls/7") {
            value["head"]["repo"]["node_id"] = json!("fork-id");
        }
        Ok(Reply::Json(value))
    })
    .unwrap();
    assert_eq!(snapshot.checks.len(), 4);
    assert!(
        paths
            .iter()
            .any(|p| p.starts_with("/repos/team/project/commits/") && p.contains(&sha('a')))
    );
    assert!(
        paths
            .iter()
            .any(|p| p.starts_with("/repos/fork/project/commits/") && p.contains(&sha('a')))
    );
    assert!(!paths.iter().any(|p| p.contains(&sha('b'))));
}
#[test]
fn bounded_check_pagination_is_reported_as_partial_without_claiming_complete_success() {
    let snapshot=load(&metadata(Provider::Github),&control(),|r| {
        let url=json_request(r);if url.path().ends_with("/check-runs") {
            let page=url.query_pairs().find(|(k,_)|k=="page").unwrap().1.parse::<u64>().unwrap();
            return Ok(Reply::Json(json!({"total_count":600,"check_runs":(0..100).map(|i|json!({"id":page*100+i,"head_sha":sha('a'),"name":format!("job-{i}"),"status":"completed","conclusion":"success"})).collect::<Vec<_>>()})));
        }Ok(Reply::Json(gh_response(&url)))
    }).unwrap();
    assert!(
        matches!(&snapshot.checks[0].checks,Availability::Known(list) if list.partial && list.items.len()==500)
    );
    assert!(snapshot.checks[0].summary().contains("partial results"));
}
fn bb_detail() -> Value {
    json!({"id":7,"source":{"commit":{"hash":sha('a')},"repository":{"uuid":"repo-id"}},"destination":{"commit":{"hash":sha('b')},"repository":{"uuid":"repo-id"}},"participants":[{"user":{"uuid":"user-1","display_name":"Alice"},"role":"REVIEWER","approved":true,"state":"approved"},{"user":{"uuid":"user-2","display_name":"Bob"},"role":"REVIEWER","approved":false,"state":"changes_requested"}],"reviewers":[{"uuid":"user-1","display_name":"Alice"},{"uuid":"user-2","display_name":"Bob"},{"uuid":"user-3","display_name":"Waiting"}]})
}
fn bb_response(url: &Url) -> Value {
    if url.path().ends_with("/pullrequests/7") {
        bb_detail()
    } else if url.path().ends_with("/statuses") {
        json!({"values":[{"key":"CI","name":"Pipeline","state":"SUCCESSFUL","url":"https://ci.example.invalid/build/1"}]})
    } else {
        panic!("unexpected synthetic route")
    }
}
#[test]
fn bitbucket_uses_reported_participant_decisions_and_pinned_commit_builds() {
    let mut paths = Vec::new();
    let snapshot = load(&metadata(Provider::Bitbucket), &control(), |r| {
        let url = json_request(r);
        paths.push(url.path().to_owned());
        Ok(Reply::Json(bb_response(&url)))
    })
    .unwrap();
    let Availability::Known(reviews) = &snapshot.reviews else {
        panic!()
    };
    assert_eq!(reviews.items.len(), 3);
    assert_eq!(reviews.items[0].decision, Decision::Approved);
    assert_eq!(reviews.items[1].decision, Decision::ChangesRequested);
    assert_eq!(reviews.items[2].decision, Decision::Awaiting);
    assert!(reviews.items.iter().all(|r| r.commit.is_none()));
    assert!(
        paths
            .iter()
            .any(|p| p.contains(&format!("/commit/{}/statuses", sha('a'))))
    );
    assert!(
        !paths
            .iter()
            .any(|p| p.ends_with("/pullrequests/7/statuses"))
    );
}
#[test]
fn bitbucket_rejects_untrusted_pagination_before_following_it() {
    let mut followed = false;
    let snapshot = load(&metadata(Provider::Bitbucket), &control(), |r| {
        let url = json_request(r);
        if url.host_str() != Some("api.bitbucket.org") {
            followed = true;
        }
        let mut value = bb_response(&url);
        if url.path().ends_with("/statuses") {
            value["next"] = json!("https://evil.example/private");
        }
        Ok(Reply::Json(value))
    })
    .unwrap();
    assert!(!followed);
    assert!(matches!(
        snapshot.checks[0].checks,
        Availability::Unavailable(Issue::Provider(Error::InvalidResponse))
    ));
}
#[test]
fn contradictory_or_new_bitbucket_decisions_are_unknown_instead_of_approved() {
    let snapshot = load(&metadata(Provider::Bitbucket), &control(), |r| {
        let url = json_request(r);
        let mut value = bb_response(&url);
        if url.path().ends_with("/pullrequests/7") {
            value["participants"][0]["approved"] = json!(false);
        }
        Ok(Reply::Json(value))
    })
    .unwrap();
    assert!(
        matches!(&snapshot.reviews,Availability::Known(list) if matches!(list.items[0].decision,Decision::Unknown(_)))
    );
}
#[test]
fn cancelled_reads_do_not_start_requests() {
    let control = control();
    control.cancel();
    let mut called = false;
    assert!(matches!(
        load(&metadata(Provider::Github), &control, |_| {
            called = true;
            Err(Error::Network)
        }),
        Err(Error::Cancelled)
    ));
    assert!(!called);
}

#[test]
fn bitbucket_status_freshness_expands_short_hashes_in_the_endpoint_repository() {
    let mut resolved = 0;
    let snapshot = load(&metadata(Provider::Bitbucket), &control(), |r| {
        let url = json_request(r);
        if url.path().contains("/commit/") && !url.path().ends_with("/statuses") {
            resolved += 1;
            return Ok(Reply::Json(
                json!({"hash":if url.path().ends_with(&sha('a')[..12]) {sha('a')} else {sha('b')}}),
            ));
        }
        let mut value = bb_response(&url);
        if url.path().ends_with("/pullrequests/7") {
            value["source"]["commit"]["hash"] = json!(&sha('a')[..12]);
            value["destination"]["commit"]["hash"] = json!(&sha('b')[..12]);
        }
        Ok(Reply::Json(value))
    })
    .unwrap();
    assert_eq!(snapshot.freshness, Freshness::Current);
    assert_eq!(
        resolved, 4,
        "both sides are resolved before and after the status read"
    );
}
#[test]
fn timeouts_are_explicit_and_do_not_issue_requests_after_the_deadline() {
    let timeout = Control::new(Duration::ZERO);
    let mut calls = 0;
    let result = load(&metadata(Provider::Github), &timeout, |_| {
        calls += 1;
        Err(Error::Network)
    });
    assert!(matches!(result, Err(Error::TimedOut)));
    assert_eq!(calls, 0);
    assert!(
        Issue::Provider(Error::TimedOut)
            .label()
            .contains("timed out")
    );
}
