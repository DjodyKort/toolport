use super::otel::fixtures::{logs_payload, metrics_payload};
use super::receiver::{probe, Probe, Receiver, StartError, MAX_BODY_BYTES};
use super::store::Locked;
use crate::plus::testutil::wait_until;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("otel-recv-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Fx {
    dir: PathBuf,
    receiver: Receiver,
}

impl Fx {
    fn new(label: &str) -> Fx {
        let dir = scratch(label);
        let receiver = Receiver::start(&dir, 0).unwrap();
        Fx { dir, receiver }
    }

    fn stored(&self) -> usize {
        Locked::acquire(&self.dir).unwrap().read_events().len()
    }

    fn post(&self, path: &str, body: &[u8]) -> Response {
        raw(self.receiver.port(), &request("POST", path, &json_headers(self.receiver.port(), body.len()), body))
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Response {
    status: u16,
    head: String,
    body: String,
}

fn json_headers(port: u16, len: usize) -> Vec<(String, String)> {
    vec![
        ("Host".into(), format!("127.0.0.1:{port}")),
        ("Content-Type".into(), "application/json".into()),
        ("Content-Length".into(), len.to_string()),
    ]
}

fn request(method: &str, path: &str, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("{method} {path} HTTP/1.1\r\n").into_bytes();
    for (k, v) in headers {
        out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

fn parse_response(bytes: &[u8]) -> Response {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Response {
        status,
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn raw(port: u16, bytes: &[u8]) -> Response {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let _ = stream.write_all(bytes);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out);
    parse_response(&out)
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

fn chunked(data: &[u8], size: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for part in data.chunks(size) {
        out.extend_from_slice(format!("{:x};ext=1\r\n", part.len()).as_bytes());
        out.extend_from_slice(part);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"0\r\nTrailer: x\r\n\r\n");
    out
}

#[test]
fn metrics_and_logs_posts_are_stored_with_an_empty_partial_success() {
    let fx = Fx::new("store");
    let metrics = serde_json::to_vec(&metrics_payload()).unwrap();
    let reply = fx.post("/v1/metrics", &metrics);
    assert_eq!(reply.status, 200, "{}", reply.head);
    assert_eq!(serde_json::from_str::<Value>(&reply.body).unwrap(), json!({"partialSuccess": {}}));
    assert!(reply.head.contains("Content-Type: application/json"));
    assert!(reply.head.contains("Connection: close"));
    assert_eq!(fx.stored(), 5);
    let logs = serde_json::to_vec(&logs_payload()).unwrap();
    assert_eq!(fx.post("/v1/logs?x=1", &logs).status, 200);
    let events = Locked::acquire(&fx.dir).unwrap().read_events();
    assert_eq!(events.len(), 10);
    assert_eq!(events[0].kind, "token_usage");
    assert_eq!(events[5].kind, "api_request");
    assert_eq!(fx.post("/v1/metrics", b"{}").status, 200);
    assert_eq!(fx.stored(), 10, "an empty export is accepted and stores nothing");
}

#[test]
fn a_metrics_body_on_the_logs_path_stores_nothing() {
    let fx = Fx::new("crosspath");
    let metrics = serde_json::to_vec(&metrics_payload()).unwrap();
    assert_eq!(fx.post("/v1/logs", &metrics).status, 200);
    assert_eq!(fx.stored(), 0);
}

#[test]
fn gzip_and_chunked_bodies_and_mixed_case_content_types_are_accepted() {
    let fx = Fx::new("encodings");
    let port = fx.receiver.port();
    let body = serde_json::to_vec(&metrics_payload()).unwrap();

    let zipped = gzip(&body);
    let mut headers = json_headers(port, zipped.len());
    headers.push(("Content-Encoding".into(), "gzip".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &zipped)).status, 200);
    assert_eq!(fx.stored(), 5);

    let mut headers = json_headers(port, 0);
    headers.retain(|(k, _)| k != "Content-Length");
    headers.push(("Transfer-Encoding".into(), "Chunked".into()));
    let reply = raw(port, &request("POST", "/v1/metrics", &headers, &chunked(&body, 97)));
    assert_eq!(reply.status, 200, "{}", reply.head);
    assert_eq!(fx.stored(), 10);

    let mut headers = json_headers(port, body.len());
    headers[1].1 = "Application/JSON; charset=utf-8".into();
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &body)).status, 200);
    assert_eq!(fx.stored(), 15);
}

#[test]
fn expect_continue_gets_an_interim_answer_before_the_body() {
    let fx = Fx::new("expect");
    let port = fx.receiver.port();
    let body = serde_json::to_vec(&metrics_payload()).unwrap();
    let mut headers = json_headers(port, body.len());
    headers.push(("Expect".into(), "100-continue".into()));
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(&request("POST", "/v1/metrics", &headers, b"")).unwrap();
    let mut interim = [0u8; 25];
    stream.read_exact(&mut interim).unwrap();
    assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
    stream.write_all(&body).unwrap();
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest);
    assert_eq!(parse_response(&rest).status, 200);
    assert_eq!(fx.stored(), 5);
}

#[test]
fn requests_with_a_non_loopback_host_are_refused() {
    let fx = Fx::new("host");
    let port = fx.receiver.port();
    let body = serde_json::to_vec(&metrics_payload()).unwrap();
    for host in ["evil.example", "evil.example:4318", "127.0.0.1.evil.example", "10.0.0.5", "[2001:db8::1]:4318", "0.0.0.0"] {
        let mut headers = json_headers(port, body.len());
        headers[0].1 = host.to_string();
        let reply = raw(port, &request("POST", "/v1/metrics", &headers, &body));
        assert_eq!(reply.status, 403, "{host}");
        assert!(!reply.body.contains(host), "the host is not echoed");
    }
    let mut headers = json_headers(port, body.len());
    headers.remove(0);
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &body)).status, 400);
    assert_eq!(fx.stored(), 0, "refused requests store nothing");
    for host in ["localhost", "localhost:4318", "LOCALHOST", "127.0.0.1", "[::1]", "[::1]:4318"] {
        let mut headers = json_headers(port, body.len());
        headers[0].1 = host.to_string();
        assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &body)).status, 200, "{host}");
    }
}

