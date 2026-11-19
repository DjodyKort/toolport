//! OTLP/HTTP JSON receiver. Loopback only, a body-size cap and a connection cap; plain
//! `std::net`, one short-lived thread per connection.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::config;
use super::otlp;
use super::store::{AppendError, Store};

pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CONNECTIONS: usize = 16;
const TURN_AWAY_LINGER: Duration = Duration::from_millis(50);
const TURN_AWAY_DRAIN: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const ACCEPT_POLL: Duration = Duration::from_millis(25);

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_body: usize,
    pub max_connections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_body: MAX_BODY_BYTES,
            max_connections: MAX_CONNECTIONS,
        }
    }
}

#[derive(Default)]
pub struct Counters {
    pub requests: AtomicU64,
    pub events: AtomicU64,
    pub rejected: AtomicU64,
    pub dropped: AtomicU64,
    pub last_event_ms: AtomicI64,
    /// Set when a batch was refused because the events file is at its cap; whoever prunes
    /// clears it.
    pub store_full: AtomicBool,
    active: AtomicUsize,
}

pub struct Receiver {
    port: u16,
    stop: Arc<AtomicBool>,
    pub counters: Arc<Counters>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Receiver {
    pub fn bind(port: u16, dir: PathBuf, limits: Limits) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(Counters::default());
        let (stop_flag, shared) = (Arc::clone(&stop), Arc::clone(&counters));
        let thread = std::thread::Builder::new()
            .name("spend-receiver".into())
            .spawn(move || accept_loop(listener, port, dir, limits, stop_flag, shared))?;
        Ok(Self {
            port,
            stop,
            counters,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(
    listener: TcpListener,
    port: u16,
    dir: PathBuf,
    limits: Limits,
    stop: Arc<AtomicBool>,
    counters: Arc<Counters>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                if !peer.ip().is_loopback() {
                    continue;
                }
                if counters.active.load(Ordering::SeqCst) >= limits.max_connections {
                    counters.rejected.fetch_add(1, Ordering::Relaxed);
                    turn_away(&stream);
                    continue;
                }
                counters.active.fetch_add(1, Ordering::SeqCst);
                let (dir, counters) = (dir.clone(), Arc::clone(&counters));
                let spawned = std::thread::Builder::new().spawn(move || {
                    handle(stream, port, &dir, limits, &counters);
                    counters.active.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(ACCEPT_POLL),
            Err(_) => std::thread::sleep(ACCEPT_POLL),
        }
    }
}

/// Answers 503 on the accept thread. The request is read for a moment after the answer: a socket
/// closed with unread bytes is reset, and the reset makes the client drop the answer it got.
fn turn_away(stream: &TcpStream) {
    respond(stream, 503, "Service Unavailable", "too many connections");
    drain(stream);
}

fn drain(mut stream: &TcpStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let until = std::time::Instant::now() + TURN_AWAY_LINGER;
    let _ = stream.set_read_timeout(Some(TURN_AWAY_LINGER));
    let mut buf = [0u8; 4096];
    let mut left = TURN_AWAY_DRAIN;
    while left > 0 && std::time::Instant::now() < until {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => left = left.saturating_sub(n),
        }
    }
}

fn respond(mut stream: &TcpStream, code: u16, reason: &str, body: &str) {
    let json = if code == 200 {
        "{}".to_string()
    } else {
        serde_json::json!({"error": body}).to_string()
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
        json.len()
    );
}

fn handle(
    mut stream: TcpStream,
    port: u16,
    dir: &std::path::Path,
    limits: Limits,
    counters: &Counters,
) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    counters.requests.fetch_add(1, Ordering::Relaxed);
    let request = match read_request(&mut stream, port, limits.max_body) {
        Ok(request) => request,
        Err((code, reason, message)) => {
            reject(&stream, counters, code, reason, message);
            drain(&stream);
            return;
        }
    };
    let Some(signal) = otlp::signal_of(&request.path) else {
        return reject(&stream, counters, 404, "Not Found", "unknown path");
    };
    if request.method != "POST" {
        return reject(&stream, counters, 405, "Method Not Allowed", "use POST");
    }
    if request.gzip || !is_json(&request.content_type) {
        return reject(
            &stream,
            counters,
            415,
            "Unsupported Media Type",
            "only uncompressed OTLP/HTTP JSON (application/json) is accepted",
        );
    }
    match otlp::parse(&signal, &request.body) {
        Ok(events) => {
            let count = events.len() as u64;
            let cap = config::load(dir).max_bytes();
            match Store::new(dir).append(&events, cap) {
                Ok(()) => {}
                Err(AppendError::Full) => {
                    counters.dropped.fetch_add(count, Ordering::Relaxed);
                    counters.store_full.store(true, Ordering::SeqCst);
                    return reject(
                        &stream,
                        counters,
                        507,
                        "Insufficient Storage",
                        "the spend store is full",
                    );
                }
                Err(AppendError::Failed(_)) => {
                    counters.dropped.fetch_add(count, Ordering::Relaxed);
                    return reject(
                        &stream,
                        counters,
                        500,
                        "Internal Server Error",
                        "store write failed",
                    );
                }
            }
            if count > 0 {
                counters.events.fetch_add(count, Ordering::Relaxed);
                counters
                    .last_event_ms
                    .store(super::now_ms(), Ordering::Relaxed);
            }
            respond(&stream, 200, "OK", "");
        }
        Err(message) => reject(&stream, counters, 400, "Bad Request", &message),
    }
}

fn reject(stream: &TcpStream, counters: &Counters, code: u16, reason: &str, message: &str) {
    counters.rejected.fetch_add(1, Ordering::Relaxed);
    respond(stream, code, reason, message);
}

/// `application/json`, with or without parameters such as `; charset=utf-8`.
fn is_json(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|media| media.trim() == "application/json")
}

/// A page in a browser can send a request to 127.0.0.1, but the `Host` it sends names the page's
/// own site after a DNS rebinding, so only the loopback names with this receiver's port pass.
fn host_allowed(host: &str, port: u16) -> bool {
    let Some((name, given)) = host.rsplit_once(':') else {
        return false;
    };
    given == port.to_string()
        && ["127.0.0.1", "localhost", "[::1]"]
            .iter()
            .any(|allowed| name.eq_ignore_ascii_case(allowed))
}

struct Request {
    method: String,
    path: String,
    content_type: String,
    gzip: bool,
    body: Vec<u8>,
}

type Refusal = (u16, &'static str, &'static str);

fn read_request(stream: &mut TcpStream, port: u16, max_body: usize) -> Result<Request, Refusal> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(at) = find(&buffer, b"\r\n\r\n") {
            break at;
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return Err((431, "Request Header Fields Too Large", "headers too large"));
        }
        let n = stream
            .read(&mut chunk)
            .map_err(|_| (408, "Request Timeout", "read timed out"))?;
        if n == 0 {
            return Err((400, "Bad Request", "connection closed early"));
        }
        buffer.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, path) = (
        first.next().unwrap_or("").to_string(),
        first.next().unwrap_or("").to_string(),
    );
    let (mut length, mut content_type, mut gzip, mut chunked) =
        (0usize, String::new(), false, false);
    let mut hosts: Vec<&str> = Vec::new();
    let mut browser_origin = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
        match name.as_str() {
            "content-length" => {
                length = value
                    .parse()
                    .map_err(|_| (400, "Bad Request", "bad content-length"))?
            }
            "content-type" => content_type = value.to_ascii_lowercase(),
            "content-encoding" => gzip = !value.eq_ignore_ascii_case("identity"),
            "transfer-encoding" => chunked = true,
            "host" => hosts.push(value),
            "origin" => browser_origin |= !value.eq_ignore_ascii_case("null"),
            _ => {}
        }
    }
    if !matches!(hosts.as_slice(), [host] if host_allowed(host, port)) {
        return Err((
            403,
            "Forbidden",
            "the host must be 127.0.0.1, localhost or [::1] with this port",
        ));
    }
    if browser_origin {
        return Err((
            403,
            "Forbidden",
            "requests from a web page are not accepted",
        ));
    }
    if chunked {
        return Err((411, "Length Required", "send a content-length"));
    }
    if length > max_body {
        return Err((413, "Payload Too Large", "body too large"));
    }
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let n = stream
            .read(&mut chunk)
            .map_err(|_| (408, "Request Timeout", "read timed out"))?;
        if n == 0 {
            return Err((400, "Bad Request", "body shorter than content-length"));
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > max_body {
            return Err((413, "Payload Too Large", "body too large"));
        }
    }
    body.truncate(length);
    Ok(Request {
        method,
        path,
        content_type,
        gzip,
        body,
    })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
