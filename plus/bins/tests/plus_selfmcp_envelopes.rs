//! Contract per self-MCP tool and resource: one golden result per case under
//! `plus/fixtures/selfmcp-envelopes/`, produced by the real `toolport-selfmcp` stdio server in
//! the synthetic world of `common/world.rs`. The GUI and agents parse these results, so a change
//! in their shape must be a reviewed change: `CTL_ENVELOPE_BLESS=1 cargo test -p
//! toolport-plus-bins --test plus_selfmcp_envelopes` rewrites the goldens (rejected when `CI` is
//! set).
//!
//! Adding a tool: `case("<tool>", &[read(...), refused(...), write(...)])`. A call that only reads
//! must leave the world as it was, so must a refusal and a failure. A resource needs no case:
//! every entry of `RESOURCES` is read in the same world.

use std::collections::BTreeSet;

use serde_json::{json, Value};
use toolport_plus::selfmcp::{RESOURCES, TOOLS};

mod common;
use common::client::Client;
use common::world::{World, FAKE_SECRET, SECRET_KEY};
use common::{golden, normalize};

/// Goldens that still hold the previous line's text; they are re-blessed in the golden-bump commit.
const BUMP_PENDING: &[&str] = &["doctor", "resource-paths", "resource-status", "where_am_i"];

#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Ok,
    Error(&'static str),
}

#[derive(Clone, Copy)]
struct Call {
    label: &'static str,
    tool: &'static str,
    args: &'static str,
    expect: Expect,
    writes: bool,
    golden: bool,
}

/// A call that succeeds and leaves the world as it was: a read or a dry run.
const fn read(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    Call {
        label,
        tool,
        args,
        expect: Expect::Ok,
        writes: false,
        golden: true,
    }
}

/// A call that fails with this error kind and changes nothing.
const fn fails(
    label: &'static str,
    kind: &'static str,
    tool: &'static str,
    args: &'static str,
) -> Call {
    Call {
        expect: Expect::Error(kind),
        ..read(label, tool, args)
    }
}

struct Case {
    tool: &'static str,
    calls: &'static [Call],
}

const fn case(tool: &'static str, calls: &'static [Call]) -> Case {
    Case { tool, calls }
}

