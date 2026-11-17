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

/// A call that changes the world and succeeds.
const fn write(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    Call {
        writes: true,
        ..read(label, tool, args)
    }
}

/// A call that must be refused for want of `confirm`, and changes nothing.
const fn refused(label: &'static str, tool: &'static str, args: &'static str) -> Call {
    fails(label, "refused", tool, args)
}

/// A call that prepares the world for the ones after it: no golden.
const fn setup(tool: &'static str, args: &'static str) -> Call {
    Call {
        golden: false,
        ..write(tool, tool, args)
    }
}

const GAMMA: &str = r#"{"name":"gamma","config":{"command":"gamma-mcp","args":["--flag"]}}"#;
const GAMMA_APPLY: &str =
    r#"{"name":"gamma","config":{"command":"gamma-mcp","args":["--flag"]},"confirm":true}"#;

struct Case {
    tool: &'static str,
    calls: &'static [Call],
    prepare: Option<fn(&World)>,
}

const fn case(tool: &'static str, calls: &'static [Call]) -> Case {
    Case {
        tool,
        calls,
        prepare: None,
    }
}

/// A case whose world first gets the skills repository, CLAUDE.md files and project folder.
const fn context_case(tool: &'static str, calls: &'static [Call]) -> Case {
    Case {
        prepare: Some(World::context_files),
        ..case(tool, calls)
    }
}

