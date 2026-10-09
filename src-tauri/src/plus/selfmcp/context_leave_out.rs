//! `context_leave_out` (tier 2): the same operation as `toolportctl context leave-out`, over
//! `plus::context::leave_out`. Calling a tier 2 tool is the approval; the call previews unless
//! `dry_run` is false.

use super::ToolError;
use crate::plus::args::{flag, flag_or, str_nonempty};
use crate::plus::context::leave_out::{api, Op, Target};
use serde_json::Value;

pub(super) fn run(args: &Value) -> Result<Value, ToolError> {
    let op = match str_nonempty(args, "action") {
        Some("add") => Op::Add,
        Some("rm") => Op::Rm,
        _ => return Err(ToolError::new("invalid_arguments", "action must be add or rm")),
    };
    let target = match (str_nonempty(args, "path"), flag(args, "memory")) {
        (Some(path), false) => Target::File(path.to_string()),
        (None, true) => Target::Memory,
        _ => return Err(ToolError::new("invalid_arguments", "give either path or memory=true")),
    };
    let cwd = str_nonempty(args, "cwd")
        .ok_or_else(|| ToolError::new("invalid_arguments", "cwd is required"))?;
    Ok(api(op, target, Some(cwd), flag_or(args, "dry_run", true))?)
}
