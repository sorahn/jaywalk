use crate::Result;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(crate) const REDIRECT: &str = "http://127.0.0.1:43761/callback";
pub(crate) struct Listener(TcpListener);

impl Listener {
    pub(crate) fn bind() -> Result<Self> {
        let socket = TcpListener::bind("127.0.0.1:43761")
            .map_err(|_| "Cannot bind loopback callback port 43761; close other probe instances")?;
        socket
            .set_nonblocking(true)
            .map_err(|_| "Cannot configure loopback listener")?;
        Ok(Self(socket))
    }
    pub(crate) fn bind_registration() -> Result<Self> {
        let socket = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| "Cannot bind registration callback listener")?;
        socket
            .set_nonblocking(true)
            .map_err(|_| "Cannot configure registration listener")?;
        Ok(Self(socket))
    }
    pub(crate) fn origin(&self) -> Result<String> {
        let address = self
            .0
            .local_addr()
            .map_err(|_| "Cannot read listener address")?;
        Ok(format!("http://{address}"))
    }
    pub(crate) fn receive_with_page(
        self,
        state: &str,
        timeout: Duration,
        cancelled: &AtomicBool,
        page: Option<(&str, &str)>,
    ) -> Result<String> {
        self.serve(state, timeout, cancelled, page)
    }
    pub(crate) fn receive(
        self,
        state: &str,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<String> {
        self.serve(state, timeout, cancelled, None)
    }
    fn serve(
        self,
        state: &str,
        timeout: Duration,
        cancelled: &AtomicBool,
        page: Option<(&str, &str)>,
    ) -> Result<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Authorization cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("Browser callback timed out".into());
            }
            let (mut socket, _) = match self.0.accept() {
                Ok(pair) => pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(_) => return Err("Cannot accept browser callback".into()),
            };
            socket
                .set_read_timeout(Some(Duration::from_millis(500)))
                .map_err(|_| "Cannot set callback read timeout")?;
            socket
                .set_write_timeout(Some(Duration::from_millis(500)))
                .map_err(|_| "Cannot set callback write timeout")?;
            // Bound both size and wall time, including a client that drips bytes forever.
            let request_deadline = Instant::now() + Duration::from_secs(2);
            let mut bytes = Vec::new();
            while bytes.len() < 8192
                && Instant::now() < request_deadline
                && Instant::now() < deadline
                && !cancelled.load(Ordering::Relaxed)
            {
                let mut chunk = [0_u8; 512];
                match socket.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                }
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            if cancelled.load(Ordering::Relaxed) {
                return Err("Authorization cancelled".into());
            }
            if Instant::now() >= deadline {
                return Err("Browser callback timed out".into());
            }
            if let Some((path, html)) = page
                && request_target(&bytes) == Ok(path)
            {
                let _ = socket.write_all(&response("200 OK", "text/html; charset=utf-8", html));
                continue;
            }
            let result = parse(&bytes, state);
            if result == Err(CallbackError::Invalid) {
                eprintln!("Ignored browser request: {}", diagnostic(&bytes, state));
            }
            let (status, body) = if result.is_ok() {
                ("200 OK", "Authorization received. Return to the terminal.")
            } else {
                ("400 Bad Request", diagnostic(&bytes, state))
            };
            // The browser may close first; callback validity does not depend on its response.
            let _ = socket.write_all(&response(status, "text/plain", body));
            match result {
                Ok(code) => return Ok(code), // consumes listener: successful callback cannot replay
                Err(CallbackError::Denied) => return Err("Authorization denied by user".into()),
                Err(CallbackError::Invalid) => continue,
            }
        }
    }
}

