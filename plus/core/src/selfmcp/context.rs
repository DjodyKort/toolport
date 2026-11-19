//! `context_bundle_ls|show|status` (tier 1), `context_bundle_apply|undo`, `context_bundle_launch`
//! and `context_leave_out` (tier 2), `context_sync`, `context_bundle_config` and
//! `context_bundle_save` (tier 3), `context_bundle_rm` (tier 4): the same operations as
//! `toolportctl context`, over `crate::context::ops`. Calling a tier 2 tool is the approval; a
//! model that wants the plan first passes `dry_run`, and the tier 3 and 4 tools and
//! `context_leave_out` preview unless told otherwise.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, flag_or, nonempty_strings, str_arg};
use crate::context::bundle::{self, Edit};
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

pub(super) fn bundle_show(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::show(host, required(args, "name")?)?)
}

pub(super) fn sync(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::sync(host, flag_or(args, "dry_run", true))?)
}

pub(super) fn bundle_config(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let wanted = match str_arg(args, "auto_apply").map(str::trim) {
        None => None,
        Some("on") => Some(true),
        Some("off") => Some(false),
        Some(_) => return Err(invalid("auto_apply takes on or off")),
    };
    let mut current = api::config(host, None)?;
    let Some(wanted) = wanted else {
        return Ok(current);
    };
    if flag_or(args, "dry_run", true) {
        current["dryRun"] = Value::Bool(true);
        current["wouldSet"] = Value::Bool(wanted);
        return Ok(current);
    }
    Ok(api::config(host, Some(wanted))?)
}

fn text_list(args: &Value, key: &str) -> Result<Option<Vec<String>>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => {
            Ok(Some(nonempty_strings(args, key).unwrap_or_default()))
        }
        Some(_) => Err(invalid(format!("{key} must be a list of strings"))),
    }
}

fn in_tool_words(error: crate::op::OpError) -> ToolError {
    let mut error = ToolError::from(error);
    error.message = error
        .message
        .replace("use `context bundle edit`", "use mode=edit")
        .replace(
            "undo it there or pass --force",
            "undo it there with context_bundle_undo first",
        );
    error
}

pub(super) fn bundle_save(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let create = match str_arg(args, "mode").map(str::trim) {
        Some("add") => true,
        Some("edit") => false,
        _ => return Err(invalid("mode must be add or edit")),
    };
    let name = required(args, "name")?;
    let edit = Edit {
        description: str_arg(args, "description").map(String::from),
        servers: str_arg(args, "servers").map(|s| s.trim().to_string()),
        skills_off: text_list(args, "skills_off")?,
        skills_name_only: text_list(args, "skills_name_only")?,
        skills_allow: text_list(args, "skills_allow")?,
        plugins_off: text_list(args, "plugins_off")?,
        layers_add: text_list(args, "layers_add")?,
        layers_exclude: text_list(args, "layers_exclude")?,
        agents_off: text_list(args, "agents_off")?,
        bind: text_list(args, "bind")?,
    };
    let mut saved = api::save(
        host,
        name,
        edit,
        None,
        create,
        flag_or(args, "dry_run", true),
    )
    .map_err(in_tool_words)?;
    let after = saved["plan"]["steps"][0]["diff"]["after"]
        .as_str()
        .unwrap_or_default();
    if let Ok(parsed) = bundle::parse(name, after) {
        saved["names"] = serde_json::json!({
            "servers": parsed.servers,
            "bind": parsed.bind,
        });
    }
    Ok(saved)
}

pub(super) fn bundle_rm(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    api::rm(
        host,
        required(args, "name")?,
        false,
        flag_or(args, "dry_run", true),
    )
    .map_err(in_tool_words)
}

pub(super) fn bundle_launch(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::launch(
        host,
        required(args, "name")?,
        str_arg(args, "cwd")
            .map(str::trim)
            .filter(|c| !c.is_empty()),
    )?)
}
