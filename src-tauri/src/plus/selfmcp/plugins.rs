//! `plugins_ls`, `plugins_show` and `hooks_ls` (tier 1) and `plugins_config` and `plugins_mcp`
//! (tier 2): the inventory of Claude Code plugins and hooks and the controls a plugin really has,
//! adapters over `plus::plugins::api` that `toolportctl plugins|hooks` render as well.

use super::ToolError;
use crate::plus::args::{flag, flag_or, str_nonempty};
use crate::plus::plugins::config::McpOp;
use crate::plus::plugins::{api, hooks};
use serde_json::Value;

pub(super) fn ls(args: &Value) -> Result<Value, ToolError> {
    Ok(api::ls(str_nonempty(args, "cwd"), flag(args, "refresh"))?)
}

pub(super) fn show(args: &Value) -> Result<Value, ToolError> {
    let id = str_nonempty(args, "id")
        .ok_or_else(|| ToolError::new("invalid_arguments", "id is required"))?;
    Ok(api::show(id, str_nonempty(args, "cwd"))?)
}

pub(super) fn hooks_ls(args: &Value) -> Result<Value, ToolError> {
    let own = |key: &str| str_nonempty(args, key).map(String::from);
    let filter = hooks::Filter {
        tool: own("tool"),
        event: own("event"),
        owner: own("owner"),
    };
    Ok(api::hooks_ls(str_nonempty(args, "cwd"), &filter)?)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_nonempty(args, key)
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

fn knob_value(knob: &str, value: &Value) -> Result<String, ToolError> {
    let text = match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(on) => Some(on.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(String::from))
            .collect::<Option<Vec<_>>>()
            .map(|items| items.join(",")),
        _ => None,
    };
    text.ok_or_else(|| {
        ToolError::new(
            "invalid_arguments",
            format!("set.{knob} must be a string, a boolean or a list of strings"),
        )
    })
}

pub(super) fn config(args: &Value) -> Result<Value, ToolError> {
    let sets = args
        .get("set")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(knob, value)| Ok((knob.clone(), knob_value(knob, value)?)))
                .collect::<Result<Vec<_>, ToolError>>()
        })
        .transpose()?
        .unwrap_or_default();
    let unsets = args
        .get("unset")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default();
    Ok(api::config(
        required(args, "id")?,
        str_nonempty(args, "cwd"),
        sets,
        unsets,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn mcp(args: &Value) -> Result<Value, ToolError> {
    let op = match required(args, "action")? {
        "deny" => McpOp::Deny,
        "allow" => McpOp::Allow,
        other => {
            return Err(ToolError::new(
                "invalid_arguments",
                format!("action is deny or allow, got '{other}'"),
            ))
        }
    };
    Ok(api::mcp(
        op,
        required(args, "id")?,
        required(args, "server")?,
        Some(required(args, "cwd")?),
        flag_or(args, "dry_run", true),
    )?)
}
