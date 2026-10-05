//! The task runner end to end (MIG-AUTO-1): the real `toolportctl` starts a run, which spawns the
//! runner as a child process; the mcp step calls the mock server, the secret-set step writes the
//! synthetic value through the encrypted-file vault, and nothing prints, stores or logs it.

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;

use ctl_world::CtlWorld;

const CANARY: &str = "FAKE-canary-task-value-5e1a";

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

fn world(tag: &str) -> CtlWorld {
    CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"))
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
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(60);
    while !ok() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn write_task(world: &CtlWorld, name: &str, task: &Value) -> String {
    let path = world.home.join(name);
    std::fs::write(&path, task.to_string()).unwrap();
    path.display().to_string()
}

fn alive(pid: u64) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn portal_task() -> Value {
    json!({
        "id": "portal-token", "title": "Refresh the portal token", "description": "", "enabled": true,
        "requires": {"servers": ["srv-alpha"], "commands": []},
        "writesSecrets": [{"server": "srv-alpha", "key": "API_TOKEN"}],
        "steps": [
            {"id": "sign-in", "title": "Sign in", "type": "needs-you", "instructions": "Sign in to the portal, then resume"},
            {"id": "read", "title": "Read the value", "type": "mcp", "server": "srv-alpha", "tool": "echo", "args": {"text": CANARY}, "capture": ["token"]},
            {"id": "store", "title": "Store the value", "type": "secret-set", "server": "srv-alpha", "key": "API_TOKEN", "from": "token"},
            {"id": "restart", "title": "Restart", "type": "restart-server", "server": "srv-alpha"}
        ],
        "triggers": {"manual": true, "cli": true, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []}
    })
}

#[test]
fn a_run_pauses_for_the_user_resumes_and_stores_the_value_without_leaking_it() {
    let w = world("run");
    let file = write_task(&w, "portal.json", &portal_task());
    assert_eq!(ctl(&w, &["task", "add", "portal-token", "--file", &file]).code, 0);
    let generation = w.registry()["secretsGeneration"].as_u64().unwrap_or(0);

    let started = ctl(&w, &["task", "run", "portal-token", "--wait"]);
    assert_eq!(started.code, 0, "{}{}", started.stdout, started.stderr);
    let data = started.data();
    assert_eq!(data["run"]["status"], "waiting", "{data}");
    let run_id = data["run"]["id"].as_str().unwrap().to_string();
    assert!(started.stderr.contains("waiting for you") && started.stderr.contains("Sign in to the portal"), "{}", started.stderr);
    assert!(started.stderr.contains(&format!("task resume {run_id}")), "{}", started.stderr);
    assert_eq!(ctl(&w, &["task", "ls"]).data()["tasks"][0]["waiting"], true);

    let resumed = ctl(&w, &["task", "resume", &run_id]);
    assert_eq!(resumed.code, 0, "{}{}", resumed.stdout, resumed.stderr);
    until("the run to finish", || ctl(&w, &["task", "history", "--run", &run_id]).data()["run"]["status"].as_str().is_some_and(|s| s != "waiting" && s != "running"));
    let history = ctl(&w, &["task", "history", "--run", &run_id]);
    let run = history.data()["run"].clone();
    assert_eq!(run["status"], "ok", "{run}");
    for step in run["steps"].as_array().unwrap() {
        assert_eq!(step["status"], "ok", "{step}");
    }

    assert!(w.registry()["secretsGeneration"].as_u64().unwrap_or(0) > generation, "restart-server must bump secretsGeneration");
    let stored = ctl(&w, &["secret", "get", "srv-alpha", "API_TOKEN"]);
    assert_eq!(stored.code, 0, "{}{}", stored.stdout, stored.stderr);

    let record = std::fs::read_to_string(w.data.join("plus/task-runs").join(format!("{run_id}.json"))).unwrap();
    let show_runs = ctl(&w, &["task", "show", "portal-token"]).data()["runs"].to_string();
    let all_history = ctl(&w, &["task", "history"]);
    let pieces = [
        ("run record", &record),
        ("run stdout", &started.stdout),
        ("run stderr", &started.stderr),
        ("resume stdout", &resumed.stdout),
        ("resume stderr", &resumed.stderr),
        ("history stdout", &history.stdout),
        ("history stderr", &history.stderr),
        ("secret get stdout", &stored.stdout),
        ("secret get stderr", &stored.stderr),
        ("show runs", &show_runs),
        ("history list stdout", &all_history.stdout),
    ];
    for (name, text) in pieces {
        assert!(!text.contains(CANARY) || name == "secret get stdout", "the value reached {name}: {text}");
    }
    assert!(record.contains("[redacted]"), "{record}");
}

#[test]
fn an_undeclared_secret_key_is_refused_before_anything_runs() {
    let w = world("undeclared");
    let mut bad = portal_task();
    bad["steps"][2]["key"] = json!("OTHER_KEY");
    let file = write_task(&w, "bad.json", &bad);
    let out = ctl(&w, &["task", "add", "portal-token", "--file", &file]);
    assert_eq!(out.code, 1);
    assert!(out.stdout.contains("not listed in writesSecrets"), "{}", out.stdout);
    assert!(!w.data.join("plus/tasks/portal-token.json").exists());
}

#[test]
fn cancel_stops_the_running_child_process() {
    let w = world("cancel");
    let task = json!({
        "id": "sleeper", "title": "Sleep", "description": "", "enabled": true,
        "requires": {"servers": [], "commands": ["sleep"]}, "writesSecrets": [],
        "steps": [{"id": "wait", "title": "Wait", "type": "exec", "program": "sleep", "args": ["60"]}],
        "triggers": {"manual": true, "cli": true, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []}
    });
    let file = write_task(&w, "sleeper.json", &task);
    assert_eq!(ctl(&w, &["task", "add", "sleeper", "--file", &file]).code, 0);
    let started = ctl(&w, &["task", "run", "sleeper"]);
    assert_eq!(started.code, 0, "{}{}", started.stdout, started.stderr);
    let run_id = started.data()["run"]["id"].as_str().unwrap().to_string();
    let record = w.data.join("plus/task-runs").join(format!("{run_id}.json"));
    let mut pid = 0;
    until("a child process", || {
        let text = std::fs::read_to_string(&record).unwrap_or_default();
        pid = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["childPid"].as_u64()).unwrap_or(0);
        pid != 0
    });
    assert!(alive(pid));
    let cancelled = ctl(&w, &["task", "cancel", &run_id]);
    assert_eq!(cancelled.code, 0, "{}{}", cancelled.stdout, cancelled.stderr);
    assert_eq!(cancelled.data()["run"]["status"], "cancelled");
    until("the child to be gone", || !alive(pid));
}
