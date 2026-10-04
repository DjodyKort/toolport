//! Loopback OTLP/HTTP-JSON receiver for Claude Code telemetry (MIG-OBS-3, D-051).
//!
//! Plain `std::net`, one request per connection. It binds `127.0.0.1` only, accepts
//! `POST /v1/metrics` and `POST /v1/logs` with `application/json` (optionally gzip), and appends
//! the parsed events to the obs event store. Request content is never logged.

use super::otel;
use super::store::{Event, Locked};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 4318;
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const SERVICE: &str = "toolport-otel-receiver";

const MAX_HEAD_BYTES: usize = 16 * 1024;
const MAX_HEADERS: usize = 64;
const MAX_CONNECTIONS: usize = 16;
const REQUEST_DEADLINE: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const LINGER_LIMIT: usize = 8 * 1024 * 1024;
const LINGER_TIME: Duration = Duration::from_secs(2);
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    InUse { ours: bool },
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::InUse { .. } => write!(f, "the port is already in use"),
            StartError::Failed(why) => write!(f, "{why}"),
        }
    }
}

pub struct Receiver {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Receiver {
    /// Port 0 asks the OS for a free port; read it back with [`Receiver::port`].
    pub fn start(dir: &Path, port: u16) -> Result<Receiver, StartError> {
        Receiver::start_with(dir, port, REQUEST_DEADLINE)
    }

    pub(crate) fn start_with(
        dir: &Path,
        port: u16,
        deadline: Duration,
    ) -> Result<Receiver, StartError> {
        if port != 0 {
            match probe(port) {
                Probe::Closed => {}
                seen => return Err(StartError::InUse { ours: seen == Probe::Ours }),
            }
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                StartError::InUse { ours: false }
            } else {
                StartError::Failed(e.to_string())
            }
        })?;
        let addr = listener
            .local_addr()
            .map_err(|e| StartError::Failed(e.to_string()))?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (dir, stop) = (dir.to_path_buf(), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("otel-receiver".into())
                .spawn(move || serve(listener, dir, stop, deadline))
                .map_err(|e| StartError::Failed(e.to_string()))?
        };
        Ok(Receiver {
            addr,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.addr, PROBE_TIMEOUT);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(listener: TcpListener, dir: PathBuf, stop: Arc<AtomicBool>, deadline: Duration) {
    let dir = Arc::new(dir);
    let active = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let Ok(stream) = conn else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::AcqRel);
            close_with(stream, &Reply::error(503, "receiver busy"), false);
            continue;
        }
        let (dir, active) = (Arc::clone(&dir), Arc::clone(&active));
        let spawned = std::thread::Builder::new()
            .name("otel-receiver-conn".into())
            .spawn(move || {
                handle(stream, &dir, deadline);
                active.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

struct Reply {
    status: u16,
    body: String,
    allow: Option<&'static str>,
}

impl Reply {
    fn ok(body: Value) -> Reply {
        Reply {
            status: 200,
            body: body.to_string(),
            allow: None,
        }
    }

    fn error(status: u16, message: &str) -> Reply {
        let code = match status {
            400 | 411 | 415 => 3,
            403 => 7,
            404 => 5,
            408 => 4,
            413 | 431 => 8,
            503 => 14,
            _ => 13,
        };
        Reply {
            status,
            body: json!({"code": code, "message": message}).to_string(),
            allow: None,
        }
    }

    fn allow(mut self, methods: &'static str) -> Reply {
        self.allow = Some(methods);
        self
    }
}

enum Stop {
    Reply(Reply),
    Silent,
}

impl From<Reply> for Stop {
    fn from(reply: Reply) -> Stop {
        Stop::Reply(reply)
    }
}

type Step<T> = Result<T, Stop>;

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    }
}

fn handle(stream: TcpStream, dir: &Path, deadline: Duration) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let reply = match respond(&stream, dir, deadline) {
        Ok(reply) => reply,
        Err(Stop::Reply(reply)) => reply,
        Err(Stop::Silent) => return,
    };
    close_with(stream, &reply, true);
}

fn close_with(mut stream: TcpStream, reply: &Reply, linger: bool) {
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let allow = reply
        .allow
        .map(|methods| format!("Allow: {methods}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{allow}Connection: close\r\n\r\n",
        reply.status,
        reason(reply.status),
        reply.body.len()
    );
    if stream.write_all(head.as_bytes()).is_err() || stream.write_all(reply.body.as_bytes()).is_err()
    {
        return;
    }
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
    if !linger {
        return;
    }
    let mut reader = Within {
        stream: &stream,
        until: Instant::now() + LINGER_TIME,
    };
    let (mut drained, mut sink) = (0usize, [0u8; 8192]);
    while drained < LINGER_LIMIT {
        match reader.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => drained += n,
        }
    }
}

