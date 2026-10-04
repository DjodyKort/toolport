//! Contract per self-MCP tool and resource (MIG-GUI-13): one golden result per case under
//! `tests/fixtures/selfmcp-envelopes/`, produced by the real `toolport-selfmcp` stdio server in
//! the synthetic world of `common/ctl_world.rs`. The GUI and the agents parse these results, so a
//! change in their shape must be a reviewed change:
//! `CTL_ENVELOPE_BLESS=1 cargo test --no-default-features --test selfmcp_envelopes` rewrites the
//! goldens (rejected when `CI` is set).
//!
//! Adding a tool (`selfmcp_envelopes/cases.rs`):
//!   1. `case("<tool>", &[read(...), refused(...), write(...)])`: a call that only reads must leave
//!      the world as it was, so must a refusal and a failure; a call that writes says so with
//!      `write`. `setup` calls prepare the world and write no golden.
//!   2. bless, read the new files, `npx prettier --write src-tauri/tests/fixtures/selfmcp-envelopes`
//!      and describe each result in `src/plus/types/`.
//! A resource needs no case: every entry of `RESOURCES` is read in the same world.
//! `selfmcp_coverage` prints what is still uncovered; `CTL_CONTRACT_STRICT=1` turns that into a
//! failure. `no_output_carries_a_canary_secret` runs every case again with a canary in the vault.

#![cfg(unix)]

use std::collections::BTreeSet;

use conduit_lib::plus::selfmcp::{RESOURCES, TOOLS};
use serde_json::{json, Value};

#[path = "selfmcp_envelopes/cases.rs"]
mod cases;
#[path = "common/claude_stub.rs"]
mod claude_stub;
#[path = "common/ctl_fixtures.rs"]
mod ctl_fixtures;
#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/golden.rs"]
mod golden;
#[path = "common/loads_world.rs"]
mod loads_world;
#[path = "common/normalize.rs"]
mod normalize;
#[path = "common/selfmcp_client.rs"]
mod selfmcp_client;
#[path = "common/sources_world.rs"]
mod sources_world;

use ctl_fixtures::PASSPHRASE;
use ctl_world::{CtlWorld, FAKE_SECRET};
use selfmcp_client::Client;

const CANARY: &str = "FAKE-canary-in-the-vault-5b07";

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
    hook: Option<fn(&CtlWorld)>,
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
        hook: None,
    }
}

/// A call that succeeds and changes the world.
const fn write(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    Call {
        writes: true,
        ..read(label, tool, args)
    }
}

/// A call that prepares the world for the calls after it and records no golden.
const fn setup(tool: &'static str, args: &'static str) -> Call {
    Call {
        golden: false,
        ..write("", tool, args)
    }
}

/// Prepares the world with the real `toolportctl` (a step the self-MCP server has no tool for).
const fn hook(prepare: fn(&CtlWorld)) -> Call {
    Call {
        hook: Some(prepare),
        ..setup("", "{}")
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

/// A gated call without `confirm`.
const fn refused(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    fails(label, "refused", tool, args)
}

struct Case {
    tool: &'static str,
    prepare: Option<fn(&CtlWorld)>,
    calls: &'static [Call],
}

const fn case(tool: &'static str, calls: &'static [Call]) -> Case {
    Case {
        tool,
        prepare: None,
        calls,
    }
}

/// A case whose world needs more than the shared fixture (a tap, a remote, a sync setup).
const fn prepared(tool: &'static str, prepare: fn(&CtlWorld), calls: &'static [Call]) -> Case {
    Case {
        tool,
        prepare: Some(prepare),
        calls,
    }
}

fn world(tag: &str) -> CtlWorld {
    CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"))
}

/// `{repo}`, `{home}`, `{data}`, `{base}` and `{mock}` in an argument stand for the synthetic world.
fn expand(world: &CtlWorld, text: &str) -> String {
    text.replace("{repo}", &world.path(&world.repo))
        .replace("{home}", &world.path(&world.home))
        .replace("{data}", &world.path(&world.data))
        .replace("{base}", &world.path(&world.base))
        .replace("{mock}", &world.mock)
}

fn world_for(case: &Case) -> CtlWorld {
    let world = world(case.tool);
    if let Some(prepare) = case.prepare {
        prepare(&world);
    }
    world
}

fn arguments(world: &CtlWorld, case: &Case, call: &Call) -> Value {
    serde_json::from_str(&expand(world, call.args))
        .unwrap_or_else(|e| panic!("{} {}: arguments are not JSON: {e}", case.tool, call.label))
}

fn all_cases() -> impl Iterator<Item = &'static Case> {
    cases::ALL.iter()
}

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
    world: &CtlWorld,
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
        .map(|p| {
            normalize::world_roots(world)
                .iter()
                .fold(p, |acc, root| acc.replace(root, "<WORLD>"))
        })
        .collect()
}

