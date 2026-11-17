//! The operations behind `toolportctl task ...` and the `tasks_*` tools: one place reads the task
//! and run files and shapes the JSON, so the command line and the tools cannot drift. Run views
//! come from the (already redacted) run records and carry no internal flags.

use super::cron::{rfc3339, Cron};
use super::model::{self, Step, Task, WaitKind};
use super::runner;
use super::store::{self, Run, Status};
use super::triggers;
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use toolport_plus_api::TaskHost;

pub const HISTORY_LIMIT: usize = 20;
pub const SHOW_RUNS: usize = 5;

pub enum Source<'a> {
    File(&'a str),
    Stdin(&'a str),
    Command(&'a str),
}

pub fn run_view(run: &Run, with_output: bool) -> Value {
    let steps: Vec<Value> = run
        .steps
        .iter()
        .map(|s| {
            let mut v = json!({"id": s.id, "title": s.title, "type": s.kind, "status": s.status, "startedAt": s.started_at, "endedAt": s.ended_at});
            if let Some(i) = &s.instructions {
                v["instructions"] = json!(i);
            }
            if with_output {
                v["output"] = json!(s.output);
            }
            v
        })
        .collect();
    let mut view = json!({"id": run.id, "task": run.task, "trigger": run.trigger, "status": run.status, "startedAt": run.started_at, "endedAt": run.ended_at, "durationMs": run.duration_ms, "error": run.error, "steps": steps});
    if let Some(action) = &run.action {
        view["action"] = action.clone();
    }
    view
}

fn last_run(runs: &[Run], task: &str) -> Value {
    runs.iter().find(|r| r.task == task).map_or(Value::Null, |r| json!({"runId": r.id, "status": r.status, "startedAt": r.started_at, "durationMs": r.duration_ms}))
}

fn next_run(task: &Task, now: i64) -> Value {
    let next = task
        .triggers
        .schedule
        .as_ref()
        .filter(|_| task.enabled)
        .and_then(|s| Cron::parse(&s.cron).ok())
        .and_then(|c| c.next_after(now));
    next.map_or(Value::Null, |t| json!(rfc3339(t)))
}

pub fn ls(host: &dyn TaskHost, all: bool) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    let runs = store::list_runs(&base, None);
    let mut tasks = Vec::new();
    let mut invalid = Vec::new();
    for item in store::list_tasks(&base) {
        match item {
            Ok(t) if t.enabled || all => {
                let waiting = runs
                    .iter()
                    .any(|r| r.task == t.id && r.status == Status::Waiting);
                let kind = if t.triggers.on_auth_failure.is_empty() && t.writes_secrets.is_empty() {
                    "script"
                } else {
                    "login"
                };
                tasks.push(json!({"id": t.id, "title": t.title, "enabled": t.enabled, "triggers": t.triggers, "lastRun": last_run(&runs, &t.id), "nextRun": next_run(&t, host.now()), "waiting": waiting, "refreshes": t.triggers.on_auth_failure, "kind": kind}));
            }
            Ok(_) => {}
            Err((id, error)) => invalid.push(json!({"id": id, "error": error})),
        }
    }
    Ok(json!({"tasks": tasks, "invalid": invalid}))
}

pub fn show(host: &dyn TaskHost, id: &str) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    let task = store::load_task(&base, id)?;
    let runs: Vec<Value> = store::list_runs(&base, Some(id))
        .iter()
        .take(SHOW_RUNS)
        .map(|r| run_view(r, false))
        .collect();
    Ok(json!({"task": task, "runs": runs}))
}

pub fn history(
    host: &dyn TaskHost,
    task: Option<&str>,
    run: Option<&str>,
    limit: Option<usize>,
) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    if let Some(run_id) = run {
        let r = store::load_run(&base, run_id)?;
        if task.is_some_and(|t| t != r.task) {
            return Err(OpError::not_found(format!(
                "run {run_id} belongs to task {:?}, not {:?}",
                r.task,
                task.unwrap_or("")
            )));
        }
        return Ok(json!({"run": run_view(&r, true)}));
    }
    if let Some(id) = task {
        store::load_task(&base, id)?;
    }
    let runs: Vec<Value> = store::list_runs(&base, task)
        .iter()
        .take(limit.unwrap_or(HISTORY_LIMIT))
        .map(|r| run_view(r, false))
        .collect();
    Ok(json!({"runs": runs}))
}

