//! Browser authorization for the developer's own GitHub App; no device-code setup.
use crate::{
    Result, Session, callback, check_cancelled, json, open_browser, protocol::Tokens,
    random_string, registration,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub(crate) const REDIRECT: &str = "http://127.0.0.1:43762/callback";

pub(crate) fn connect(client: &Client, client_id: &str, cancelled: &AtomicBool) -> Result<Session> {
    // Bind first so a busy port cannot leave a browser authorization without a receiver.
    let listener = callback::Listener::bind_github()?;
    let secret = match registration::browser_secret(client_id)? {
        Some(secret) => secret,
        None => {
            println!(
                "Existing developer registration: add {REDIRECT} as a Redirect URI under General > Identifying and authorizing users and save in GitHub App settings. Enter that app's client secret locally. This one-time publisher setup is not an end-user connection step."
            );
            rpassword::prompt_password("GitHub development client secret (hidden): ")
                .map_err(|_| "Cannot read developer secret from the terminal")?
        }
    };
    if secret.is_empty() {
        return Err("Browser authorization requires the development app's client secret".into());
    }
    check_cancelled(cancelled)?;
    let state = random_string();
    let verifier = random_string();
    let url = authorization_url(client_id, &state, &challenge(&verifier))?;
    println!("Opening GitHub sign-in in your browser; authorize and return here (Ctrl+C cancels).");
    open_browser(url.as_str())?;
    let code = listener.receive(&state, Duration::from_secs(3600), cancelled)?;
    check_cancelled(cancelled)?;
    let response = json(
        client
            .post(crate::GITHUB_TOKEN)
            .form(&exchange_parameters(client_id, &secret, &code, &verifier)),
    )?;
    check_cancelled(cancelled)?;
    Ok(Session {
        client_id: client_id.into(),
        tokens: Tokens::parse(&response)?,
        developer_secret: Some(secret),
        workspace: None,
    })
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn authorization_url(client_id: &str, state: &str, challenge: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse("https://github.com/login/oauth/authorize")
        .map_err(|_| "Invalid built-in GitHub authorization URL")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("redirect_uri", REDIRECT),
        ("state", state),
        ("scope", "offline_access"),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("prompt", "select_account"),
    ]);
    Ok(url)
}

fn exchange_parameters<'a>(
    client_id: &'a str,
    secret: &'a str,
    code: &'a str,
    verifier: &'a str,
) -> [(&'static str, &'a str); 5] {
    [
        ("client_id", client_id),
        ("client_secret", secret),
        ("code", code),
        ("redirect_uri", REDIRECT),
        ("code_verifier", verifier),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn pkce_matches_the_rfc7636_s256_test_vector() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let verifier = random_string();
        assert!((43..=128).contains(&verifier.len()));
        assert_eq!(challenge(&verifier).len(), 43);
    }

    #[test]
    fn browser_request_exposes_only_the_challenge_and_encodes_parameters() {
        let url = authorization_url(
            "synthetic&other=bad",
            "state with & ?",
            &challenge("verifier"),
        )
        .unwrap();
        assert_eq!(url.origin().ascii_serialization(), "https://github.com");
        assert_eq!(url.path(), "/login/oauth/authorize");
        let parameters: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(parameters.len(), 7);
        assert_eq!(parameters["client_id"], "synthetic&other=bad");
        assert_eq!(parameters["state"], "state with & ?");
        assert_eq!(parameters["redirect_uri"], REDIRECT);
        assert_eq!(parameters["code_challenge_method"], "S256");
        assert_eq!(parameters["scope"], "offline_access");
        assert!(!parameters.contains_key("client_secret"));
        assert!(!parameters.contains_key("code_verifier"));
        assert!(!parameters.contains_key("other"));
    }

    #[test]
    fn code_exchange_binds_the_code_to_the_original_callback_and_verifier() {
        let parameters: HashMap<_, _> =
            exchange_parameters("app", "synthetic-secret", "code", "verifier")
                .into_iter()
                .collect();
        assert_eq!(parameters["code_verifier"], "verifier");
        assert_eq!(parameters["redirect_uri"], REDIRECT);
        assert_eq!(parameters["client_secret"], "synthetic-secret");
        assert_eq!(parameters["code"], "code");
        assert!(!parameters.contains_key("device_code"));
    }
}
