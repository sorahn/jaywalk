//! Fixed-provider read requests; continuation URLs never get arbitrary bearer access.
use super::{Batch, Locator, PullRequest, RepositoryName, Scope, model::bounded_text};
use crate::accounts::{Control, Error, Identity, Provider, Workspace};
use reqwest::Url;
use serde_json::{Value, json};
use std::collections::HashSet;

pub(crate) enum Request {
    Get(Url),
    GithubSearch {
        query: String,
        cursor: Option<String>,
    },
}
pub(crate) const GITHUB_QUERY: &str = r#"query($query:String!, $cursor:String) {
 search(query:$query, type:ISSUE, first:30, after:$cursor) {
  pageInfo { hasNextPage endCursor }
  nodes { ... on PullRequest { id number title url state headRefName
   author { login } repository { id nameWithOwner }
  } }
 }
}"#;
pub(crate) fn discover(
    identity: &Identity,
    scope: &Scope,
    workspaces: &[Workspace],
    control: &Control,
    mut request: impl FnMut(Request) -> Result<Value, Error>,
) -> Result<Batch, Error> {
    if !scope.supports(identity.key.provider) {
        return Err(Error::Unsupported);
    }
    if let Scope::Direct(locator) = scope {
        if locator.number == 0 {
            return Err(Error::InvalidResponse);
        }
        let mut url = api_url(locator.provider)?;
        let number = locator.number.to_string();
        let parts = match locator.provider {
            Provider::Github => vec![
                "repos",
                &locator.repository.owner,
                &locator.repository.name,
                "pulls",
                &number,
            ],
            Provider::Bitbucket => vec![
                "2.0",
                "repositories",
                &locator.repository.owner,
                &locator.repository.name,
                "pullrequests",
                &number,
            ],
        };
        url.path_segments_mut()
            .map_err(|_| Error::InvalidResponse)?
            .extend(parts);
        let value = request(Request::Get(url))?;
        control.check()?;
        let item = if locator.provider == Provider::Github {
            github_detail(&value)?
        } else {
            bitbucket_item(&value)?
        };
        if item.locator.number != locator.number
            || !item
                .locator
                .repository
                .to_string()
                .eq_ignore_ascii_case(&locator.repository.to_string())
        {
            return Err(Error::InvalidResponse);
        }
        return Ok(Batch {
            items: vec![item],
            priority_count: 0,
            coverage: vec![locator.label()],
            partial: false,
        });
    }
    if identity.key.provider == Provider::Github {
        return github(identity, scope, control, &mut request);
    }
    // Bitbucket's documented workspace endpoint only supports authored PRs.
    // Keep that limitation explicit rather than treating it as an empty inbox.
    if matches!(scope, Scope::ForMe) {
        let mut batch = discover(identity, &Scope::Authored, workspaces, control, request)?;
        batch.partial = true;
        return Ok(batch);
    }
    let mut batch = Batch::default();
    let starts = match scope {
        Scope::Authored => {
            batch.partial = workspaces.len() > 10;
            workspaces
                .iter()
                .take(10)
                .map(|workspace| {
                    let mut url = api_url(Provider::Bitbucket)?;
                    url.path_segments_mut()
                        .map_err(|_| Error::InvalidResponse)?
                        .extend([
                            "2.0",
                            "workspaces",
                            &workspace.slug,
                            "pullrequests",
                            &identity.key.subject,
                        ]);
                    Ok((url, workspace.slug.clone()))
                })
                .collect::<Result<Vec<_>, Error>>()?
        }
        Scope::Repository(_, repo) => {
            let mut url = api_url(Provider::Bitbucket)?;
            url.path_segments_mut()
                .map_err(|_| Error::InvalidResponse)?
                .extend([
                    "2.0",
                    "repositories",
                    &repo.owner,
                    &repo.name,
                    "pullrequests",
                ]);
            vec![(url, repo.to_string())]
        }
        _ => return Err(Error::Unsupported),
    };
    let mut seen = HashSet::new();
    let mut pages = 0;
    for (mut first, coverage) in starts {
        if pages >= 10 {
            batch.partial = true;
            break;
        }
        first.query_pairs_mut().extend_pairs([
            ("state", "OPEN"),
            ("pagelen", "20"),
            ("sort", "-updated_on"),
        ]);
        let path = first.path().to_owned();
        let mut next = Some(first);
        let mut per_scope = 0;
        while let Some(url) = next.take() {
            control.check()?;
            if pages >= 10 || per_scope >= 3 {
                batch.partial = true;
                break;
            }
            validate_page(&url, &path)?;
            if !seen.insert(url.to_string()) {
                return Err(Error::InvalidResponse);
            }
            let value = request(Request::Get(url))?;
            pages += 1;
            per_scope += 1;
            for value in value
                .get("values")
                .and_then(Value::as_array)
                .ok_or(Error::InvalidResponse)?
            {
                batch.items.push(bitbucket_item(value)?);
            }
            next = match value.get("next") {
                None | Some(Value::Null) => None,
                Some(Value::String(url)) => {
                    Some(Url::parse(url).map_err(|_| Error::InvalidResponse)?)
                }
                _ => return Err(Error::InvalidResponse),
            };
            if let Some(url) = &next {
                validate_page(url, &path)?;
            }
        }
        batch.coverage.push(coverage);
    }
    control.check()?;
    let mut ids = HashSet::new();
    batch
        .items
        .retain(|item| ids.insert((item.repository_id.clone(), item.locator.number)));
    Ok(batch)
}
fn github(
    identity: &Identity,
    scope: &Scope,
    control: &Control,
    request: &mut impl FnMut(Request) -> Result<Value, Error>,
) -> Result<Batch, Error> {
    if matches!(scope, Scope::ForMe) {
        let mut batch = Batch::default();
        let mut seen = HashSet::new();
        for scope in [Scope::Assigned, Scope::ReviewRequested, Scope::Authored] {
            let found = github(identity, &scope, control, request)?;
            batch.partial |= found.partial;
            batch.coverage.extend(found.coverage);
            batch.items.extend(
                found
                    .items
                    .into_iter()
                    .filter(|item| seen.insert(item.id.clone())),
            );
            if scope != Scope::Authored {
                batch.priority_count = batch.items.len();
            }
        }
        // Snapshots are bounded to 256 items, including all three search groups.
        batch.partial |= batch.items.len() > 256;
        batch.items.truncate(256);
        batch.priority_count = batch.priority_count.min(batch.items.len());
        return Ok(batch);
    }
    let user = &identity.label;
    if user.is_empty()
        || user.len() > 100
        || !user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(Error::InvalidResponse);
    }
    let filter = match scope {
        Scope::Authored => format!("author:{user}"),
        Scope::Assigned => format!("assignee:{user}"),
        Scope::ReviewRequested => format!("review-requested:{user}"),
        Scope::Repository(_, repo) => format!("repo:{repo}"),
        _ => return Err(Error::Unsupported),
    };
    let query = format!("is:pr is:open {filter} sort:updated-desc");
    let mut batch = Batch {
        coverage: vec![filter],
        ..Batch::default()
    };
    let mut cursor = None;
    let mut seen = HashSet::new();
    for _ in 0..3 {
        control.check()?;
        let value = request(Request::GithubSearch {
            query: query.clone(),
            cursor: cursor.clone(),
        })?;
        if let Some(errors) = value.get("errors").and_then(Value::as_array)
            && !errors.is_empty()
        {
            return Err(
                if errors
                    .iter()
                    .any(|error| error.get("type").and_then(Value::as_str) == Some("RATE_LIMITED"))
                {
                    Error::RateLimited
                } else {
                    Error::NoAccess
                },
            );
        }
        let search = value
            .pointer("/data/search")
            .ok_or(Error::InvalidResponse)?;
        for node in search
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidResponse)?
        {
            batch.items.push(github_node(node)?);
        }
        let more = search
            .pointer("/pageInfo/hasNextPage")
            .and_then(Value::as_bool)
            .ok_or(Error::InvalidResponse)?;
        batch.partial = more;
        if !more {
            break;
        }
        let next = bounded_text(
            search
                .pointer("/pageInfo/endCursor")
                .and_then(Value::as_str),
            2048,
        )?;
        if !seen.insert(next.clone()) {
            return Err(Error::InvalidResponse);
        }
        cursor = Some(next);
    }
    control.check()?;
    let mut seen = HashSet::new();
    batch.items.retain(|pr| seen.insert(pr.id.clone()));
    Ok(batch)
}
fn api_url(provider: Provider) -> Result<Url, Error> {
    Url::parse(match provider {
        Provider::Github => "https://api.github.com/",
        Provider::Bitbucket => "https://api.bitbucket.org/",
    })
    .map_err(|_| Error::InvalidResponse)
}
fn validate_page(url: &Url, path: &str) -> Result<(), Error> {
    if url.scheme() != "https"
        || url.host_str() != Some("api.bitbucket.org")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != path
    {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}
fn locator(url: Option<&str>, provider: Provider) -> Result<Locator, Error> {
    match super::EntryInput::parse(url.ok_or(Error::InvalidResponse)?)? {
        super::EntryInput::Url(locator) if locator.provider == provider => Ok(locator),
        _ => Err(Error::InvalidResponse),
    }
}
fn text(value: &Value, path: &str, limit: usize) -> Result<String, Error> {
    bounded_text(value.pointer(path).and_then(Value::as_str), limit)
}
fn github_node(value: &Value) -> Result<PullRequest, Error> {
    let locator = locator(value.get("url").and_then(Value::as_str), Provider::Github)?;
    let repo = RepositoryName::parse(&text(value, "/repository/nameWithOwner", 513)?)?;
    if repo != locator.repository
        || value.get("number").and_then(Value::as_u64) != Some(locator.number)
    {
        return Err(Error::InvalidResponse);
    }
    Ok(PullRequest {
        locator,
        id: text(value, "/id", 256)?,
        repository_id: text(value, "/repository/id", 256)?,
        title: text(value, "/title", 4096)?,
        author: value
            .pointer("/author/login")
            .and_then(Value::as_str)
            .map(|text| bounded_text(Some(text), 256))
            .transpose()?
            .unwrap_or_else(|| "Deleted account".into()),
        branch: text(value, "/headRefName", 1024)?,
        state: text(value, "/state", 32)?,
    })
}
fn github_detail(value: &Value) -> Result<PullRequest, Error> {
    let state = if value.get("merged").and_then(Value::as_bool) == Some(true) {
        Some("MERGED")
    } else {
        value.get("state").and_then(Value::as_str)
    };
    let adapted = json!({"id":value.get("node_id"),"number":value.get("number"),"url":value.get("html_url"),"title":value.get("title"),"author":value.get("user"),"repository":{"id":value.pointer("/base/repo/node_id"),"nameWithOwner":value.pointer("/base/repo/full_name")},"headRefName":value.pointer("/head/ref"),"state":state});
    github_node(&adapted)
}
fn bitbucket_item(value: &Value) -> Result<PullRequest, Error> {
    let locator = locator(
        value.pointer("/links/html/href").and_then(Value::as_str),
        Provider::Bitbucket,
    )?;
    let repo = RepositoryName::parse(&text(value, "/destination/repository/full_name", 513)?)?;
    if repo != locator.repository || value.get("id").and_then(Value::as_u64) != Some(locator.number)
    {
        return Err(Error::InvalidResponse);
    }
    let repository_id = text(value, "/destination/repository/uuid", 256)?;
    Ok(PullRequest {
        id: format!("{}:{}", repository_id, locator.number),
        repository_id,
        locator,
        title: text(value, "/title", 4096)?,
        author: bounded_text(
            value
                .pointer("/author/display_name")
                .and_then(Value::as_str)
                .or(Some("Deleted account")),
            256,
        )?,
        branch: text(value, "/source/branch/name", 1024)?,
        state: text(value, "/state", 32)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::AccountKey;
    use std::time::Duration;
    fn identity(provider: Provider) -> Identity {
        Identity {
            key: AccountKey {
                provider,
                subject: "{person}".into(),
            },
            label: "person".into(),
        }
    }
    fn control() -> Control {
        Control::new(Duration::from_secs(3))
    }
    fn github_value() -> Value {
        json!({"data":{"search":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"id":"PR_example","number":12,"title":"Change λ scrolling","url":"https://github.com/team/repo/pull/12","state":"OPEN","headRefName":"fix/momentum","author":{"login":"person"},"repository":{"id":"R_example","nameWithOwner":"team/repo"}}]}}})
    }
    fn bitbucket_value() -> Value {
        json!({"id":12,"title":"Fix λ scrolling","links":{"html":{"href":"https://bitbucket.org/team/repo/pull-requests/12"}},"destination":{"repository":{"full_name":"team/repo","uuid":"{repo}"}},"source":{"branch":{"name":"fix/momentum"}},"author":{"display_name":"Person"},"state":"OPEN"})
    }
    #[test]
    fn inbox_prioritizes_assignments_and_reviews_deduplicating_authored_results() {
        let mut queries = vec![];
        let batch = discover(
            &identity(Provider::Github),
            &Scope::ForMe,
            &[],
            &control(),
            |request| {
                let Request::GithubSearch { query, .. } = request else {
                    panic!()
                };
                let mut value = github_value();
                let number = if query.contains("assignee:") {
                    1
                } else if query.contains("review-requested:") {
                    2
                } else {
                    3
                };
                let mut node = value["data"]["search"]["nodes"][0].clone();
                node["number"] = json!(number);
                node["id"] = json!(format!("PR_{number}"));
                node["url"] = json!(format!("https://github.com/team/repo/pull/{number}"));
                let mut nodes = vec![node.clone()];
                if number == 1 {
                    node["number"] = json!(2);
                    node["id"] = json!("PR_2");
                    node["url"] = json!("https://github.com/team/repo/pull/2");
                    nodes.push(node);
                }
                if number == 3 {
                    let mut duplicate = nodes[0].clone();
                    duplicate["number"] = json!(1);
                    duplicate["id"] = json!("PR_1");
                    duplicate["url"] = json!("https://github.com/team/repo/pull/1");
                    nodes.push(duplicate);
                }
                value["data"]["search"]["nodes"] = json!(nodes);
                queries.push(query);
                Ok(value)
            },
        )
        .unwrap();
        assert_eq!(
            batch
                .items
                .iter()
                .map(|item| item.locator.number)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(batch.priority_count, 2);
        assert_eq!(queries.len(), 3);
        assert!(queries[0].contains("assignee:person"));
        assert!(queries[1].contains("review-requested:person"));
        assert!(queries[2].contains("author:person"));
        assert!(!batch.partial);
    }
    #[test]
    fn bitbucket_inbox_is_explicitly_partial_and_uses_authored_endpoint() {
        let batch = discover(
            &identity(Provider::Bitbucket),
            &Scope::ForMe,
            &[Workspace {
                slug: "team".into(),
                uuid: "{team}".into(),
            }],
            &control(),
            |request| {
                let Request::Get(url) = request else { panic!() };
                assert!(url.path().contains("/workspaces/team/pullrequests/"));
                Ok(json!({"values":[bitbucket_value()]}))
            },
        )
        .unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.priority_count, 0);
        assert!(batch.partial);
    }
    #[test]
    fn github_search_returns_branches_and_stable_ids_in_one_request() {
        let batch = discover(
            &identity(Provider::Github),
            &Scope::ReviewRequested,
            &[],
            &control(),
            |request| {
                let Request::GithubSearch { query, cursor } = request else {
                    panic!()
                };
                assert_eq!(
                    query,
                    "is:pr is:open review-requested:person sort:updated-desc"
                );
                assert!(cursor.is_none());
                Ok(github_value())
            },
        )
        .unwrap();
        assert_eq!(batch.items[0].repository_id, "R_example");
        assert_eq!(batch.items[0].branch, "fix/momentum");
        assert!(!batch.partial);
        let mut pages = 0;
        let batch = discover(
            &identity(Provider::Github),
            &Scope::Authored,
            &[],
            &control(),
            |_| {
                pages += 1;
                let mut value = github_value();
                value["data"]["search"]["pageInfo"] =
                    json!({"hasNextPage":true,"endCursor":format!("page-{pages}")});
                Ok(value)
            },
        )
        .unwrap();
        assert_eq!(pages, 3);
        assert!(batch.partial);
        assert_eq!(batch.items.len(), 1);
    }
    #[test]
    fn bitbucket_rejects_foreign_continuations_before_an_authenticated_request() {
        let workspace = Workspace {
            uuid: "{team}".into(),
            slug: "team".into(),
        };
        for next in [
            "https://evil.example/steal",
            "http://api.bitbucket.org/2.0/workspaces/team/pullrequests/%7Bperson%7D",
            "https://api.bitbucket.org/2.0/repositories/other/repo/pullrequests",
        ] {
            let mut calls = 0;
            let result = discover(
                &identity(Provider::Bitbucket),
                &Scope::Authored,
                std::slice::from_ref(&workspace),
                &control(),
                |request| {
                    calls += 1;
                    let Request::Get(url) = request else { panic!() };
                    assert_eq!(url.host_str(), Some("api.bitbucket.org"));
                    Ok(json!({"values":[bitbucket_value()],"next":next}))
                },
            );
            assert!(result.is_err());
            assert_eq!(calls, 1);
        }
        assert!(matches!(
            discover(
                &identity(Provider::Bitbucket),
                &Scope::ReviewRequested,
                &[],
                &control(),
                |_| panic!("unsupported scope never requests")
            ),
            Err(Error::Unsupported)
        ));
    }
    #[test]
    fn graph_errors_and_cancelled_reads_never_report_complete_empty_results() {
        assert!(matches!(
            discover(
                &identity(Provider::Github),
                &Scope::Authored,
                &[],
                &control(),
                |_| Ok(json!({"errors":[{"type":"RATE_LIMITED","message":"do not display"}]}))
            ),
            Err(Error::RateLimited)
        ));
        let cancelled = control();
        cancelled.cancel();
        assert!(matches!(
            discover(
                &identity(Provider::Github),
                &Scope::Authored,
                &[],
                &cancelled,
                |_| panic!("cancelled")
            ),
            Err(Error::Cancelled)
        ));
    }
    #[test]
    fn direct_lookup_verifies_provider_repository_and_number() {
        let locator = Locator {
            provider: Provider::Bitbucket,
            repository: RepositoryName::parse("team/repo").unwrap(),
            number: 12,
        };
        let batch = discover(
            &identity(Provider::Bitbucket),
            &Scope::Direct(locator.clone()),
            &[],
            &control(),
            |request| {
                let Request::Get(url) = request else { panic!() };
                assert_eq!(url.path(), "/2.0/repositories/team/repo/pullrequests/12");
                Ok(bitbucket_value())
            },
        )
        .unwrap();
        assert_eq!(batch.items[0].locator, locator);
        let other = Locator {
            number: 99,
            ..locator
        };
        assert!(
            discover(
                &identity(Provider::Bitbucket),
                &Scope::Direct(other),
                &[],
                &control(),
                |_| Ok(bitbucket_value())
            )
            .is_err()
        );
    }
    #[test]
    fn direct_github_lookup_distinguishes_merged_from_closed() {
        let value = json!({"node_id":"PR_example","number":12,"html_url":"https://github.com/team/repo/pull/12","title":"Example","user":{"login":"person"},"base":{"repo":{"node_id":"R_example","full_name":"team/repo"}},"head":{"ref":"topic"},"state":"closed","merged":true});
        assert_eq!(github_detail(&value).unwrap().state, "MERGED");
    }
}