fn describe(step: &Step) -> (&'static str, String) {
    match step {
        Step::NeedsYou {
            title, wait_for, ..
        } => {
            let wait = match wait_for {
                None => "until you resume".to_string(),
                Some(w) => match w.kind {
                    WaitKind::SecretUnset => {
                        format!("until the secrets are set (up to {}s)", w.timeout_sec)
                    }
                    WaitKind::Url => format!("until you resume (up to {}s)", w.timeout_sec),
                    WaitKind::Manual => format!("until you resume (up to {}s)", w.timeout_sec),
                },
            };
            ("note", format!("pause for you: {title} ({wait})"))
        }
        Step::Mcp { server, tool, .. } => ("note", format!("call {server}/{tool}")),
        Step::Script { .. } => ("note", "run an inline script".to_string()),
        Step::Exec { program, args, .. } => (
            "exec",
            format!(
                "run {program}{}",
                if args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", args.join(" "))
                }
            ),
        ),
        Step::Prompt {
            title,
            allowed_tools,
            ..
        } => (
            "note",
            format!(
                "ask Claude: {title} (tools: {})",
                if allowed_tools.is_empty() {
                    "none".to_string()
                } else {
                    allowed_tools.join(", ")
                }
            ),
        ),
        Step::SecretSet { server, key, .. } => (
            "update",
            format!("write the secret {server}/{key} from a captured value (never shown)"),
        ),
        Step::RestartServer { server, .. } => ("exec", format!("restart {server}")),
    }
}

fn run_plan(host: &dyn TaskHost, base: &Path, task: &Task) -> (Value, Value, Value) {
    let mut steps: Vec<Value> = task
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (op, detail) = describe(s);
            json!({"op": op, "detail": format!("{}. {} ({}): {detail}", i + 1, s.title(), s.kind())})
        })
        .collect();
    for s in &task.writes_secrets {
        steps.push(json!({"op": "note", "detail": format!("may write the secret {}/{} (writesSecrets)", s.server, s.key)}));
    }
    for s in &task.requires.servers {
        steps.push(json!({"op": "note", "detail": format!("needs the server {s}")}));
    }
    let mut warnings = Vec::new();
    if let Err(why) = triggers::allowed(task, "manual") {
        warnings.push(why);
    }
    if let Some(busy) = store::live_run(base, &task.id) {
        warnings.push(format!(
            "task {:?} is already running as {}",
            task.id, busy.id
        ));
    }
    for s in &task.requires.servers {
        if !host.task_server_installed(s) {
            warnings.push(format!(
                "the server {s} is not installed: run toolportctl server install {s}"
            ));
        }
    }
    let plan = json!({"summary": format!("Run task {} ({} step{})", task.id, task.steps.len(), if task.steps.len() == 1 { "" } else { "s" }), "steps": steps, "effects": {}, "warnings": warnings, "undo": "toolportctl task cancel <run-id>"});
    let secrets = json!(task.writes_secrets);
    let needs = json!({"servers": task.requires.servers, "commands": task.requires.commands});
    (plan, secrets, needs)
}

pub fn run(id: &str, trigger: &str, dry_run: bool, host: &dyn TaskHost) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    let task = store::load_task(&base, id)?;
    let (plan, secrets, needs) = run_plan(host, &base, &task);
    if dry_run {
        return Ok(
            json!({"dryRun": true, "task": id, "plan": plan, "writesSecrets": secrets, "requires": needs, "run": null, "result": null}),
        );
    }
    let run = triggers::start(&task, trigger, None, host)?;
    let path = store::run_path(&base, &run.id)?.display().to_string();
    let result = json!({"applied": true, "changed": [path], "undo": format!("toolportctl task cancel {}", run.id), "backups": []});
    Ok(
        json!({"dryRun": false, "task": id, "plan": plan, "writesSecrets": secrets, "requires": needs, "run": run_view(&run, false), "result": result}),
    )
}

pub fn resume(host: &dyn TaskHost, run_id: &str) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    let run = store::load_run(&base, run_id)?;
    if run.status != Status::Waiting {
        return Err(OpError::conflict(format!(
            "run {run_id} is {}, not waiting for you",
            serde_json::to_value(run.status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        )));
    }
    if run.runner_pid.is_some_and(|p| !host.task_process_alive(p)) {
        return Err(OpError::failed(
            "runner_gone",
            format!("the process that runs {run_id} is gone: cancel it and start the task again"),
        ));
    }
    let (run, ()) = store::update_run(&base, run_id, |r| r.resume_requested = true)?;
    let path = store::run_path(&base, run_id)?.display().to_string();
    Ok(
        json!({"run": run_view(&run, false), "result": {"applied": true, "changed": [path], "undo": format!("toolportctl task cancel {run_id}"), "backups": []}}),
    )
}

