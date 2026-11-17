//! `context_bundle_ls|status` (tier 1), `context_bundle_apply|undo` and `context_leave_out`
//! (tier 2): the same operations as `toolportctl context`, over `crate::context::ops`. Calling a
//! tier 2 tool is the approval; a model that wants the plan first passes `dry_run`, and
//! `context_leave_out` previews unless told otherwise.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, flag_or, str_arg};
use crate::context::leave_out::{Op, Target};
use crate::context::ops as api;

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::new("invalid_arguments", message)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_arg(args, key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("{key} is required")))
}

pub(super) fn ls(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(api::ls(host)?)
}

pub(super) fn context_status(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::folder_status(host, Some(required(args, "cwd")?))?)
}

pub(super) fn apply(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::apply(
        host,
        required(args, "name")?,
        Some(required(args, "cwd")?),
        flag(args, "dry_run"),
    )?)
}

pub(super) fn undo(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::undo(
        host,
        Some(required(args, "cwd")?),
        flag(args, "dry_run"),
    )?)
}

pub(super) fn leave_out(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let op = match str_arg(args, "action").map(str::trim) {
        Some("add") => Op::Add,
        Some("rm") => Op::Rm,
        _ => return Err(invalid("action must be add or rm")),
    };
    let target = match (
        str_arg(args, "path")
            .map(str::trim)
            .filter(|p| !p.is_empty()),
        flag(args, "memory"),
    ) {
        (Some(path), false) => Target::File(path.to_string()),
        (None, true) => Target::Memory,
        _ => return Err(invalid("give either path or memory=true")),
    };
    Ok(api::leave_out(
        host,
        op,
        target,
        Some(required(args, "cwd")?),
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn loads(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::loads(
        host,
        Some(required(args, "cwd")?),
        flag(args, "measured"),
    )?)
}

pub(super) fn compose(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::compose(host, Some(required(args, "cwd")?))?)
}

pub(super) fn status(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::status(host, Some(required(args, "cwd")?))?)
}

pub(super) fn measure(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let without = args["without"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect();
    Ok(api::measure(
        host,
        Some(required(args, "cwd")?),
        &api::MeasureOpts {
            model: str_arg(args, "model")
                .map(str::trim)
                .filter(|m| !m.is_empty()),
            force: flag(args, "force"),
            confirm: flag(args, "start"),
            without,
            bundle: str_arg(args, "bundle")
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .map(String::from),
        },
    )?)
}
