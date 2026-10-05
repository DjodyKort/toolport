//! `tasks_list|get|history` (tier 1) and `tasks_run|cancel` (tier 2): adapters over
//! `plus::tasks::api`, which `toolportctl task` renders as well. `tasks_run` asks the approval
//! broker on every run; adding, editing and removing tasks is not exposed (D-067), so a prompt
//! injected model cannot define a task that writes secrets. No result carries a captured value.

use super::ToolError;
use crate::plus::args::{flag, str_nonempty};
use crate::plus::op::{ErrorKind, OpError};
use crate::plus::tasks::host::RealHost;
use crate::plus::tasks::{api, approval};
use serde_json::Value;

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_nonempty(args, key).ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

pub(super) fn list(args: &Value) -> Result<Value, ToolError> {
    Ok(api::ls(flag(args, "all"))?)
}

pub(super) fn get(args: &Value) -> Result<Value, ToolError> {
    Ok(api::show(required(args, "id")?)?)
}

pub(super) fn history(args: &Value) -> Result<Value, ToolError> {
    let limit = args.get("limit").and_then(Value::as_u64).map(|n| n as usize);
    Ok(api::history(str_nonempty(args, "id"), str_nonempty(args, "run"), limit)?)
}

fn refused(error: OpError) -> ToolError {
    match error.kind {
        ErrorKind::Failed("approval_unavailable") => ToolError::new("approval_unavailable", error.message),
        ErrorKind::Failed("approval_denied") => ToolError::new("approval_denied", error.message),
        _ => error.into(),
    }
}

pub(super) fn run(args: &Value) -> Result<Value, ToolError> {
    let id = required(args, "id")?;
    approval::run(id, flag(args, "dry_run"), &RealHost, &crate::approval::request_human_decision).map_err(refused)
}

pub(super) fn cancel(args: &Value) -> Result<Value, ToolError> {
    Ok(api::cancel(required(args, "run")?)?)
}
