//! `tasks_list|get|history` (tier 1) and `tasks_run|cancel` (tier 2): adapters over
//! `tasks::api`, which `toolportctl task` renders as well. `tasks_run` asks the approval
//! broker on every run; adding, editing and removing tasks is not exposed (D-067), so a prompt
//! injected model cannot define a task that writes secrets. No result carries a captured value.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, str_nonempty};
use crate::op::{ErrorKind, OpError};
use crate::tasks::{api, approval};

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

fn refused(error: OpError) -> ToolError {
    match error.kind {
        ErrorKind::Failed("approval_unavailable") => {
            ToolError::new("approval_unavailable", error.message)
        }
        ErrorKind::Failed("approval_denied") => ToolError::new("approval_denied", error.message),
        _ => error.into(),
    }
}

pub(super) fn run(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let id = required(args, "id")?;
    approval::run(id, flag(args, "dry_run"), host).map_err(refused)
}

pub(super) fn cancel(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::cancel(host, required(args, "run")?)?)
}
