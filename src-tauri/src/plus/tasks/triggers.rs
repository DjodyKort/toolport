//! Who may start a run. A schedule or a failed login starts only a task that has no `needs-you`
//! step and an `autoRun` schedule; every other case raises an attention record and starts nothing.

use super::cron::{rfc3339, Cron};
use super::host::Host;
use super::model::Task;
use super::store::{self, AttentionRecord, Run, Status};
use crate::plus::op::OpError;
use serde_json::json;

pub fn allowed(task: &Task, trigger: &str) -> Result<(), String> {
    if !task.enabled {
        return Err(format!("task {:?} is disabled: enable it first (task edit)", task.id));
    }
    match trigger {
        "manual" => Ok(()),
        "cli" if task.triggers.cli => Ok(()),
        "cli" => Err(format!("task {:?} does not allow the CLI trigger (triggers.cli is false)", task.id)),
        "selfMcp" if task.triggers.self_mcp.enabled => Ok(()),
        "selfMcp" => Err(format!("task {:?} does not allow the self-MCP trigger", task.id)),
        "schedule" | "onAuthFailure" if auto_runs(task) => Ok(()),
        other => Err(format!("task {:?} is not started by {other}: it needs you or has no autoRun schedule", task.id)),
    }
}

pub fn auto_runs(task: &Task) -> bool {
    !task.has_needs_you() && task.triggers.schedule.as_ref().is_some_and(|s| s.auto_run)
}

pub fn start(task: &Task, trigger: &str, host: &dyn Host) -> Result<Run, OpError> {
    allowed(task, trigger).map_err(OpError::conflict)?;
    let busy = store::list_runs(Some(&task.id))?.into_iter().find(|r| matches!(r.status, Status::Running | Status::Waiting));
    if let Some(r) = busy {
        return Err(OpError::conflict(format!("task {:?} is already running as {}", task.id, r.id)));
    }
    let run = store::new_run(task, trigger);
    store::create_run(&run)?;
    if let Err(e) = host.spawn_runner(&run.id) {
        let _ = store::update_run(&run.id, |r| {
            r.status = Status::Failed;
            r.error = Some(e.clone());
            r.ended_at = Some(rfc3339(super::cron::now()));
        });
        return Err(OpError::failed("spawn", e));
    }
    Ok(run)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Raised {
    Started(String),
    Attention(String),
}

fn attention(task: &Task, key: &str, title: String, detail: String) -> Result<Raised, OpError> {
    let record = AttentionRecord {
        id: format!("tasks:{}:{key}", task.id),
        level: "needs-you".into(),
        title,
        detail,
        from: "tasks".into(),
        task: task.id.clone(),
        since: rfc3339(super::cron::now()),
        action: Some(json!({"label": "Run task", "command": ["toolportctl", "task", "run", task.id]})),
    };
    store::raise_attention(record)?;
    Ok(Raised::Attention(task.id.clone()))
}

fn fire(task: &Task, trigger: &str, key: &str, title: String, host: &dyn Host) -> Result<Raised, OpError> {
    if allowed(task, trigger).is_ok() {
        return start(task, trigger, host).map(|r| Raised::Started(r.id));
    }
    let detail = if task.has_needs_you() { "the task has a step that needs you, so it is never started on its own".to_string() } else { "the task has no autoRun schedule".to_string() };
    attention(task, key, title, detail)
}

pub fn on_auth_failure(server: &str, host: &dyn Host) -> Vec<Raised> {
    let tasks = store::list_tasks().unwrap_or_default();
    tasks
        .into_iter()
        .flatten()
        .filter(|t| t.enabled && t.triggers.on_auth_failure.iter().any(|s| s == server))
        .filter_map(|t| fire(&t, "onAuthFailure", &format!("auth:{server}"), format!("{server} login failed: run task {}?", t.id), host).ok())
        .collect()
}

pub fn run_due(epoch: i64, host: &dyn Host) -> Vec<Raised> {
    let minute = epoch.div_euclid(60);
    let tasks = store::list_tasks().unwrap_or_default();
    tasks
        .into_iter()
        .flatten()
        .filter(|t| t.enabled)
        .filter(|t| t.triggers.schedule.as_ref().and_then(|s| Cron::parse(&s.cron).ok()).is_some_and(|c| c.matches(minute * 60)))
        .filter(|t| claim(&t.id, minute))
        .filter_map(|t| fire(&t, "schedule", "due", format!("task {} is due", t.id), host).ok())
        .collect()
}

fn claim(id: &str, minute: i64) -> bool {
    let Ok(dir) = store::runs_dir() else { return false };
    let _ = std::fs::create_dir_all(&dir);
    std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(format!(".fired-{id}-{minute}"))).is_ok()
}
