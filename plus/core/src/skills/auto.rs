//! Whether the unattended sync may run, and what its last run did. A pause is a flag under
//! `<data dir>/plus/skills-auto.json` with when and why it was set: `undo` pauses so the next run
//! does not redo what was undone, `manual` is the user's own pause. What runs it is the shipped
//! task [`TASK_ID`]; `on` and `off` turn that task on and off, `pause` leaves it scheduled.

use super::clock::Clock;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use crate::tasks::builtin::SKILLS_AUTO_SYNC;
use crate::tasks::cron::{rfc3339, Cron};
use crate::tasks::{api as task_api, schedule, store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::TaskHost;

pub const TASK_ID: &str = SKILLS_AUTO_SYNC;

pub const REASON_UNDO: &str = "undo";
pub const REASON_MANUAL: &str = "manual";
const FILE: &str = "skills-auto.json";

/// A change the last unattended run left for the user, as the `held` rows of a sync report it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldItem {
    pub skill: String,
    pub client: String,
    pub path: String,
    pub op: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastRun {
    pub at: String,
    pub applied: u64,
    pub held: Vec<HeldItem>,
    pub skipped: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub paused: bool,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, rename = "lastRun", skip_serializing_if = "Option::is_none")]
    pub last_run: Option<LastRun>,
}

impl State {
    pub fn to_json(&self) -> Value {
        json!({
            "paused": self.paused,
            "since": self.since,
            "reason": self.reason,
            "lastRun": self.last_run,
        })
    }

    pub fn describe(&self) -> String {
        format!(
            "auto-sync is paused since {} (reason: {}); resume it with `skills auto resume`",
            self.since.as_deref().unwrap_or("an unknown time"),
            self.reason.as_deref().unwrap_or("unknown")
        )
    }
}

fn path(base: &Path) -> PathBuf {
    base.join("plus").join(FILE)
}

/// No file is a run that is not paused; a file that cannot be read is an error, so an unreadable
/// pause never lets an unattended run through.
pub fn read(base: &Path) -> Result<State, OpError> {
    let file = path(base);
    match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            OpError::failed(
                "skills",
                format!("{} is not valid: {e}; remove it to resume", file.display()),
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(OpError::failed(
            "skills",
            format!("{}: {e}", file.display()),
        )),
    }
}

fn write(base: &Path, state: &State) -> Result<(), OpError> {
    let text = serde_json::to_string_pretty(state).unwrap_or_default() + "\n";
    atomic_write(&path(base), &text, Mode::Private)
        .map_err(|e| OpError::failed("skills", format!("{}: {e}", path(base).display())))
}

/// A pause that is already set stays as it is, with its own since and reason; the flag says
/// whether this call changed anything.
pub fn pause(base: &Path, reason: &str, clock: &dyn Clock) -> Result<(State, bool), OpError> {
    let state = read(base)?;
    if state.paused {
        return Ok((state, false));
    }
    let state = State {
        paused: true,
        since: Some(clock.now().zulu()),
        reason: Some(reason.to_string()),
        last_run: state.last_run,
    };
    write(base, &state)?;
    Ok((state, true))
}

pub fn resume(base: &Path) -> Result<(State, bool), OpError> {
    let was = read(base)?;
    if !was.paused {
        return Ok((was, false));
    }
    let state = State {
        last_run: was.last_run,
        ..State::default()
    };
    write(base, &state)?;
    Ok((state, true))
}

/// An unattended run's outcome replaces the previous one.
pub fn record_run(base: &Path, run: LastRun) -> Result<(), OpError> {
    let mut state = read(base)?;
    state.last_run = Some(run);
    write(base, &state)
}

/// A run that failed keeps what the run before it left held: those changes still wait.
pub fn record_error(base: &Path, message: &str, clock: &dyn Clock) -> Result<(), OpError> {
    let held = read(base)?.last_run.map(|run| run.held).unwrap_or_default();
    record_run(
        base,
        LastRun {
            at: clock.now().zulu(),
            applied: 0,
            held,
            skipped: 0,
            error: Some(message.to_string()),
        },
    )
}

