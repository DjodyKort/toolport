//! The shipped `moodle-token` task end to end (MIG-AUTO-3): the real `toolportctl` runs it against
//! a fake Playwright server, a fake moodle server and the encrypted-file vault in a synthetic data
//! dir. The capture script of the shipped task is set on the Mac, so the tests install a stand-in
//! that does what the contract in the shipped data asks. The synthetic value must end up in the
//! vault and nowhere else. `task add --from-command` marker rules live in
//! `tests/fixtures/tasks/moodle/rules.json`.

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};

#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/selfmcp_client.rs"]
mod selfmcp_client;

use ctl_world::CtlWorld;
use selfmcp_client::Client;

const CANARY: &str = "FAKE-canary-moodle-7c2e41";
const PRIVATE_PART: &str = "FAKE-private-part-90b3";

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Out {
    fn data(&self) -> Value {
        let envelope: Value = serde_json::from_str(self.stdout.trim()).unwrap_or_else(|e| panic!("not an envelope ({e}): {}", self.stdout));
        envelope["data"].clone()
    }
}

fn world(tag: &str, playwright: bool) -> CtlWorld {
    let mock = env!("CARGO_BIN_EXE_mock-mcp-server");
    let w = CtlWorld::new(tag, mock);
    let server = |id: &str| json!({"id": id, "name": id, "transport": "stdio", "command": mock, "args": [], "mcpmSource": {"type": "unknown", "reason": "synthetic fixture"}});
    let mut servers = vec![server("moodle")];
    if playwright {
        servers.insert(0, server("playwright"));
    }
    let ids: Vec<Value> = servers.iter().map(|s| s["id"].clone()).collect();
    let registry = json!({"version": 1, "servers": servers, "profiles": [{"id": "default", "name": "Default", "enabledServerIds": ids}], "activeProfileId": "default"});
    std::fs::write(w.data.join("registry.json"), registry.to_string()).unwrap();
    w
}

fn ctl(world: &CtlWorld, args: &[&str]) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
        .arg("--json")
        .args(args)
        .env_clear()
        .envs(world.env())
        .env("TOOLPORT_TASK_POLL_MS", "50")
        .current_dir(&world.home)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Out { code: out.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&out.stdout).into_owned(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(60);
    while !ok() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn finished(w: &CtlWorld, run_id: &str) -> Value {
    until("the run to finish", || ctl(w, &["task", "history", "--run", run_id]).data()["run"]["status"].as_str().is_some_and(|s| s != "waiting" && s != "running"));
    ctl(w, &["task", "history", "--run", run_id]).data()["run"].clone()
}

/// The shipped task as `task show` prints it, enabled, with the capture script replaced when given.
fn edit_shipped(w: &CtlWorld, script: Option<&str>) {
    let mut task = ctl(w, &["task", "show", "moodle-token"]).data()["task"].clone();
    assert_eq!(task["enabled"], false, "the shipped task starts disabled");
    task["enabled"] = json!(true);
    if let Some(script) = script {
        task["steps"][1]["script"] = json!(script);
    }
    let file = w.home.join("moodle-token.edit.json");
    std::fs::write(&file, task.to_string()).unwrap();
    let edited = ctl(w, &["task", "edit", "moodle-token", "--file", &file.display().to_string()]);
    assert_eq!(edited.code, 0, "{}{}", edited.stdout, edited.stderr);
}

fn capture_script() -> String {
    let blob = base64::engine::general_purpose::STANDARD.encode(format!("{CANARY}:::{PRIVATE_PART}"));
    format!(
        r#"var alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function decode(s) {{
  var out = "", bits = 0, acc = 0;
  for (var i = 0; i < s.length; i++) {{
    var v = alphabet.indexOf(s.charAt(i));
    if (v < 0) continue;
    acc = ((acc << 6) | v) & 0xffff;
    bits += 6;
    if (bits >= 8) {{ bits -= 8; out += String.fromCharCode((acc >> bits) & 255); }}
  }}
  return out;
}}
var seen = toolport.call("playwright__echo", {{ text: "moodlemobile://token=" + "{blob}" }});
var launch = seen.content[0].text;
return {{ moodleToken: decode(launch.split("token=")[1]).split(":::")[0] }};"#
    )
}

fn assert_clean(label: &str, text: &str) {
    assert!(!text.contains(CANARY), "the value reached {label}: {text}");
    assert!(!text.contains(PRIVATE_PART), "the private part reached {label}: {text}");
}

