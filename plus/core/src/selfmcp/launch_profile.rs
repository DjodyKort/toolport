//! The `context_launch_profile_*` tools: what `toolportctl context launch-profile` does, for
//! agents. Writes preview unless told otherwise.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag_or, str_arg};
use crate::context::launch_profile::{self as profiles, AddArgs};

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_arg(args, key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

pub(super) fn ls(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(profiles::ls(
        host,
        str_arg(args, "name")
            .map(str::trim)
            .filter(|n| !n.is_empty()),
        str_arg(args, "cwd")
            .map(str::trim)
            .filter(|n| !n.is_empty()),
    )?)
}

pub(super) fn add(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(profiles::add(
        host,
        &AddArgs {
            name: required(args, "name")?,
            set: str_arg(args, "set")
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            deny_direct: args.get("deny_direct").and_then(Value::as_bool),
            dry_run: flag_or(args, "dry_run", true),
        },
    )?)
}

pub(super) fn rm(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(profiles::rm(
        host,
        required(args, "name")?,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn policy(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(profiles::policy(
        host,
        required(args, "name")?,
        required(args, "cwd")?,
        flag_or(args, "dry_run", true),
    )?)
}
