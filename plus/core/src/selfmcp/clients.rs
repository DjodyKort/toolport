//! The `client_direct_*` tools: what `toolportctl client direct` does, for agents.

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, flag_or, str_arg};
use crate::direct::{self, Args};

type Outcome = Result<Value, ToolError>;

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_arg(args, key)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.starts_with('-'))
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

fn args_of(args: &Value) -> Result<Args<'_>, ToolError> {
    Ok(Args {
        server: required(args, "name")?,
        client: required(args, "client")?,
        force: flag(args, "force"),
        dry_run: flag_or(args, "dry_run", true),
        plan_hash: str_arg(args, "plan_hash"),
    })
}

pub(super) fn ls(host: &dyn Host, args: &Value) -> Outcome {
    let rows = direct::list(host, str_arg(args, "client"))?;
    Ok(direct::list_value(&rows))
}

pub(super) fn check(host: &dyn Host, args: &Value) -> Outcome {
    let (rows, healthy) = direct::check(host, str_arg(args, "client"))?;
    let mut data = direct::list_value(&rows);
    data["healthy"] = json!(healthy);
    Ok(data)
}

pub(super) fn add(host: &dyn Host, args: &Value) -> Outcome {
    Ok(direct::add(host, &args_of(args)?)?)
}

pub(super) fn rm(host: &dyn Host, args: &Value) -> Outcome {
    Ok(direct::remove(host, &args_of(args)?)?)
}