#[derive(Debug, PartialEq)]
enum CallbackError {
    Invalid,
    Denied,
}
fn response(status: &str, content_type: &str, body: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; form-action https://github.com; frame-ancestors 'none'; base-uri 'none'\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}
fn request_target(bytes: &[u8]) -> std::result::Result<&str, CallbackError> {
    if bytes.len() >= 8192 || !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
        return Err(CallbackError::Invalid);
    }
    let end = bytes
        .windows(2)
        .position(|pair| pair == b"\r\n")
        .ok_or(CallbackError::Invalid)?;
    let request_line = std::str::from_utf8(&bytes[..end]).map_err(|_| CallbackError::Invalid)?;
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target), Some("HTTP/1.1" | "HTTP/1.0"), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(CallbackError::Invalid);
    };
    Ok(target)
}
fn diagnostic<'a>(bytes: &[u8], expected_state: &'a str) -> &'a str {
    let Ok(target) = request_target(bytes) else {
        return "Incomplete, oversized or unsupported HTTP request. Reload the page.";
    };
    if target.starts_with("/register/") {
        return "This setup link does not match the running probe. Use the newly opened setup page.";
    }
    if !target.starts_with("/callback?") {
        return "This is not the registration callback. Return to the setup page.";
    }
    let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else {
        return "Malformed callback URL.";
    };
    let parameters: Vec<_> = url.query_pairs().collect();
    for (index, (key, _)) in parameters.iter().enumerate() {
        if parameters[..index]
            .iter()
            .any(|(previous, _)| previous == key)
        {
            return "Callback contains duplicate parameters.";
        }
    }
    if !parameters.iter().any(|(key, _)| key == "state") {
        return "GitHub did not return the required state parameter. Keep this tab open and return to the terminal.";
    }
    if !parameters
        .iter()
        .any(|(key, value)| key == "state" && value == expected_state)
    {
        return "Callback state does not match this registration attempt. Keep this tab open and return to the terminal.";
    }
    if parameters.iter().any(|(key, _)| key == "error") {
        return "GitHub declined registration or authorization. Return to the terminal.";
    }
    "Callback is missing a valid registration code. Keep this tab open and return to the terminal."
}
fn parse(bytes: &[u8], expected_state: &str) -> std::result::Result<String, CallbackError> {
    let target = request_target(bytes)?;
    if !target.starts_with("/callback?") || target.contains('#') {
        return Err(CallbackError::Invalid);
    }
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| CallbackError::Invalid)?;
    if url.path() != "/callback" {
        return Err(CallbackError::Invalid);
    }
    let parameters: Vec<_> = url.query_pairs().collect();
    for (index, (key, _)) in parameters.iter().enumerate() {
        if parameters[..index]
            .iter()
            .any(|(previous, _)| previous == key)
        {
            return Err(CallbackError::Invalid);
        }
    }
    let single = |key: &str| {
        let values: Vec<_> = parameters.iter().filter(|(k, _)| k == key).collect();
        if values.len() == 1 {
            Some(values[0].1.as_ref())
        } else {
            None
        }
    };
    if single("state") != Some(expected_state) || expected_state.is_empty() {
        return Err(CallbackError::Invalid);
    }
    if single("error").is_some() {
        return Err(CallbackError::Denied);
    }
    let code = single("code")
        .filter(|code| !code.is_empty() && code.len() <= 2048)
        .ok_or(CallbackError::Invalid)?;
    Ok(code.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(target: &str) -> Vec<u8> {
        format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").into_bytes()
    }
    #[test]
    fn state_is_required_exact_and_not_duplicated() {
        assert_eq!(
            parse(
                &request("/callback?state=expected&code=synthetic"),
                "expected"
            ),
            Ok("synthetic".into())
        );
        for target in [
            "/callback?code=x",
            "/callback?state=other&code=x",
            "/callback?state=expected&state=other&code=x",
            "/callback?state=expected&code=a&code=b",
        ] {
            assert_eq!(
                parse(&request(target), "expected"),
                Err(CallbackError::Invalid)
            );
        }
    }
    #[test]
    fn unrelated_malformed_and_partial_requests_are_rejected() {
        for target in [
            "/favicon.ico",
            "http://evil/callback?state=expected&code=x",
            "/callback/extra?state=expected&code=x",
            "/callback?state=expected&code=",
            "/callback?state=expected&code=x#fragment",
        ] {
            assert_eq!(
                parse(&request(target), "expected"),
                Err(CallbackError::Invalid)
            );
        }
        assert!(
            parse(
                b"GET /callback?state=expected&code=x HTTP/1.1\r\n",
                "expected"
            )
            .is_err()
        );
        assert!(parse(&vec![b'x'; 8192], "expected").is_err());
        assert!(
            parse(
                b"POST /callback?state=expected&code=x HTTP/1.1\r\n\r\n",
                "expected"
            )
            .is_err()
        );
    }
    #[test]
    fn denial_requires_valid_state_and_does_not_reflect_provider_text() {
        assert_eq!(
            parse(
                &request("/callback?state=expected&error=access_denied"),
                "expected"
            ),
            Err(CallbackError::Denied)
        );
        assert_eq!(
            parse(
                &request("/callback?state=wrong&error=access_denied"),
                "expected"
            ),
            Err(CallbackError::Invalid)
        );
    }
    #[test]
    fn duplicate_errors_and_code_with_error_are_not_success() {
        assert_eq!(
            parse(
                &request("/callback?state=expected&code=x&error=a&error=b"),
                "expected"
            ),
            Err(CallbackError::Invalid)
        );
        assert_eq!(
            parse(
                &request("/callback?state=expected&code=x&error=access_denied"),
                "expected"
            ),
            Err(CallbackError::Denied)
        );
    }
    #[test]
    fn listener_times_out_and_cancels_without_browser() {
        let listener = Listener(TcpListener::bind("127.0.0.1:0").unwrap());
        listener.0.set_nonblocking(true).unwrap();
        assert!(
            listener
                .receive("state", Duration::ZERO, &AtomicBool::new(false))
                .unwrap_err()
                .contains("timed out")
        );
        let listener = Listener(TcpListener::bind("127.0.0.1:0").unwrap());
        listener.0.set_nonblocking(true).unwrap();
        assert!(
            listener
                .receive("state", Duration::from_secs(1), &AtomicBool::new(true))
                .unwrap_err()
                .contains("cancelled")
        );
    }
    #[test]
    fn headers_do_not_control_request_line_parsing_or_leak_into_diagnostics() {
        let bytes =
            b"GET /callback?state=expected&code=synthetic HTTP/1.1\r\nCookie: legacy=\xff\r\n\r\n";
        assert_eq!(parse(bytes, "expected"), Ok("synthetic".into()));
        for target in [
            "/callback?code=SENSITIVE",
            "/callback?state=SENSITIVE&code=SENSITIVE",
            "/register/SENSITIVE",
        ] {
            let message = diagnostic(&request(target), "expected");
            assert!(!message.contains("SENSITIVE"));
        }
        assert!(diagnostic(&request("/callback?code=x"), "expected").contains("state parameter"));
    }
    #[test]
    fn registration_page_and_callback_share_one_bounded_listener() {
        let listener = Listener::bind_registration().unwrap();
        let origin = listener.origin().unwrap();
        let address = origin.strip_prefix("http://").unwrap().to_owned();
        let thread = std::thread::spawn(move || {
            listener.receive_with_page(
                "expected",
                Duration::from_secs(3),
                &AtomicBool::new(false),
                Some(("/register/nonce", "<p>Setup</p>")),
            )
        });
        let mut client = std::net::TcpStream::connect(&address).unwrap();
        client.write_all(&request("/register/nonce")).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.contains("Content-Type: text/html"));
        assert!(response.contains("Cache-Control: no-store"));
        assert!(response.contains("form-action https://github.com"));
        assert!(response.ends_with("<p>Setup</p>"));
        let mut client = std::net::TcpStream::connect(&address).unwrap();
        client
            .write_all(&request("/callback?state=expected&code=synthetic"))
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert_eq!(thread.join().unwrap().unwrap(), "synthetic");
        assert!(std::net::TcpStream::connect(address).is_err());
    }
    #[test]
    fn native_socket_accepts_only_one_successful_callback() {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        socket.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            Listener(socket).receive("state", Duration::from_secs(3), &AtomicBool::new(false))
        });
        let mut client = std::net::TcpStream::connect(address).unwrap();
        client
            .write_all(&request("/callback?state=wrong&code=x"))
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 400"));
        let mut client = std::net::TcpStream::connect(address).unwrap();
        client
            .write_all(&request("/callback?state=state&code=synthetic"))
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
        assert_eq!(thread.join().unwrap().unwrap(), "synthetic");
        assert!(std::net::TcpStream::connect(address).is_err());
    }
}
