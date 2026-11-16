use serde_json::{json, Value};
use toolport_plus_api::{AccessSetInfo, MemoryHost, MemoryRegistry, MemorySecrets, ServerRecord};

use super::policy::{row_of, Preview, Surface, Tier, ROWS, SUBS, TOOL_ROWS};
use super::{find_command, parse, registry, run_with, COMMANDS};
use crate::selfmcp::{Gate, TOOLS};

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn host() -> MemoryHost {
    let record = |id: &str, enabled: bool| ServerRecord {
        id: id.into(),
        name: id.into(),
        enabled,
        ..Default::default()
    };
    let registry = MemoryRegistry::with_servers(vec![record("a", true), record("b", false)]);
    *registry.access_sets.borrow_mut() = vec![AccessSetInfo {
        id: "focus".into(),
        name: "Focus".into(),
        server_ids: vec!["a".into()],
    }];
    MemoryHost {
        registry,
        secrets: MemorySecrets {
            data_dir: Some(std::env::temp_dir().join("plus-ctl-tests-no-such-dir")),
            ..Default::default()
        },
        seam: true,
        ..Default::default()
    }
}

fn run(host: &MemoryHost, argv: &[&str]) -> (i32, Value, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(host, &strings(argv), &mut out, &mut err);
    let out = String::from_utf8(out).unwrap();
    let value = serde_json::from_str(out.trim()).unwrap_or(Value::Null);
    (code, value, String::from_utf8(err).unwrap())
}

#[test]
fn parse_collects_global_flags_and_keeps_command_flags_in_order() {
    let parsed = parse(&strings(&[
        "--json",
        "mcp",
        "install",
        "--access-set",
        "focus",
        "--data-dir",
        "/x",
    ]))
    .unwrap();
    assert!(parsed.json);
    assert_eq!(parsed.data_dir.as_deref(), Some("/x"));
    assert_eq!(
        parsed.positional,
        strings(&["mcp", "install", "--access-set", "focus"])
    );
    assert!(parse(&strings(&["--bogus"])).is_err());
    assert!(parse(&strings(&["--data-dir"])).is_err());
    assert!(parse(&strings(&["--data-dir="])).is_err());
    assert_eq!(
        parse(&strings(&["--data-dir=/y", "status"]))
            .unwrap()
            .data_dir
            .as_deref(),
        Some("/y")
    );
    let after_dashes = parse(&strings(&["mcp", "call", "x", "--", "--json"])).unwrap();
    assert!(!after_dashes.json);
}

#[test]
fn the_longest_matching_command_wins_and_the_rest_are_its_arguments() {
    let words = strings(&["mcp", "call", "where_am_i"]);
    let (command, rest) = find_command(&words).unwrap();
    assert_eq!(command.path, ["mcp"]);
    assert_eq!(rest, strings(&["call", "where_am_i"]));
    assert!(find_command(&strings(&["nope"])).is_none());
}

#[test]
fn status_and_doctor_run_through_the_trait_without_touching_disk() {
    let host = host();
    let (code, status, _) = run(&host, &["--json", "status"]);
    assert_eq!(code, 0);
    assert_eq!(status["command"], "status");
    assert_eq!(status["schemaVersion"], 1);
    assert_eq!(status["data"]["serverCount"], 2);
    assert_eq!(status["data"]["accessSetCount"], 1);
    let (code, doctor, _) = run(&host, &["--json", "doctor"]);
    assert_eq!(code, 0, "{doctor}");
    assert_eq!(doctor["data"]["healthy"], true);
    let seamless = MemoryHost {
        seam: false,
        ..host
    };
    let (code, doctor, _) = run(&seamless, &["--json", "doctor"]);
    assert_eq!(code, 1);
    assert_eq!(doctor["error"]["code"], "unhealthy");
    assert_eq!(doctor["data"]["healthy"], false);
}

#[test]
fn human_output_goes_to_stdout_on_success_and_to_stderr_on_failure() {
    let host = host();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        run_with(&host, &strings(&["status"]), &mut out, &mut err),
        0
    );
    assert!(String::from_utf8(out).unwrap().contains("Toolport+"));
    assert!(err.is_empty());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(run_with(&host, &strings(&["nope"]), &mut out, &mut err), 2);
    assert!(out.is_empty());
    assert!(String::from_utf8(err)
        .unwrap()
        .contains("unknown command: nope"));
}

#[test]
fn data_dir_goes_to_the_host_before_the_command_runs() {
    let host = host();
    run(&host, &["--data-dir", "/somewhere", "status"]);
    assert_eq!(
        host.data_dir_used.borrow().as_deref(),
        Some(std::path::Path::new("/somewhere"))
    );
}