fn run_case(case: &Case) {
    let world = world_for(case);
    let mut client = Client::spawn(&world, &[FAKE_SECRET, PASSPHRASE]);
    client.handshake();
    for call in case.calls {
        if let Some(prepare) = call.hook {
            prepare(&world);
            continue;
        }
        let name = format!("{} {}", case.tool, call.label);
        let args = arguments(&world, case, call);
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

        if call.golden {
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
                &stem(case, call),
                &normalize::normalize(&world, &result),
            );
        }
    }
    assert!(client.close().success(), "{}: server exit", case.tool);
}

fn failures_of(results: Vec<(String, std::thread::Result<()>)>) -> Vec<String> {
    results
        .into_iter()
        .filter_map(|(name, result)| {
            result.err().map(|payload| {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                format!("{name}: {message}")
            })
        })
        .collect()
}

#[test]
fn every_tool_call_matches_its_golden_result() {
    let results = all_cases()
        .map(|case| {
            (
                case.tool.to_string(),
                std::panic::catch_unwind(|| run_case(case)),
            )
        })
        .collect();
    let failures = failures_of(results);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn run_resources(forbidden: &[&str], canary: bool, check: &dyn Fn(&CtlWorld, &str, &Value)) {
    let world = world("resources");
    if canary {
        ctl_fixtures::ctl(
            &world,
            &["secret", "set", "alpha", "CANARY_KEY"],
            Some(CANARY),
        );
    }
    let mut client = Client::spawn(&world, forbidden);
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
        assert!(seen.insert(resource_stem(def.uri)), "{} collides", def.uri);
        check(&world, &resource_stem(def.uri), &body);
    }
    let changed = changed_files(&world, &before);
    assert!(
        changed.is_empty(),
        "reading resources changed the world: {changed:?}"
    );
    assert!(client.close().success());
}

#[test]
fn every_resource_matches_its_golden_result() {
    run_resources(&[FAKE_SECRET], false, &|world, stem, body| {
        golden::assert_golden_in(
            &golden::selfmcp_root(),
            stem,
            &normalize::normalize(world, body),
        );
    });
}

fn tool_names() -> BTreeSet<&'static str> {
    TOOLS.iter().map(|t| t.name).collect()
}

