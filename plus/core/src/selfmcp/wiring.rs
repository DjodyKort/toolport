//! The `context_layers_*`, `context_wiring_*` and `context_checkpoint_*` tools: what
//! `toolportctl context layers|wiring|checkpoint-*` does, for agents. Writes preview unless told
//! otherwise.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag_or, str_arg};
use crate::context::{checkpoint, layer_deploy, layer_list, wiring};

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    str_arg(args, key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn layers_ls(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(layer_list::ls(host)?)
}

pub(super) fn layers_undo(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(layer_deploy::undo(host, flag_or(args, "dry_run", true))?)
}

pub(super) fn status(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(wiring::status(host)?)
}

pub(super) fn init(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(wiring::init(host, flag_or(args, "dry_run", true))?)
}

pub(super) fn disable(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(wiring::disable(host, flag_or(args, "dry_run", true))?)
}

pub(super) fn checkpoint_status(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(checkpoint::status(host, text(args, "cwd"))?)
}

pub(super) fn checkpoint_config(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let off = flag_or(args, "off", false);
    let at = args.get("at").and_then(Value::as_u64);
    match (at, off) {
        (Some(_), true) => Err(ToolError::new(
            "invalid_arguments",
            "at and off cannot be used together",
        )),
        (None, false) => Err(ToolError::new(
            "invalid_arguments",
            "give at (tokens before compaction) or off",
        )),
        (at, _) => Ok(checkpoint::set(host, at, flag_or(args, "dry_run", true))?),
    }
}
