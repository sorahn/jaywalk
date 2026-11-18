use super::{AccountKey, ClientRegistration, Control, Error, Identity, Provider, Secret, Tokens};
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::Value;
use std::{io::Read, time::Duration};

/// Refresh has no automatic retry: a lost response may already have consumed a
/// rotating refresh token. Session coordination decides what may be retried.
pub trait HostingProvider: Send + Sync {
    fn review_request(
        &self,
        _: &Secret,
        _: crate::hosting::review::Request,
        _: &Control,
    ) -> Result<crate::hosting::review::Reply, Error> {
        Err(Error::Unsupported)
    }

    fn pull_requests(
        &self,
        _: &Secret,
        _: &Identity,
        _: &crate::hosting::Scope,
        _: &[super::Workspace],
        _: &Control,
    ) -> Result<crate::hosting::Batch, Error> {
        Err(Error::Unsupported)
    }

    fn exchange(
        &self,
        registration: &ClientRegistration,
        code: &str,
        verifier: &str,
        control: &Control,
    ) -> Result<Tokens, Error>;
    fn refresh(
        &self,
        registration: &ClientRegistration,
        token: &Secret,
        control: &Control,
    ) -> Result<Tokens, Error>;
    fn identity(
        &self,
        provider: Provider,
        token: &Secret,
        control: &Control,
    ) -> Result<Identity, Error>;
    fn workspaces(&self, _: &Secret, _: &Control) -> Result<Vec<super::Workspace>, Error> {
        Err(Error::Unsupported)
    }
}
pub struct HttpProvider {
    client: Client,
}
impl HttpProvider {
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            client: Client::builder()
                .https_only(true)
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(20))
                .user_agent("jaywalk/hosting-auth")
                .build()
                .map_err(|_| Error::Network)?,
        })
    }
    fn token(&self, registration: &ClientRegistration) -> RequestBuilder {
        let url = match registration.provider {
            Provider::Github => "https://github.com/login/oauth/access_token",
            Provider::Bitbucket => "https://bitbucket.org/site/oauth2/access_token",
        };
        let request = self.client.post(url);
        match registration.provider {
            Provider::Github => request,
            Provider::Bitbucket => {
                request.basic_auth(&registration.client_id, Some(registration.secret.expose()))
            }
        }
    }
}
impl HostingProvider for HttpProvider {
    fn review_request(
        &self,
        token: &Secret,
        request: crate::hosting::review::Request,
        control: &Control,
    ) -> Result<crate::hosting::review::Reply, Error> {
        use crate::hosting::review::{Reply, Request};
        let (url, limit, raw) = match request {
            Request::Json(url) => (url, 16 * 1024 * 1024, false),
            Request::Bytes(url, limit) => (url, limit, true),
        };
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port_or_known_default() != Some(443)
            || !matches!(url.host_str(), Some("api.github.com" | "api.bitbucket.org"))
        {
            return Err(Error::InvalidResponse);
        }
        control.check()?;
        let request = self
            .client
            .get(url.clone())
            .bearer_auth(token.expose())
            .header(
                "Accept",
                if raw && url.host_str() == Some("api.github.com") {
                    "application/vnd.github.raw+json"
                } else if raw {
                    "application/octet-stream"
                } else {
                    "application/json"
                },
            );
        let mut response = request.send().map_err(|_| Error::Network)?;
        control.check()?;
        match response.status().as_u16() {
            200 => (),
            301 | 302 | 303 | 307 | 308 => {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(Error::InvalidResponse)?;
                return Ok(Reply::Redirect(
                    url.join(location).map_err(|_| Error::InvalidResponse)?,
                ));
            }
            401 => return Err(Error::NeedsSignIn),
            403 if response
                .headers()
                .get("x-ratelimit-remaining")
                .is_some_and(|value| value == "0") =>
            {
                return Err(Error::RateLimited);
            }
            403 => return Err(Error::NoAccess),
            404 => return Err(Error::NotFound),
            429 => return Err(Error::RateLimited),
            _ => return Err(Error::Network),
        }
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(Error::ContentLimit);
        }
        let mut bytes = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            control.check()?;
            let count = response.read(&mut chunk).map_err(|_| Error::Network)?;
            if count == 0 {
                break;
            }
            if bytes.len().saturating_add(count) > limit {
                return Err(Error::ContentLimit);
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        if raw {
            Ok(Reply::Bytes(bytes))
        } else {
            serde_json::from_slice(&bytes)
                .map(Reply::Json)
                .map_err(|_| Error::InvalidResponse)
        }
    }