/// After a sync that did not run unattended: what the last run held and a later sync no longer
/// holds was decided by the user, so it leaves the record.
pub fn settle_held(base: &Path, still_held: &[HeldItem]) -> Result<(), OpError> {
    let mut state = read(base)?;
    let Some(run) = state.last_run.as_mut() else {
        return Ok(());
    };
    let before = run.held.len();
    run.held.retain(|item| still_held.contains(item));
    if run.held.len() == before {
        return Ok(());
    }
    write(base, &state)
}

fn data_dir() -> Result<PathBuf, OpError> {
    crate::fsx::data_dir().ok_or_else(|| OpError::not_found("data directory unknown"))
}

fn result(state: &State, changed: bool) -> Value {
    let mut data = state.to_json();
    data["changed"] = json!(changed);
    data
}

pub fn status_op() -> Result<Value, OpError> {
    Ok(read(&data_dir()?)?.to_json())
}

fn when(cron: &str) -> String {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    match parts.as_slice() {
        [minute, hour, "*", "*", "*"] => match (minute.parse::<u32>(), hour.parse::<u32>()) {
            (Ok(minute), Ok(hour)) => format!("daily {hour:02}:{minute:02} UTC"),
            _ => cron.to_string(),
        },
        _ => cron.to_string(),
    }
}

/// What runs the unattended sync: the shipped task, whether it is on, when it is next due and how
/// its last run went. `present` is false once the task was removed.
pub fn schedule_json(host: &dyn TaskHost) -> Value {
    let Some(base) = host.tasks_base() else {
        return Value::Null;
    };
    let Ok(task) = store::load_task(&base, TASK_ID) else {
        return json!({"task": TASK_ID, "present": false, "enabled": false, "cron": null,
            "when": null, "autoRun": false, "nextRun": null, "lastRun": null, "scheduler": null});
    };
    let schedule_of = task.triggers.schedule.as_ref();
    let next = schedule_of
        .filter(|_| task.enabled)
        .and_then(|s| Cron::parse(&s.cron).ok())
        .and_then(|cron| cron.next_after(host.now()));
    let last = store::list_runs(&base, Some(TASK_ID))
        .into_iter()
        .next()
        .map(|run| {
            json!({"runId": run.id, "status": run.status, "trigger": run.trigger,
                "startedAt": run.started_at})
        });
    json!({
        "task": TASK_ID,
        "present": true,
        "enabled": task.enabled,
        "cron": schedule_of.map(|s| s.cron.as_str()),
        "when": schedule_of.map(|s| when(&s.cron)),
        "autoRun": schedule_of.is_some_and(|s| s.auto_run),
        "nextRun": next.map(rfc3339),
        "lastRun": last,
        "scheduler": schedule::read_heartbeat(&base),
    })
}

pub fn with_schedule(host: &dyn TaskHost, mut data: Value) -> Value {
    data["schedule"] = schedule_json(host);
    data
}

pub fn full_status_op(host: &dyn TaskHost) -> Result<Value, OpError> {
    Ok(with_schedule(host, status_op()?))
}

/// Turns the shipped task on or off through the task code, which claims it on the first change.
/// `undo` names this command's opposite, not the task command.
pub fn switch_op(host: &dyn TaskHost, enabled: bool, dry_run: bool) -> Result<Value, OpError> {
    finish_switch(
        host,
        task_api::set_enabled(host, TASK_ID, enabled, dry_run)?,
        enabled,
    )
}

pub fn finish_switch(
    host: &dyn TaskHost,
    mut data: Value,
    enabled: bool,
) -> Result<Value, OpError> {
    let undo = format!(
        "toolportctl skills auto {}",
        if enabled { "off" } else { "on" }
    );
    data["plan"]["undo"] = json!(undo);
    if data["result"]["undo"].is_string() {
        data["result"]["undo"] = json!(undo);
    }
    Ok(with_schedule(host, data))
}

pub fn pause_op(clock: &dyn Clock) -> Result<Value, OpError> {
    let (state, changed) = pause(&data_dir()?, REASON_MANUAL, clock)?;
    Ok(result(&state, changed))
}

pub fn resume_op() -> Result<Value, OpError> {
    let (state, changed) = resume(&data_dir()?)?;
    Ok(result(&state, changed))
}