const BUNDLE: &str = r#"{"bundle_path":"{repo}/skills-repo-bundle.zip"}"#;
const ADD_TAP: &str = r#"{"repo":"{base}/tap-src","name":"local","dry_run":false}"#;
const APPLY: &str = r#"{"dry_run":false,"confirm":true}"#;
const UNCONFIRMED: &str = r#"{"dry_run":false}"#;
const CLAUDE: &str = r#"{"client_keys":["claude-code"]}"#;
const CLAUDE_UNCONFIRMED: &str = r#"{"client_keys":["claude-code"],"dry_run":false}"#;
const CLAUDE_APPLY: &str = r#"{"client_keys":["claude-code"],"dry_run":false,"confirm":true}"#;

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
    case(
        "servers_list",
        &[
            read("default", "servers_list", "{}"),
            read("sources", "servers_list", r#"{"include_sources":true}"#),
        ],
    ),
    case(
        "servers_get",
        &[
            read("found", "servers_get", r#"{"name":"alpha"}"#),
            fails("missing", "not_found", "servers_get", r#"{"name":"nope"}"#),
        ],
    ),
    case(
        "servers_list_profiles",
        &[read("", "servers_list_profiles", "{}")],
    ),
    case(
        "servers_add_profile_tag",
        &[
            write(
                "apply",
                "servers_add_profile_tag",
                r#"{"name":"beta","profile_tag":"Default"}"#,
            ),
            fails(
                "missing",
                "not_found",
                "servers_add_profile_tag",
                r#"{"name":"nope","profile_tag":"Default"}"#,
            ),
        ],
    ),
    case(
        "servers_remove_profile_tag",
        &[
            setup(
                "servers_add_profile_tag",
                r#"{"name":"beta","profile_tag":"Default"}"#,
            ),
            write(
                "apply",
                "servers_remove_profile_tag",
                r#"{"name":"beta","profile_tag":"Default"}"#,
            ),
        ],
    ),
    case(
        "servers_install",
        &[
            refused("refused", "servers_install", GAMMA),
            write("apply", "servers_install", GAMMA_APPLY),
            fails("exists", "conflict", "servers_install", GAMMA_APPLY),
        ],
    ),
    case(
        "servers_update_config",
        &[
            setup("servers_install", GAMMA_APPLY),
            refused(
                "refused",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"args":["--changed"]}}"#,
            ),
            write(
                "apply",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"args":["--changed"]},"confirm":true}"#,
            ),
            write(
                "env",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"env":{"JAVA_HOME":"/opt/jdk","MODE":"fast"}},"confirm":true}"#,
            ),
            fails(
                "env-refused",
                "invalid_arguments",
                "servers_update_config",
                r#"{"name":"gamma","patch":{"env":{"API_KEY":"plain"}},"confirm":true}"#,
            ),
            fails(
                "missing",
                "not_found",
                "servers_update_config",
                r#"{"name":"nope","patch":{"args":[]},"confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_set_mode",
        &[
            refused(
                "refused",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"search"}"#,
            ),
            fails(
                "unknown_mode",
                "invalid_arguments",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"bogus","confirm":true}"#,
            ),
            fails(
                "retired_mode",
                "invalid_arguments",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"direct","confirm":true}"#,
            ),
            fails(
                "missing",
                "not_found",
                "servers_set_mode",
                r#"{"name":"nope","mode":"search","confirm":true}"#,
            ),
            write(
                "apply",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"search","confirm":true}"#,
            ),
            write(
                "back_to_auto",
                "servers_set_mode",
                r#"{"name":"alpha","mode":"auto","confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_uninstall",
        &[
            refused("refused", "servers_uninstall", r#"{"name":"beta"}"#),
            write(
                "apply",
                "servers_uninstall",
                r#"{"name":"beta","confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "servers_uninstall",
                r#"{"name":"beta","confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_auth",
        &[
            refused("refused", "servers_auth", r#"{"name":"alpha"}"#),
            read(
                "apply",
                "servers_auth",
                r#"{"name":"alpha","confirm":true}"#,
            ),
            fails(
                "unknown",
                "not_found",
                "servers_auth",
                r#"{"name":"ghost","confirm":true}"#,
            ),
        ],
    ),
    case(
        "servers_restart",
        &[
            read("preview", "servers_restart", r#"{"name":"alpha"}"#),
            refused(
                "refused",
                "servers_restart",
                r#"{"name":"alpha","dry_run":false}"#,
            ),
        ],
    ),
    context_case("context_bundle_ls", &[read("", "context_bundle_ls", "{}")]),
    context_case(
        "context_bundle_status",
        &[
            read("none", "context_bundle_status", r#"{"cwd":"{home}/proj"}"#),
            fails("no-cwd", "invalid_arguments", "context_bundle_status", "{}"),
            fails(
                "bad-cwd",
                "invalid_arguments",
                "context_bundle_status",
                r#"{"cwd":"{home}/gone"}"#,
            ),
        ],
    ),
    context_case(
        "context_bundle_apply",
        &[
            read(
                "plan",
                "context_bundle_apply",
                r#"{"name":"acme-dev","cwd":"{home}/proj","dry_run":true}"#,
            ),
            write(
                "result",
                "context_bundle_apply",
                r#"{"name":"acme-dev","cwd":"{home}/proj"}"#,
            ),
            fails(
                "missing",
                "not_found",
                "context_bundle_apply",
                r#"{"name":"nope","cwd":"{home}/proj"}"#,
            ),
            fails(
                "no-name",
                "invalid_arguments",
                "context_bundle_apply",
                r#"{"cwd":"{home}/proj"}"#,
            ),
        ],
    ),
    context_case(
        "context_bundle_undo",
        &[
            fails(
                "none",
                "conflict",
                "context_bundle_undo",
                r#"{"cwd":"{home}/proj"}"#,
            ),
            setup(
                "context_bundle_apply",
                r#"{"name":"acme-dev","cwd":"{home}/proj"}"#,
            ),
            read(
                "plan",
                "context_bundle_undo",
                r#"{"cwd":"{home}/proj","dry_run":true}"#,
            ),
            write("result", "context_bundle_undo", r#"{"cwd":"{home}/proj"}"#),
        ],
    ),
    context_case(
        "context_leave_out",
        &[
            read(
                "plan",
                "context_leave_out",
                r#"{"action":"add","path":"{home}/.claude/CLAUDE.md","cwd":"{home}/proj"}"#,
            ),
            write(
                "apply",
                "context_leave_out",
                r#"{"action":"add","path":"{home}/.claude/CLAUDE.md","cwd":"{home}/proj","dry_run":false}"#,
            ),
            write(
                "memory",
                "context_leave_out",
                r#"{"action":"add","memory":true,"cwd":"{home}/proj","dry_run":false}"#,
            ),
            fails(
                "bad-action",
                "invalid_arguments",
                "context_leave_out",
                r#"{"action":"drop","memory":true,"cwd":"{home}/proj"}"#,
            ),
            fails(
                "both-targets",
                "invalid_arguments",
                "context_leave_out",
                r#"{"action":"add","memory":true,"path":"{home}/x","cwd":"{home}/proj"}"#,
            ),
        ],
    ),
    context_case(
        "context_loads",
        &[
            read("folder", "context_loads", r#"{"cwd":"{home}/proj"}"#),
            read(
                "measured",
                "context_loads",
                r#"{"cwd":"{home}/proj","measured":true}"#,
            ),
            fails("no-cwd", "invalid_arguments", "context_loads", "{}"),
        ],
    ),
    context_case(
        "context_compose",
        &[
            read("folder", "context_compose", r#"{"cwd":"{home}/proj"}"#),
            fails(
                "bad-cwd",
                "invalid_arguments",
                "context_compose",
                r#"{"cwd":"{home}/gone"}"#,
            ),
        ],
    ),
    context_case(
        "context_measure",
        &[
            read("plan", "context_measure", r#"{"cwd":"{home}/proj"}"#),
            read(
                "variants",
                "context_measure",
                r#"{"cwd":"{home}/proj","without":["plugin:kit@market"],"bundle":"acme-dev"}"#,
            ),
            fails("no-cwd", "invalid_arguments", "context_measure", "{}"),
        ],
    ),
    context_case(
        "context_status",
        &[
            read("folder", "context_status", r#"{"cwd":"{home}/proj"}"#),
            fails("no-cwd", "invalid_arguments", "context_status", "{}"),
        ],
    ),
    case("skills_list", &[read("", "skills_list", "{}")]),
    case(
        "skills_get",
        &[
            read("found", "skills_get", r#"{"name":"demo"}"#),
            fails("missing", "not_found", "skills_get", r#"{"name":"nope"}"#),
        ],
    ),
    case("skills_lint", &[read("", "skills_lint", "{}")]),
    case(
        "skills_status",
        &[
            read("fresh", "skills_status", "{}"),
            setup("skills_sync", CLAUDE_APPLY),
            read("synced", "skills_status", CLAUDE),
        ],
    ),
    case(
        "skills_list_transpilers",
        &[read("", "skills_list_transpilers", "{}")],
    ),
    case(
        "skills_scaffold",
        &[
            write("skill", "skills_scaffold", r#"{"name":"fresh"}"#),
            write(
                "rule",
                "skills_scaffold",
                r#"{"name":"house-rule","skill_type":"rule"}"#,
            ),
            fails(
                "exists",
                "conflict",
                "skills_scaffold",
                r#"{"name":"fresh"}"#,
            ),
            fails(
                "escaping_name",
                "invalid_arguments",
                "skills_scaffold",
                r#"{"name":"../escape"}"#,
            ),
            fails(
                "unknown_type",
                "invalid_arguments",
                "skills_scaffold",
                r#"{"name":"odd","skill_type":"bogus"}"#,
            ),
        ],
    ),
    case(
        "skills_sync",
        &[
            read("preview", "skills_sync", CLAUDE),
            refused("refused", "skills_sync", CLAUDE_UNCONFIRMED),
            write("apply", "skills_sync", CLAUDE_APPLY),
            read(
                "unknown_client",
                "skills_sync",
                r#"{"client_keys":["no-such-client"],"dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_diff",
        &[
            read("fresh", "skills_diff", "{}"),
            setup("skills_sync", CLAUDE_APPLY),
            read("synced", "skills_diff", "{}"),
        ],
    ),
    case("skills_audit", &[read("", "skills_audit", "{}")]),
    case("skills_tap_list", &[read("empty", "skills_tap_list", "{}")]),
    case(
        "skills_search",
        &[read("empty", "skills_search", r#"{"query":"tapskill"}"#)],
    ),
    case(
        "skills_tap_add",
        &[
            read("preview", "skills_tap_add", r#"{"repo":"acme/skills"}"#),
            fails(
                "escaping_name",
                "invalid_arguments",
                "skills_tap_add",
                r#"{"repo":"acme/skills","name":"../x"}"#,
            ),
        ],
    ),
    case(
        "skills_tap_remove",
        &[
            fails(
                "unknown",
                "not_found",
                "skills_tap_remove",
                r#"{"name":"local"}"#,
            ),
            setup("skills_tap_add", ADD_TAP),
            read("preview", "skills_tap_remove", r#"{"name":"local"}"#),
            write(
                "apply",
                "skills_tap_remove",
                r#"{"name":"local","dry_run":false}"#,
            ),
        ],
    ),
    case(
        "skills_tap_update",
        &[read("none", "skills_tap_update", "{}")],
    ),
    case(
        "skills_install",
        &[
            read("preview", "skills_install", r#"{"spec":"@acme/skills"}"#),
            fails(
                "malformed_spec",
                "invalid_arguments",
                "skills_install",
                r#"{"spec":"no-owner-or-repo"}"#,
            ),
        ],
    ),
    case(
        "skills_bundle",
        &[
            read("preview", "skills_bundle", "{}"),
            write("apply", "skills_bundle", r#"{"dry_run":false}"#),
            fails(
                "exists",
                "conflict",
                "skills_bundle",
                r#"{"dry_run":false}"#,
            ),
        ],
    ),
    case(
        "skills_unbundle",
        &[
            setup("skills_bundle", r#"{"dry_run":false}"#),
            setup(
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false,"confirm":true}"#,
            ),
            read("preview", "skills_unbundle", BUNDLE),
            refused(
                "refused",
                "skills_unbundle",
                r#"{"bundle_path":"{repo}/skills-repo-bundle.zip","dry_run":false}"#,
            ),
            write(
                "apply",
                "skills_unbundle",
                r#"{"bundle_path":"{repo}/skills-repo-bundle.zip","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "missing_bundle",
                "not_found",
                "skills_unbundle",
                r#"{"bundle_path":"{repo}/none.zip","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_clean",
        &[
            setup("skills_sync", CLAUDE_APPLY),
            read("preview", "skills_clean", "{}"),
            refused("refused", "skills_clean", UNCONFIRMED),
            write("apply", "skills_clean", APPLY),
        ],
    ),
    case(
        "skills_uninstall",
        &[
            setup("skills_sync", CLAUDE_APPLY),
            read("preview", "skills_uninstall", r#"{"name":"demo"}"#),
            refused(
                "refused",
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false}"#,
            ),
            fails(
                "escaping_name",
                "invalid_arguments",
                "skills_uninstall",
                r#"{"name":"../agents/helper","dry_run":false,"confirm":true}"#,
            ),
            write(
                "apply",
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false,"confirm":true}"#,
            ),
            fails(
                "gone",
                "not_found",
                "skills_uninstall",
                r#"{"name":"demo","dry_run":false,"confirm":true}"#,
            ),
        ],
    ),
    case(
        "skills_resolve",
        &[
            setup("skills_sync", CLAUDE_APPLY),
            read("preview", "skills_resolve", "{}"),
            refused("refused", "skills_resolve", UNCONFIRMED),
            write("apply", "skills_resolve", APPLY),
        ],
    ),
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

fn arguments(world: &World, case: &Case, call: &Call) -> Value {
    let expanded = call
        .args
        .replace("{home}", &world.path(&world.home))
        .replace("{repo}", &world.repo.to_string_lossy())
        .replace("{base}", &world.base.to_string_lossy());
    serde_json::from_str(&expanded)
        .unwrap_or_else(|e| panic!("{} {}: arguments are not JSON: {e}", case.tool, call.label))
}

/// A local git repository with one commit, so a tap can be added without a network.
fn tap_source(world: &World) {
    let dir = world.base.join("tap-src");
    let skill = dir.join("skills/tapskill/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        skill,
        "---\nname: tapskill\ndescription: A synthetic tap skill\n---\nTap body\n",
    )
    .unwrap();
    for args in [
        &["init", "-q"][..],
        &["symbolic-ref", "HEAD", "refs/heads/main"],
        &["add", "-A"],
        &["commit", "-q", "-m", "fixture"],
    ] {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &world.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    }
}

fn run_case(case: &Case) {
    let world = World::new(case.tool);
    if case.tool == "skills_tap_remove" {
        tap_source(&world);
    }
    if let Some(prepare) = case.prepare {
        prepare(&world);
    }
    if case.tool == "skills_tap_remove" {
        tap_source(&world);
    }
    let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
    client.handshake();
    for call in case.calls {
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
        let stem = stem(case, call);
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
        golden::assert_golden_in(
            &golden::selfmcp_root(),
            &stem,
            &normalize::normalize(&world, &body),
        );
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
            // A setup call (no golden) may prepare the world with another tool.
            assert!(
                call.tool == case.tool || !call.golden,
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
        if let Some(prepare) = case.prepare {
            prepare(&world);
        }
        let mut client = Client::spawn(&world, &[FAKE_SECRET, SECRET_KEY]);
        client.handshake();
        for call in case.calls {
            let reply = client.call(call.tool, arguments(&world, case, call));
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
