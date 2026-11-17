//! End-to-end MCP client against the real `toolport-selfmcp` stdio binary: handshake, the tool and
//! resource catalog, errors for malformed requests, and the same answer as `toolportctl mcp call`.
//! The binary runs with a synthetic data directory and home, so nothing outside the scratch tree
//! is read or written.

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use toolport_plus::selfmcp::{Gate, RESOURCES, TOOLS};

mod common;
use common::client::Client;
use common::world::{World, SECRET_KEY};

#[test]
fn handshake_then_tools_and_resources_match_the_catalog() {
    let world = World::new("stdio-catalog");
    let mut client = Client::spawn(&world, &[]);
    let init = client.request(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    let result = &init["result"];
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert_eq!(result["serverInfo"]["name"], "toolport-plus-self");
    assert_eq!(
        result["capabilities"],
        json!({"tools": {}, "resources": {}})
    );
    assert!(result["instructions"]
        .as_str()
        .unwrap()
        .contains("mcpm://paths"));
    client.send_raw(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string());
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));

    let tools = client.list_tools();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, TOOLS.iter().map(|t| t.name).collect::<Vec<_>>());
    assert_eq!(
        names.iter().collect::<BTreeSet<_>>().len(),
        TOOLS.len(),
        "tool names are unique"
    );
    assert_eq!(
        Value::Array(tools.clone()),
        toolport_plus::selfmcp::tools_list()
    );
    for (tool, def) in tools.iter().zip(TOOLS) {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{}", def.name);
        assert_eq!(schema["additionalProperties"], false, "{}", def.name);
        assert!(
            tool["description"]
                .as_str()
                .unwrap()
                .starts_with(&format!("[tier {} ", def.tier)),
            "{}",
            def.name
        );
        assert_eq!(tool["annotations"]["readOnlyHint"], def.tier == 1);
        assert_eq!(tool["annotations"]["destructiveHint"], def.tier >= 4);
        assert_eq!(
            schema["properties"].get("confirm").is_some(),
            def.gate != Gate::None,
            "{} exposes confirm exactly when it is gated",
            def.name
        );
    }
    let listed = client.request("resources/list", json!({}))["result"]["resources"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        Value::Array(listed),
        toolport_plus::selfmcp::resources_list()
    );
    for def in RESOURCES {
        assert!(def.uri.starts_with("mcpm://"));
    }
    assert!(client.close().success());
}

#[test]
fn malformed_and_unsupported_requests_get_errors_and_the_server_keeps_serving() {
    let world = World::new("stdio-protocol");
    let mut client = Client::spawn(&world, &[]);
    client.handshake();

    client.send_raw("{ this is not json");
    let parse = client.read_line("a malformed line");
    assert_eq!(parse["error"]["code"], -32700);
    assert_eq!(parse["id"], Value::Null);

    assert_eq!(
        client.request("no/such/method", json!({}))["error"]["code"],
        -32601
    );
    assert_eq!(
        client.request("tools/call", json!({"arguments": {}}))["error"]["code"],
        -32602
    );
    assert_eq!(
        client.request("resources/read", json!({}))["error"]["code"],
        -32602
    );
    assert_eq!(
        client.request("resources/read", json!({"uri": "mcpm://does-not-exist"}))["error"]["code"],
        -32002
    );
    let unknown = client.call("no_such_tool", json!({}));
    assert_eq!(unknown.error_kind(), Some("unknown_tool"));

    client.send_raw(&json!({"jsonrpc": "2.0", "method": "notifications/unknown"}).to_string());
    client.send_raw("");
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));
    assert_eq!(client.list_tools().len(), TOOLS.len());
    assert!(client.close().success());
}

#[test]
fn arguments_are_validated_before_a_tool_runs() {
    let world = World::new("stdio-validation");
    let mut client = Client::spawn(&world, &[]);
    client.handshake();
    let before = world.snapshot();
    let reply = client.call("where_am_i", json!({"confirm": true}));
    assert_eq!(
        reply.error_kind(),
        Some("invalid_arguments"),
        "a read-only tool has no confirm parameter"
    );
    let reply = client.call("where_am_i", json!({"bogus": 1}));
    assert_eq!(reply.error_kind(), Some("invalid_arguments"));
    assert_eq!(world.snapshot(), before);
    assert!(client.close().success());
}

#[test]
fn the_self_mcp_server_and_toolportctl_mcp_call_give_the_same_result() {
    let world = World::new("stdio-parity");
    common::library_world::library_home(&world);
    let mut client = Client::spawn(&world, &[]);
    client.handshake();
    for name in TOOLS.iter().filter(|t| t.tier == 1).map(|t| t.name) {
        // The tools that name a server, a skill or a folder need one; the others take no arguments.
        let arguments = match name {
            "servers_get" => json!({"name": "alpha"}),
            "skills_get" => json!({"name": "demo"}),
            "agents_get" => json!({"name": "helper"}),
            "styles_get" => json!({"name": "plain"}),
            "skills_search" => json!({"query": "demo"}),
            "context_bundle_status" | "context_loads" | "context_compose" | "context_status" => {
                json!({"cwd": world.home.display().to_string()})
            }
            _ => json!({}),
        };
        let served = client.call(name, arguments.clone());
        assert!(!served.is_error(), "{name}: {}", served.0);
        let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
        command
            .args(["--json", "mcp", "call", name, "--args"])
            .arg(arguments.to_string())
            .env_clear()
            .current_dir(&world.repo)
            .stdin(Stdio::null());
        for (key, value) in world.env() {
            command.env(key, value);
        }
        let output = command.output().unwrap();
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["ok"], true, "{name}: {envelope}");
        assert_eq!(&envelope["data"]["result"], served.data(), "{name}");
    }
    assert!(client.close().success());
}

#[test]
fn the_server_exits_when_stdin_closes_and_never_prints_the_vault_key() {
    let world = World::new("stdio-exit");
    let mut client = Client::spawn(&world, &[SECRET_KEY]);
    client.handshake();
    client.call("where_am_i", json!({}));
    client.call("doctor", json!({}));
    assert!(client.close().success());
}