#[test]
fn wrong_method_path_and_content_type_are_rejected_with_the_right_status() {
    let fx = Fx::new("routes");
    let port = fx.receiver.port();
    let body = serde_json::to_vec(&metrics_payload()).unwrap();
    let get = raw(port, &request("GET", "/v1/metrics", &json_headers(port, 0), b""));
    assert_eq!(get.status, 405);
    assert!(get.head.contains("Allow: POST"));
    assert_eq!(raw(port, &request("PUT", "/v1/logs", &json_headers(port, body.len()), &body)).status, 405);
    assert_eq!(raw(port, &request("OPTIONS", "/v1/logs", &json_headers(port, 0), b"")).status, 405);
    assert_eq!(fx.post("/v1/traces", &body).status, 404);
    assert_eq!(fx.post("/", &body).status, 404);
    assert_eq!(fx.post("/v1/metrics/", &body).status, 404);
    for content_type in ["text/plain", "application/x-protobuf", "application/x-www-form-urlencoded", ""] {
        let mut headers = json_headers(port, body.len());
        headers[1].1 = content_type.into();
        assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &body)).status, 415, "{content_type:?}");
    }
    let mut headers = json_headers(port, body.len());
    headers.retain(|(k, _)| k != "Content-Type");
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &body)).status, 415);
    let mut headers = json_headers(port, body.len());
    headers.push(("Content-Encoding".into(), "br".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &body)).status, 415);
    assert_eq!(fx.stored(), 0);
}

