//! Seeded randomized and edge-case tests for the mcpm to Toolport tool-name mapping: the 64
//! character guard, sanitising collisions, unicode and empty names, and short id derivation.

use super::*;
use crate::plus::randutil::{run_cases, Rng};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet};

const SERVER_NAMES: &[&str] = &[
    "alpha",
    "Alpha MCP",
    "alpha-mcp",
    "alpha_extra",
    "alpha-extra",
    "google-docs-mcp",
    "A-B",
    "a--b",
    "123",
    "mcp",
    "-mcp",
    "a/b",
    "",
    " ",
    "日本語",
    "Üñí-Server",
    "𝔘nicode",
    "x",
    "UPPER",
    "slack",
    "miro-community",
];

const TOOL_NAMES: &[&str] = &[
    "list-items",
    "list_items",
    "get_item",
    "search",
    "a",
    "a-b",
    "a_b",
    "a.b",
    "",
    "日本",
    "中国",
    "tool with space",
    "Ünï",
    "UPPER",
    "a__b",
    "create-note",
];

fn server_input(rng: &mut Rng) -> McpmInput {
    let mut servers = Map::new();
    for _ in 0..rng.range(0, 8) {
        let name = if rng.chance(75) {
            rng.pick(SERVER_NAMES).to_string()
        } else {
            rng.garbage(12)
        };
        let entry = if rng.chance(70) {
            json!({"command": "run", "args": [], "env": {}})
        } else {
            json!({"url": "https://example.invalid/mcp"})
        };
        servers.insert(name, entry);
    }
    let mut short_ids = BTreeMap::new();
    for name in servers.keys() {
        if rng.chance(30) {
            short_ids.insert(name.clone(), rng.slug(12));
        }
    }
    McpmInput {
        servers,
        sources: Map::new(),
        home: "{{HOME}}".into(),
        short_ids,
    }
}

