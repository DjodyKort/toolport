//! `context_bundle_ls|status` (tier 1) and `context_bundle_apply|undo` (tier 2): the same
//! operations as `toolportctl context bundle`, over `plus::context::bundle_api`. Calling a tier 2
//! tool is the approval; a model that wants to see the plan first passes `dry_run`.

use super::ToolError;
use crate::plus::args::{flag, str_nonempty};
use crate::plus::context::bundle_api as api;
use serde_json::Value;

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_nonempty(args, key).ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

pub(super) fn ls(_args: &Value) -> Result<Value, ToolError> {
    Ok(api::ls()?)
}

pub(super) fn status(args: &Value) -> Result<Value, ToolError> {
    Ok(api::status(Some(required(args, "cwd")?))?)
}

pub(super) fn apply(args: &Value) -> Result<Value, ToolError> {
    Ok(api::apply(required(args, "name")?, Some(required(args, "cwd")?), flag(args, "dry_run"))?)
}

pub(super) fn undo(args: &Value) -> Result<Value, ToolError> {
    Ok(api::undo(Some(required(args, "cwd")?), flag(args, "dry_run"))?)
}
