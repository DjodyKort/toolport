//! `tasks_list|get|history|missed` (tier 1) and `tasks_run|cancel|set_enabled` (tier 2): adapters over
//! `tasks::api`, which `toolportctl task` renders as well. `tasks_run` asks the approval
//! broker on every run; adding, editing and removing tasks is not exposed (D-067), so a prompt
//! injected model cannot define a task that writes secrets. No result carries a captured value.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, str_nonempty};
use crate::op::{ErrorKind, OpError};
use crate::tasks::model::WaitKind;
use crate::tasks::{api, approval, store, triggers};

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_nonempty(args, key)
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

pub(super) fn list(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::ls(host, flag(args, "all"))?)
}

pub(super) fn get(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::show(host, required(args, "id")?)?)
}

pub(super) fn history(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    Ok(api::history(
        host,
        str_nonempty(args, "id"),
        str_nonempty(args, "run"),
        limit,
    )?)
}

pub(super) fn refused(error: OpError) -> ToolError {
    match error.kind {
        ErrorKind::Failed("approval_unavailable") => {
            ToolError::new("approval_unavailable", error.message)
        }
        ErrorKind::Failed("approval_denied") => ToolError::new("approval_denied", error.message),
        _ => error.into(),
    }
}

const MAX_WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(200);

pub(super) fn run(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let id = required(args, "id")?;
    let dry_run = flag(args, "dry_run");
    let mut data = approval::run(id, dry_run, host).map_err(refused)?;
    if dry_run || !flag(args, "wait") {
        return Ok(data);
    }
    let base = triggers::base_of(host)?;
    let run_id = data["run"]["id"].as_str().unwrap_or_default().to_string();
    let (run, settled) = wait_for(&base, store::load_run(&base, &run_id)?, MAX_WAIT)?;
    data["run"] = api::run_view(&run, false);
    data["waited"] = json!({"settled": settled, "seconds": MAX_WAIT.as_secs()});
    Ok(data)
}

/// Polls until the run finishes, stops for a person, or `limit` passes; the flag says which of
/// the first two ended it.
pub(super) fn wait_for(
    base: &std::path::Path,
    mut run: store::Run,
    limit: Duration,
) -> Result<(store::Run, bool), ToolError> {
    let task = store::load_task(base, &run.task)?;
    let started = Instant::now();
    loop {
        if run.status.finished() {
            return Ok((run, true));
        }
        let resume_only = api::step_wait_kind(&task, &run).is_some_and(|kind| {
            kind.is_none_or(|kind| matches!(kind, WaitKind::Manual | WaitKind::Url))
        });
        if run.status == store::Status::Waiting && resume_only {
            return Ok((run, true));
        }
        if started.elapsed() >= limit {
            return Ok((run, false));
        }
        std::thread::sleep(POLL);
        run = store::load_run(base, &run.id)?;
    }
}

pub(super) fn missed(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::missed(host, str_nonempty(args, "id"))?)
}

pub(super) fn set_enabled(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let id = required(args, "id")?;
    let Some(enabled) = args.get("enabled").and_then(Value::as_bool) else {
        return Err(ToolError::new("invalid_arguments", "enabled is required"));
    };
    approval::set_enabled(id, enabled, flag(args, "dry_run"), host).map_err(refused)
}

pub(super) fn cancel(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::cancel(host, required(args, "run")?)?)
}
