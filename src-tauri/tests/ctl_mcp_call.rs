//! `toolportctl mcp call` (GUI-wave contract section 15): the real `toolportctl` binary runs a
//! self-MCP tool through the same dispatch as the real `toolport-selfmcp` stdio server, in the
//! synthetic world of `common/ctl_world.rs`. The per-tool golden envelopes live in
//! `ctl_contract` (case "mcp call"); this file checks the properties the GUI relies on across
//! them: `--args` and `--args-stdin` agree, the result is the MCP result, errors keep the tool's
//! kind, a secret never reaches a command line or an output.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};

#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/selfmcp_client.rs"]
mod selfmcp_client;

use ctl_world::{CtlWorld, FAKE_SECRET};
use selfmcp_client::Client;

const CANARY: &str = "FAKE-canary-in-the-vault-5b07";
const STDIN_SECRET: &str = "sk-FAKE-stdin-token-3c1d";
const STDIN_ENV_SECRET: &str = "FAKE-stdin-env-value-8e22";
const RUN_TIMEOUT: Duration = Duration::from_secs(120);

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn envelope(&self) -> Value {
        let lines: Vec<&str> = self.stdout.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "stdout is exactly one envelope line, got {:?} (stderr {:?})",
            self.stdout,
            self.stderr
        );
        serde_json::from_str(lines[0]).unwrap_or_else(|e| panic!("stdout is not JSON: {e}"))
    }

    fn assert_clean_of(&self, secrets: &[&str], what: &str) {
        for secret in secrets {
            assert!(
                !self.stdout.contains(secret) && !self.stderr.contains(secret),
                "{what}: leaked a secret value"
            );
        }
    }
}

fn world(tag: &str) -> CtlWorld {
    CtlWorld::new(
        &format!("mcp-call-{tag}"),
        env!("CARGO_BIN_EXE_mock-mcp-server"),
    )
}

