use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
pub(crate) struct Tokens {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_in: Option<u64>,
}

impl Tokens {
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        if value.get("error").is_some() {
            return Err(oauth_error(value));
        }
        if value
            .get("token_type")
            .and_then(Value::as_str)
            .is_none_or(|kind| !kind.eq_ignore_ascii_case("bearer"))
        {
            return Err("Unsupported or missing token type".into());
        }
        let access_token = required(value, "access_token")?;
        let refresh_token = match value.get("refresh_token") {
            None | Some(Value::Null) => None,
            Some(Value::String(token)) if !token.is_empty() => Some(token.clone()),
            _ => return Err("Provider returned an invalid refresh token".into()),
        };
        let expires_in = match value.get("expires_in") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or("Provider returned an invalid token lifetime")?,
            ),
        };
        Ok(Self {
            access_token,
            refresh_token,
            expires_in,
        })
    }
}

pub(crate) struct Device {
    pub(crate) device_code: String,
    pub(crate) user_code: String,
    pub(crate) interval: u64,
    pub(crate) expires_in: u64,
}
impl Device {
    pub(crate) fn parse(value: &Value) -> Result<Self> {
        if value.get("error").is_some() {
            return Err(oauth_error(value));
        }
        let interval = value.get("interval").and_then(Value::as_u64).unwrap_or(5);
        let expires_in = value.get("expires_in").and_then(Value::as_u64).unwrap_or(0);
        if !(1..=900).contains(&interval) || !(1..=900).contains(&expires_in) {
            return Err("Provider returned an unsupported device lifetime or interval".into());
        }
        let user_code = required(value, "user_code")?;
        // Only this user-facing code is printed; reject control/terminal escape sequences.
        if user_code.len() > 32
            || !user_code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("Provider returned an invalid user code".into());
        }
        Ok(Self {
            device_code: required(value, "device_code")?,
            user_code,
            interval,
            expires_in,
        })
    }
}

pub(crate) enum Poll {
    Wait(u64),
    Authorized(Tokens),
}
pub(crate) fn poll(value: &Value, interval: u64) -> Result<Poll> {
    match value.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => Ok(Poll::Wait(interval)),
        Some("slow_down") => {
            let next = interval
                .saturating_add(5)
                .max(value.get("interval").and_then(Value::as_u64).unwrap_or(0));
            if next > 900 {
                return Err("Provider polling interval exceeds authorization lifetime".into());
            }
            Ok(Poll::Wait(next))
        }
        Some(_) => Err(oauth_error(value)),
        None => Tokens::parse(value).map(Poll::Authorized),
    }
}
fn required(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("Provider response missing {key}"))
}
fn oauth_error(value: &Value) -> String {
    // Do not reflect arbitrary provider error/error_description strings or credentials.
    match value.get("error").and_then(Value::as_str) {
        Some("access_denied") => "Authorization denied by user",
        Some("expired_token" | "token_expired") => "Authorization expired; start again",
        Some("bad_refresh_token" | "invalid_grant") => "Token grant rejected; reconnect",
        Some("device_flow_disabled") => "Enable device flow in the GitHub App registration",
        Some("incorrect_client_credentials" | "invalid_client") => {
            "Check development client registration"
        }
        _ => "Provider rejected the authorization request; details suppressed",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pending_and_slow_down_respect_provider_interval() {
        assert!(matches!(
            poll(&json!({"error":"authorization_pending"}), 5).unwrap(),
            Poll::Wait(5)
        ));
        assert!(matches!(
            poll(&json!({"error":"slow_down","interval":3}), 5).unwrap(),
            Poll::Wait(10)
        ));
        assert!(matches!(
            poll(&json!({"error":"slow_down","interval":30}), 10).unwrap(),
            Poll::Wait(30)
        ));
    }
    #[test]
    fn terminal_oauth_errors_do_not_leak_arbitrary_response_text() {
        for error in [
            "access_denied",
            "expired_token",
            "token_expired",
            "bad_refresh_token",
            "device_flow_disabled",
            "SENSITIVE",
        ] {
            let response = json!({"error":error,"error_description":"SENSITIVE"});
            let Err(message) = poll(&response, 5) else {
                panic!("terminal failure must stop polling")
            };
            assert!(!message.contains("SENSITIVE"));
        }
    }
    #[test]
    fn accepts_rotating_and_non_expiring_bearer_tokens() {
        let token = Tokens::parse(&json!({"access_token":"synthetic","token_type":"bearer","refresh_token":"rotated","expires_in":3600})).unwrap();
        assert_eq!(token.refresh_token.as_deref(), Some("rotated"));
        assert_eq!(token.expires_in, Some(3600));
        let token =
            Tokens::parse(&json!({"access_token":"synthetic","token_type":"Bearer"})).unwrap();
        assert!(token.expires_in.is_none());
        assert!(token.refresh_token.is_none());
    }
    #[test]
    fn malformed_tokens_and_lifetimes_are_rejected() {
        for value in [
            json!({"access_token":"x"}),
            json!({"access_token":"","token_type":"bearer"}),
            json!({"access_token":"x","token_type":"basic"}),
            json!({"access_token":"x","token_type":"bearer","expires_in":-1}),
            json!({"access_token":"x","token_type":"bearer","refresh_token":42}),
        ] {
            assert!(Tokens::parse(&value).is_err());
        }
    }
    #[test]
    fn device_codes_cannot_inject_terminal_escapes() {
        let mut value = json!({"device_code":"synthetic","user_code":"ABCD-EFGH","expires_in":900,"interval":5});
        assert!(Device::parse(&value).is_ok());
        value["user_code"] = json!("\u{1b}[2J");
        assert!(Device::parse(&value).is_err());
        value["user_code"] = json!("ABCD-EFGH");
        value["expires_in"] = json!(u64::MAX);
        assert!(Device::parse(&value).is_err());
    }
}
