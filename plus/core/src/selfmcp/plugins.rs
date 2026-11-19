//! `plugins_ls`, `plugins_show` and `hooks_ls` read the inventory behind `toolportctl plugins
//! ls|show` and `hooks ls`. `plugins_config` and `plugins_mcp` read too, and write when called
//! with `dry_run=false`: the plan comes first, and every apply asks the person through the
//! approval broker. No value a call sets comes back in a result. Turning a plugin on or off is
//! not offered here.

use std::path::Path;

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use serde_json::json;

use super::approval;
use crate::args::{flag, flag_or, str_nonempty, str_trimmed};
use crate::context::plugin_config::{self, ConfigArgs, McpArgs, McpOp};
use crate::plugins::inventory::{self, HookFilter, World};
use crate::plugins::{ClaudeRunner, SystemClaude};

fn with_world<R>(
    host: &dyn Host,
    sessions: bool,
    claude: &dyn ClaudeRunner,
    run: impl FnOnce(&World, Option<&dyn ClaudeRunner>) -> Result<R, ToolError>,
) -> Result<R, ToolError> {
    let mut world = World::host(host)
        .ok_or_else(|| ToolError::new("no_home", "home directory could not be resolved"))?;
    if sessions {
        world = world.with_sessions();
    }
    run(&world, Some(claude))
}

pub(super) fn ls(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_world(host, true, &SystemClaude::from_env(), |world, runner| {
        Ok(inventory::ls(
            world,
            runner,
            str_nonempty(args, "cwd").map(Path::new),
            flag(args, "refresh"),
        ))
    })
}

pub(super) fn show(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let id = str_trimmed(args, "id")
        .ok_or_else(|| ToolError::new("invalid_arguments", "id is required"))?;
    with_world(host, false, &SystemClaude::from_env(), |world, runner| {
        Ok(inventory::show(
            world,
            runner,
            id,
            str_nonempty(args, "cwd").map(Path::new),
        )?)
    })
}

pub(super) fn hooks(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    with_world(host, false, &SystemClaude::from_env(), |world, runner| {
        Ok(inventory::hooks(
            world,
            runner,
            str_nonempty(args, "cwd").map(Path::new),
            &HookFilter {
                tool: str_nonempty(args, "tool"),
                event: str_nonempty(args, "event"),
                owner: str_nonempty(args, "owner"),
            },
        ))
    })
}

fn id_of(args: &Value) -> Result<&str, ToolError> {
    str_trimmed(args, "id").ok_or_else(|| ToolError::new("invalid_arguments", "id is required"))
}

/// The `set` argument: `{knob: value}` where a value is a string, a boolean or a list of strings.
fn knob_values(args: &Value) -> Result<Vec<(String, String)>, ToolError> {
    let invalid = |message: String| ToolError::new("invalid_arguments", message);
    let Some(set) = args.get("set").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let map = set
        .as_object()
        .ok_or_else(|| invalid("set must be an object of {knob: value}".into()))?;
    map.iter()
        .map(|(knob, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Bool(on) => on.to_string(),
                Value::Array(items) => items
                    .iter()
                    .map(|item| item.as_str().map(String::from))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| invalid(format!("set.{knob}: a list holds strings only")))?
                    .join(","),
                _ => {
                    return Err(invalid(format!(
                        "set.{knob} must be a string, a boolean or a list of strings"
                    )))
                }
            };
            Ok((knob.clone(), text))
        })
        .collect()
}

/// The values a `plugins_config` call carries: no result or error may hand them back.
pub(super) fn supplied_values(args: &Value) -> Vec<String> {
    knob_values(args)
        .map(|pairs| pairs.into_iter().map(|(_, value)| value).collect())
        .unwrap_or_default()
}

