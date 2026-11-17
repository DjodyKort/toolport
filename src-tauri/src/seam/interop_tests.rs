use std::time::{Duration, Instant};

use serde_json::{json, Value};

use std::sync::{Arc, Mutex};

use crate::downstream::{DownstreamServer, StdioTransport, Transport, TransportError};
use crate::registry::{self, DataDirTestEnv, Registry, ServerEntry};

const FAKE_SERVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../plus/fixtures/stdio/fake-mcp.mjs"
);

fn spawn(env: &[(&str, &str)]) -> StdioTransport {
    let env: Vec<(String, String)> = env
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect();
    StdioTransport::spawn("node", &[FAKE_SERVER.to_string()], &env, None, false)
        .expect("node starts the fake server")
}

fn connected(transport: &mut StdioTransport, window: Duration) {
    transport.set_server_id("odh");
    transport.set_read_timeout(Duration::from_secs(10));
    transport
        .request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
        )
        .expect("initialize");
    transport.set_read_timeout(window);
}

fn slow(
    transport: &mut StdioTransport,
    total_ms: u64,
    tick_ms: u64,
    token: Option<&str>,
) -> Result<Value, TransportError> {
    let mut params = json!({
        "name": "slow",
        "arguments": { "totalMs": total_ms, "tickMs": tick_ms }
    });
    if let Some(token) = token {
        params["_meta"] = json!({ "progressToken": token });
    }
    transport.request("tools/call", params)
}

fn write_registry(max_request_timeout_ms: Option<u64>) {
    let mut entry: ServerEntry = serde_json::from_value(json!({
        "id": "odh", "name": "odh", "transport": "stdio", "command": "node", "enabled": true
    }))
    .unwrap();
    if let Some(ms) = max_request_timeout_ms {
        entry
            .unknown_fields
            .insert("maxRequestTimeoutMs".into(), json!(ms));
    }
    let mut reg = Registry::default();
    reg.servers.push(entry);
    registry::save(&reg).unwrap();
}

#[test]
fn test01_progress_for_the_calls_token_keeps_a_call_alive_past_its_timeout() {
    let _env = DataDirTestEnv::new("s8-progress-alive");
    write_registry(None);
    let mut transport = spawn(&[]);
    connected(&mut transport, Duration::from_millis(1000));

    let started = Instant::now();
    let result = slow(&mut transport, 2600, 200, Some("tp-alive")).expect("kept alive by progress");
    assert_eq!(result["content"][0]["text"], "done");
    assert!(started.elapsed() > Duration::from_millis(2500));
}

#[test]
fn test02_a_call_without_a_token_or_without_progress_times_out_with_the_unchanged_text() {
    let _env = DataDirTestEnv::new("s8-progress-none");
    write_registry(None);
    let mut transport = spawn(&[]);
    connected(&mut transport, Duration::from_millis(800));

    let without_token = slow(&mut transport, 3000, 200, None).unwrap_err();
    assert_eq!(
        without_token.to_string(),
        "timed out waiting for 'tools/call' response"
    );

    let mut transport = spawn(&[]);
    connected(&mut transport, Duration::from_millis(800));
    let silent = slow(&mut transport, 3000, 0, Some("tp-silent")).unwrap_err();
    assert_eq!(
        silent.to_string(),
        "timed out waiting for 'tools/call' response"
    );
}

#[test]
fn test03_max_request_timeout_ends_a_call_that_keeps_reporting_progress() {
    let _env = DataDirTestEnv::new("s8-progress-cap");
    write_registry(Some(1500));
    let mut transport = spawn(&[]);
    connected(&mut transport, Duration::from_millis(800));

    let started = Instant::now();
    let error = slow(&mut transport, 10_000, 150, Some("tp-cap")).unwrap_err();
    assert!(error.to_string().contains("maxRequestTimeoutMs"), "{error}");
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(1400) && took < Duration::from_millis(4000),
        "{took:?}"
    );
}