#[test]
fn mcp_install_then_call_through_the_same_core_as_the_server() {
    let host = host();
    let (code, installed, _) = run(&host, &["--json", "mcp", "install"]);
    assert_eq!(code, 0, "{installed}");
    assert_eq!(installed["data"]["action"], "created");
    let (code, called, _) = run(&host, &["--json", "mcp", "call", "where_am_i"]);
    assert_eq!(code, 0, "{called}");
    assert_eq!(
        called["data"]["result"],
        crate::selfmcp::call_tool(&host, "where_am_i", &json!({})).unwrap()
    );
    assert_eq!(called["data"]["result"]["serverCount"], 3);
}

fn leaf_ids() -> Vec<String> {
    let mut ids = Vec::new();
    for command in COMMANDS {
        let id = command.path.join(" ");
        let subs: Vec<String> = SUBS
            .iter()
            .filter(|s| s.parent == id)
            .map(|s| s.id())
            .collect();
        if subs.is_empty() {
            ids.push(id);
        } else {
            ids.extend(subs);
        }
    }
    ids
}

#[test]
fn every_leaf_command_has_exactly_one_policy_row_and_every_row_a_command() {
    let leaves = leaf_ids();
    for id in &leaves {
        assert_eq!(
            ROWS.iter().filter(|r| r.id == id).count(),
            1,
            "{id}: one policy row"
        );
    }
    for row in ROWS {
        assert!(
            leaves.contains(&row.id.to_string()),
            "{}: stale row",
            row.id
        );
    }
    for sub in SUBS {
        assert!(
            COMMANDS.iter().any(|c| c.path.join(" ") == sub.parent),
            "{}: no parent row",
            sub.id()
        );
    }
}

#[test]
fn every_row_says_how_the_smoke_test_covers_it() {
    for row in ROWS {
        match row.smoke {
            super::Smoke::Skip(reason) => {
                assert!(row.tier != Tier::Read, "{}: reads are run", row.id);
                assert!(
                    !reason.trim().is_empty(),
                    "{}: skipped without reason",
                    row.id
                );
            }
            super::Smoke::Read(_) => {
                let reads = row.tier == Tier::Read
                    || row.reads_by_default
                    || matches!(row.preview, Preview::Flag(_));
                assert!(reads || row.id == "mcp call", "{}: runs but writes", row.id);
            }
        }
    }
}

#[test]
fn dry_run_in_the_policy_equals_what_the_parser_accepts() {
    for row in ROWS {
        let flags = super::commands_json::row_flags(row.id, Some(row));
        let has = |name: &str| flags.iter().any(|f| f.name() == name);
        match row.preview {
            Preview::None => assert!(
                !has("--dry-run") && !has("--plan"),
                "{}: the parser takes a preview flag but the row says none",
                row.id
            ),
            Preview::Flag(flag) | Preview::UnlessApplied(flag) => assert!(
                has(flag),
                "{}: the preview flag {flag} is not a flag of the command",
                row.id
            ),
        }
    }
}

#[test]
fn every_flag_has_a_type_and_an_effect() {
    for row in ROWS {
        for flag in super::commands_json::row_flags(row.id, Some(row)) {
            let meta = super::commands_json::meta_for(row.id, flag.name());
            assert!(
                meta.is_some_and(|m| !m.effect.is_empty()),
                "{} {}: no effect text",
                row.id,
                flag.name()
            );
        }
    }
}

#[test]
fn every_self_mcp_tool_has_one_policy_row_with_the_same_tier() {
    for tool in TOOLS {
        let rows: Vec<_> = TOOL_ROWS.iter().filter(|r| r.name == tool.name).collect();
        assert_eq!(rows.len(), 1, "{}: one policy row", tool.name);
        assert_eq!(rows[0].tier, Tier::of_tool(tool.tier), "{}", tool.name);
        let previews = tool.params.iter().find(|p| p.name == "dry_run");
        let expected = match previews {
            None => "none",
            Some(p) if p.default == Some(true) => "default-on",
            Some(_) => "param",
        };
        assert_eq!(rows[0].preview.as_str(), expected, "{}", tool.name);
        assert!(
            tool.tier < 3 || tool.gate != Gate::None,
            "{}: tier 3 and up is gated",
            tool.name
        );
        if let Some(command) = rows[0].command {
            assert!(
                row_of(command).is_some() || COMMANDS.iter().any(|c| c.path.join(" ") == command),
                "{}: maps to unknown command {command}",
                tool.name
            );
        }
    }
    for row in TOOL_ROWS {
        assert!(
            TOOLS.iter().any(|t| t.name == row.name),
            "{}: stale",
            row.name
        );
    }
}

#[test]
fn the_registry_counts_what_the_tables_hold() {
    let data = registry();
    assert_eq!(data["counts"]["tools"], TOOLS.len());
    let commands = data["commands"].as_array().unwrap();
    assert_eq!(
        commands.iter().filter(|c| c["kind"] == "command").count(),
        leaf_ids().len()
    );
    assert_eq!(row_of("mcp call").map(|r| r.surface), Some(Surface::Screen));
}