#[test]
fn the_task_runs_end_to_end_and_the_value_ends_up_only_in_the_vault() {
    let w = world("e2e", true);
    let shown = ctl(&w, &["task", "ls", "--all"]);
    assert_eq!(shown.data()["tasks"][0]["id"], "moodle-token");
    assert!(!w.data.join("plus/tasks").exists(), "listing a shipped task must not write");
    edit_shipped(&w, Some(&capture_script()));
    let generation = w.registry()["secretsGeneration"].as_u64().unwrap_or(0);

    let started = ctl(&w, &["task", "run", "moodle-token", "--wait"]);
    assert_eq!(started.code, 0, "{}{}", started.stdout, started.stderr);
    let run_id = started.data()["run"]["id"].as_str().unwrap().to_string();
    assert_eq!(started.data()["run"]["status"], "waiting", "{}", started.stdout);
    assert!(started.stderr.contains("Sign in with SSO in the browser window"), "{}", started.stderr);
    assert_eq!(ctl(&w, &["task", "ls"]).data()["tasks"][0]["waiting"], true);
    assert_eq!(ctl(&w, &["secret", "get", "moodle", "MOODLE_TOKEN"]).code, 1, "nothing is stored before the user signs in");

    let resumed = ctl(&w, &["task", "resume", &run_id]);
    assert_eq!(resumed.code, 0, "{}{}", resumed.stdout, resumed.stderr);
    let run = finished(&w, &run_id);
    assert_eq!(run["status"], "ok", "{run}");
    let kinds: Vec<&str> = run["steps"].as_array().unwrap().iter().map(|s| s["type"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["needs-you", "routine", "secret-set", "restart-server"]);
    assert!(run["steps"].as_array().unwrap().iter().all(|s| s["status"] == "ok"), "{run}");
    assert!(w.registry()["secretsGeneration"].as_u64().unwrap_or(0) > generation, "restart-server must bump secretsGeneration");

    let stored = ctl(&w, &["secret", "get", "moodle", "MOODLE_TOKEN", "--reveal"]);
    assert_eq!(stored.code, 0, "{}{}", stored.stdout, stored.stderr);
    assert!(stored.stdout.contains(CANARY), "the vault must hold the decoded first part: {}", stored.stdout);
    assert!(!stored.stdout.contains(PRIVATE_PART), "only the first part is kept");

    let record = std::fs::read_to_string(w.data.join("plus/task-runs").join(format!("{run_id}.json"))).unwrap();
    let show = ctl(&w, &["task", "show", "moodle-token"]);
    let history = ctl(&w, &["task", "history", "--run", &run_id]);
    let all = ctl(&w, &["task", "history", "moodle-token"]);
    let ls = ctl(&w, &["task", "ls", "--all"]);
    let dry = ctl(&w, &["task", "run", "moodle-token", "--dry-run"]);
    for (label, text) in [
        ("run record", &record),
        ("run stdout", &started.stdout),
        ("run stderr", &started.stderr),
        ("resume stdout", &resumed.stdout),
        ("resume stderr", &resumed.stderr),
        ("task show", &show.stdout),
        ("history of the run", &history.stdout),
        ("history of the task", &all.stdout),
        ("task ls", &ls.stdout),
        ("dry run", &dry.stdout),
        ("dry run stderr", &dry.stderr),
    ] {
        assert_clean(label, text);
    }

    let mut selfmcp = Client::spawn(&w, &[CANARY, PRIVATE_PART]);
    selfmcp.handshake();
    let got = selfmcp.call("tasks_get", json!({"id": "moodle-token"}));
    assert!(!got.is_error(), "{}", got.text());
    assert_eq!(got.data()["task"]["id"], "moodle-token");
    let past = selfmcp.call("tasks_history", json!({"id": "moodle-token"}));
    assert!(!past.is_error(), "{}", past.text());
    assert_eq!(past.data()["runs"][0]["status"], "ok");
    let one = selfmcp.call("tasks_history", json!({"id": "moodle-token", "run": run_id}));
    assert_eq!(one.data()["run"]["steps"][1]["status"], "ok", "{}", one.text());
    selfmcp.close();

    for (path, bytes) in w.snapshot() {
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(CANARY) && !text.contains(PRIVATE_PART), "the value is on disk in {}", path.display());
    }
}

#[test]
fn the_shipped_capture_slot_fails_clearly_and_stores_nothing() {
    let w = world("slot", true);
    edit_shipped(&w, None);
    let started = ctl(&w, &["task", "run", "moodle-token", "--wait"]);
    let run_id = started.data()["run"]["id"].as_str().unwrap().to_string();
    assert_eq!(ctl(&w, &["task", "resume", &run_id]).code, 0);
    let run = finished(&w, &run_id);
    assert_eq!(run["status"], "failed", "{run}");
    assert_eq!(run["steps"][1]["status"], "failed");
    assert!(run["steps"][1]["output"].as_str().unwrap().contains("capture script is not installed (set on the Mac, runbook G5)"), "{run}");
    assert_eq!(run["steps"][2]["status"], "skipped");
    assert_eq!(run["steps"][3]["status"], "skipped");
    assert_eq!(ctl(&w, &["secret", "get", "moodle", "MOODLE_TOKEN"]).code, 1);
}