#[test]
fn every_case_names_a_tool_and_every_golden_belongs_to_a_case() {
    let names = tool_names();
    let mut owned: BTreeSet<String> = RESOURCES.iter().map(|r| resource_stem(r.uri)).collect();
    let mut seen = BTreeSet::new();
    for case in all_cases() {
        assert!(
            names.contains(case.tool),
            "case `{}` is not a catalog tool",
            case.tool
        );
        assert!(seen.insert(case.tool), "case `{}` appears twice", case.tool);
        let labels: BTreeSet<(&str, bool)> = case
            .calls
            .iter()
            .filter(|c| c.golden)
            .map(|c| (c.label, c.golden))
            .collect();
        let goldens = case.calls.iter().filter(|c| c.golden).count();
        assert_eq!(labels.len(), goldens, "{}: golden labels repeat", case.tool);
        if goldens > 1 {
            assert!(
                !labels.iter().any(|(label, _)| label.is_empty()),
                "{}: several goldens need labels",
                case.tool
            );
        }
        for call in case.calls.iter().filter(|c| c.hook.is_none()) {
            assert!(
                names.contains(call.tool),
                "{}: `{}` is not a catalog tool",
                case.tool,
                call.tool
            );
            if call.golden {
                assert_eq!(
                    call.tool, case.tool,
                    "{}: a golden belongs to the tool of its case",
                    call.label
                );
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
    let world = world("coverage");
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

    let covered: BTreeSet<&str> = all_cases()
        .filter(|case| case.calls.iter().any(|c| c.golden))
        .map(|case| case.tool)
        .collect();
    let missing: Vec<&str> = tool_names()
        .into_iter()
        .filter(|name| !covered.contains(name))
        .collect();
    let error_only: Vec<&str> = all_cases()
        .filter(|case| {
            !case
                .calls
                .iter()
                .any(|c| c.golden && c.expect == Expect::Ok)
        })
        .map(|case| case.tool)
        .collect();
    let missing_resources: Vec<String> = RESOURCES
        .iter()
        .map(|r| resource_stem(r.uri))
        .filter(|stem| golden::read_in(&golden::selfmcp_root(), stem).is_none())
        .collect();
    eprintln!(
        "selfmcp contract: {} of {} tools have a golden result, {} still to add; \
         {} of {} resources have one, {} still to add; tools whose only golden is an error: {error_only:?}",
        TOOLS.len() - missing.len(),
        TOOLS.len(),
        missing.len(),
        RESOURCES.len() - missing_resources.len(),
        RESOURCES.len(),
        missing_resources.len(),
    );
    if std::env::var_os("CTL_CONTRACT_STRICT").is_some_and(|v| !v.is_empty() && v != "0") {
        assert!(
            missing.is_empty(),
            "no golden result for tools: {missing:?}"
        );
        assert!(
            missing_resources.is_empty(),
            "no golden result for resources: {missing_resources:?}"
        );
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// Every case runs again in a world whose vault holds a canary: it must not show up in a tool
/// result, in a file a tool wrote, or in a golden. The client fails on the first line that holds one.
#[test]
fn no_output_carries_a_canary_secret() {
    let secrets = [CANARY, PASSPHRASE, FAKE_SECRET];
    let mut calls = 0;
    for case in all_cases() {
        let world = world_for(case);
        ctl_fixtures::ctl(
            &world,
            &["secret", "set", "alpha", "CANARY_KEY"],
            Some(CANARY),
        );
        let mut client = Client::spawn(&world, &secrets);
        client.handshake();
        for call in case.calls {
            if let Some(prepare) = call.hook {
                prepare(&world);
                continue;
            }
            let args = arguments(&world, case, call);
            let reply = client.call(call.tool, args);
            calls += 1;
            let text = reply.0.to_string();
            for secret in secrets {
                assert!(
                    !text.contains(secret),
                    "{} {}: a secret value in the result",
                    case.tool,
                    call.label
                );
            }
        }
        client.close();
        for (path, bytes) in world.snapshot() {
            for secret in [CANARY, PASSPHRASE] {
                assert!(
                    !contains(&bytes, secret),
                    "{}: {} holds a secret in plain text",
                    case.tool,
                    path.display()
                );
            }
        }
    }
    assert!(calls > 100, "{calls} calls");
    run_resources(&secrets, true, &|_, _, _| {});

    for entry in std::fs::read_dir(golden::selfmcp_root()).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for secret in secrets {
            assert!(
                !text.contains(secret),
                "golden {} contains a secret value",
                entry.path().display()
            );
        }
    }
}
