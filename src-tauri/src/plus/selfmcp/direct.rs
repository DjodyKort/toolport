//! `client_direct_add|rm|ls`: thin adapters over `plus::direct`, the same core behind
//! `toolportctl client direct` and the `plus.client.direct*` handlers.

use super::ToolError;
use crate::plus::args::{flag, flag_or, str_nonempty};
use crate::plus::direct;
use crate::plus::profiles::{Error, Kind};
use serde_json::Value;

type Outcome = Result<Value, ToolError>;

fn tool_error(error: Error) -> ToolError {
    match error.kind {
        Kind::Invalid => ToolError::new("invalid_arguments", error.message),
        Kind::NotFound => ToolError::new("not_found", error.message),
        Kind::Conflict => ToolError::new("conflict", error.message),
        Kind::Failed("refused") => ToolError::new("invalid_arguments", error.message),
        Kind::Failed(_) => ToolError::backend(error.message),
    }
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_nonempty(args, key)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("invalid {key}")))
}

fn dry_run(args: &Value) -> bool {
    flag_or(args, "dry_run", true)
}

pub(super) fn client_direct_add(args: &Value) -> Outcome {
    direct::add(
        required(args, "server")?,
        required(args, "client")?,
        flag(args, "force"),
        dry_run(args),
    )
    .map(|outcome| outcome.to_value())
    .map_err(tool_error)
}

pub(super) fn client_direct_rm(args: &Value) -> Outcome {
    direct::remove(
        required(args, "server")?,
        required(args, "client")?,
        flag(args, "force"),
        dry_run(args),
    )
    .map(|outcome| outcome.to_value())
    .map_err(tool_error)
}

pub(super) fn client_direct_ls(args: &Value) -> Outcome {
    let client = str_nonempty(args, "client");
    direct::list(client)
        .map(|rows| direct::list_value(&rows))
        .map_err(tool_error)
}