struct Within<'a> {
    stream: &'a TcpStream,
    until: Instant,
}

impl Read for Within<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        let mut stream = self.stream;
        stream.read(buf)
    }
}

fn timed_out(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

fn io_stop(e: &std::io::Error, started: bool) -> Stop {
    if timed_out(e) && started {
        Reply::error(408, "request timed out").into()
    } else if started {
        Reply::error(400, "connection error").into()
    } else {
        Stop::Silent
    }
}

struct Head {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

fn find_end(buf: &[u8], from: usize) -> Option<usize> {
    buf.get(from..)?
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + from)
}

fn read_head(stream: &TcpStream, until: Instant) -> Step<(Vec<u8>, Vec<u8>)> {
    let mut reader = Within { stream, until };
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let scanned = buf.len().saturating_sub(3);
        let n = match reader.read(&mut chunk) {
            Ok(0) => return Err(io_stop(&std::io::ErrorKind::UnexpectedEof.into(), !buf.is_empty())),
            Ok(n) => n,
            Err(e) => return Err(io_stop(&e, !buf.is_empty())),
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = find_end(&buf, scanned) {
            if end > MAX_HEAD_BYTES {
                return Err(Reply::error(431, "headers too large").into());
            }
            let leftover = buf.split_off(end + 4);
            buf.truncate(end);
            return Ok((buf, leftover));
        }
        if buf.len() > MAX_HEAD_BYTES {
            return Err(Reply::error(431, "headers too large").into());
        }
    }
}

fn parse_head(raw: &[u8]) -> Step<Head> {
    let bad = || Stop::from(Reply::error(400, "malformed request"));
    let text = std::str::from_utf8(raw).map_err(|_| bad())?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or_else(bad)?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(bad());
    };
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") || method.is_empty() || !target.starts_with('/') {
        return Err(bad());
    }
    let path = target.split(['?', '#']).next().unwrap_or("").to_string();
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if headers.len() >= MAX_HEADERS || line.starts_with([' ', '\t']) {
            return Err(bad());
        }
        let (name, value) = line.split_once(':').ok_or_else(bad)?;
        if name.is_empty() || name.bytes().any(|b| b.is_ascii_whitespace() || b.is_ascii_control()) {
            return Err(bad());
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim().to_string();
        let single = matches!(name.as_str(), "host" | "content-length" | "content-type" | "transfer-encoding");
        if single && headers.iter().any(|(k, _)| *k == name) {
            return Err(bad());
        }
        headers.push((name, value));
    }
    Ok(Head {
        method: method.to_string(),
        path,
        headers,
    })
}

fn loopback_host(value: &str) -> bool {
    let (host, port) = match value.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':').or(tail.is_empty().then_some(""))),
            None => return false,
        },
        None => match value.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (value, Some("")),
        },
    };
    let Some(port) = port else {
        return false;
    };
    port.bytes().all(|b| b.is_ascii_digit())
        && ["127.0.0.1", "localhost", "::1"]
            .iter()
            .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

enum Signal {
    Metrics,
    Logs,
}

fn respond(stream: &TcpStream, dir: &Path, deadline: Duration) -> Step<Reply> {
    let until = Instant::now() + deadline;
    let peer_ok = stream
        .peer_addr()
        .map(|peer| peer.ip().is_loopback())
        .unwrap_or(false);
    if !peer_ok {
        return Err(Reply::error(403, "loopback clients only").into());
    }
    let (raw, leftover) = read_head(stream, until)?;
    let head = parse_head(&raw)?;
    match head.header("host") {
        Some(host) if loopback_host(host) => {}
        Some(_) => return Err(Reply::error(403, "host not allowed").into()),
        None => return Err(Reply::error(400, "missing host").into()),
    }
    let signal = match head.path.as_str() {
        "/v1/metrics" => Signal::Metrics,
        "/v1/logs" => Signal::Logs,
        "/healthz" => {
            return if head.method == "GET" {
                Ok(Reply::ok(json!({"ok": true, "service": SERVICE})))
            } else {
                Err(Reply::error(405, "method not allowed").allow("GET").into())
            }
        }
        _ => return Err(Reply::error(404, "not found").into()),
    };
    if head.method != "POST" {
        return Err(Reply::error(405, "method not allowed").allow("POST").into());
    }
    let media = head
        .header("content-type")
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase());
    if media.as_deref() != Some("application/json") {
        return Err(Reply::error(415, "content type must be application/json").into());
    }
    let gzip = match head.header("content-encoding").map(str::to_ascii_lowercase).as_deref() {
        None | Some("identity") => false,
        Some("gzip") => true,
        Some(_) => return Err(Reply::error(415, "unsupported content encoding").into()),
    };
    let framing = framing(&head)?;
    if head
        .header("expect")
        .is_some_and(|v| v.eq_ignore_ascii_case("100-continue"))
    {
        let mut out = stream;
        let _ = out.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let mut body_reader = BufReader::new(Cursor::new(leftover).chain(Within { stream, until }));
    let body = match framing {
        Framing::Length(len) => read_exact_body(&mut body_reader, len)?,
        Framing::Chunked => read_chunked(&mut body_reader)?,
    };
    let body = if gzip { gunzip(&body)? } else { body };
    let value: Value = serde_json::from_slice(&body)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| Stop::from(Reply::error(400, "body is not a JSON object")))?;
    let events: Vec<Event> = match signal {
        Signal::Metrics => otel::parse_metrics(&value),
        Signal::Logs => otel::parse_logs(&value),
    };
    if !events.is_empty() {
        Locked::acquire(dir)
            .and_then(|lock| lock.append_events(&events))
            .map_err(|_| Stop::from(Reply::error(500, "storage error")))?;
    }
    Ok(Reply::ok(json!({"partialSuccess": {}})))
}