pub fn cancel(host: &dyn TaskHost, run_id: &str) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    let run = store::load_run(&base, run_id)?;
    if run.status.finished() {
        return Err(OpError::conflict(format!("run {run_id} already ended")));
    }
    let (run, ()) = store::update_run(&base, run_id, |r| r.cancel_requested = true)?;
    if let Some(pid) = run.child_pid {
        host.task_kill_group(pid);
    }
    let mut run = run;
    if run.runner_pid.is_some_and(|p| !host.task_process_alive(p)) {
        run = runner::abandon(host, &base, run_id)?;
    } else {
        let end = Instant::now() + Duration::from_secs(3);
        while !run.status.finished() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
            run = store::load_run(&base, run_id)?;
        }
    }
    let path = store::run_path(&base, run_id)?.display().to_string();
    Ok(
        json!({"run": run_view(&run, false), "result": {"applied": true, "changed": [path], "undo": format!("toolportctl task run {}", run.task), "backups": []}}),
    )
}

fn abs(path: &str) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| PathBuf::from(path))
        .display()
        .to_string()
}

fn read(path: &str) -> Result<String, OpError> {
    std::fs::read_to_string(path)
        .map_err(|e| OpError::failed("read", format!("cannot read {}: {e}", abs(path))))
}

fn invalid(e: String) -> OpError {
    OpError::failed("invalid_task", e)
}

fn secret_warnings(before: Option<&Task>, after: &Task) -> Vec<String> {
    let mut out = Vec::new();
    for s in &after.writes_secrets {
        if !before.is_some_and(|b| b.declares(&s.server, &s.key)) {
            out.push(format!(
                "the task may write the secret {}/{} when it runs",
                s.server, s.key
            ));
        }
    }
    if after.triggers.schedule.as_ref().is_some_and(|s| s.auto_run)
        && !before.is_some_and(|b| b.triggers.schedule.as_ref().is_some_and(|s| s.auto_run))
    {
        out.push("the schedule runs the task on its own (autoRun)".into());
    }
    if after.triggers.self_mcp.enabled && !before.is_some_and(|b| b.triggers.self_mcp.enabled) {
        out.push("an AI client may ask to run it (you approve every run)".into());
    }
    if !after.enabled {
        out.push("the task is disabled: enable it (enabled: true) before it runs".into());
    }
    out
}

fn save(host: &dyn TaskHost, task: Task, create: bool, dry_run: bool) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    model::validate(&task).map_err(invalid)?;
    let id = task.id.clone();
    let path = store::task_path(&base, &id)?;
    let on_disk = std::fs::read_to_string(&path).ok();
    let before_text = if create {
        on_disk
    } else {
        on_disk.or_else(|| super::builtin::unclaimed_text(&base, &id).map(String::from))
    };
    let before = before_text.as_ref().and_then(|t| model::parse(t).ok());
    match (create, before_text.is_some()) {
        (true, true) => {
            return Err(OpError::conflict(format!(
                "task {id:?} already exists: change it with task edit"
            )))
        }
        (false, false) => {
            return Err(OpError::not_found(format!(
                "no task {id:?}: create it with task add"
            )))
        }
        _ => {}
    }
    let after = serde_json::to_string_pretty(&task).unwrap_or_default() + "\n";
    let shown = |t: &str| crate::redact::scrub_text(t.to_string());
    let path_text = path.display().to_string();
    let step = json!({
        "op": if create { "create" } else { "update" }, "path": path_text,
        "detail": if create { format!("write the new task {id}") } else { format!("change task {id}") },
        "diff": {"before": shown(before_text.as_deref().unwrap_or("")), "after": shown(&after)},
    });
    let warnings = secret_warnings(before.as_ref(), &task);
    let (backup, result) = if dry_run {
        (None, Value::Null)
    } else {
        let backup = if create {
            None
        } else {
            store::backup_task(&base, &id)?
        };
        store::save_task(&base, &task)?;
        let undo = match &backup {
            Some(b) => format!("cp {} {path_text}", b.display()),
            None => format!("toolportctl task rm {id}"),
        };
        let backups: Vec<String> = backup.iter().map(|b| b.display().to_string()).collect();
        (
            backup,
            json!({"applied": true, "changed": [path_text], "undo": undo, "backups": backups}),
        )
    };
    let undo = match (&backup, create) {
        (Some(b), _) => format!("cp {} {path_text}", b.display()),
        (None, true) => format!("toolportctl task rm {id}"),
        (None, false) => format!("restore {path_text} from the backup the apply prints"),
    };
    let verb = if create { "Add" } else { "Edit" };
    Ok(json!({
        "dryRun": dry_run, "task": id, "path": path_text, "created": create,
        "plan": {"summary": format!("{verb} task {id}"), "steps": [step], "effects": {}, "warnings": warnings, "undo": undo},
        "result": result,
    }))
}

