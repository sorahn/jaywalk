use super::*;
use std::collections::{BTreeMap, HashSet};
pub(super) fn load(
    metadata: &Metadata,
    control: &Control,
    mut request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Snapshot, Error> {
    let pr = url(
        Provider::Bitbucket,
        &metadata.locator.repository,
        &["pullrequests", &metadata.locator.number.to_string()],
    )?;
    let detail = get(pr.clone(), &mut request)
        .and_then(|detail| resolve_hashes(metadata, detail, control, &mut request));
    // Commit statuses use the pinned head in its own fork repository. The PR
    // statuses endpoint follows the live head and can silently change versions.
    let endpoint = url(
        Provider::Bitbucket,
        &metadata.source.repository.name,
        &["commit", &metadata.source.commit, "statuses"],
    )?;
    let checks = vec![CheckGroup {
        label: "Head builds".into(),
        checks: Availability::from_result(checks(endpoint, control, &mut request)),
    }];
    let reviews = match &detail {
        Ok(value) if freshness(metadata, value) == Ok(Freshness::Current) => {
            Availability::from_result(reviews(value))
        }
        Ok(_) => Availability::Unavailable(Issue::NewVersion),
        Err(error) => Availability::Unavailable(Issue::Provider(*error)),
    };
    control.check()?;
    let verify = get(pr, &mut request)
        .and_then(|detail| resolve_hashes(metadata, detail, control, &mut request));
    Ok(finish(metadata, &detail, verify, checks, reviews))
}
fn checks(
    mut endpoint: Url,
    control: &Control,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Listing<Check>, Error> {
    endpoint.query_pairs_mut().append_pair("pagelen", "100");
    let original = endpoint.clone();
    let mut seen = HashSet::new();
    let mut ids = HashSet::new();
    let mut items = Vec::new();
    for page in 0..5 {
        control.check()?;
        if !seen.insert(endpoint.to_string()) {
            return Err(Error::InvalidResponse);
        }
        let value = get(endpoint, request)?;
        let rows = bounded_rows(&value, "/values", 100)?;
        for row in rows {
            let id = text(row, "/key", 512)?;
            if !ids.insert(id.clone()) {
                return Err(Error::InvalidResponse);
            }
            if let Some(link) = row.pointer("/links/commit/href").and_then(Value::as_str) {
                let actual = Url::parse(link).map_err(|_| Error::InvalidResponse)?;
                let expected = original.path().trim_end_matches("/statuses");
                if actual.scheme() != "https"
                    || actual.origin() != original.origin()
                    || actual.path() != expected
                    || !actual.username().is_empty()
                    || actual.password().is_some()
                    || actual.query().is_some()
                    || actual.fragment().is_some()
                {
                    return Err(Error::InvalidResponse);
                }
            }
            let state = match text(row, "/state", 64)?.as_str() {
                "SUCCESSFUL" => CheckState::Passed,
                "FAILED" | "ERROR" => CheckState::Failed,
                "INPROGRESS" | "PENDING" => CheckState::Pending,
                "STOPPED" => CheckState::Cancelled,
                other => CheckState::Unknown(other.into()),
            };
            let name = row["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(|_| text(row, "/name", 512))
                .transpose()?
                .unwrap_or_else(|| id.clone());
            items.push(Check {
                id,
                name,
                state,
                url: safe_url(row["url"].as_str()),
            });
        }
        let Some(next) = value["next"].as_str() else {
            return Ok(Listing {
                items,
                partial: false,
            });
        };
        let next = Url::parse(next).map_err(|_| Error::InvalidResponse)?;
        if next.origin() != original.origin()
            || next.path() != original.path()
            || !next.username().is_empty()
            || next.password().is_some()
            || next.fragment().is_some()
            || seen.contains(next.as_str())
        {
            return Err(Error::InvalidResponse);
        }
        if page == 4 {
            return Ok(Listing {
                items,
                partial: true,
            });
        }
        endpoint = next;
    }
    unreachable!("the last bounded page returns partial results")
}
fn reviews(detail: &Value) -> Result<Listing<Reviewer>, Error> {
    let participants = bounded_rows(detail, "/participants", 1000)?;
    let mut items = BTreeMap::new();
    for row in participants {
        let approved = row["approved"].as_bool().ok_or(Error::InvalidResponse)?;
        let role = text(row, "/role", 64)?;
        let state = row["state"].as_str();
        if role != "REVIEWER" && !approved && state != Some("changes_requested") {
            continue;
        }
        let decision = match state {
            Some("approved") if approved => Decision::Approved,
            Some("changes_requested") if !approved => Decision::ChangesRequested,
            None if approved => Decision::Approved,
            None => Decision::Awaiting,
            Some(other) if other.len() <= 64 && !other.chars().any(char::is_control) => {
                Decision::Unknown(other.into())
            }
            _ => return Err(Error::InvalidResponse),
        };
        let id = text(row, "/user/uuid", 512)?;
        if items
            .insert(
                id.clone(),
                Reviewer {
                    id,
                    name: text(row, "/user/display_name", 256)?,
                    decision,
                    commit: None,
                    requested: role == "REVIEWER",
                },
            )
            .is_some()
        {
            return Err(Error::InvalidResponse);
        }
    }
    // Reviewers may not have participated yet, so they need explicit waiting rows.
    for user in bounded_rows(detail, "/reviewers", 1000)? {
        let id = text(user, "/uuid", 512)?;
        if let Some(review) = items.get_mut(&id) {
            review.requested = true;
            continue;
        }
        items.insert(
            id.clone(),
            Reviewer {
                id,
                name: text(user, "/display_name", 256)?,
                decision: Decision::Awaiting,
                commit: None,
                requested: true,
            },
        );
    }
    Ok(Listing {
        items: items.into_values().collect(),
        partial: false,
    })
}

fn resolve_hashes(
    metadata: &Metadata,
    mut detail: Value,
    control: &Control,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Value, Error> {
    for (pointer, repository, id_pointer) in [
        (
            "/source/commit/hash",
            &metadata.source.repository,
            "/source/repository/uuid",
        ),
        (
            "/destination/commit/hash",
            &metadata.target.repository,
            "/destination/repository/uuid",
        ),
    ] {
        if text(&detail, id_pointer, 512)? != repository.id {
            return Err(Error::InvalidResponse);
        }
        let hash = text(&detail, pointer, 40)?;
        if oid(&hash).is_ok() {
            continue;
        }
        if !(12..40).contains(&hash.len()) || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::InvalidResponse);
        }
        control.check()?;
        let value = get(
            url(Provider::Bitbucket, &repository.name, &["commit", &hash])?,
            request,
        )?;
        let full = oid(&text(&value, "/hash", 40)?)?;
        if !full.starts_with(&hash.to_ascii_lowercase()) {
            return Err(Error::InvalidResponse);
        }
        *detail.pointer_mut(pointer).ok_or(Error::InvalidResponse)? = Value::String(full);
    }
    Ok(detail)
}
