use super::*;
use crate::router::sanitize_segment;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::PathBuf;

const HOME: &str = "{{HOME}}";

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import_mcpm")
}

fn read(rel: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir().join(rel)).unwrap()).unwrap()
}

fn object(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

fn fixture_input() -> McpmInput {
    let short_ids = [
        ("anna-mcp", "anna"),
        ("google-docs-mcp", "gdocs"),
        ("google-docs-a", "gdocs-a"),
        ("google-docs-b", "gdocs-b"),
        ("moodle-mcp", "moodle"),
    ]
    .iter()
    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
    .collect();
    McpmInput {
        servers: object(read("input/servers.json")),
        sources: object(read("input/sources.json")),
        home: HOME.into(),
        short_ids,
    }
}

fn fixture_clients() -> Vec<ClientConfig> {
    ["claude-code", "claude-desktop", "cursor", "gemini-cli"]
        .iter()
        .map(|id| ClientConfig {
            client_id: (*id).into(),
            servers: object(read(&format!("input/{id}.json")))
                .get("mcpServers")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        })
        .collect()
}

fn fixture_mapping() -> Mapping {
    map_all(&fixture_input(), &fixture_clients())
}

#[test]
fn matches_expected_registry_golden() {
    let actual = fixture_mapping().golden();
    let path = dir().join("expected-registry.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn mapping_is_deterministic() {
    assert_eq!(fixture_mapping().golden(), fixture_mapping().golden());
}

#[test]
fn maps_twenty_servers_with_unique_short_ids() {
    let m = fixture_mapping();
    assert_eq!(m.servers.len(), 20);
    let mut seen = HashSet::new();
    for s in &m.servers {
        assert!(s.entry.id.len() <= 20, "{}", s.entry.id);
        assert!(seen.insert(sanitize_segment(&s.entry.id)), "{}", s.entry.id);
        assert_eq!(s.entry.source.as_deref(), Some(IMPORT_SOURCE));
    }
}

#[test]
fn longest_known_tool_name_fits_the_cap() {
    let m = fixture_mapping();
    let odoo = m
        .servers
        .iter()
        .find(|s| s.mcpm_name == "org-odoo")
        .unwrap();
    assert!(tool_name_fits(
        &odoo.entry.id,
        "get_criticmarkup_latest_content"
    ));
    assert!(!tool_name_fits(
        &"x".repeat(40),
        "get_criticmarkup_latest_content"
    ));
}

#[test]
fn no_secret_value_reaches_the_registry_json() {
    let m = fixture_mapping();
    let rendered = serde_json::to_string(&m.golden()).unwrap();
    let plain: Vec<&str> = m
        .servers
        .iter()
        .flat_map(|s| s.entry.env.iter())
        .filter_map(|e| e.value.as_deref())
        .collect();
    assert!(!m.secret_writes().is_empty());
    for w in m.secret_writes() {
        if plain.contains(&w.value.as_str()) {
            continue;
        }
        assert!(!rendered.contains(&w.value), "{}", w.vault_key());
    }
    for s in &m.servers {
        for e in &s.entry.env {
            assert_eq!(e.secret, e.value.is_none(), "{}::{}", s.entry.id, e.key);
        }
    }
}

#[test]
fn secrets_route_to_server_scoped_vault_keys() {
    let m = fixture_mapping();
    let keys: HashSet<String> = m.secret_writes().iter().map(|w| w.vault_key()).collect();
    assert!(keys.contains("gdocs::GOOGLE_CLIENT_ID"));
    assert!(keys.contains("gdocs-b::GOOGLE_CLIENT_SECRET"));
    assert!(keys.contains("gdocs-a::GOOGLE_CLIENT_SECRET"));
    assert!(keys.contains("slack::__http_auth__"));
    assert!(!keys.contains("gdocs-b::GOOGLE_MCP_PROFILE"));
}

#[test]
fn remote_servers_are_classified() {
    let m = fixture_mapping();
    let oauth: Vec<&str> = m
        .servers
        .iter()
        .filter(|s| s.oauth_via_gateway)
        .map(|s| s.mcpm_name.as_str())
        .collect();
    assert_eq!(oauth, ["clickup", "figma", "miro"]);
    let slack = m.servers.iter().find(|s| s.mcpm_name == "slack").unwrap();
    assert!(!slack.oauth_via_gateway);
    assert_eq!(slack.entry.transport, "http");
    assert!(slack.entry.command.is_none());
}

#[test]
fn client_profiles_bind_through_client_scopes() {
    let m = fixture_mapping();
    assert_eq!(m.client_scopes.len(), 4);
    let cc = m.profiles.iter().find(|p| p.name == "claude-code").unwrap();
    assert!(cc.enabled_server_ids.contains(&"gdocs-a".to_string()));
    assert!(cc.enabled_server_ids.contains(&"miro".to_string()));
    assert!(cc
        .enabled_server_ids
        .contains(&"miro-community".to_string()));
    assert_eq!(
        m.client_discovery.get("claude-code").map(String::as_str),
        Some("full")
    );
    let tag = m.profiles.iter().find(|p| p.name == "miro-stack").unwrap();
    assert_eq!(tag.enabled_server_ids, ["miro", "miro-community"]);
    let ids: HashSet<&str> = m.servers.iter().map(|s| s.entry.id.as_str()).collect();
    for p in &m.profiles {
        for id in &p.enabled_server_ids {
            assert!(ids.contains(id.as_str()), "{id}");
        }
    }
}

#[test]
fn desktop_proxy_shims_resolve_to_remote_servers() {
    let m = fixture_mapping();
    let desktop = m
        .profiles
        .iter()
        .find(|p| p.name == "claude-desktop")
        .unwrap();
    assert!(desktop.enabled_server_ids.contains(&"clickup".to_string()));
    assert!(desktop.enabled_server_ids.contains(&"figma".to_string()));
    assert!(m
        .skipped_clients
        .iter()
        .any(|s| s.client_id == "claude-desktop" && s.entry == "playwright"));
}

#[test]
fn helper_paths_are_flagged_for_relocation() {
    let m = fixture_mapping();
    let flagged: HashSet<&str> = m
        .warnings
        .iter()
        .filter(|w| w.kind == "relocate-path")
        .map(|w| w.server.as_str())
        .collect();
    assert!(flagged.contains("miro-community"));
    assert!(flagged.contains("scihub"));
    assert!(flagged.contains("anna"));
}

#[test]
fn secret_looking_args_are_bound_as_launch_inputs() {
    let input = McpmInput {
        servers: object(
            json!({"x": {"name": "x", "command": "run", "args": ["--token=abc", "ok"], "env": {}}}),
        ),
        sources: Map::new(),
        home: HOME.into(),
        short_ids: Default::default(),
    };
    let (servers, _) = map_servers(&input);
    let e = &servers[0].entry;
    assert_eq!(e.args, ["<launch-input>", "ok"]);
    let launch = e.launch.as_ref().unwrap();
    assert!(launch.validate(&e.args, true).is_ok());
    assert_eq!(servers[0].secrets[0].vault_key(), "x::ARG_0");
}

#[test]
fn bearer_prefix_sse_and_extra_headers() {
    let input = McpmInput {
        servers: object(json!({
            "a": {"name": "a", "url": "https://h.example/sse", "headers": {"Authorization": "Bearer tok", "X-Other": "v"}},
            "b": {"name": "b", "url": "https://h.example/mcp", "enabled": false}
        })),
        sources: Map::new(),
        home: HOME.into(),
        short_ids: Default::default(),
    };
    let (servers, warnings) = map_servers(&input);
    assert_eq!(servers[0].entry.transport, "sse");
    assert_eq!(servers[0].secrets[0].value, "tok");
    assert!(warnings.iter().any(|w| w.kind == "unsupported-header"));
    assert!(warnings
        .iter()
        .any(|w| w.kind == "disabled" && w.server == "b"));
}

#[test]
fn colliding_names_get_distinct_ids() {
    let input = McpmInput {
        servers: object(json!({
            "gh-api": {"name": "gh-api", "command": "a"},
            "gh_api": {"name": "gh_api", "command": "b"}
        })),
        sources: Map::new(),
        home: HOME.into(),
        short_ids: Default::default(),
    };
    let (servers, _) = map_servers(&input);
    assert_ne!(
        sanitize_segment(&servers[0].entry.id),
        sanitize_segment(&servers[1].entry.id)
    );
}
