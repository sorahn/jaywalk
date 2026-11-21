//! Minimal one-shot browser callback. The caller opens the authorization URL;
//! credentials, state, code and PKCE verifier never enter diagnostics.
use super::{Control, Error, Provider};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

pub struct BrowserAttempt {
    listener: TcpListener,
    url: String,
    state: String,
    verifier: String,
}
impl BrowserAttempt {
    pub fn new(provider: Provider, client_id: &str) -> Result<Self, Error> {
        let redirect = Url::parse(provider.redirect()).map_err(|_| Error::Registration)?;
        let listener =
            TcpListener::bind(("127.0.0.1", redirect.port().ok_or(Error::Registration)?))
                .map_err(|_| Error::CallbackBusy)?;
        Self::with_listener(provider, client_id, listener)
    }
    pub(super) fn with_listener(
        provider: Provider,
        client_id: &str,
        listener: TcpListener,
    ) -> Result<Self, Error> {
        listener
            .set_nonblocking(true)
            .map_err(|_| Error::CallbackBusy)?;
        let state = random();
        let verifier = random();
        let endpoint = match provider {
            Provider::Github => "https://github.com/login/oauth/authorize",
            Provider::Bitbucket => "https://bitbucket.org/site/oauth2/authorize",
        };
        let mut url = Url::parse(endpoint).map_err(|_| Error::Registration)?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", client_id),
            ("redirect_uri", provider.redirect()),
            ("state", &state),
        ]);
        match provider {
            Provider::Github => {
                url.query_pairs_mut().extend_pairs([
                    ("scope", "offline_access"),
                    ("code_challenge_method", "S256"),
                    ("code_challenge", &challenge(&verifier)),
                    ("prompt", "select_account"),
                ]);
            }
            Provider::Bitbucket => {
                url.query_pairs_mut().append_pair("response_type", "code");
            }
        }
        Ok(Self {
            listener,
            url: url.into(),
            state,
            verifier,
        })
    }
    /// Treat the URL as transient browser input, not text for logs or clipboard.
    pub fn authorization_url(&self) -> &str {
        &self.url
    }
    pub fn receive(self, control: &Control) -> Result<(String, String), Error> {
        loop {
            control.check()?;
            let (mut socket, _) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                Err(_) => return Err(Error::Network),
            };
            // macOS accepted sockets inherit nonblocking mode. Browsers can
            // connect before sending any data, so switch before timed reads.
            socket.set_nonblocking(false).map_err(|_| Error::Network)?;
            socket
                .set_read_timeout(Some(Duration::from_millis(100)))
                .map_err(|_| Error::Network)?;
            socket
                .set_write_timeout(Some(Duration::from_millis(100)))
                .map_err(|_| Error::Network)?;
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut bytes = Vec::new();
            while bytes.len() < 8192 && Instant::now() < deadline {
                control.check()?;
                let mut chunk = [0_u8; 512];
                match socket.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        continue;
                    }
                    Err(_) => break,
                }
                if bytes.windows(4).any(|pair| pair == b"\r\n\r\n") {
                    break;
                }
            }
            control.check()?;
            let result = parse(&bytes, &self.state);
            let (status, message) = match &result {
                Ok(_) => ("200 OK", "Authorization received. Return to Jaywalk."),
                Err(Error::Denied) => (
                    "400 Bad Request",
                    "Authorization declined. Return to Jaywalk.",
                ),
                Err(_) => (
                    "400 Bad Request",
                    "This callback is invalid or expired. Return to Jaywalk.",
                ),
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{message}",
                message.len()
            );
            let _ = socket.write_all(response.as_bytes());
            match result {
                Ok(code) => return Ok((code, self.verifier)),
                Err(Error::Denied) => return Err(Error::Denied),
                Err(_) => continue,
            }
        }
    }
}
fn random() -> String {
    let mut bytes = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
fn parse(bytes: &[u8], state: &str) -> Result<String, Error> {
    if state.is_empty() || bytes.len() >= 8192 || !bytes.windows(4).any(|pair| pair == b"\r\n\r\n")
    {
        return Err(Error::InvalidResponse);
    }
    let line = bytes
        .windows(2)
        .position(|pair| pair == b"\r\n")
        .ok_or(Error::InvalidResponse)?;
    let mut parts = std::str::from_utf8(&bytes[..line])
        .map_err(|_| Error::InvalidResponse)?
        .split_whitespace();
    let (Some("GET"), Some(target), Some("HTTP/1.0" | "HTTP/1.1"), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::InvalidResponse);
    };
    if !target.starts_with("/callback?") || target.contains('#') {
        return Err(Error::InvalidResponse);
    }
    let url =
        Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| Error::InvalidResponse)?;
    if url.path() != "/callback" {
        return Err(Error::InvalidResponse);
    }
    let parameters: Vec<_> = url.query_pairs().collect();
    let mut seen = HashSet::new();
    if parameters.iter().any(|(key, _)| !seen.insert(key.as_ref())) {
        return Err(Error::InvalidResponse);
    }
    let get = |key: &str| {
        parameters
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_ref())
    };
    if get("state") != Some(state) {
        return Err(Error::InvalidResponse);
    }
    if get("error").is_some() {
        return if get("code").is_some() {
            Err(Error::InvalidResponse)
        } else {
            Err(Error::Denied)
        };
    }
    get("code")
        .filter(|code| {
            !code.is_empty() && code.len() <= 2048 && !code.chars().any(char::is_control)
        })
        .map(str::to_owned)
        .ok_or(Error::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpStream, thread};
    fn request(target: &str) -> Vec<u8> {
        format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").into_bytes()
    }
    #[test]
    fn state_duplicates_and_provider_text_are_not_authorization() {
        for target in [
            "/callback?code=x",
            "/callback?state=wrong&code=x",
            "/callback?state=ok&state=ok&code=x",
            "/callback?state=ok&code=x&code=y",
            "/callback?state=ok&code=x&error=denied",
            "/callback?state=ok&code=x#fragment",
            "http://evil/callback?state=ok&code=x",
        ] {
            assert!(parse(&request(target), "ok").is_err());
        }
        assert_eq!(
            parse(
                &request("/callback?state=ok&error=private-description"),
                "ok"
            ),
            Err(Error::Denied)
        );
    }
    #[test]
    fn delayed_native_callback_ignores_invalid_requests_and_closes_the_success_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let attempt =
            BrowserAttempt::with_listener(Provider::Github, "synthetic", listener).unwrap();
        let state = attempt.state.clone();
        let worker = thread::spawn(move || attempt.receive(&Control::new(Duration::from_secs(3))));
        let mut invalid = TcpStream::connect(address).unwrap();
        invalid
            .write_all(&request("/callback?state=wrong&code=discard"))
            .unwrap();
        let mut ignored = String::new();
        invalid.read_to_string(&mut ignored).unwrap();
        let mut valid = TcpStream::connect(address).unwrap();
        thread::sleep(Duration::from_millis(150));
        valid
            .write_all(&request(&format!("/callback?state={state}&code=synthetic")))
            .unwrap();
        assert_eq!(worker.join().unwrap().unwrap().0, "synthetic");
        // Check the connection this attempt owns. Once the listener closes,
        // another parallel test can reuse its ephemeral port.
        valid
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut response = String::new();
        valid.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(response.matches("HTTP/1.1").count(), 1);
    }
    #[test]
    fn callback_wait_checks_timeout_cancellation_and_port_conflicts() {
        for cancel in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let attempt =
                BrowserAttempt::with_listener(Provider::Bitbucket, "synthetic", listener).unwrap();
            let control = Control::new(if cancel {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            });
            if cancel {
                control.cancel();
            }
            assert_eq!(
                attempt.receive(&control).unwrap_err(),
                if cancel {
                    Error::Cancelled
                } else {
                    Error::TimedOut
                }
            );
        }
        let attempt = BrowserAttempt::new(Provider::Github, "synthetic").unwrap();
        assert!(matches!(
            BrowserAttempt::new(Provider::Github, "other"),
            Err(Error::CallbackBusy)
        ));
        drop(attempt);
    }
    #[test]
    fn github_browser_url_uses_pkce_without_exposing_the_verifier() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let attempt =
            BrowserAttempt::with_listener(Provider::Github, "synthetic", listener).unwrap();
        assert!(!attempt.authorization_url().contains(&attempt.verifier));
        assert!(
            attempt
                .authorization_url()
                .contains("code_challenge_method=S256")
        );
    }
}