/// What the person is asked to approve: the same lines the plan shows, with the values, which
/// reach the person and never the model.
fn approval_steps(plan: &Value, set: &[(String, String)], unset: &[String]) -> Vec<String> {
    let mut steps: Vec<String> = plan["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["detail"].as_str().map(String::from))
        .collect();
    steps.extend(
        set.iter()
            .map(|(knob, value)| format!("set {knob} = {value}")),
    );
    steps.extend(unset.iter().map(|knob| format!("remove {knob}")));
    steps
}

/// A plan or result without the values it would write: names, files and counts stay.
fn without_values(mut data: Value) -> Value {
    for row in data["changes"].as_array_mut().into_iter().flatten() {
        if row.get("knob").is_some() {
            if let Some(object) = row.as_object_mut() {
                object.remove("value");
            }
        }
    }
    for step in data["plan"]["steps"].as_array_mut().into_iter().flatten() {
        if let Some(object) = step.as_object_mut() {
            object.remove("diff");
        }
    }
    data
}

pub(super) fn config(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    config_with(host, args, &SystemClaude::from_env())
}

pub(super) fn config_with(
    host: &dyn Host,
    args: &Value,
    claude: &dyn ClaudeRunner,
) -> Result<Value, ToolError> {
    let id = id_of(args)?;
    let cwd = str_nonempty(args, "cwd").map(Path::new);
    let set = knob_values(args)?;
    let unset: Vec<String> = args
        .get("unset")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|k| k.as_str().map(String::from))
        .collect();
    if set.is_empty() && unset.is_empty() {
        return with_world(host, false, claude, |world, runner| {
            Ok(plugin_config::read(world, runner, id, cwd)?)
        });
    }
    let dry_run = flag_or(args, "dry_run", true);
    let run = |dry_run: bool| {
        with_world(host, false, claude, |world, runner| {
            Ok(plugin_config::run(
                world,
                runner,
                &ConfigArgs {
                    id,
                    cwd,
                    set: &set,
                    unset: &unset,
                    dry_run,
                },
            )?)
        })
    };
    let preview = run(true)?;
    if dry_run {
        return Ok(without_values(preview));
    }
    approval::ask(
        host,
        &format!("plugins_config {id}"),
        approval_steps(&preview["plan"], &set, &unset),
        json!([]),
    )?;
    Ok(without_values(run(false)?))
}

pub(super) fn mcp(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    mcp_with(host, args, &SystemClaude::from_env())
}

pub(super) fn mcp_with(
    host: &dyn Host,
    args: &Value,
    claude: &dyn ClaudeRunner,
) -> Result<Value, ToolError> {
    let id = id_of(args)?;
    let cwd = str_nonempty(args, "cwd").map(Path::new);
    let op = match str_nonempty(args, "action").unwrap_or("ls") {
        "ls" => None,
        "deny" => Some(McpOp::Deny),
        "allow" => Some(McpOp::Allow),
        other => {
            return Err(ToolError::new(
                "invalid_arguments",
                format!("action is ls, deny or allow, not {other}"),
            ))
        }
    };
    let Some(op) = op else {
        return with_world(host, false, claude, |world, runner| {
            Ok(plugin_config::mcp_read(world, runner, id, cwd)?)
        });
    };
    let server = str_trimmed(args, "server")
        .ok_or_else(|| ToolError::new("invalid_arguments", "server is required"))?;
    let cwd = cwd.ok_or_else(|| {
        ToolError::new(
            "invalid_arguments",
            "cwd is required: a server is denied per folder",
        )
    })?;
    let run = |dry_run: bool| {
        with_world(host, false, claude, |world, runner| {
            Ok(plugin_config::mcp(
                world,
                runner,
                &McpArgs {
                    op,
                    id,
                    server,
                    cwd,
                    dry_run,
                },
            )?)
        })
    };
    let preview = run(true)?;
    if flag_or(args, "dry_run", true) {
        return Ok(without_values(preview));
    }
    approval::ask(
        host,
        &format!("plugins_mcp {id} {server}"),
        approval_steps(&preview["plan"], &[], &[]),
        json!([]),
    )?;
    Ok(without_values(run(false)?))
}