fn definition(id: &str, text: &str, origin: &str) -> Result<Task, OpError> {
    let task = model::parse(text).map_err(invalid)?;
    if task.id != id {
        return Err(OpError::usage(format!(
            "{origin} defines the task {:?}, not {id:?}",
            task.id
        )));
    }
    Ok(task)
}

pub fn add(host: &dyn TaskHost, id: &str, source: Source, dry_run: bool) -> Result<Value, OpError> {
    let task = match source {
        Source::File(path) => definition(id, &read(path)?, &abs(path))?,
        Source::Stdin(text) => definition(id, text, "the definition on stdin")?,
        Source::Command(path) => {
            if !model::valid_id(id) {
                return Err(OpError::usage(format!(
                    "{id:?} is not a valid id: use 1-64 characters of a-z, 0-9 and '-'"
                )));
            }
            model::from_command(id, &abs(path), &read(path)?)
        }
    };
    save(host, task, true, dry_run)
}

pub fn edit(host: &dyn TaskHost, id: &str, file: &str, dry_run: bool) -> Result<Value, OpError> {
    edit_with(host, id, Source::File(file), dry_run)
}

pub fn edit_with(
    host: &dyn TaskHost,
    id: &str,
    source: Source,
    dry_run: bool,
) -> Result<Value, OpError> {
    let task = match source {
        Source::File(path) => definition(id, &read(path)?, &abs(path))?,
        Source::Stdin(text) => definition(id, text, "the definition on stdin")?,
        Source::Command(_) => {
            return Err(OpError::usage(
                "task edit takes a definition, not a command file",
            ))
        }
    };
    save(host, task, false, dry_run)
}

pub fn rm(host: &dyn TaskHost, id: &str, dry_run: bool) -> Result<Value, OpError> {
    let base = triggers::base_of(host)?;
    let path = store::task_path(&base, id)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|_| OpError::not_found(format!("no task {id:?}")))?;
    if let Some(busy) = store::live_run(&base, id) {
        return Err(OpError::conflict(format!(
            "task {id:?} is running as {}: cancel the run first",
            busy.id
        )));
    }
    let path_text = path.display().to_string();
    let step = json!({"op": "delete", "path": path_text, "detail": format!("remove the task {id} (its run history stays)"), "diff": {"before": crate::redact::scrub_text(text), "after": ""}});
    let mut plan = json!({"summary": format!("Remove task {id}"), "steps": [step], "effects": {}, "warnings": [], "undo": "restore the task file from the backup the apply prints"});
    if dry_run {
        return Ok(json!({"dryRun": true, "task": id, "plan": plan, "result": null}));
    }
    let backup = store::backup_task(&base, id)?.map(|b| b.display().to_string());
    store::remove_task(&base, id)?;
    let undo = backup
        .as_ref()
        .map_or_else(|| "none".to_string(), |b| format!("cp {b} {path_text}"));
    plan["undo"] = json!(undo);
    let backups: Vec<String> = backup.into_iter().collect();
    Ok(
        json!({"dryRun": false, "task": id, "plan": plan, "result": {"applied": true, "changed": [path_text], "undo": undo, "backups": backups}}),
    )
}

pub fn step_wait_kind(task: &Task, run: &Run) -> Option<Option<WaitKind>> {
    let index = run.steps.iter().position(|s| s.status == Status::Waiting)?;
    match task.steps.get(index)? {
        Step::NeedsYou { wait_for, .. } => Some(wait_for.as_ref().map(|w| w.kind.clone())),
        _ => None,
    }
}
