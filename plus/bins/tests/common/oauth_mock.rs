#![allow(dead_code)]

//! A loopback MCP endpoint that is also its own OAuth authorization server, for tests that run a
//! remote sign-in end to end without a network. Every URL it hands out is a literal `127.0.0.1`
//! address: a configured server on a loopback address is local, so the SSRF guard of the product
//! stays on its normal path, and no host name is ever resolved.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub const ACCESS_TOKEN: &str = "FAKE-oauth-access-token-5d2b";
pub const REFRESH_TOKEN: &str = "FAKE-oauth-refresh-token-8e1c";
pub const CLIENT_ID: &str = "synthetic-client";
pub const AUTH_CODE: &str = "synthetic-code";

pub struct OauthMock {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OauthMock {
    pub fn start() -> Self {
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).expect("bind the mock authorization server");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    std::thread::spawn(move || serve(stream, port));
                }
            }
        });
        Self {
            port,
            stop,
            thread: Some(thread),
        }
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn mcp_url(&self) -> String {
        format!("{}/mcp", self.base())
    }

    pub fn authorize_prefix(&self) -> String {
        format!("{}/authorize?", self.base())
    }
}

impl Drop for OauthMock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(stream: TcpStream, port: u16) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; content_length];
    let _ = reader.read_exact(&mut body);
    let mut words = request_line.split_whitespace();
    let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let path = target.split('?').next().unwrap_or("");
    let base = format!("http://127.0.0.1:{port}");
    let (status, payload) = match (method, path) {
        ("POST", "/mcp") => (401, r#"{"error":"unauthorized"}"#.to_string()),
        ("GET", "/.well-known/oauth-protected-resource/mcp") => (
            200,
            format!(r#"{{"resource":"{base}/mcp","authorization_servers":["{base}"]}}"#),
        ),
        ("GET", "/.well-known/oauth-authorization-server") => (
            200,
            format!(
                r#"{{"issuer":"{base}","authorization_endpoint":"{base}/authorize","token_endpoint":"{base}/token","registration_endpoint":"{base}/register"}}"#
            ),
        ),
        ("POST", "/register") => (201, format!(r#"{{"client_id":"{CLIENT_ID}"}}"#)),
        ("POST", "/token") => (
            200,
            format!(
                r#"{{"access_token":"{ACCESS_TOKEN}","refresh_token":"{REFRESH_TOKEN}","expires_in":3600}}"#
            ),
        ),
        _ => (404, r#"{"error":"not_found"}"#.to_string()),
    };
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let mut stream = reader.into_inner();
    let _ = stream.write_all(response.as_bytes());
}

/// What a user's browser does with a consent URL: the authorization server approves at once and
/// redirects to the loopback callback of the client, with the `state` it was given.
pub fn approve_in_browser(consent_url: &str) {
    let query = consent_url.split_once('?').map_or("", |(_, query)| query);
    let param = |name: &str| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| percent_decode(value))
        })
    };
    let redirect = param("redirect_uri").expect("the consent URL carries a redirect_uri");
    let state = param("state").expect("the consent URL carries a state");
    assert_eq!(
        param("client_id").as_deref(),
        Some(CLIENT_ID),
        "the consent URL names the registered client"
    );
    let (host, path) = redirect
        .strip_prefix("http://")
        .and_then(|rest| rest.split_once('/'))
        .expect("the redirect_uri is an http loopback URL");
    assert!(
        host.starts_with("127.0.0.1:"),
        "the redirect_uri is a loopback address, got {host}"
    );
    let mut stream = TcpStream::connect(host).expect("connect to the callback listener");
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    write!(
        stream,
        "GET /{path}?code={AUTH_CODE}&state={state} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .expect("deliver the authorization code");
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[at + 1..at + 3], 16) {
                out.push(byte);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