fn manifest(rng: &mut Rng, servers: &[MappedServer]) -> ToolManifest {
    let mut out = ToolManifest::new();
    for _ in 0..rng.range(0, 5) {
        let server = if servers.is_empty() || rng.chance(10) {
            rng.garbage(6)
        } else {
            rng.pick(servers).mcpm_name.clone()
        };
        let tools = (0..rng.range(0, 6))
            .map(|_| match rng.below(8) {
                0 => "t".repeat(rng.range(30, 70)),
                1 => rng.garbage(10),
                _ => rng.pick(TOOL_NAMES).to_string(),
            })
            .collect();
        out.insert(server, tools);
    }
    out
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

struct Model {
    too_long: usize,
    collisions: usize,
    unknown: usize,
}

fn model(servers: &[MappedServer], manifest: &ToolManifest) -> Model {
    let mut m = Model {
        too_long: 0,
        collisions: 0,
        unknown: 0,
    };
    let mut by_new: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for (name, tools) in manifest {
        let Some(server) = servers.iter().find(|s| &s.mcpm_name == name) else {
            m.unknown += 1;
            continue;
        };
        let mut seen = HashSet::new();
        for tool in tools {
            if !seen.insert(tool) {
                continue;
            }
            let exposed = format!(
                "mcp__toolport__{}__{}",
                sanitize(&server.entry.id),
                sanitize(tool)
            );
            if exposed.len() > 64 {
                m.too_long += 1;
            }
            by_new
                .entry(exposed)
                .or_default()
                .insert(format!("mcp__mcpm_{name}__{tool}"));
        }
    }
    m.collisions = by_new.values().filter(|olds| olds.len() > 1).count();
    m
}

#[test]
fn name_map_agrees_with_an_independent_model_on_random_inputs() {
    let (mut ok_cases, mut err_cases) = (0, 0);
    run_cases("name-map-model", 1500, |_, rng| {
        let (servers, _) = map_servers(&server_input(rng));
        let manifest = manifest(rng, &servers);
        let expected = model(&servers, &manifest);
        let first = build_name_map(&servers, &manifest);
        assert_eq!(first, build_name_map(&servers, &manifest), "deterministic");
        match first {
            Ok(nm) => {
                ok_cases += 1;
                assert_eq!(
                    (expected.too_long, expected.collisions, expected.unknown),
                    (0, 0, 0)
                );
                let mut seen = HashSet::new();
                for (old, new) in &nm.map {
                    assert!(old.starts_with("mcp__mcpm_"), "{old}");
                    assert!(
                        new.starts_with("mcp__toolport__") && new.len() <= 64,
                        "{new}"
                    );
                    assert!(new.is_ascii() && !new.contains(['-', '.', ' ']), "{new}");
                    assert!(seen.insert(new.clone()), "duplicate {new}");
                }
                assert_eq!(nm.servers.len(), manifest.len());
                assert_eq!(nm.to_value()["count"], nm.map.len());
            }
            Err(err) => {
                err_cases += 1;
                assert_eq!(err.too_long.len(), expected.too_long);
                assert_eq!(err.collisions.len(), expected.collisions);
                assert_eq!(err.unknown_servers.len(), expected.unknown);
                assert!(!err.to_string().is_empty());
                for o in &err.too_long {
                    assert!(o.exposed.len() == o.length && o.length > 64);
                    if let Some(id) = &o.suggested_id {
                        assert!(!id.is_empty() && o.id.starts_with(id.as_str()));
                        assert!(!id.ends_with(['-', '_']));
                        assert!(tool_name_fits(id, &o.tool), "{id} {}", o.tool);
                    }
                }
                for c in &err.collisions {
                    assert!(c.old_names.len() >= 2);
                    assert!(c.old_names.iter().all(|n| n.starts_with("mcp__mcpm_")));
                }
            }
        }
    });
    assert!(ok_cases > 100 && err_cases > 100, "{ok_cases} {err_cases}");
}

#[test]
fn short_ids_are_unique_bounded_and_non_empty() {
    run_cases("name-map-short-ids", 600, |_, rng| {
        let mut taken = HashSet::new();
        let table = BTreeMap::new();
        for _ in 0..rng.range(1, 40) {
            let name = if rng.chance(60) {
                rng.pick(SERVER_NAMES).to_string()
            } else {
                rng.garbage(14)
            };
            let id = short_id(&name, &table, &taken);
            assert!(!id.is_empty(), "{name:?}");
            assert!(id.chars().count() <= 20, "{id}");
            assert!(
                !taken.iter().any(|t| sanitize(t) == sanitize(&id)),
                "{id} taken"
            );
            assert!(tool_name_fits(&id, "x"));
            assert_eq!(id, short_id(&name, &table, &taken));
            taken.insert(id);
        }
    });
}

#[test]
fn ids_survive_a_table_that_maps_everything_to_one_id() {
    let mut input = McpmInput::default();
    let mut table = BTreeMap::new();
    for i in 0..12 {
        input
            .servers
            .insert(format!("srv-{i}"), json!({"command": "x"}));
        table.insert(format!("srv-{i}"), "same".to_string());
    }
    input.short_ids = table;
    let (servers, _) = map_servers(&input);
    let ids: HashSet<String> = servers.iter().map(|s| sanitize(&s.entry.id)).collect();
    assert_eq!(ids.len(), 12);
    assert!(servers.iter().all(|s| s.entry.id.starts_with("same")));
}

#[test]
fn unicode_and_empty_names_are_sanitised_not_rejected() {
    let mut input = McpmInput::default();
    for name in ["日本語", "", "𝔘nicode", "Üñí"] {
        input.servers.insert(name.into(), json!({"command": "x"}));
    }
    let (servers, _) = map_servers(&input);
    assert_eq!(servers.len(), 4);
    let ids: HashSet<&str> = servers.iter().map(|s| s.entry.id.as_str()).collect();
    assert_eq!(ids.len(), 4, "{ids:?}");
    let tools: ToolManifest = servers
        .iter()
        .map(|s| {
            (
                s.mcpm_name.clone(),
                vec!["日本".into(), String::new(), "ok".into()],
            )
        })
        .collect();
    let nm = build_name_map(&servers, &tools).unwrap();
    assert_eq!(nm.map.len(), 12);
    assert!(nm.map.values().all(|v| v.is_ascii() && v.len() <= 64));
}

#[test]
fn distinct_unicode_tools_collapse_into_a_reported_collision() {
    let mut input = McpmInput::default();
    input.servers.insert("srv".into(), json!({"command": "x"}));
    let (servers, _) = map_servers(&input);
    let tools = ToolManifest::from([(
        "srv".to_string(),
        vec!["日本".to_string(), "中国".to_string()],
    )]);
    let err = build_name_map(&servers, &tools).unwrap_err();
    assert_eq!(err.collisions.len(), 1);
    assert_eq!(err.collisions[0].exposed, "mcp__toolport__srv____");
    assert_eq!(err.collisions[0].old_names.len(), 2);
}

#[test]
fn the_length_guard_counts_the_sanitised_name_in_bytes() {
    let mut input = McpmInput::default();
    input.servers.insert("srv".into(), json!({"command": "x"}));
    let (servers, _) = map_servers(&input);
    let room = 64 - exposed_prefix("srv").len();
    for (tool, ok) in [
        ("é".repeat(room), true),
        ("é".repeat(room + 1), false),
        ("日".repeat(room), true),
        ("日".repeat(room + 1), false),
        (String::new(), true),
    ] {
        let tools = ToolManifest::from([("srv".to_string(), vec![tool.clone()])]);
        let result = build_name_map(&servers, &tools);
        assert_eq!(result.is_ok(), ok, "{} chars", tool.chars().count());
        if let Ok(nm) = result {
            assert!(nm.map.values().all(|v| v.len() <= 64));
        }
    }
}

#[test]
fn manifests_naming_no_server_or_no_tools_map_to_nothing() {
    let nm = build_name_map(&[], &ToolManifest::new()).unwrap();
    assert!(nm.map.is_empty() && nm.servers.is_empty());
    let mut input = McpmInput::default();
    input.servers.insert("srv".into(), json!({"command": "x"}));
    let (servers, _) = map_servers(&input);
    let empty_tools = ToolManifest::from([("srv".to_string(), Vec::new())]);
    let nm = build_name_map(&servers, &empty_tools).unwrap();
    assert!(nm.map.is_empty());
    assert_eq!(nm.servers.len(), 1);
}

#[test]
fn manifest_files_that_are_not_name_to_tools_maps_are_errors() {
    use std::fs;
    let dir = std::env::temp_dir().join(format!("plus-prop-manifest-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tools.json");
    run_cases("name-map-manifest-files", 200, |_, rng| {
        let text = match rng.below(4) {
            0 => rng.garbage(40),
            1 => json!({"a": "not a list"}).to_string(),
            2 => json!({"a": [1, 2]}).to_string(),
            _ => json!([["a"]]).to_string(),
        };
        fs::write(&path, text).unwrap();
        assert!(load_manifest_for_test(&path).is_err());
    });
    fs::write(&path, b"\xff\xfe").unwrap();
    assert!(load_manifest_for_test(&path).is_err());
    fs::write(&path, r#"{"a": ["x"], "b": []}"#).unwrap();
    assert_eq!(load_manifest_for_test(&path).unwrap().len(), 2);
    assert!(load_manifest_for_test(&dir.join("missing.json")).is_err());
    let _ = fs::remove_dir_all(&dir);
}

fn load_manifest_for_test(path: &std::path::Path) -> Result<ToolManifest, String> {
    super::name_map::load_manifest(path)
}

#[test]
fn handler_rejects_missing_arguments() {
    for args in [
        json!({}),
        json!({"root": 5}),
        json!({"root": "/nonexistent-root-xyz"}),
        Value::Null,
    ] {
        assert!(name_map_handler(args.clone()).is_err(), "{args}");
        assert!(rename_refs_handler(args).is_err());
    }
}
