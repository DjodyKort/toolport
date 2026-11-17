#![allow(dead_code)]

//! Tasks of the task contract: a task with a `needs-you` step that writes one secret, a scheduled
//! task that runs on its own, a disabled draft and a broken file, run records with fixed ids (one
//! waiting for the user, one finished, one whose runner process is gone) and the definition
//! files the `task add|edit` cases read.

use serde_json::json;

use super::world::{write_json, World};

const SIGN_IN_TASK: &str = "---\ndescription: Refresh the portal token\n---\n# Refresh the portal token\n- Sign in to the portal in the browser\nCall mcp__toolport__srv-alpha__get_token and keep the value.\n";

pub fn tasks_home(world: &World) {
    let tasks = world.data.join("plus/tasks");
    let runs = world.data.join("plus/task-runs");
    let defs = world.home.join("defs");
    let every_trigger = json!({"manual": true, "cli": true, "selfMcp": {"enabled": true, "approval": "every-run"}, "schedule": null, "onAuthFailure": ["srv-alpha"]});
    let portal = json!({
        "id": "portal-token", "title": "Refresh the portal token", "description": "Sign in and store a fresh token", "enabled": true,
        "requires": {"servers": ["srv-alpha"], "commands": []},
        "writesSecrets": [{"server": "srv-alpha", "key": "API_KEY"}],
        "steps": [
            {"id": "sign-in", "title": "Sign in", "type": "needs-you", "instructions": "Sign in to the portal in the browser window", "waitFor": {"kind": "manual", "timeoutSec": 900}},
            {"id": "read-token", "title": "Read the token", "type": "mcp", "server": "srv-alpha", "tool": "get_token", "args": {}, "capture": ["token"]},
            {"id": "store-token", "title": "Store the token", "type": "secret-set", "server": "srv-alpha", "key": "API_KEY", "from": "token"},
            {"id": "restart", "title": "Restart the server", "type": "restart-server", "server": "srv-alpha"}
        ],
        "triggers": every_trigger,
        "createdFrom": {"kind": "manual"}
    });
    let nightly = json!({
        "id": "nightly-report", "title": "Nightly report", "description": "", "enabled": true,
        "requires": {"servers": [], "commands": ["echo"]}, "writesSecrets": [],
        "steps": [{"id": "say", "title": "Say hello", "type": "exec", "program": "echo", "args": ["report"]}],
        "triggers": {"manual": true, "cli": false, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": {"cron": "0 3 * * *", "autoRun": true}, "onAuthFailure": []},
        "createdFrom": null
    });
    let draft = json!({
        "id": "draft-cleanup", "title": "Clean up", "description": "", "enabled": false,
        "requires": {"servers": [], "commands": []}, "writesSecrets": [],
        "steps": [{"id": "ask", "title": "Ask Claude", "type": "prompt", "prompt": "Tidy the notes", "allowedTools": ["Read"]}],
        "triggers": {"manual": true, "cli": false, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []},
        "createdFrom": {"kind": "command", "path": "/example/commands/cleanup.md"}
    });
    write_json(&tasks.join("portal-token.json"), &portal);
    write_json(&tasks.join("nightly-report.json"), &nightly);
    write_json(&tasks.join("draft-cleanup.json"), &draft);
    std::fs::write(tasks.join("broken.json"), "{ not json").unwrap();
    let step = |id: &str, title: &str, kind: &str, status: &str, output: &str| json!({"id": id, "title": title, "type": kind, "status": status, "startedAt": null, "endedAt": null, "output": output});
    write_json(
        &runs.join("run-fixture-waiting.json"),
        &json!({
            "id": "run-fixture-waiting", "task": "portal-token", "trigger": "manual", "status": "waiting",
            "startedAt": "2026-10-05T08:00:00Z", "endedAt": null, "durationMs": null,
            "steps": [
                {"id": "sign-in", "title": "Sign in", "type": "needs-you", "status": "waiting", "startedAt": "2026-10-05T08:00:01Z", "endedAt": null, "output": "", "instructions": "Sign in to the portal in the browser window"},
                step("read-token", "Read the token", "mcp", "pending", ""),
                step("store-token", "Store the token", "secret-set", "pending", ""),
                step("restart", "Restart the server", "restart-server", "pending", "")
            ]
        }),
    );
    write_json(
        &runs.join("run-fixture-ok.json"),
        &json!({
            "id": "run-fixture-ok", "task": "nightly-report", "trigger": "schedule", "status": "ok",
            "startedAt": "2026-10-04T03:00:00Z", "endedAt": "2026-10-04T03:00:01Z", "durationMs": 1200,
            "steps": [{"id": "say", "title": "Say hello", "type": "exec", "status": "ok", "startedAt": "2026-10-04T03:00:00Z", "endedAt": "2026-10-04T03:00:01Z", "output": "report"}]
        }),
    );
    write_json(
        &runs.join("run-fixture-stale.json"),
        &json!({
            "id": "run-fixture-stale", "task": "draft-cleanup", "trigger": "manual", "status": "running",
            "startedAt": "2026-10-03T09:00:00Z", "endedAt": null, "durationMs": null, "runnerPid": 2_147_000_000u32,
            "steps": [step("ask", "Ask Claude", "prompt", "running", "")]
        }),
    );
    write_json(
        &defs.join("weekly-cleanup.json"),
        &json!({
            "id": "weekly-cleanup", "title": "Weekly cleanup", "description": "", "enabled": true,
            "requires": {"servers": [], "commands": ["echo"]}, "writesSecrets": [],
            "steps": [{"id": "say", "title": "Say hello", "type": "exec", "program": "echo", "args": ["clean"]}],
            "triggers": {"manual": true, "cli": true, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []},
            "createdFrom": {"kind": "manual"}
        }),
    );
    let mut changed = nightly.clone();
    changed["title"] = json!("Nightly report (changed)");
    write_json(&defs.join("nightly-report.json"), &changed);
    let mut undeclared = portal.clone();
    undeclared["steps"][2]["key"] = json!("OTHER_KEY");
    write_json(&defs.join("undeclared.json"), &undeclared);
    std::fs::write(defs.join("refresh-login.md"), SIGN_IN_TASK).unwrap();
}

/// What the scheduler screen needs on top of `tasks_home`: a 1.x task that names a routine, a
/// catch-up run, the scheduler ledger with one record of each outcome and a heartbeat.
pub fn scheduler_extras(world: &World) {
    let plus = world.data.join("plus");
    let routine = json!({
        "id": "refresh-reports", "title": "Refresh the report cache", "description": "", "enabled": true,
        "requires": {"servers": [], "commands": ["echo"]}, "writesSecrets": [],
        "steps": [
            {"id": "warm", "title": "Warm up", "type": "exec", "program": "echo", "args": ["warm"]},
            {"id": "refresh", "title": "Run the routine", "type": "routine", "routineId": "reports-refresh"}
        ],
        "triggers": {"manual": true, "cli": false, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []},
        "createdFrom": null
    });
    write_json(&plus.join("tasks/refresh-reports.json"), &routine);
    write_json(
        &plus.join("task-runs/run-fixture-catchup.json"),
        &json!({
            "id": "run-fixture-catchup", "task": "nightly-report", "trigger": "catch-up", "status": "ok",
            "startedAt": "2026-10-04T21:03:00Z", "endedAt": "2026-10-04T21:03:01Z", "durationMs": 900,
            "missed": {"slots": 2, "firstDue": "2026-10-04T14:07:00Z", "lastDue": "2026-10-04T18:07:00Z"},
            "steps": [{"id": "say", "title": "Say hello", "type": "exec", "status": "ok", "startedAt": "2026-10-04T21:03:00Z", "endedAt": "2026-10-04T21:03:01Z", "output": "report"}]
        }),
    );
    let record = |task: &str,
                  slots: u32,
                  first: &str,
                  last: &str,
                  outcome: &str,
                  at: &str,
                  run: Option<&str>| {
        let mut r = json!({"task": task, "slots": slots, "firstDue": first, "lastDue": last, "outcome": outcome, "at": at});
        if let Some(run) = run {
            r["run"] = json!(run);
        }
        r
    };
    write_json(
        &plus.join("task-schedule.json"),
        &json!({
            "tasks": {"nightly-report": {"handledSlot": 1_759_600_000}},
            "missed": [
                record("nightly-report", 2, "2026-10-04T14:07:00Z", "2026-10-04T18:07:00Z", "caught-up", "2026-10-04T21:03:00Z", Some("run-fixture-catchup")),
                record("nightly-report", 1, "2026-10-05T02:07:00Z", "2026-10-05T02:07:00Z", "busy", "2026-10-05T02:07:00Z", None),
                record("portal-token", 1, "2026-10-01T08:00:00Z", "2026-10-01T08:00:00Z", "too-old", "2026-10-03T10:12:00Z", None),
            ]
        }),
    );
    write_json(
        &plus.join("task-heartbeat.json"),
        &json!({"at": "2026-10-05T09:58:00Z", "by": "app", "intervalSecs": 20}),
    );
}

pub fn scheduler_home(world: &World) {
    tasks_home(world);
    scheduler_extras(world);
}