#[test]
fn malformed_bodies_and_framing_are_rejected_and_store_nothing() {
    let fx = Fx::new("malformed");
    let port = fx.receiver.port();
    for bad in [&b"{not json"[..], b"[1,2]", b"\"text\"", b"", b"null", b"\xff\xfe"] {
        let reply = fx.post("/v1/metrics", bad);
        assert_eq!(reply.status, 400, "{bad:?}");
        assert_eq!(serde_json::from_str::<Value>(&reply.body).unwrap()["code"], 3);
        assert!(!reply.body.contains("not json"), "the content is not echoed");
    }
    let mut headers = json_headers(port, 5);
    headers[2].1 = "abc".into();
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"{}")).status, 400);
    let mut headers = json_headers(port, 2);
    headers.retain(|(k, _)| k != "Content-Length");
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"{}")).status, 411);
    let mut headers = json_headers(port, 2);
    headers.push(("Transfer-Encoding".into(), "chunked".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"0\r\n\r\n")).status, 400);
    let mut headers = json_headers(port, 0);
    headers.retain(|(k, _)| k != "Content-Length");
    headers.push(("Transfer-Encoding".into(), "gzip".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"")).status, 400);
    let mut headers = json_headers(port, 2);
    headers.push(("Content-Length".into(), "2".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"{}")).status, 400, "duplicate content-length");
    let truncated = raw(port, &request("POST", "/v1/metrics", &json_headers(port, 500), b"{\"a\":"));
    assert_eq!(truncated.status, 400);
    let mut headers = json_headers(port, 0);
    headers.retain(|(k, _)| k != "Content-Length");
    headers.push(("Transfer-Encoding".into(), "chunked".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"zz\r\n{}\r\n0\r\n\r\n")).status, 400);
    assert_eq!(raw(port, b"GARBAGE\r\n\r\n").status, 400);
    assert_eq!(raw(port, b"POST /v1/metrics HTTP/2\r\nHost: 127.0.0.1\r\n\r\n").status, 400);
    assert_eq!(raw(port, b"POST /v1/metrics HTTP/1.1\r\nHost: 127.0.0.1\r\n folded: x\r\n\r\n").status, 400);
    let mut headers = json_headers(port, 2);
    headers.push(("X-Pad".into(), "a".repeat(20_000)));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"{}")).status, 431);
    let mut headers = json_headers(port, 2);
    let many: Vec<(String, String)> = (0..80).map(|i| (format!("X-{i}"), "v".into())).collect();
    headers.extend(many);
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"{}")).status, 400);
    assert_eq!(fx.stored(), 0);
    assert_eq!(fx.post("/v1/metrics", b"{}").status, 200, "the receiver survived all of it");
}

#[test]
fn oversized_bodies_are_refused_before_and_while_reading() {
    let fx = Fx::new("oversize");
    let port = fx.receiver.port();
    let padded = format!("{{\"pad\":\"{}\"}}", "a".repeat(MAX_BODY_BYTES));
    let reply = fx.post("/v1/metrics", padded.as_bytes());
    assert_eq!(reply.status, 413);
    assert_eq!(serde_json::from_str::<Value>(&reply.body).unwrap()["code"], 8);

    let just_under = format!("{{\"pad\":\"{}\"}}", "a".repeat(MAX_BODY_BYTES - 20));
    assert_eq!(fx.post("/v1/metrics", just_under.as_bytes()).status, 200);

    let mut headers = json_headers(port, 0);
    headers.retain(|(k, _)| k != "Content-Length");
    headers.push(("Transfer-Encoding".into(), "chunked".into()));
    let body = chunked(padded.as_bytes(), 64 * 1024);
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &body)).status, 413);

    let bomb = gzip(&vec![b' '; MAX_BODY_BYTES + 1]);
    assert!(bomb.len() < 64 * 1024);
    let mut headers = json_headers(port, bomb.len());
    headers.push(("Content-Encoding".into(), "gzip".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, &bomb)).status, 413);
    let mut headers = json_headers(port, 9);
    headers.push(("Content-Encoding".into(), "gzip".into()));
    assert_eq!(raw(port, &request("POST", "/v1/metrics", &headers, b"not gzip!")).status, 400);
    assert_eq!(fx.stored(), 0);
}

// a child spawned by a parallel test can briefly hold a copy of the closed listener's fd
pub(super) fn wait_closed(port: u16) {
    wait_until(&format!("port {port} to close"), || probe(port) == Probe::Closed);
}

fn start_when_free(dir: &Path, port: u16) -> Receiver {
    let mut started = None;
    wait_until(&format!("port {port} to take a receiver"), || match Receiver::start(dir, port) {
        Ok(receiver) => {
            started = Some(receiver);
            true
        }
        Err(StartError::InUse { .. }) => false,
        Err(other) => panic!("{other:?}"),
    });
    started.unwrap()
}