const CASES: &[Case] = &[
    case(
        "where_am_i",
        &[
            read("", "where_am_i", "{}"),
            fails(
                "invalid",
                "invalid_arguments",
                "where_am_i",
                r#"{"depth":2}"#,
            ),
            fails("wrong-type", "invalid_arguments", "where_am_i", r#"[1]"#),
        ],
    ),
    case("doctor", &[read("", "doctor", "{}")]),
    case("flow_diagram", &[read("", "flow_diagram", "{}")]),
];

fn stem(case: &Case, call: &Call) -> String {
    if call.label.is_empty() {
        case.tool.to_string()
    } else {
        format!("{}.{}", case.tool, call.label)
    }
}

fn resource_stem(uri: &str) -> String {
    format!(
        "resource-{}",
        uri.trim_start_matches("mcpm://").replace('/', "-")
    )
}

fn changed_files(
    world: &World,
    before: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
) -> Vec<String> {
    let after = world.snapshot();
    after
        .iter()
        .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
        .map(|(path, _)| path.display().to_string())
        .chain(
            before
                .keys()
                .filter(|p| !after.contains_key(*p))
                .map(|p| format!("removed {}", p.display())),
        )
        .collect()
}

fn arguments(case: &Case, call: &Call) -> Value {
    serde_json::from_str(call.args)
        .unwrap_or_else(|e| panic!("{} {}: arguments are not JSON: {e}", case.tool, call.label))
}

fn run_case(case: &Case) {
    let world = World::new(case.tool);
    let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
    client.handshake();
    for call in case.calls {
        let name = format!("{} {}", case.tool, call.label);
        let args = arguments(case, call);
        let before = world.snapshot();
        let reply = client.call(call.tool, args.clone());
        match call.expect {
            Expect::Ok => {
                assert!(
                    !reply.is_error(),
                    "{name}: expected success, got {}",
                    reply.0
                );
                let parsed: Value = serde_json::from_str(reply.text())
                    .unwrap_or_else(|e| panic!("{name}: text is not the JSON payload: {e}"));
                assert_eq!(
                    &parsed,
                    reply.data(),
                    "{name}: text and structuredContent must agree"
                );
            }
            Expect::Error(kind) => {
                assert!(
                    reply.is_error(),
                    "{name}: expected an error, got {}",
                    reply.0
                );
                assert_eq!(reply.error_kind(), Some(kind), "{name}: {}", reply.0);
                assert!(
                    reply.text().starts_with(&format!("{kind}: ")),
                    "{name}: text {:?}",
                    reply.text()
                );
            }
        }
        if !call.writes {
            let changed = changed_files(&world, &before);
            assert!(
                changed.is_empty(),
                "{name}: a read, a refusal or a failure changed the world: {changed:?}"
            );
        }
        let stem = stem(case, call);
        if call.golden && !BUMP_PENDING.contains(&stem.as_str()) {
            let mut result = json!({
                "tool": call.tool,
                "arguments": args,
                "isError": reply.is_error(),
                "result": reply.data(),
            });
            if reply.is_error() {
                result["text"] = json!(reply.text());
            }
            golden::assert_golden_in(
                &golden::selfmcp_root(),
                &stem,
                &normalize::normalize(&world, &result),
            );
        }
    }
    assert!(client.close().success(), "{}: server exit", case.tool);
}

#[test]
fn every_tool_call_matches_its_golden_result() {
    let failures: Vec<String> = CASES
        .iter()
        .filter_map(|case| {
            std::panic::catch_unwind(|| run_case(case))
                .err()
                .map(|payload| {
                    let message = payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    format!("{}: {message}", case.tool)
                })
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn every_resource_matches_its_golden_result() {
    let world = World::new("resources");
    let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
    client.handshake();
    let before = world.snapshot();
    let mut seen = BTreeSet::new();
    for def in RESOURCES {
        let content = client.read(def.uri);
        assert_eq!(content["uri"], def.uri);
        assert_eq!(content["mimeType"], def.mime);
        let text = content["text"].as_str().expect("text content");
        let body = if def.mime == "application/json" {
            json!({"uri": def.uri, "mimeType": def.mime,
                   "json": serde_json::from_str::<Value>(text)
                       .unwrap_or_else(|e| panic!("{} is not JSON ({e}): {text}", def.uri))})
        } else {
            json!({"uri": def.uri, "mimeType": def.mime, "text": text})
        };
        let stem = resource_stem(def.uri);
        assert!(seen.insert(stem.clone()), "{} collides", def.uri);
        if !BUMP_PENDING.contains(&stem.as_str()) {
            golden::assert_golden_in(
                &golden::selfmcp_root(),
                &stem,
                &normalize::normalize(&world, &body),
            );
        }
    }
    assert_eq!(changed_files(&world, &before), Vec::<String>::new());
    assert!(client.close().success());
}

fn tool_names() -> BTreeSet<&'static str> {
    TOOLS.iter().map(|t| t.name).collect()
}

#[test]
fn every_case_names_a_tool_and_every_golden_belongs_to_a_case() {
    let names = tool_names();
    let mut owned: BTreeSet<String> = RESOURCES.iter().map(|r| resource_stem(r.uri)).collect();
    let mut seen = BTreeSet::new();
    for case in CASES {
        assert!(
            names.contains(case.tool),
            "case `{}` is not a catalog tool",
            case.tool
        );
        assert!(seen.insert(case.tool), "case `{}` appears twice", case.tool);
        let labels: BTreeSet<&str> = case.calls.iter().map(|c| c.label).collect();
        assert_eq!(
            labels.len(),
            case.calls.len(),
            "{}: labels repeat",
            case.tool
        );
        for call in case.calls {
            assert_eq!(
                call.tool, case.tool,
                "{}: a case calls its own tool",
                call.label
            );
            if call.golden {
                owned.insert(stem(case, call));
            }
        }
    }
    for entry in std::fs::read_dir(golden::selfmcp_root()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = name.strip_suffix(".json").unwrap_or(&name);
        assert!(owned.contains(stem), "golden {name} belongs to no case");
    }
}

#[test]
fn selfmcp_coverage() {
    let world = World::new("coverage");
    let mut client = Client::spawn(&world, &[]);
    client.handshake();
    let listed: BTreeSet<String> = client
        .list_tools()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        listed,
        tool_names().iter().map(|n| n.to_string()).collect(),
        "tools/list and the catalog differ"
    );
    let served: BTreeSet<String> = client.request("resources/list", json!({}))["result"]
        ["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        served,
        RESOURCES.iter().map(|r| r.uri.to_string()).collect(),
        "resources/list and the catalog differ"
    );
    client.close();

    let covered: BTreeSet<&str> = CASES
        .iter()
        .filter(|case| {
            case.calls.iter().any(|c| {
                c.golden
                    && c.expect == Expect::Ok
                    && golden::read_in(&golden::selfmcp_root(), &stem(case, c)).is_some()
            })
        })
        .map(|case| case.tool)
        .collect();
    let missing: Vec<&str> = tool_names()
        .into_iter()
        .filter(|name| !covered.contains(name))
        .collect();
    let missing_resources: Vec<String> = RESOURCES
        .iter()
        .map(|r| resource_stem(r.uri))
        .filter(|stem| golden::read_in(&golden::selfmcp_root(), stem).is_none())
        .collect();
    eprintln!(
        "selfmcp contract: {} tools, {} with a golden result, {} uncovered",
        TOOLS.len(),
        TOOLS.len() - missing.len(),
        missing.len() + missing_resources.len(),
    );
    assert!(
        missing.is_empty(),
        "no golden result for tools: {missing:?}"
    );
    assert!(
        missing_resources.is_empty(),
        "no golden result for resources: {missing_resources:?}"
    );
}

/// The vault key is a live secret in the process: it must not show up in a result, a resource or a
/// golden. The client fails on the first line that holds it.
#[test]
fn no_output_carries_the_vault_key_or_a_server_secret() {
    for case in CASES {
        let world = World::new(case.tool);
        let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
        client.handshake();
        for call in case.calls {
            let reply = client.call(call.tool, arguments(case, call));
            let text = reply.0.to_string();
            for secret in [FAKE_SECRET, SECRET_KEY] {
                assert!(!text.contains(secret), "{} {}", case.tool, call.label);
            }
        }
        for def in RESOURCES {
            let content = client.read(def.uri);
            let text = content.to_string();
            for secret in [FAKE_SECRET, SECRET_KEY] {
                assert!(!text.contains(secret), "{}", def.uri);
            }
        }
        client.close();
    }
    for entry in std::fs::read_dir(golden::selfmcp_root()).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for secret in [FAKE_SECRET, SECRET_KEY] {
            assert!(
                !text.contains(secret),
                "golden {} contains a secret value",
                entry.path().display()
            );
        }
    }
}