fn run_ctl(world: &CtlWorld, argv: &[&str], stdin: Option<&str>) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .arg("--json")
        .args(argv)
        .env_clear()
        .current_dir(&world.home)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in world.env() {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn toolportctl");
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    let output = match receiver.recv_timeout(RUN_TIMEOUT) {
        Ok(output) => output.expect("wait for toolportctl"),
        Err(_) => {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            panic!("toolportctl {argv:?} did not finish in {RUN_TIMEOUT:?}");
        }
    };
    Run {
        code: output.status.code().expect("exit code, not a signal"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn call_inline(world: &CtlWorld, tool: &str, args: &Value) -> Run {
    run_ctl(
        world,
        &["mcp", "call", tool, "--args", &args.to_string()],
        None,
    )
}

fn call_stdin(world: &CtlWorld, tool: &str, args: &Value) -> Run {
    run_ctl(
        world,
        &["mcp", "call", tool, "--args-stdin"],
        Some(&args.to_string()),
    )
}

fn repo(world: &CtlWorld) -> String {
    world.path(&world.repo)
}

fn selfmcp(world: &CtlWorld) -> Client {
    let mut client = Client::spawn(world, &[FAKE_SECRET, CANARY]);
    client.handshake();
    client
}

#[test]
fn args_and_args_stdin_give_byte_identical_envelopes() {
    let world = world("identical");
    let repo = repo(&world);
    let calls = [
        ("skills_get", json!({"name": "demo", "repo_path": repo})),
        ("servers_get", json!({"name": "alpha"})),
        ("where_am_i", json!({})),
        ("skills_clean", json!({"repo_path": repo})),
        (
            "skills_edit_body",
            json!({"name": "demo", "new_body": "Changed\n", "repo_path": repo}),
        ),
        ("skills_get", json!({"name": "nope", "repo_path": repo})),
        ("skills_get", json!({})),
    ];
    let before = world.snapshot();
    let mut exits = BTreeSet::new();
    for (tool, args) in &calls {
        let inline = call_inline(&world, tool, args);
        let piped = call_stdin(&world, tool, args);
        assert_eq!(inline.code, piped.code, "{tool} {args}: exit code");
        assert_eq!(inline.stdout, piped.stdout, "{tool} {args}: envelope bytes");
        assert_eq!(inline.stderr, piped.stderr, "{tool} {args}: stderr");
        inline.envelope();
        exits.insert(inline.code);
    }
    assert_eq!(
        exits,
        BTreeSet::from([0, 1]),
        "the calls cover successes and tool errors"
    );
    assert_eq!(
        world.snapshot(),
        before,
        "reads, previews and refusals changed the world"
    );
}

#[test]
fn blank_stdin_is_the_empty_object() {
    let world = world("blank");
    let blank = run_ctl(
        &world,
        &["mcp", "call", "where_am_i", "--args-stdin"],
        Some("  \n"),
    );
    let empty = call_inline(&world, "where_am_i", &json!({}));
    assert_eq!(blank.code, 0, "{}", blank.stdout);
    assert_eq!(blank.stdout, empty.stdout);
}

#[test]
fn the_result_is_the_tools_call_structured_content_of_the_self_mcp_server() {
    let world = world("equal");
    let repo = repo(&world);
    let mut client = selfmcp(&world);
    let tiers: Value = run_ctl(&world, &["mcp", "tools"], None).envelope()["data"]["tools"].clone();
    let reads = [
        ("skills_get", json!({"name": "demo", "repo_path": repo})),
        ("agents_get", json!({"name": "helper", "repo_path": repo})),
        ("styles_get", json!({"name": "plain", "repo_path": repo})),
        ("skills_list", json!({"repo_path": repo})),
        ("servers_list", json!({})),
        ("servers_get", json!({"name": "alpha"})),
        ("where_am_i", json!({})),
        ("flow_diagram", json!({})),
        ("skills_list_transpilers", json!({})),
        ("skills_clean", json!({"repo_path": repo})),
    ];
    for (tool, args) in &reads {
        let reply = client.call(tool, args.clone());
        assert!(!reply.is_error(), "{tool}: {}", reply.0);
        let run = call_inline(&world, tool, args);
        assert_eq!(run.code, 0, "{tool}: {}", run.stdout);
        let envelope = run.envelope();
        assert_eq!(envelope["ok"], true, "{tool}");
        let data = &envelope["data"];
        assert_eq!(&data["result"], reply.data(), "{tool}: result");
        assert_eq!(data["tool"], *tool);
        assert_eq!(data["isError"], false);
        let listed = tiers
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == *tool)
            .unwrap_or_else(|| panic!("{tool} is not in `mcp tools`"));
        assert_eq!(data["tier"], listed["tier"], "{tool}: tier");
    }
    client.close();
}

#[test]
fn a_tool_error_exits_1_with_the_tool_kind_and_message() {
    let world = world("errors");
    let repo = repo(&world);
    let mut client = selfmcp(&world);
    let failing = [
        (
            "not_found",
            "skills_get",
            json!({"name": "nope", "repo_path": repo}),
        ),
        ("invalid_arguments", "skills_get", json!({})),
        (
            "refused",
            "skills_edit_body",
            json!({"name": "demo", "new_body": "Changed\n", "repo_path": repo}),
        ),
        ("not_found", "servers_get", json!({"name": "ghost"})),
    ];
    let before = world.snapshot();
    for (kind, tool, args) in &failing {
        let reply = client.call(tool, args.clone());
        assert!(reply.is_error(), "{tool}: {}", reply.0);
        assert_eq!(reply.error_kind(), Some(*kind), "{tool}: {}", reply.0);

        let run = call_inline(&world, tool, args);
        assert_eq!(run.code, 1, "{tool}: {} {}", run.stdout, run.stderr);
        let envelope = run.envelope();
        assert_eq!(envelope["ok"], false, "{tool}");
        assert_eq!(envelope["command"], "mcp", "{tool}");
        assert_eq!(envelope["error"]["code"], *kind, "{tool}");
        assert_eq!(
            envelope["error"]["message"],
            reply.data()["error"]["message"],
            "{tool}: the tool's own message"
        );
        assert!(envelope.get("data").is_none(), "{tool}: no data on error");
    }
    assert_eq!(world.snapshot(), before, "a failed call changed the world");
    client.close();
}

#[test]
fn an_unknown_tool_exits_2_and_names_the_closest_tools() {
    let world = world("unknown");
    let typo = run_ctl(&world, &["mcp", "call", "skils_get"], None);
    assert_eq!(typo.code, 2, "{}", typo.stdout);
    let envelope = typo.envelope();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "usage");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(message.contains("unknown tool: skils_get"), "{message}");
    let closest = message.split_once("closest: ").expect(message).1;
    assert!(
        closest.split(", ").any(|name| name == "skills_get"),
        "{message}"
    );

    let far = run_ctl(&world, &["mcp", "call", "zzzzzzzzzzzzzzzz"], None);
    assert_eq!(far.code, 2);
    let message = far.envelope()["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("mcp tools"), "{message}");

    let none = run_ctl(&world, &["mcp", "call"], None);
    assert_eq!(none.code, 2, "{}", none.stdout);
    assert_eq!(none.envelope()["error"]["code"], "usage");
}

#[test]
fn malformed_arguments_are_usage_errors_that_run_nothing() {
    let world = world("usage");
    let before = world.snapshot();
    let inline = |args: &str| run_ctl(&world, &["mcp", "call", "where_am_i", "--args", args], None);
    for bad in ["{not json", "[1, 2]", "\"text\""] {
        let run = inline(bad);
        assert_eq!(run.code, 2, "{bad}: {}", run.stdout);
        assert_eq!(run.envelope()["error"]["code"], "usage", "{bad}");
    }
    let both = run_ctl(
        &world,
        &["mcp", "call", "where_am_i", "--args", "{}", "--args-stdin"],
        Some("{}"),
    );
    assert_eq!(both.code, 2, "{}", both.stdout);
    assert_eq!(both.envelope()["error"]["code"], "usage");
    let piped = run_ctl(
        &world,
        &["mcp", "call", "where_am_i", "--args-stdin"],
        Some("[1]"),
    );
    assert_eq!(piped.code, 2, "{}", piped.stdout);
    assert_eq!(world.snapshot(), before);
}

#[test]
fn args_stdin_accepts_a_secret_looking_value_that_inline_args_refuses() {
    let world = world("stdin-secret");
    let repo = repo(&world);
    let body = format!("{STDIN_SECRET}\n");
    let args = json!({"name": "demo", "new_body": body, "repo_path": repo, "confirm": true});
    let skill = world.repo.join("skills/demo/SKILL.md");
    let before = world.snapshot();

    let inline = call_inline(&world, "skills_edit_body", &args);
    assert_eq!(inline.code, 2, "{}", inline.stdout);
    let envelope = inline.envelope();
    assert_eq!(envelope["error"]["code"], "usage");
    let message = envelope["error"]["message"].as_str().unwrap();
    assert!(message.contains("new_body"), "{message}");
    assert!(message.contains("--args-stdin"), "{message}");
    inline.assert_clean_of(&[STDIN_SECRET], "the refusal");
    assert_eq!(world.snapshot(), before, "a refused call changed the world");

    let piped = call_stdin(&world, "skills_edit_body", &args);
    assert_eq!(piped.code, 0, "{} {}", piped.stdout, piped.stderr);
    assert_eq!(piped.envelope()["ok"], true);
    let written = std::fs::read_to_string(&skill).unwrap();
    assert!(written.contains(STDIN_SECRET), "{written}");
}

#[test]
fn a_secret_passed_on_stdin_is_not_echoed_when_the_tool_rejects_it() {
    let world = world("stdin-env");
    let args = json!({
        "name": "gamma",
        "config": {"command": "gamma-mcp", "env": {"API_KEY": STDIN_ENV_SECRET}},
        "confirm": true
    });
    let before = world.snapshot();

    let inline = call_inline(&world, "servers_install", &args);
    assert_eq!(inline.code, 2, "{}", inline.stdout);
    let message = inline.envelope()["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("config.env.API_KEY"), "{message}");
    assert!(message.contains("--args-stdin"), "{message}");
    inline.assert_clean_of(&[STDIN_ENV_SECRET], "the refusal");

    let piped = call_stdin(&world, "servers_install", &args);
    assert_eq!(piped.code, 1, "{} {}", piped.stdout, piped.stderr);
    assert_eq!(piped.envelope()["error"]["code"], "invalid_arguments");
    piped.assert_clean_of(&[STDIN_ENV_SECRET], "the tool's rejection");
    assert_eq!(
        world.snapshot(),
        before,
        "a rejected install changed the world"
    );
}

#[test]
fn the_vault_canary_never_appears_in_the_output_of_mcp_call() {
    let world = world("canary");
    let repo = repo(&world);
    let seeded = run_ctl(
        &world,
        &["secret", "set", "alpha", "CANARY_KEY"],
        Some(CANARY),
    );
    assert_eq!(seeded.code, 0, "{}", seeded.stdout);

    let listing = call_inline(&world, "servers_get", &json!({"name": "alpha"}));
    assert_eq!(listing.code, 0, "{}", listing.stdout);
    assert!(
        listing.stdout.contains("CANARY_KEY"),
        "the canary key is part of the server the calls read: {}",
        listing.stdout
    );

    let calls = [
        ("servers_get", json!({"name": "alpha"})),
        ("servers_list", json!({})),
        ("servers_check_updates", json!({"name": "alpha"})),
        ("where_am_i", json!({})),
        ("skills_get", json!({"name": "demo", "repo_path": repo})),
        ("skills_get", json!({"name": "nope", "repo_path": repo})),
        ("servers_get", json!({"name": "ghost"})),
        ("flow_diagram", json!({})),
        (
            "servers_install",
            json!({"name": "alpha", "config": {"command": "x"}}),
        ),
        (
            "servers_update_config",
            json!({"name": "alpha", "patch": {"args": ["x"]}}),
        ),
    ];
    for (tool, args) in &calls {
        for run in [
            call_inline(&world, tool, args),
            call_stdin(&world, tool, args),
        ] {
            run.envelope();
            run.assert_clean_of(&[CANARY, FAKE_SECRET], tool);
        }
    }
    let tools = run_ctl(&world, &["mcp", "tools"], None);
    tools.assert_clean_of(&[CANARY, FAKE_SECRET], "mcp tools");
    for (path, bytes) in world.snapshot() {
        let plain = String::from_utf8_lossy(&bytes);
        assert!(
            !plain.contains(CANARY),
            "{} holds the canary in plain text",
            path.display()
        );
    }
}

#[test]
fn mcp_tools_describes_every_parameter_of_every_tool_the_server_lists() {
    let world = world("tools");
    let mut client = selfmcp(&world);
    let listed = client.list_tools();
    client.close();
    let run = run_ctl(&world, &["mcp", "tools"], None);
    assert_eq!(run.code, 0, "{}", run.stdout);
    let tools = run.envelope()["data"]["tools"].as_array().unwrap().clone();
    assert_eq!(tools.len(), listed.len());
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        assert!(tool["tier"].is_u64(), "{name}: tier");
        assert!(tool["dryRunDefault"].is_boolean(), "{name}: dryRunDefault");
        let params = tool["params"]
            .as_array()
            .unwrap_or_else(|| panic!("{name}: params"));
        let schema = &listed
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not in tools/list"))["inputSchema"];
        let described: BTreeSet<&str> =
            params.iter().map(|p| p["name"].as_str().unwrap()).collect();
        let schema_names: BTreeSet<&str> = schema["properties"]
            .as_object()
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(described, schema_names, "{name}: parameter names");
        let required: BTreeSet<&str> = params
            .iter()
            .filter(|p| p["required"] == true)
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        let schema_required: BTreeSet<&str> = schema["required"]
            .as_array()
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        assert_eq!(required, schema_required, "{name}: required parameters");
        for param in params {
            assert!(param["type"].is_string(), "{name}: {param}");
            assert!(param["description"].is_string(), "{name}: {param}");
            assert!(param["required"].is_boolean(), "{name}: {param}");
        }
    }
}