#[test]
fn a_missing_playwright_server_stops_the_run_at_the_first_step_with_the_install_action() {
    let w = world("missing", false);
    edit_shipped(&w, Some(&capture_script()));
    let dry = ctl(&w, &["task", "run", "moodle-token", "--dry-run"]);
    assert!(dry.stdout.contains("server playwright is not installed") || dry.stdout.contains("the server playwright is not installed"), "{}", dry.stdout);

    let started = ctl(&w, &["task", "run", "moodle-token", "--wait"]);
    let run_id = started.data()["run"]["id"].as_str().unwrap().to_string();
    let run = finished(&w, &run_id);
    assert_eq!(run["status"], "failed", "{run}");
    assert_eq!(run["steps"][0]["status"], "failed", "the user is not asked to sign in first: {run}");
    assert!(run["steps"][0]["output"].as_str().unwrap().contains("install the Playwright server"), "{run}");
    assert!(run["error"].as_str().unwrap().contains("install the Playwright server"), "{run}");
    assert_eq!(run["action"]["command"], json!(["toolportctl", "server", "install", "Playwright"]));
    assert!(run["steps"].as_array().unwrap().iter().skip(1).all(|s| s["status"] == "skipped"), "{run}");
    assert_eq!(ctl(&w, &["task", "ls"]).data()["tasks"][0]["waiting"], false);
    assert_eq!(ctl(&w, &["secret", "get", "moodle", "MOODLE_TOKEN"]).code, 1);
}

#[test]
fn a_task_shipped_with_the_binary_stays_removed_after_task_rm() {
    let w = world("rm", true);
    assert_eq!(ctl(&w, &["task", "show", "moodle-token"]).code, 0);
    assert_eq!(ctl(&w, &["task", "rm", "moodle-token"]).code, 1, "nothing is copied yet, so there is nothing to remove");
    edit_shipped(&w, None);
    assert_eq!(ctl(&w, &["task", "rm", "moodle-token"]).code, 0);
    assert!(ctl(&w, &["task", "ls", "--all"]).data()["tasks"].as_array().unwrap().is_empty());
    assert_eq!(ctl(&w, &["task", "show", "moodle-token"]).code, 1);
}

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tasks/moodle")
}

#[test]
fn from_command_recognises_the_markers_and_refuses_what_it_cannot_turn_into_a_task() {
    let w = world("import", true);
    let rules: Value = serde_json::from_str(&std::fs::read_to_string(fixtures().join("rules.json")).unwrap()).unwrap();
    for case in rules["recognised"].as_array().unwrap() {
        let file = fixtures().join(case["file"].as_str().unwrap());
        let id = case["id"].as_str().unwrap();
        let out = ctl(&w, &["task", "add", id, "--from-command", &file.display().to_string(), "--dry-run"]);
        assert_eq!(out.code, 0, "{id}: {}{}", out.stdout, out.stderr);
        let after = out.data()["plan"]["steps"][0]["diff"]["after"].as_str().unwrap().to_string();
        let task: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(task["title"], case["title"], "{id}");
        assert_eq!(task["description"], case["description"], "{id}");
        let types: Vec<&str> = task["steps"].as_array().unwrap().iter().map(|s| s["type"].as_str().unwrap()).collect();
        assert_eq!(json!(types), case["stepTypes"], "{id}");
        let needs: Vec<Value> = task["steps"].as_array().unwrap().iter().filter(|s| s["type"] == "needs-you").map(|s| json!({"id": s["id"], "instructions": s["instructions"]})).collect();
        assert_eq!(json!(needs), case["needsYou"], "{id}");
        assert_eq!(task["requires"]["servers"], case["requiresServers"], "{id}");
        let prompt = task["steps"].as_array().unwrap().last().unwrap();
        assert_eq!(prompt["allowedTools"], case["allowedTools"], "{id}");
        assert_eq!(task["enabled"], false, "{id}: an import is never enabled");
        assert_eq!(task["writesSecrets"], json!([]), "{id}: an import never grants a secret write");
        assert_eq!(task["requires"]["commands"], json!([]), "{id}");
        assert_eq!(task["triggers"]["schedule"], Value::Null, "{id}");
        assert_eq!(task["triggers"]["selfMcp"]["enabled"], false, "{id}");
        assert_eq!(task["triggers"]["onAuthFailure"], json!([]), "{id}");
        assert_eq!(task["createdFrom"]["kind"], "command", "{id}");
        assert!(!w.data.join("plus/tasks").join(format!("{id}.json")).exists(), "{id}: a dry run writes nothing");
    }
    for case in rules["refused"].as_array().unwrap() {
        let file = fixtures().join(case["file"].as_str().unwrap());
        let id = case["id"].as_str().unwrap();
        let out = ctl(&w, &["task", "add", id, "--from-command", &file.display().to_string(), "--dry-run"]);
        assert_ne!(out.code, 0, "{id} must be refused: {}", out.stdout);
        let said = format!("{}{}", out.stdout, out.stderr);
        assert!(said.contains(case["error"].as_str().unwrap()), "{id}: {said}");
    }

    let file = fixtures().join("moodle-token.command.md").display().to_string();
    let added = ctl(&w, &["task", "add", "imported-login", "--from-command", &file]);
    assert_eq!(added.code, 0, "{}{}", added.stdout, added.stderr);
    let refused = ctl(&w, &["task", "run", "imported-login"]);
    assert_eq!(refused.code, 1, "a draft does not run until the user enables it: {}", refused.stdout);
    assert!(format!("{}{}", refused.stdout, refused.stderr).contains("disabled"));
}
