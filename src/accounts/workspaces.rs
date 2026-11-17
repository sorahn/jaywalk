//! Current-account Bitbucket discovery, never inferred from the client's owner.
use super::{AccountKey, Control, Error};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub(crate) const ENDPOINT: &str = "https://api.bitbucket.org/2.0/user/workspaces";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub uuid: String,
    pub slug: String,
}
impl Workspace {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.uuid.is_empty()
            || self.uuid.len() > 128
            || self.uuid.chars().any(char::is_control)
            || self.slug.is_empty()
            || self.slug.len() > 256
            || !self
                .slug
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        {
            return Err(Error::InvalidResponse);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct WorkspaceListing {
    pub account: AccountKey,
    pub workspaces: Vec<Workspace>,
    pub(crate) epoch: u64,
}
pub(crate) fn discover(
    control: &Control,
    mut request: impl FnMut(Url) -> Result<Value, Error>,
) -> Result<Vec<Workspace>, Error> {
    let mut next = Some(format!("{ENDPOINT}?pagelen=20"));
    let mut visited = HashSet::new();
    let mut ids = HashSet::new();
    let mut found = Vec::new();
    while let Some(url) = next {
        control.check()?;
        let url = page_url(&url)?;
        if visited.len() >= 100 || !visited.insert(url.to_string()) {
            return Err(Error::InvalidResponse);
        }
        let value = request(url)?;
        for item in value
            .get("values")
            .and_then(Value::as_array)
            .ok_or(Error::InvalidResponse)?
        {
            let workspace: Workspace = serde_json::from_value(
                item.get("workspace").ok_or(Error::InvalidResponse)?.clone(),
            )
            .map_err(|_| Error::InvalidResponse)?;
            workspace.validate()?;
            if ids.insert(workspace.uuid.clone()) {
                found.push(workspace);
            }
        }
        next = match value.get("next") {
            None | Some(Value::Null) => None,
            Some(Value::String(next)) => Some(next.clone()),
            _ => return Err(Error::InvalidResponse),
        };
    }
    control.check()?;
    found.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(found)
}
fn page_url(value: &str) -> Result<Url, Error> {
    let url = Url::parse(value).map_err(|_| Error::InvalidResponse)?;
    if url.scheme() != "https"
        || url.host_str() != Some("api.bitbucket.org")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != "/2.0/user/workspaces"
    {
        return Err(Error::InvalidResponse);
    }
    Ok(url)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;
    #[test]
    fn pagination_deduplicates_and_rejects_foreign_urls_before_sending_bearer_auth() {
        let control = Control::new(Duration::from_secs(1));
        let mut calls = 0;
        let found = discover(&control, |_| {
            calls+=1;
            Ok(if calls==1 {json!({"values":[{"workspace":{"uuid":"one","slug":"team"}}],"next":format!("{ENDPOINT}?page=2")})}
                else {json!({"values":[{"workspace":{"uuid":"one","slug":"team"}},{"workspace":{"uuid":"two","slug":"another"}}]})})
        }).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].slug, "another");
        calls = 0;
        assert_eq!(
            discover(&control, |_| {
                calls += 1;
                Ok(json!({"values":[],"next":"https://other.invalid/steal"}))
            })
            .unwrap_err(),
            Error::InvalidResponse
        );
        assert_eq!(calls, 1);
        assert_eq!(
            discover(&control, |_| Ok(json!({"values":[],"next":ENDPOINT}))).unwrap_err(),
            Error::InvalidResponse
        );
    }
}
