use serde_json::{json, Value};
use toolport_plus_api::MemoryHost;

use super::{call_tool, handle_message, read_resource, tools_list, Gate, RESOURCES, TOOLS};

fn host() -> MemoryHost {
    MemoryHost {
        registry: toolport_plus_api::MemoryRegistry::with_servers(Vec::new()),
        seam: true,
        ..Default::default()
    }
}

#[test]
fn tool_names_are_unique_and_every_gated_tool_exposes_confirm() {
    let names: std::collections::BTreeSet<&str> = TOOLS.iter().map(|t| t.name).collect();
    assert_eq!(names.len(), TOOLS.len());
    for descriptor in tools_list().as_array().unwrap() {
        let name = descriptor["name"].as_str().unwrap();
        let tool = TOOLS.iter().find(|t| t.name == name).unwrap();
        assert_eq!(
            descriptor["inputSchema"]["properties"]
                .get("confirm")
                .is_some(),
            tool.gate != Gate::None,
            "{name}"
        );
    }
}

#[test]
fn a_call_validates_arguments_before_it_runs() {
    let host = host();
    for (args, kind) in [
        (json!({"x": 1}), "invalid_arguments"),
        (json!([1]), "invalid_arguments"),
        (json!({"confirm": true}), "invalid_arguments"),
    ] {
        assert_eq!(
            call_tool(&host, "where_am_i", &args).unwrap_err().kind,
            kind
        );
    }
    assert_eq!(
        call_tool(&host, "nope", &json!({})).unwrap_err().kind,
        "unknown_tool"
    );
    assert!(call_tool(&host, "where_am_i", &Value::Null).is_ok());
}

#[test]
fn a_result_never_carries_a_live_secret() {
    let host = host();
    let value = call_tool(&host, "where_am_i", &json!({})).unwrap();
    assert!(value.get("secretsBackend").is_some());
    assert!(!value.to_string().contains("[redacted]"));
}

#[test]
fn json_rpc_answers_initialize_ping_and_unknown_methods() {
    let host = host();
    let reply = |message: Value| handle_message(&host, &message).unwrap();
    let init = reply(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}));
    assert_eq!(init["result"]["serverInfo"]["name"], super::SERVER_NAME);
    assert_eq!(init["result"]["serverInfo"]["version"], "0.0.0-test");
    assert_eq!(
        reply(json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}))["result"],
        json!({})
    );
    assert_eq!(
        reply(json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}))["error"]["code"],
        -32601
    );
    assert!(handle_message(
        &host,
        &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
    )
    .is_none());
}

#[test]
fn every_resource_reads_through_the_host() {
    let host = host();
    for def in RESOURCES {
        let (text, mime) = read_resource(&host, def.uri).unwrap();
        assert_eq!(mime, def.mime);
        assert!(
            !text.is_empty() || def.uri.ends_with("servers"),
            "{}",
            def.uri
        );
    }
    assert_eq!(
        read_resource(&host, "mcpm://nope").unwrap_err().kind,
        "unknown_resource"
    );
}
