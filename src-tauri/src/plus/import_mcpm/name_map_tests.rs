use super::*;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import_mcpm")
}

fn read(rel: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(dir().join(rel)).unwrap()).unwrap()
}

fn object(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

fn fixture_servers() -> Vec<MappedServer> {
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
    let input = McpmInput {
        servers: object(read("input/servers.json")),
        sources: object(read("input/sources.json")),
        home: "{{HOME}}".into(),
        short_ids,
    };
    map_servers(&input).0
}

fn fixture_manifest() -> ToolManifest {
    serde_json::from_value(read("input/tools.json")).unwrap()
}

fn manifest(entries: &[(&str, &[&str])]) -> ToolManifest {
    entries
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_string(),
                v.iter().map(|t| (*t).to_string()).collect(),
            )
        })
        .collect()
}

#[test]
fn matches_name_map_golden() {
    let nm = build_name_map(&fixture_servers(), &fixture_manifest()).unwrap();
    let actual = nm.to_value();
    let path = dir().join("expected-name-map.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn twenty_servers_all_fit_and_are_unique() {
    let nm = build_name_map(&fixture_servers(), &fixture_manifest()).unwrap();
    assert_eq!(nm.servers.len(), 20);
    let mut seen = std::collections::HashSet::new();
    for new in nm.map.values() {
        assert!(new.len() <= 64, "{new}");
        assert!(new.starts_with("mcp__toolport__"));
        assert!(seen.insert(new.clone()), "{new}");
    }
}

#[test]
fn names_follow_router_aggregation() {
    let nm = build_name_map(&fixture_servers(), &fixture_manifest()).unwrap();
    assert_eq!(
        nm.map["mcp__mcpm_context7__resolve-library-id"],
        "mcp__toolport__context7__resolve_library_id"
    );
    assert_eq!(
        nm.map["mcp__mcpm_google-docs-mcp__list-items"],
        "mcp__toolport__gdocs__list_items"
    );
}

#[test]
fn exactly_64_chars_passes_and_65_fails() {
    let servers = fixture_servers();
    let room = 64 - exposed_prefix("miro-community").len();
    let ok = "a".repeat(room);
    let nm = build_name_map(&servers, &manifest(&[("miro-community", &[ok.as_str()])])).unwrap();
    assert_eq!(nm.map.values().next().unwrap().len(), 64);

    let long = "a".repeat(room + 1);
    let err =
        build_name_map(&servers, &manifest(&[("miro-community", &[long.as_str()])])).unwrap_err();
    assert_eq!(err.too_long.len(), 1);
    assert_eq!(err.too_long[0].length, 65);
    assert_eq!(err.too_long[0].id, "miro-community");
    assert_eq!(
        err.too_long[0].suggested_id.as_deref(),
        Some("miro-communit")
    );
    let text = err.to_string();
    assert!(text.contains("65 chars"), "{text}");
    assert!(text.contains("suggested short id: miro-communit"), "{text}");
}

#[test]
fn no_suggestion_when_tool_name_alone_is_too_long() {
    let long = "t".repeat(60);
    let err =
        build_name_map(&fixture_servers(), &manifest(&[("lib", &[long.as_str()])])).unwrap_err();
    assert_eq!(err.too_long[0].suggested_id, None);
    assert!(err.to_string().contains("no short id can fit"));
}

#[test]
fn collisions_within_and_across_servers_are_reported() {
    let servers = fixture_servers();
    let err = build_name_map(&servers, &manifest(&[("slack", &["list-x", "list_x"])])).unwrap_err();
    assert_eq!(err.collisions.len(), 1);
    assert_eq!(err.collisions[0].exposed, "mcp__toolport__slack__list_x");

    let err = build_name_map(
        &servers,
        &manifest(&[("miro", &["community__a"]), ("miro-community", &["a"])]),
    );
    assert!(err.is_ok(), "different ids never collide: {err:?}");
}

#[test]
fn cross_server_collision_after_sanitising() {
    let mut servers = fixture_servers();
    let mut twin = servers
        .iter()
        .find(|s| s.mcpm_name == "slack")
        .unwrap()
        .clone();
    twin.mcpm_name = "slack-twin".into();
    twin.entry.id = "sl_ack".into();
    servers.push(twin);
    let ok = build_name_map(
        &servers,
        &manifest(&[("slack", &["post"]), ("slack-twin", &["post"])]),
    );
    assert!(ok.is_ok());
    servers.last_mut().unwrap().entry.id = "slack".into();
    let err = build_name_map(
        &servers,
        &manifest(&[("slack", &["post"]), ("slack-twin", &["post"])]),
    )
    .unwrap_err();
    assert_eq!(err.collisions[0].old_names.len(), 2);
}

#[test]
fn duplicate_tools_in_manifest_are_not_collisions() {
    let nm = build_name_map(
        &fixture_servers(),
        &manifest(&[("slack", &["post", "post"])]),
    )
    .unwrap();
    assert_eq!(nm.map.len(), 1);
}

#[test]
fn unknown_manifest_server_is_an_error() {
    let err = build_name_map(&fixture_servers(), &manifest(&[("nope", &["x"])])).unwrap_err();
    assert_eq!(err.unknown_servers, vec!["nope".to_string()]);
}

#[test]
fn handler_reads_root_and_tools_files() {
    let root = dir().join("input");
    let tools = dir().join("input/tools.json");
    let out = name_map_handler(json!({
        "root": root.to_str().unwrap(),
        "tools": tools.to_str().unwrap(),
        "home": "{{HOME}}",
    }))
    .unwrap();
    assert_eq!(out["servers"].as_object().unwrap().len(), 20);

    let long = std::env::temp_dir().join(format!("name-map-long-{}.json", std::process::id()));
    std::fs::write(&long, json!({"lib": ["t".repeat(70)]}).to_string()).unwrap();
    let err = name_map_handler(json!({
        "root": root.to_str().unwrap(),
        "tools": long.to_str().unwrap(),
    }))
    .unwrap_err();
    let _ = std::fs::remove_file(&long);
    assert!(err.contains("max 64"), "{err}");
}