enum Framing {
    Length(usize),
    Chunked,
}

fn framing(head: &Head) -> Step<Framing> {
    let bad = |message: &str| Stop::from(Reply::error(400, message));
    match (head.header("transfer-encoding"), head.header("content-length")) {
        (Some(_), Some(_)) => Err(bad("both transfer-encoding and content-length")),
        (Some(encoding), None) if encoding.eq_ignore_ascii_case("chunked") => Ok(Framing::Chunked),
        (Some(_), None) => Err(bad("unsupported transfer encoding")),
        (None, Some(len)) => {
            if len.is_empty() || !len.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad("bad content-length"));
            }
            match len.parse::<usize>() {
                Ok(n) if n <= MAX_BODY_BYTES => Ok(Framing::Length(n)),
                _ => Err(Reply::error(413, "body too large").into()),
            }
        }
        (None, None) => Err(Reply::error(411, "content-length required").into()),
    }
}

fn read_exact_body(reader: &mut impl Read, len: usize) -> Step<Vec<u8>> {
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .map_err(|e| io_stop(&e, true))?;
    Ok(body)
}

fn read_limited_line(reader: &mut impl BufRead, limit: u64) -> Step<String> {
    let mut line = String::new();
    reader
        .by_ref()
        .take(limit)
        .read_line(&mut line)
        .map_err(|e| io_stop(&e, true))?;
    if line.ends_with('\n') {
        Ok(line)
    } else {
        Err(Reply::error(400, "malformed chunk").into())
    }
}

fn read_chunked(reader: &mut impl BufRead) -> Step<Vec<u8>> {
    let mut body: Vec<u8> = Vec::new();
    loop {
        let line = read_limited_line(reader, 128)?;
        let size_text = line.trim().split(';').next().unwrap_or("");
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| Stop::from(Reply::error(400, "malformed chunk")))?;
        if size == 0 {
            for _ in 0..MAX_HEADERS {
                if read_limited_line(reader, 8192)?.trim().is_empty() {
                    return Ok(body);
                }
            }
            return Err(Reply::error(400, "malformed chunk").into());
        }
        if body.len().saturating_add(size) > MAX_BODY_BYTES {
            return Err(Reply::error(413, "body too large").into());
        }
        let from = body.len();
        body.resize(from + size, 0);
        reader
            .read_exact(&mut body[from..])
            .map_err(|e| io_stop(&e, true))?;
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf).map_err(|e| io_stop(&e, true))?;
        if &crlf != b"\r\n" {
            return Err(Reply::error(400, "malformed chunk").into());
        }
    }
}

fn gunzip(body: &[u8]) -> Step<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(body)
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| Stop::from(Reply::error(400, "invalid gzip body")))?;
    if out.len() > MAX_BODY_BYTES {
        return Err(Reply::error(413, "body too large").into());
    }
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Probe {
    Ours,
    Foreign,
    Closed,
}

/// Tells this receiver apart from another listener on the port, and from nothing at all.
pub fn probe(port: u16) -> Probe {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) else {
        return Probe::Closed;
    };
    let _ = stream.set_write_timeout(Some(PROBE_TIMEOUT));
    let request = format!("GET /healthz HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return Probe::Foreign;
    }
    let mut reader = Within {
        stream: &stream,
        until: Instant::now() + PROBE_TIMEOUT,
    };
    let mut answer = Vec::new();
    let _ = reader.by_ref().take(4096).read_to_end(&mut answer);
    let text = String::from_utf8_lossy(&answer);
    if text.starts_with("HTTP/1.1 200") && text.contains(SERVICE) {
        Probe::Ours
    } else {
        Probe::Foreign
    }
}
