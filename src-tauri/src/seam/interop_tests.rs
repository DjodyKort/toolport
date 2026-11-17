use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::downstream::{StdioTransport, Transport, TransportError};
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
