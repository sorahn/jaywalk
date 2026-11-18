use super::*;
use std::collections::{BTreeMap, HashSet};
const LIMIT: usize = 500;
pub(super) fn load(
    metadata: &Metadata,
    control: &Control,
    mut request: impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Snapshot, Error> {
    let pr = url(
        Provider::Github,
        &metadata.locator.repository,
        &["pulls", &metadata.locator.number.to_string()],
    )?;
    let detail = get(pr.clone(), &mut request);
    let mut checks = Vec::new();
    let mut repositories = vec![("Target", &metadata.target.repository.name)];
    if metadata.source.repository.id != metadata.target.repository.id {
        repositories.push(("Fork", &metadata.source.repository.name));
    }
    for (label, repository) in repositories {
        control.check()?;
        for runs in [true, false] {
            let endpoint = url(
                Provider::Github,
                repository,
                &[
                    "commits",
                    &metadata.source.commit,
                    if runs { "check-runs" } else { "status" },
                ],
            )?;
            let listing = check_list(
                endpoint,
                runs,
                &metadata.source.commit,
                control,
                &mut request,
            );
            checks.push(CheckGroup {
                label: format!("{label} {}", if runs { "checks" } else { "statuses" }),
                checks: Availability::from_result(listing),
            });
        }
    }
    let reviews = match &detail {
        Ok(value) if freshness(metadata, value) == Ok(Freshness::Current) => {
            Availability::from_result(reviews(metadata, value, control, &mut request))
        }
        Ok(_) => Availability::Unavailable(Issue::NewVersion),
        Err(error) => Availability::Unavailable(Issue::Provider(*error)),
    };
    control.check()?;
    let verify = get(pr, &mut request);
    Ok(finish(metadata, &detail, verify, checks, reviews))
}
fn check_list(
    endpoint: Url,
    runs: bool,
    head: &str,
    control: &Control,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Listing<Check>, Error> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for page in 1..=5 {
        control.check()?;
        let mut url = endpoint.clone();
        url.query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        if runs {
            url.query_pairs_mut().append_pair("filter", "latest");
        }
        let value = get(url, request)?;
        if !runs && oid(&text(&value, "/sha", 40)?)? != head {
            return Err(Error::InvalidResponse);
        }
        let total = value["total_count"]
            .as_u64()
            .ok_or(Error::InvalidResponse)?;
        let rows = bounded_rows(&value, if runs { "/check_runs" } else { "/statuses" }, 100)?;
        if rows.is_empty() && (items.len() as u64) < total {
            return Err(Error::InvalidResponse);
        }
        for row in rows {
            if runs && oid(&text(row, "/head_sha", 40)?)? != head {
                return Err(Error::InvalidResponse);
            }
            let id = if runs {
                row["id"]
                    .as_u64()
                    .ok_or(Error::InvalidResponse)?
                    .to_string()
            } else {
                text(row, "/context", 512)?
            };
            if !seen.insert(id.clone()) {
                return Err(Error::InvalidResponse);
            }
            let name = text(row, if runs { "/name" } else { "/context" }, 512)?;
            let state = if runs {
                check_state(&text(row, "/status", 64)?, row["conclusion"].as_str())?
            } else {
                match text(row, "/state", 64)?.as_str() {
                    "success" => CheckState::Passed,
                    "failure" | "error" => CheckState::Failed,
                    "pending" => CheckState::Pending,
                    other => CheckState::Unknown(other.into()),
                }
            };
            items.push(Check {
                id,
                name,
                state,
                url: safe_url(row[if runs { "details_url" } else { "target_url" }].as_str()),
            });
        }
        if items.len() as u64 >= total {
            return Ok(Listing {
                items,
                partial: false,
            });
        }
    }
    Ok(Listing {
        items,
        partial: true,
    })
}
fn check_state(status: &str, conclusion: Option<&str>) -> Result<CheckState, Error> {
    if matches!(
        status,
        "queued" | "in_progress" | "requested" | "waiting" | "pending"
    ) {
        return Ok(CheckState::Pending);
    }
    if status != "completed" {
        return Ok(CheckState::Unknown(status.into()));
    }
    Ok(match conclusion {
        Some("success") => CheckState::Passed,
        Some("failure" | "timed_out" | "action_required" | "startup_failure") => CheckState::Failed,
        Some("neutral") => CheckState::Neutral,
        Some("skipped") => CheckState::Skipped,
        Some("cancelled") => CheckState::Cancelled,
        Some(other) if other.len() <= 128 && !other.chars().any(char::is_control) => {
            CheckState::Unknown(other.into())
        }
        None => CheckState::Unknown("No conclusion reported".into()),
        _ => return Err(Error::InvalidResponse),
    })
}
fn reviews(
    metadata: &Metadata,
    detail: &Value,
    control: &Control,
    request: &mut impl FnMut(Request) -> Result<Reply, Error>,
) -> Result<Listing<Reviewer>, Error> {
    let mut latest = BTreeMap::new();
    let mut seen = HashSet::new();
    let mut partial = false;
    let mut complete = false;
    for page in 1..=10 {
        control.check()?;
        let mut endpoint = url(
            Provider::Github,
            &metadata.locator.repository,
            &["pulls", &metadata.locator.number.to_string(), "reviews"],
        )?;
        endpoint
            .query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        let value = get(endpoint, request)?;
        let rows = bounded_rows(&value, "", 100)?;
        for row in rows {
            let id = row["id"].as_u64().ok_or(Error::InvalidResponse)?;
            if !seen.insert(id) {
                return Err(Error::InvalidResponse);
            }
            let state = text(row, "/state", 64)?;
            if state == "PENDING" {
                continue;
            }
            let user = row.get("user").filter(|v| !v.is_null());
            let (id, reviewer, decision) = match user {
                Some(user) => (
                    text(user, "/node_id", 512)?,
                    text(user, "/login", 256)?,
                    match state.as_str() {
                        "APPROVED" => Decision::Approved,
                        "CHANGES_REQUESTED" => Decision::ChangesRequested,
                        "COMMENTED" => Decision::Commented,
                        "DISMISSED" => Decision::Dismissed,
                        other => Decision::Unknown(other.into()),
                    },
                ),
                None => {
                    partial = true;
                    (
                        format!("deleted:{id}"),
                        "Unavailable reviewer".into(),
                        Decision::Unknown("Reviewer identity unavailable".into()),
                    )
                }
            };
            let commit = row["commit_id"].as_str().map(oid).transpose()?;
            // A comment-only review does not revoke an earlier approval or change request.
            if decision == Decision::Commented
                && latest
                    .get(&id)
                    .is_some_and(|r: &Reviewer| r.decision != Decision::Commented)
            {
                continue;
            }
            latest.insert(
                id.clone(),
                Reviewer {
                    id,
                    name: reviewer,
                    decision,
                    commit,
                    requested: false,
                },
            );
        }
        if rows.len() < 100 {
            complete = true;
            break;
        }
    }
    for (field, team) in [("/requested_reviewers", false), ("/requested_teams", true)] {
        let Some(rows) = detail.pointer(field) else {
            continue;
        };
        let rows = rows
            .as_array()
            .filter(|rows| rows.len() <= LIMIT)
            .ok_or(Error::InvalidResponse)?;
        for user in rows {
            let id = if team {
                format!("team:{}", text(user, "/node_id", 512)?)
            } else {
                text(user, "/node_id", 512)?
            };
            if let Some(review) = latest.get_mut(&id) {
                review.requested = true;
                continue;
            }
            latest.insert(
                id.clone(),
                Reviewer {
                    id,
                    name: text(user, if team { "/slug" } else { "/login" }, 256)?,
                    decision: Decision::Awaiting,
                    commit: None,
                    requested: true,
                },
            );
        }
    }
    if latest.len() > 1500 {
        return Err(Error::ContentLimit);
    }
    Ok(Listing {
        items: latest.into_values().collect(),
        partial: partial || !complete,
    })
}
