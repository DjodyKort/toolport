use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;

use super::tasks::wait_for;
use crate::tasks::store::{self, Run};

fn base_with_tasks() -> TempDir {
    let dir = TempDir::new().unwrap();
    let tasks = store::tasks_dir(dir.path());
    std::fs::create_dir_all(&tasks).unwrap();
    let common = json!({
        "description": "", "enabled": true,
        "requires": {"servers": [], "commands": []}, "writesSecrets": [],
        "triggers": {"manual": true, "cli": true, "selfMcp": {"enabled": true, "approval": "every-run"}, "schedule": null, "onAuthFailure": []},
        "createdFrom": null,
    });
    let mut say = common.clone();
    say["id"] = json!("say");
    say["title"] = json!("Say");
    say["steps"] =
        json!([{"id": "say", "title": "Say", "type": "exec", "program": "echo", "args": ["x"]}]);
    let mut ask = common;
    ask["id"] = json!("ask");
    ask["title"] = json!("Ask");
    ask["steps"] = json!([{"id": "in", "title": "In", "type": "needs-you", "instructions": "go", "waitFor": {"kind": "manual", "timeoutSec": 60}}]);
    for task in [say, ask] {
        std::fs::write(
            tasks.join(format!("{}.json", task["id"].as_str().unwrap())),
            task.to_string(),
        )
        .unwrap();
    }
    dir
}

fn run(task: &str, status: &str, step_status: &str) -> Run {
    let step_type = if task == "ask" { "needs-you" } else { "exec" };
    let value: Value = json!({
        "id": format!("run-{task}-{status}"), "task": task, "trigger": "manual", "status": status,
        "startedAt": "2026-10-05T08:00:00Z", "endedAt": null, "durationMs": null,
        "steps": [{"id": "s", "title": "S", "type": step_type, "status": step_status,
                   "startedAt": null, "endedAt": null, "output": ""}],
    });
    serde_json::from_value(value).unwrap()
}

#[test]
fn test01_a_finished_run_ends_the_wait_at_once() {
    let dir = base_with_tasks();
    let done = run("say", "ok", "ok");
    store::create_run(dir.path(), &done).unwrap();
    let (run, settled) = wait_for(dir.path(), done, Duration::from_secs(30)).unwrap();
    assert!(settled);
    assert!(run.status.finished());
}

#[test]
fn test02_a_run_that_stops_for_a_person_ends_the_wait() {
    let dir = base_with_tasks();
    let waiting = run("ask", "waiting", "waiting");
    store::create_run(dir.path(), &waiting).unwrap();
    let (run, settled) = wait_for(dir.path(), waiting, Duration::from_secs(30)).unwrap();
    assert!(settled);
    assert_eq!(run.status, store::Status::Waiting);
}

#[test]
fn test03_a_run_that_keeps_running_is_returned_when_the_limit_passes() {
    let dir = base_with_tasks();
    let running = run("say", "running", "running");
    store::create_run(dir.path(), &running).unwrap();
    let (run, settled) = wait_for(dir.path(), running, Duration::ZERO).unwrap();
    assert!(!settled);
    assert_eq!(run.status, store::Status::Running);
}
