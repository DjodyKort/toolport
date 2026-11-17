//! Who may start a run. A schedule, a catch-up or a failed login starts only a task that has no
//! `needs-you` step and an `autoRun` schedule; every other case raises an attention record and
//! starts nothing.

use std::path::Path;
use std::time::Duration;

use serde_json::json;
use toolport_plus_api::TaskHost;

use super::cron::rfc3339;
use super::model::Task;
use super::store::{self, AttentionRecord, Missed, Run, Status};
use crate::op::OpError;

const START_LOCK_WAIT: Duration = Duration::from_secs(5);

pub fn allowed(task: &Task, trigger: &str) -> Result<(), String> {
    if !task.enabled {
        return Err(format!(
            "task {:?} is disabled: enable it first (task edit)",
            task.id
        ));
    }
    match trigger {
        "manual" => Ok(()),
        "cli" if task.triggers.cli => Ok(()),
        "cli" => Err(format!(
            "task {:?} does not allow the CLI trigger (triggers.cli is false)",
            task.id
        )),
        "selfMcp" if task.triggers.self_mcp.enabled => Ok(()),
        "selfMcp" => Err(format!(
            "task {:?} does not allow the self-MCP trigger",
            task.id
        )),
        "schedule" | "catch-up" | "onAuthFailure" if auto_runs(task) => Ok(()),
        other => Err(format!(
            "task {:?} is not started by {other}: it needs you or has no autoRun schedule",
            task.id
        )),
    }
}

pub fn auto_runs(task: &Task) -> bool {
    !task.has_needs_you() && task.triggers.schedule.as_ref().is_some_and(|s| s.auto_run)
}

/// Starts a run and its runner. Single-flight: the check for a live run and the new record
/// happen under one lock per task, so two processes cannot both start the same task.
pub fn start(
    task: &Task,
    trigger: &str,
    missed: Option<Missed>,
    host: &dyn TaskHost,
) -> Result<Run, OpError> {
    let base = base_of(host)?;
    allowed(task, trigger).map_err(OpError::conflict)?;
    let _guard = store::lock(
        &store::runs_dir(&base).join(format!(".start-{}.lock", task.id)),
        START_LOCK_WAIT,
    )
    .ok_or_else(|| OpError::conflict(format!("task {:?} is being started elsewhere", task.id)))?;
    if let Some(busy) = store::live_run(&base, &task.id) {
        return Err(OpError::conflict(format!(
            "task {:?} is already running as {}",
            task.id, busy.id
        ))
        .with_details(json!({"task": task.id, "run": busy.id, "status": busy.status})));
    }
    let mut run = store::new_run(task, trigger, host.now());
    run.missed = missed;
    store::create_run(&base, &run)?;
    if let Err(e) = host.task_spawn_runner(&run.id) {
        let now = host.now();
        let _ = store::update_run(&base, &run.id, |r| {
            r.status = Status::Failed;
            r.error = Some(e.clone());
            r.ended_at = Some(rfc3339(now));
        });
        return Err(OpError::failed("spawn", e));
    }
    Ok(run)
}

pub fn base_of(host: &dyn TaskHost) -> Result<std::path::PathBuf, OpError> {
    host.tasks_base()
        .ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))
}

#[derive(Debug, PartialEq, Eq)]
pub enum Raised {
    Started(String),
    Attention(String),
}

pub fn attention(
    base: &Path,
    task: &Task,
    key: &str,
    title: String,
    detail: String,
    now: i64,
) -> Result<Raised, OpError> {
    let record = AttentionRecord {
        id: format!("tasks:{}:{key}", task.id),
        level: "needs-you".into(),
        title,
        detail,
        from: "tasks".into(),
        task: task.id.clone(),
        since: rfc3339(now),
        action: Some(
            json!({"label": "Run task", "command": ["toolportctl", "task", "run", task.id]}),
        ),
    };
    store::raise_attention(base, record)?;
    Ok(Raised::Attention(task.id.clone()))
}

pub fn refused_detail(task: &Task) -> String {
    if task.has_needs_you() {
        "the task has a step that needs you, so it is never started on its own".to_string()
    } else {
        "the task has no autoRun schedule".to_string()
    }
}

fn fire(
    task: &Task,
    trigger: &str,
    key: &str,
    title: String,
    host: &dyn TaskHost,
) -> Result<Raised, OpError> {
    if allowed(task, trigger).is_ok() {
        return start(task, trigger, None, host).map(|r| Raised::Started(r.id));
    }
    attention(
        &base_of(host)?,
        task,
        key,
        title,
        refused_detail(task),
        host.now(),
    )
}

pub fn on_auth_failure(server: &str, host: &dyn TaskHost) -> Vec<Raised> {
    let Ok(base) = base_of(host) else {
        return Vec::new();
    };
    store::list_tasks(&base)
        .into_iter()
        .flatten()
        .filter(|t| t.enabled && t.triggers.on_auth_failure.iter().any(|s| s == server))
        .filter_map(|t| {
            fire(
                &t,
                "onAuthFailure",
                &format!("auth:{server}"),
                format!("{server} login failed: run task {}?", t.id),
                host,
            )
            .ok()
        })
        .collect()
}

static SEEN: std::sync::Mutex<Vec<(String, i64)>> = std::sync::Mutex::new(Vec::new());

/// Reacts once per failure: a login that stays broken is probed again and again, and each probe
/// carries the same `since`.
pub fn on_auth_failure_once(server: &str, since: i64, host: &dyn TaskHost) -> Vec<Raised> {
    let mut seen = SEEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if seen.iter().any(|(s, at)| s == server && *at == since) {
        return Vec::new();
    }
    seen.retain(|(s, _)| s != server);
    seen.push((server.to_string(), since));
    drop(seen);
    on_auth_failure(server, host)
}