    fn pull_requests(
        &self,
        token: &Secret,
        identity: &Identity,
        scope: &crate::hosting::Scope,
        workspaces: &[super::Workspace],
        control: &Control,
    ) -> Result<crate::hosting::Batch, Error> {
        crate::hosting::discover(identity, scope, workspaces, control, |request| {
            let request=match request {
                crate::hosting::Request::Get(url)=>self.client.get(url),
                crate::hosting::Request::GithubSearch {query,cursor}=>self.client.post("https://api.github.com/graphql").json(&serde_json::json!({"query":crate::hosting::GITHUB_QUERY,"variables":{"query":query,"cursor":cursor}})),
            };
            response_limited(request.bearer_auth(token.expose()), control, 2_097_152)
        })
    }

    fn exchange(
        &self,
        registration: &ClientRegistration,
        code: &str,
        verifier: &str,
        control: &Control,
    ) -> Result<Tokens, Error> {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", registration.provider.redirect()),
        ];
        if registration.provider == Provider::Github {
            form.extend([
                ("client_id", registration.client_id.as_str()),
                ("client_secret", registration.secret.expose()),
                ("code_verifier", verifier),
            ]);
        }
        tokens(response(self.token(registration).form(&form), control)?)
    }
    fn refresh(
        &self,
        registration: &ClientRegistration,
        token: &Secret,
        control: &Control,
    ) -> Result<Tokens, Error> {
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", token.expose()),
        ];
        if registration.provider == Provider::Github {
            form.extend([
                ("client_id", registration.client_id.as_str()),
                ("client_secret", registration.secret.expose()),
            ]);
        }
        tokens(response(self.token(registration).form(&form), control)?)
    }
    fn identity(
        &self,
        provider: Provider,
        token: &Secret,
        control: &Control,
    ) -> Result<Identity, Error> {
        let url = match provider {
            Provider::Github => "https://api.github.com/user",
            Provider::Bitbucket => "https://api.bitbucket.org/2.0/user",
        };
        let value = response(self.client.get(url).bearer_auth(token.expose()), control)?;
        let (subject, label) = match provider {
            Provider::Github => (
                value
                    .get("id")
                    .and_then(Value::as_u64)
                    .map(|id| id.to_string()),
                value.get("login"),
            ),
            Provider::Bitbucket => (
                value.get("uuid").and_then(Value::as_str).map(str::to_owned),
                value.get("display_name").or_else(|| value.get("nickname")),
            ),
        };
        let identity = Identity {
            key: AccountKey {
                provider,
                subject: subject.ok_or(Error::InvalidResponse)?,
            },
            label: label
                .and_then(Value::as_str)
                .ok_or(Error::InvalidResponse)?
                .into(),
        };
        identity.validate()?;
        Ok(identity)
    }
    fn workspaces(
        &self,
        token: &Secret,
        control: &Control,
    ) -> Result<Vec<super::Workspace>, Error> {
        super::workspaces::discover(control, |url| {
            response(self.client.get(url).bearer_auth(token.expose()), control)
        })
    }
}
fn response(request: RequestBuilder, control: &Control) -> Result<Value, Error> {
    response_limited(request, control, 65_536)
}
fn response_limited(
    request: RequestBuilder,
    control: &Control,
    limit: u64,
) -> Result<Value, Error> {
    // Deadline checks happen before side effects. A successful token response is
    // returned even if the caller cancelled meanwhile, so rotation can be saved.
    let timeout = control.remaining()?.min(Duration::from_secs(20));
    let response = request
        .timeout(timeout)
        .header("Accept", "application/json")
        .send()
        .map_err(|_| Error::Network)?;
    let status = response.status().as_u16();
    if response.status().as_u16() == 403
        && response
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|value| value == "0")
    {
        return Err(Error::RateLimited);
    }
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Network)?;
    if bytes.len() as u64 > limit {
        return Err(Error::InvalidResponse);
    }
    decode(status, &bytes)
}
fn decode(status: u16, bytes: &[u8]) -> Result<Value, Error> {
    let value = serde_json::from_slice::<Value>(bytes).map_err(|_| match status {
        401 => Error::NeedsSignIn,
        429 => Error::RateLimited,
        500..=599 => Error::Network,
        _ => Error::InvalidResponse,
    })?;
    match value.get("error").and_then(Value::as_str) {
        Some("access_denied") => return Err(Error::Denied),
        Some("invalid_grant" | "bad_refresh_token" | "expired_token") => {
            return Err(Error::NeedsSignIn);
        }
        Some("invalid_client" | "incorrect_client_credentials") => return Err(Error::Registration),
        Some(_) => return Err(Error::InvalidResponse),
        None => (),
    }
    match status {
        200..=299 => Ok(value),
        401 => Err(Error::NeedsSignIn),
        403 => Err(Error::NoAccess),
        404 => Err(Error::NotFound),
        429 => Err(Error::RateLimited),
        500..=599 => Err(Error::Network),
        _ => Err(Error::InvalidResponse),
    }
}
fn tokens(value: Value) -> Result<Tokens, Error> {
    if !value
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(Error::InvalidResponse);
    }
    let access = Secret::new(
        value
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidResponse)?
            .into(),
    )?;
    let refresh = match value.get("refresh_token") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(Secret::new(value.clone())?),
        _ => return Err(Error::InvalidResponse),
    };
    let expires_at = match value.get("expires_in") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            super::model::now()
                .checked_add(
                    value
                        .as_u64()
                        .filter(|value| *value > 0)
                        .ok_or(Error::InvalidResponse)?,
                )
                .ok_or(Error::InvalidResponse)?,
        ),
    };
    Ok(Tokens {
        access,
        refresh,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn provider_responses_cannot_leak_tokens_or_arbitrary_error_bodies() {
        for (status, body) in [
            (
                400,
                br#"{"error":"private-error","error_description":"secret"}"#.as_slice(),
            ),
            (401, b"private html"),
            (500, b"secret"),
        ] {
            let error = decode(status, body).unwrap_err();
            assert!(!error.to_string().contains("secret"));
            assert!(!error.to_string().contains("private"));
        }
        assert_eq!(decode(429, b"{}"), Err(Error::RateLimited));
        assert_eq!(
            decode(400, br#"{"error":"invalid_grant"}"#),
            Err(Error::NeedsSignIn)
        );
    }
    #[test]
    fn token_expiry_is_absolute_and_invalid_lifetimes_are_rejected() {
        let before = super::super::model::now();
        let parsed =
            tokens(json!({"access_token":"synthetic","token_type":"bearer","expires_in":3600}))
                .unwrap();
        assert!(parsed.expires_at.unwrap() >= before + 3600);
        for expiry in [json!(0), json!(-1), json!("3600"), json!(u64::MAX)] {
            assert!(
                tokens(
                    json!({"access_token":"synthetic","token_type":"bearer","expires_in":expiry})
                )
                .is_err()
            );
        }
        assert_eq!(
            format!("{:?}", Secret::new("hidden".into()).unwrap()),
            "[redacted]"
        );
    }
}
