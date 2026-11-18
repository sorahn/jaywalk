use super::*;
use crate::{accounts::Provider, repository_file::ChangeKind};
pub(super) fn resolve(
    locator: &Locator,
    options: Options,
    control: &Control,
    mut request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Data, Error> {
    let pr_url = url(
        Provider::Bitbucket,
        &locator.repository,
        &["pullrequests", &locator.number.to_string()],
    )?;
    let detail = json(request(Request::Json(pr_url.clone()))?)?;
    if detail["id"].as_u64() != Some(locator.number) {
        return Err(Error::InvalidResponse);
    }
    let repo = |pointer: &str| -> Result<Repository, Error> {
        let value = detail.pointer(pointer).ok_or(Error::NotFound)?;
        Ok(Repository {
            name: RepositoryName::parse(&text(value, "/full_name", 513)?)?,
            id: text(value, "/uuid", 512)?,
        })
    };
    let source_repo = repo("/source/repository")?;
    let target_repo = repo("/destination/repository")?;
    if target_repo.name != locator.repository {
        return Err(Error::InvalidResponse);
    }
    let source = Endpoint {
        commit: full_commit(
            &source_repo,
            &text(&detail, "/source/commit/hash", 40)?,
            &mut request,
        )?,
        repository: source_repo,
    };
    let target = Endpoint {
        commit: full_commit(
            &target_repo,
            &text(&detail, "/destination/commit/hash", 40)?,
            &mut request,
        )?,
        repository: target_repo,
    };
    let endpoint = url(
        Provider::Bitbucket,
        &locator.repository,
        &["pullrequests", &locator.number.to_string(), "diffstat"],
    )?;
    let (mut value, stat_url) = match request(Request::Json(endpoint.clone()))? {
        Reply::Json(value) => (value, endpoint),
        Reply::Redirect(next) => {
            validate_stat(&next, &target.repository.name)?;
            (json(request(Request::Json(next.clone()))?)?, next)
        }
        _ => return Err(Error::InvalidResponse),
    };
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    seen.insert(stat_url.to_string());
    loop {
        control.check()?;
        let values = value["values"].as_array().ok_or(Error::InvalidResponse)?;
        if rows.len() + values.len() > options.max_files {
            return Err(Error::ContentLimit);
        }
        rows.extend(values.iter().cloned());
        let Some(next) = value["next"].as_str() else {
            break;
        };
        let next = Url::parse(next).map_err(|_| Error::InvalidResponse)?;
        if next.origin() != stat_url.origin()
            || next.path() != stat_url.path()
            || !next.username().is_empty()
            || next.password().is_some()
            || next.fragment().is_some()
            || !seen.insert(next.to_string())
            || seen.len() > 100
        {
            return Err(Error::InvalidResponse);
        }
        value = json(request(Request::Json(next))?)?;
    }
    let (files, base) = files(rows, &source, &target, &mut request)?;
    let base = match base {
        Some(base) => base,
        None => {
            let spec = if source.repository.name == target.repository.name {
                format!("{}..{}", source.commit, target.commit)
            } else {
                format!(
                    "{}:{}..{}",
                    source.repository.name, source.commit, target.commit
                )
            };
            let value = json(request(Request::Json(url(
                Provider::Bitbucket,
                &target.repository.name,
                &["merge-base", &spec],
            )?))?)?;
            Endpoint {
                repository: target.repository.clone(),
                commit: oid(&text(&value, "/hash", 40)?)?,
            }
        }
    };
    let mut commits = Vec::new();
    let mut history_partial = false;
    let mut endpoint = url(
        Provider::Bitbucket,
        &locator.repository,
        &["pullrequests", &locator.number.to_string(), "commits"],
    )?;
    endpoint.query_pairs_mut().append_pair("pagelen", "100");
    let mut visited = std::collections::HashSet::new();
    loop {
        control.check()?;
        if !visited.insert(endpoint.to_string()) || visited.len() > 100 {
            return Err(Error::InvalidResponse);
        }
        let values = json(request(Request::Json(endpoint.clone()))?)?;
        let rows = values["values"].as_array().ok_or(Error::InvalidResponse)?;
        let page_truncated = commits.len() + rows.len() > options.history_limit;
        for row in rows
            .iter()
            .take(options.history_limit.saturating_sub(commits.len()))
        {
            let parents = row["parents"]
                .as_array()
                .ok_or(Error::InvalidResponse)?
                .iter()
                .map(|value| oid(&text(value, "/hash", 40)?))
                .collect::<Result<Vec<_>, Error>>()?;
            commits.push(revision(
                oid(&text(row, "/hash", 40)?)?,
                multiline(row["message"].as_str().unwrap_or(""), 65536)?,
                text(row, "/author/raw", 512)?,
                text(row, "/date", 128)?,
                parents,
            ));
        }
        let next = values["next"].as_str();
        if commits.len() >= options.history_limit {
            history_partial = next.is_some() || page_truncated;
            break;
        }
        let Some(next) = next else {
            break;
        };
        let next = Url::parse(next).map_err(|_| Error::InvalidResponse)?;
        if next.origin() != endpoint.origin()
            || next.path() != endpoint.path()
            || !next.username().is_empty()
            || next.password().is_some()
            || next.fragment().is_some()
        {
            return Err(Error::InvalidResponse);
        }
        endpoint = next;
    }
    let verify = json(request(Request::Json(pr_url))?)?;
    for pointer in [
        "/source/commit/hash",
        "/destination/commit/hash",
        "/source/repository/uuid",
        "/destination/repository/uuid",
    ] {
        if verify.pointer(pointer) != detail.pointer(pointer) {
            return Err(Error::Superseded);
        }
    }
    Ok(Data {
        metadata: Metadata {
            locator: locator.clone(),
            id: format!("{}:{}", target.repository.id, locator.number),
            title: text(&detail, "/title", 4096)?,
            body: multiline(detail["description"].as_str().unwrap_or(""), 262144)?,
            source_branch: detail
                .pointer("/source/branch/name")
                .filter(|v| !v.is_null())
                .map(|_| text(&detail, "/source/branch/name", 512))
                .transpose()?,
            target_branch: detail
                .pointer("/destination/branch/name")
                .filter(|v| !v.is_null())
                .map(|_| text(&detail, "/destination/branch/name", 512))
                .transpose()?,
            draft: detail["draft"].as_bool(),
            author: text(&detail, "/author/display_name", 256)?,
            state: text(&detail, "/state", 64)?,
            source,
            target,
            base,
            history_partial,
        },
        files,
        commits,
    })
}
fn full_commit(
    repo: &Repository,
    value: &str,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<String, Error> {
    if let Ok(full) = oid(value) {
        return Ok(full);
    }
    if value.len() < 12 || value.len() > 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidResponse);
    }
    let result = json(request(Request::Json(url(
        Provider::Bitbucket,
        &repo.name,
        &["commit", value],
    )?))?)?;
    let full = oid(&text(&result, "/hash", 40)?)?;
    if !full.starts_with(&value.to_ascii_lowercase()) {
        return Err(Error::InvalidResponse);
    }
    Ok(full)
}
fn validate_stat(value: &Url, repo: &RepositoryName) -> Result<(), Error> {
    let prefix = url(Provider::Bitbucket, repo, &["diffstat", ""])?;
    if value.scheme() != "https"
        || value.host_str() != Some("api.bitbucket.org")
        || value.port_or_known_default() != Some(443)
        || !value.username().is_empty()
        || value.password().is_some()
        || value.fragment().is_some()
        || !value.path().starts_with(prefix.path())
    {
        Err(Error::InvalidResponse)
    } else {
        Ok(())
    }
}
fn entry_endpoint(
    value: &Value,
    path: &str,
    source: &Endpoint,
    target: &Endpoint,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Endpoint, Error> {
    let value =
        Url::parse(&text(value, "/links/self/href", 8192)?).map_err(|_| Error::InvalidResponse)?;
    if value.scheme() != "https"
        || value.host_str() != Some("api.bitbucket.org")
        || value.port_or_known_default() != Some(443)
        || !value.username().is_empty()
        || value.password().is_some()
        || value.query().is_some()
        || value.fragment().is_some()
    {
        return Err(Error::InvalidResponse);
    }
    let parts = value
        .path_segments()
        .ok_or(Error::InvalidResponse)?
        .collect::<Vec<_>>();
    if parts.len() < 7 || parts[0] != "2.0" || parts[1] != "repositories" || parts[4] != "src" {
        return Err(Error::InvalidResponse);
    }
    let name = RepositoryName::parse(&format!("{}/{}", parts[2], parts[3]))?;
    let repo = if name == source.repository.name {
        source.repository.clone()
    } else if name == target.repository.name {
        target.repository.clone()
    } else {
        return Err(Error::InvalidResponse);
    };
    let expected = raw_url(&repo.name, parts[5], path)?;
    if expected.path() != value.path() {
        return Err(Error::InvalidResponse);
    }
    Ok(Endpoint {
        commit: full_commit(&repo, parts[5], request)?,
        repository: repo,
    })
}
fn raw_url(repo: &RepositoryName, commit: &str, path: &str) -> Result<Url, Error> {
    let mut value = url(Provider::Bitbucket, repo, &["src", commit])?;
    value
        .path_segments_mut()
        .map_err(|_| Error::InvalidResponse)?
        .extend(path.split('/'));
    Ok(value)
}
fn blob(
    endpoint: &Endpoint,
    path: &str,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Blob, Error> {
    let url = raw_url(&endpoint.repository.name, &endpoint.commit, path)?;
    let mut meta = url.clone();
    meta.query_pairs_mut().append_pair("format", "meta");
    let value = json(request(Request::Json(meta))?)?;
    let attributes = value["attributes"]
        .as_array()
        .ok_or(Error::InvalidResponse)?;
    let has = |name: &str| attributes.iter().any(|value| value.as_str() == Some(name));
    Ok(Blob {
        url,
        kind: if has("subrepository") {
            FileKind::Submodule
        } else if has("link") {
            FileKind::Symlink
        } else {
            FileKind::Regular
        },
        executable: has("executable"),
    })
}

fn files(
    rows: Vec<Value>,
    source: &Endpoint,
    target: &Endpoint,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<(Vec<File>, Option<Endpoint>), Error> {
    let mut files = Vec::new();
    let mut base = None;
    let mut paths = std::collections::HashSet::new();
    for row in rows {
        let kind = match text(&row, "/status", 32)?.as_str() {
            "added" => ChangeKind::Added,
            "removed" => ChangeKind::Removed,
            "renamed" => ChangeKind::Renamed,
            "modified" => ChangeKind::Modified,
            "copied" => ChangeKind::Copied,
            _ => return Err(Error::InvalidResponse),
        };
        let old = row.get("old").filter(|value| !value.is_null());
        let new = row.get("new").filter(|value| !value.is_null());
        let old_path = old
            .map(|value| path(&text(value, "/path", 4096)?))
            .transpose()?;
        let new_path = new
            .map(|value| path(&text(value, "/path", 4096)?))
            .transpose()?;
        let before_path = old_path
            .as_ref()
            .or(new_path.as_ref())
            .ok_or(Error::InvalidResponse)?
            .clone();
        let after_path = new_path
            .as_ref()
            .or(old_path.as_ref())
            .ok_or(Error::InvalidResponse)?
            .clone();
        if !paths.insert(after_path.clone()) {
            return Err(Error::InvalidResponse);
        }
        let before = if let Some(old) = old {
            let endpoint = entry_endpoint(old, &before_path, source, target, request)?;
            if base.as_ref().is_some_and(|base: &Endpoint| {
                base.commit != endpoint.commit || base.repository.id != endpoint.repository.id
            }) {
                return Err(Error::InvalidResponse);
            }
            base = Some(endpoint.clone());
            Some(blob(&endpoint, &before_path, request)?)
        } else {
            None
        };
        let after = if let Some(new) = new {
            let endpoint = entry_endpoint(new, &after_path, source, target, request)?;
            if endpoint.commit != source.commit {
                return Err(Error::Superseded);
            }
            Some(blob(&endpoint, &after_path, request)?)
        } else {
            None
        };
        if (kind == ChangeKind::Added && before.is_some())
            || (kind == ChangeKind::Removed && after.is_some())
        {
            return Err(Error::InvalidResponse);
        }
        files.push(File {
            change: ChangedFile::new(kind, before_path, after_path),
            before,
            after,
        });
    }
    Ok((files, base))
}

pub(super) fn for_revision(
    data: &Data,
    revision: &Revision,
    options: Options,
    control: &Control,
    mut request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Data, Error> {
    let parent = revision.parents.first().ok_or(Error::Unsupported)?;
    let source = Endpoint {
        repository: data.metadata.source.repository.clone(),
        commit: revision.commit_id.clone(),
    };
    let base = Endpoint {
        repository: source.repository.clone(),
        commit: parent.clone(),
    };
    let mut endpoint = url(
        Provider::Bitbucket,
        &source.repository.name,
        &["diffstat", &source.commit],
    )?;
    endpoint.query_pairs_mut().append_pair("pagelen", "100");
    let original = endpoint.clone();
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    loop {
        control.check()?;
        if !seen.insert(endpoint.to_string()) || seen.len() > 100 {
            return Err(Error::InvalidResponse);
        }
        let value = json(request(Request::Json(endpoint))?)?;
        let page = value["values"].as_array().ok_or(Error::InvalidResponse)?;
        if rows.len() + page.len() > options.max_files {
            return Err(Error::ContentLimit);
        }
        rows.extend(page.iter().cloned());
        let Some(next) = value["next"].as_str() else {
            break;
        };
        endpoint = Url::parse(next).map_err(|_| Error::InvalidResponse)?;
        if endpoint.origin() != original.origin()
            || endpoint.path() != original.path()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Error::InvalidResponse);
        }
    }
    let (files, actual_base) = files(rows, &source, &base, &mut request)?;
    if actual_base.is_some_and(|actual| actual.commit != base.commit) {
        return Err(Error::InvalidResponse);
    }
    let mut metadata = data.metadata.clone();
    metadata.source = source;
    metadata.target = base.clone();
    metadata.base = base;
    Ok(Data {
        metadata,
        files,
        commits: vec![revision.clone()],
    })
}