struct Tee {
    inner: StdioTransport,
    replies: Arc<Mutex<Vec<(String, Value)>>>,
}

impl Transport for Tee {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, TransportError> {
        let reply = self.inner.request(method, params)?;
        self.replies
            .lock()
            .unwrap()
            .push((method.to_string(), reply.clone()));
        Ok(reply)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), TransportError> {
        self.inner.notify(method, params)
    }
}

fn connect_through_tee(
    id: &str,
    env: &[(&str, &str)],
) -> (DownstreamServer, Arc<Mutex<Vec<(String, Value)>>>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let transport = Tee {
        inner: spawn(env),
        replies: Arc::clone(&replies),
    };
    let server = DownstreamServer::connect(id.to_string(), Box::new(transport)).expect("connect");
    (server, replies)
}

fn received_capabilities(replies: &Mutex<Vec<(String, Value)>>) -> Value {
    let replies = replies.lock().unwrap();
    let (_, init) = replies.iter().find(|(m, _)| m == "initialize").unwrap();
    init["serverInfo"]["received"].clone()
}

fn registry_with(flags: &[(&str, &str, bool)]) {
    let mut reg = Registry::default();
    for (id, key, on) in flags {
        let mut entry: ServerEntry = serde_json::from_value(json!({
            "id": id, "name": id, "transport": "stdio", "command": "node", "enabled": true
        }))
        .unwrap();
        if *on {
            entry.unknown_fields.insert((*key).into(), json!(true));
        }
        reg.servers.push(entry);
    }
    registry::save(&reg).unwrap();
}

struct DeclarationsReset;

impl Drop for DeclarationsReset {
    fn drop(&mut self) {
        toolport_plus::handshake::DECLARATIONS.reset();
    }
}

#[test]
fn test04_a_server_that_opted_in_is_told_the_capabilities_its_client_declared() {
    let _env = DataDirTestEnv::new("s8-declare");
    let _reset = DeclarationsReset;
    toolport_plus::handshake::DECLARATIONS.reset();
    registry_with(&[
        ("odh", "declareClientCapabilities", true),
        ("plain", "declareClientCapabilities", false),
    ]);

    let (_, before) = connect_through_tee("odh", &[]);
    assert_eq!(received_capabilities(&before), json!({}), "no client yet");

    toolport_plus::handshake::DECLARATIONS.note_client(Some(&json!({
        "elicitation": { "form": {} }, "roots": { "listChanged": true }, "tasks": {}
    })));
    let (_, opted) = connect_through_tee("odh", &[]);
    assert_eq!(
        received_capabilities(&opted),
        json!({ "elicitation": { "form": {} }, "roots": { "listChanged": true } }),
        "only the capabilities the gateway relays"
    );
    let (_, plain) = connect_through_tee("plain", &[]);
    assert_eq!(received_capabilities(&plain), json!({}));
    let (_, unknown) = connect_through_tee("not-in-the-registry", &[]);
    assert_eq!(received_capabilities(&unknown), json!({}));
}

#[test]
fn test05_a_servers_instructions_are_kept_from_its_handshake() {
    let _env = DataDirTestEnv::new("s8-instructions-kept");
    let _ = connect_through_tee("s8-told", &[("FAKE_INSTRUCTIONS", "FAKE-odh-instructions")]);
    assert_eq!(
        toolport_plus::handshake::stored_instructions("s8-told").as_deref(),
        Some("FAKE-odh-instructions")
    );
    let _ = connect_through_tee("s8-quiet", &[]);
    assert_eq!(
        toolport_plus::handshake::stored_instructions("s8-quiet"),
        None
    );
    let _ = connect_through_tee("s8-told", &[]);
    assert_eq!(
        toolport_plus::handshake::stored_instructions("s8-told"),
        None,
        "a reconnect that sends none clears the old text"
    );
}