#[test]
fn a_taken_port_is_a_clear_error_and_a_freed_one_can_be_reused() {
    let dir = scratch("taken");
    let squatter = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = squatter.local_addr().unwrap().port();
    assert_eq!(Receiver::start(&dir, port).err(), Some(StartError::InUse { ours: false }));
    assert_eq!(probe(port), Probe::Foreign);
    drop(squatter);
    wait_closed(port);

    let receiver = start_when_free(&dir, port);
    assert_eq!(receiver.port(), port);
    assert!(receiver.local_addr().ip().is_loopback());
    assert_eq!(probe(port), Probe::Ours);
    assert_eq!(
        Receiver::start(&dir, port).err(),
        Some(StartError::InUse { ours: true }),
        "a second receiver on the port"
    );
    drop(receiver);
    wait_closed(port);
    let again = start_when_free(&dir, port);
    assert_eq!(probe(port), Probe::Ours);
    drop(again);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn healthz_answers_for_get_only() {
    let fx = Fx::new("health");
    let port = fx.receiver.port();
    let reply = raw(port, &request("GET", "/healthz", &json_headers(port, 0), b""));
    assert_eq!(reply.status, 200);
    assert_eq!(serde_json::from_str::<Value>(&reply.body).unwrap()["service"], "toolport-otel-receiver");
    assert_eq!(raw(port, &request("POST", "/healthz", &json_headers(port, 0), b"")).status, 405);
    let mut headers = json_headers(port, 0);
    headers[0].1 = "evil.example".into();
    assert_eq!(raw(port, &request("GET", "/healthz", &headers, b"")).status, 403);
}

#[test]
fn a_silent_or_stalled_client_does_not_block_others_and_times_out() {
    let dir = scratch("stall");
    let receiver = Receiver::start_with(&dir, 0, Duration::from_millis(600)).unwrap();
    let port = receiver.port();
    let idle = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let mut partial = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    partial.write_all(b"POST /v1/metrics HTTP/1.1\r\nHost: 127.0.0.1\r\n").unwrap();
    let body = serde_json::to_vec(&metrics_payload()).unwrap();
    let served = raw(port, &request("POST", "/v1/metrics", &json_headers(port, body.len()), &body));
    assert_eq!(served.status, 200, "other clients are served meanwhile");
    partial.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut out = Vec::new();
    let _ = partial.read_to_end(&mut out);
    assert_eq!(parse_response(&out).status, 408);
    let mut idle = idle;
    idle.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut nothing = Vec::new();
    let _ = idle.read_to_end(&mut nothing);
    assert!(nothing.is_empty(), "a client that never sent a byte gets no answer");
    drop(receiver);
    let _ = std::fs::remove_dir_all(&dir);
}

fn assert_eventually(port: u16, status: u16) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let reply = raw(port, &request("GET", "/healthz", &json_headers(port, 0), b""));
        if reply.status == status {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "never saw {status}, last {}", reply.status);
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn connections_beyond_the_cap_are_turned_away_with_503() {
    let dir = scratch("cap");
    let receiver = Receiver::start_with(&dir, 0, Duration::from_secs(3)).unwrap();
    let port = receiver.port();
    let held: Vec<TcpStream> = (0..16)
        .map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap())
        .collect();
    assert_eventually(port, 503);
    drop(held);
    assert_eventually(port, 200);
    drop(receiver);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parallel_exports_are_all_stored() {
    let fx = Fx::new("parallel");
    let port = fx.receiver.port();
    let body = serde_json::to_vec(&metrics_payload()).unwrap();
    let threads: Vec<_> = (0..12)
        .map(|_| {
            let body = body.clone();
            std::thread::spawn(move || {
                raw(port, &request("POST", "/v1/metrics", &json_headers(port, body.len()), &body)).status
            })
        })
        .collect();
    for t in threads {
        assert_eq!(t.join().unwrap(), 200);
    }
    assert_eq!(fx.stored(), 60);
}
