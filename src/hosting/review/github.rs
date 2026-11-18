use super::*;
use crate::{accounts::Provider, repository_file::ChangeKind};
pub(super) fn resolve(
    locator: &Locator,
    options: Options,
    control: &Control,
    mut request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Data, Error> {
    let pr_url = url(
        Provider::Github,
        &locator.repository,
        &["pulls", &locator.number.to_string()],
    )?;
    let detail = json(request(Request::Json(pr_url.clone()))?)?;
    if detail["number"].as_u64() != Some(locator.number) {
        return Err(Error::InvalidResponse);
    }
    let repo = |pointer: &str| -> Result<Repository, Error> {
        let value = detail.pointer(pointer).ok_or(Error::NotFound)?;
        Ok(Repository {
            name: RepositoryName::parse(&text(value, "/full_name", 513)?)?,
            id: text(value, "/node_id", 512)?,
        })
    };
    let target = Endpoint {
        repository: repo("/base/repo")?,
        commit: oid(&text(&detail, "/base/sha", 40)?)?,
    };
    let source = Endpoint {
        repository: repo("/head/repo")?,
        commit: oid(&text(&detail, "/head/sha", 40)?)?,
    };
    if target.repository.name != locator.repository {
        return Err(Error::InvalidResponse);
    }
    let compare = json(request(Request::Json(url(
        Provider::Github,
        &target.repository.name,
        &["compare", &format!("{}...{}", target.commit, source.commit)],
    )?))?)?;
    let base = Endpoint {
        repository: target.repository.clone(),
        commit: oid(&text(&compare, "/merge_base_commit/sha", 40)?)?,
    };
    let expected = detail["changed_files"]
        .as_u64()
        .ok_or(Error::InvalidResponse)? as usize;
    if expected > options.max_files || expected > 3000 {
        return Err(Error::ContentLimit);
    }
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while rows.len() < expected {
        control.check()?;
        let page = rows.len() / 100 + 1;
        let mut endpoint = url(
            Provider::Github,
            &locator.repository,
            &["pulls", &locator.number.to_string(), "files"],
        )?;
        endpoint
            .query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        let values = json(request(Request::Json(endpoint))?)?;
        let values = values
            .as_array()
            .filter(|values| !values.is_empty() && values.len() <= 100)
            .ok_or(Error::InvalidResponse)?;
        for value in values {
            let name = path(&text(value, "/filename", 4096)?)?;
            if !seen.insert(name) {
                return Err(Error::InvalidResponse);
            }
            rows.push(file_row(value)?);
        }
        if rows.len() > expected {
            return Err(Error::Superseded);
        }
    }
    let files = files(rows, &base, &source, control, &mut request)?;
    let mut commits = Vec::new();
    let commit_count = detail["commits"].as_u64().ok_or(Error::InvalidResponse)? as usize;
    let available = commit_count.min(250);
    let wanted = available.min(options.history_limit);
    let start = available - wanted;
    let mut page = start / 100 + 1;
    let mut skip = start % 100;
    while commits.len() < wanted {
        control.check()?;
        let mut endpoint = url(
            Provider::Github,
            &locator.repository,
            &["pulls", &locator.number.to_string(), "commits"],
        )?;
        endpoint
            .query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        let values = json(request(Request::Json(endpoint))?)?;
        let values = values
            .as_array()
            .filter(|values| !values.is_empty() && values.len() <= 100)
            .ok_or(Error::InvalidResponse)?;
        if values.len() <= skip {
            return Err(Error::InvalidResponse);
        }
        for value in values.iter().skip(skip).take(wanted - commits.len()) {
            let parents = value["parents"]
                .as_array()
                .ok_or(Error::InvalidResponse)?
                .iter()
                .map(|parent| oid(&text(parent, "/sha", 40)?))
                .collect::<Result<Vec<_>, Error>>()?;
            commits.push(revision(
                oid(&text(value, "/sha", 40)?)?,
                multiline(
                    value
                        .pointer("/commit/message")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    65536,
                )?,
                text(value, "/commit/author/name", 256)?,
                text(value, "/commit/author/date", 128)?,
                parents,
            ));
        }
        page += 1;
        skip = 0;
    }
    commits.reverse();
    let verify = json(request(Request::Json(pr_url))?)?;
    if verify.pointer("/head/sha") != detail.pointer("/head/sha")
        || verify.pointer("/base/sha") != detail.pointer("/base/sha")
        || verify["id"] != detail["id"]
        || verify.pointer("/head/repo/node_id") != detail.pointer("/head/repo/node_id")
        || verify.pointer("/base/repo/node_id") != detail.pointer("/base/repo/node_id")
    {
        return Err(Error::Superseded);
    }
    Ok(Data {
        metadata: Metadata {
            locator: locator.clone(),
            id: text(&detail, "/node_id", 512)?,
            title: text(&detail, "/title", 4096)?,
            body: multiline(detail["body"].as_str().unwrap_or(""), 262144)?,
            source_branch: detail
                .pointer("/head/ref")
                .filter(|v| !v.is_null())
                .map(|_| text(&detail, "/head/ref", 512))
                .transpose()?,
            target_branch: detail
                .pointer("/base/ref")
                .filter(|v| !v.is_null())
                .map(|_| text(&detail, "/base/ref", 512))
                .transpose()?,
            draft: detail["draft"].as_bool(),
            author: text(&detail, "/user/login", 256)?,
            state: if detail["merged"].as_bool() == Some(true) {
                "MERGED".into()
            } else {
                text(&detail, "/state", 64)?.to_ascii_uppercase()
            },
            source,
            target,
            base,
            history_partial: wanted < commit_count,
        },
        files,
        commits,
    })
}
fn tree_blob(
    endpoint: &Endpoint,
    path: &str,
    cache: &mut TreeCache,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
    control: &Control,
) -> Result<Blob, Error> {
    let mut tree = endpoint.commit.clone();
    let segments = path.split('/').collect::<Vec<_>>();
    for (index, part) in segments.iter().enumerate() {
        control.check()?;
        let endpoint_url = url(
            Provider::Github,
            &endpoint.repository.name,
            &["git", "trees", &tree],
        )?;
        if !cache.values.contains_key(endpoint_url.as_str()) {
            if cache.values.len() >= 1024 {
                return Err(Error::ContentLimit);
            }
            let value = json(request(Request::Json(endpoint_url.clone()))?)?;
            if value["truncated"].as_bool() == Some(true) {
                return Err(Error::ContentLimit);
            }
            let bytes = serde_json::to_vec(&value)
                .map_err(|_| Error::InvalidResponse)?
                .len();
            if cache.bytes.saturating_add(bytes) > 32 * 1_048_576 {
                return Err(Error::ContentLimit);
            }
            cache.bytes += bytes;
            cache.values.insert(endpoint_url.to_string(), value);
        }
        let value = &cache.values[endpoint_url.as_str()];
        let entry = value["tree"]
            .as_array()
            .ok_or(Error::InvalidResponse)?
            .iter()
            .find(|entry| entry["path"].as_str() == Some(part))
            .ok_or(Error::NotFound)?;
        tree = oid(&text(entry, "/sha", 40)?)?;
        if index + 1 < segments.len() {
            if entry["type"] != "tree" {
                return Err(Error::InvalidResponse);
            }
            continue;
        }
        let mode = text(entry, "/mode", 8)?;
        let kind = FileKind::from(mode.as_str());
        if !matches!(
            kind,
            FileKind::Regular | FileKind::Symlink | FileKind::Submodule
        ) {
            return Err(Error::InvalidResponse);
        }
        return Ok(Blob {
            url: url(
                Provider::Github,
                &endpoint.repository.name,
                &["git", "blobs", &tree],
            )?,
            kind,
            executable: mode == "100755",
        });
    }
    Err(Error::InvalidResponse)
}

fn files(
    rows: Vec<Value>,
    base: &Endpoint,
    source: &Endpoint,
    control: &Control,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Vec<File>, Error> {
    let mut trees = TreeCache::default();
    let mut files = Vec::new();
    for value in rows {
        control.check()?;
        let after = path(&text(&value, "/filename", 4096)?)?;
        let kind = match text(&value, "/status", 32)?.as_str() {
            "added" => ChangeKind::Added,
            "removed" => ChangeKind::Removed,
            "renamed" => ChangeKind::Renamed,
            "copied" => ChangeKind::Copied,
            "modified" | "changed" => ChangeKind::Modified,
            _ => return Err(Error::InvalidResponse),
        };
        let before = if matches!(kind, ChangeKind::Renamed | ChangeKind::Copied) {
            path(&text(&value, "/previous_filename", 4096)?)?
        } else {
            after.clone()
        };
        let before_blob = if kind == ChangeKind::Added {
            None
        } else {
            Some(tree_blob(base, &before, &mut trees, request, control)?)
        };
        let after_blob = if kind == ChangeKind::Removed {
            None
        } else {
            Some(tree_blob(source, &after, &mut trees, request, control)?)
        };
        files.push(File {
            change: ChangedFile::new(kind, before, after),
            before: before_blob,
            after: after_blob,
        });
    }
    Ok(files)
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
    let mut rows = Vec::new();
    loop {
        control.check()?;
        let mut endpoint = url(
            Provider::Github,
            &source.repository.name,
            &["commits", &source.commit],
        )?;
        endpoint
            .query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &(rows.len() / 100 + 1).to_string());
        let value = json(request(Request::Json(endpoint))?)?;
        if value["sha"].as_str() != Some(source.commit.as_str())
            || value.pointer("/parents/0/sha").and_then(Value::as_str) != Some(parent.as_str())
        {
            return Err(Error::InvalidResponse);
        }
        let page = value["files"].as_array().ok_or(Error::InvalidResponse)?;
        if rows.len() + page.len() > options.max_files {
            return Err(Error::ContentLimit);
        }
        rows.extend(
            page.iter()
                .map(file_row)
                .collect::<Result<Vec<_>, Error>>()?,
        );
        if page.len() < 100 {
            break;
        }
        if rows.len() >= 3000 {
            return Err(Error::ContentLimit);
        }
    }
    let files = files(rows, &base, &source, control, &mut request)?;
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

#[derive(Default)]
struct TreeCache {
    values: BTreeMap<String, Value>,
    bytes: usize,
}
// PR file rows can include large patches. Only retain the metadata needed to
// address immutable blobs; provider patch text never becomes the review source.
fn file_row(value: &Value) -> Result<Value, Error> {
    let name = path(&text(value, "/filename", 4096)?)?;
    let status = text(value, "/status", 32)?;
    let previous = value
        .get("previous_filename")
        .filter(|v| !v.is_null())
        .map(|_| path(&text(value, "/previous_filename", 4096)?))
        .transpose()?;
    Ok(serde_json::json!({"filename":name,"status":status,"previous_filename":previous}))
}
