//! The `access_set_*`, `client_scope` and `folder_route_*` tools: what `toolportctl access-set`,
//! `client scope` and `folder route` do, for agents. Writes preview unless told otherwise.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::access::{
    self, finish, note, step, AddArgs, Change, Draft, Orphans, RmArgs, ToolChange, ToolsArgs,
};
use crate::args::{flag, flag_or, list, nonempty_strings, str_trimmed};

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_trimmed(args, key)
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

fn change<'a>(args: &'a Value) -> Result<Change<'a>, ToolError> {
    match (str_trimmed(args, "set"), flag(args, "none")) {
        (Some(_), true) => Err(ToolError::new(
            "invalid_arguments",
            "give set or none=true, not both",
        )),
        (Some(set), false) => Ok(Change::Set(set)),
        (None, true) => Ok(Change::Clear),
        (None, false) => Err(ToolError::new(
            "invalid_arguments",
            "give set, or none=true to remove the binding",
        )),
    }
}

pub(super) fn set_ls(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(access::ls(host)?)
}

pub(super) fn set_add(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let servers = nonempty_strings(args, "servers").unwrap_or_default();
    Ok(access::add(
        host,
        &AddArgs {
            name: required(args, "name")?,
            servers: &servers,
            dry_run: flag_or(args, "dry_run", true),
        },
    )?)
}

pub(super) fn set_rm(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let orphans = match str_trimmed(args, "orphans") {
        None => None,
        Some("detach") => Some(Orphans::Detach),
        Some("leave") => Some(Orphans::Leave),
        Some(_) => {
            return Err(ToolError::new(
                "invalid_arguments",
                "orphans is detach or leave",
            ))
        }
    };
    Ok(access::rm(
        host,
        &RmArgs {
            set: required(args, "set")?,
            orphans,
            dry_run: flag_or(args, "dry_run", true),
        },
    )?)
}

pub(super) fn set_default(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(access::default(
        host,
        change(args)?,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn client_scope(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(access::scope(
        host,
        required(args, "client")?,
        change(args)?,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn route_ls(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(access::routes(host)?)
}

pub(super) fn route_add(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(access::route_add(
        host,
        required(args, "folder")?,
        required(args, "set")?,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn route_rm(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(access::route_rm(
        host,
        required(args, "folder")?,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn set_tools(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let invalid = |message: &str| ToolError::new("invalid_arguments", message);
    let tools = match list(args, "tools") {
        None => None,
        Some(items) => Some(
            items
                .iter()
                .map(|item| item.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| invalid("tools must be a list of tool names"))?,
        ),
    };
    let Some(server) = str_trimmed(args, "server") else {
        if tools.is_some() || flag(args, "all") {
            return Err(invalid("server is required to change a tool list"));
        }
        return Ok(access::tools_ls(host, str_trimmed(args, "set"))?);
    };
    let change = match (&tools, flag(args, "all")) {
        (Some(_), true) => return Err(invalid("give tools or all=true, not both")),
        (Some(tools), false) => ToolChange::Only(tools),
        (None, true) => ToolChange::All,
        (None, false) => {
            return Err(invalid(
                "give tools (an empty list exposes none) or all=true",
            ))
        }
    };
    Ok(access::tools_set(
        host,
        &ToolsArgs {
            set: required(args, "set")?,
            server,
            change,
            dry_run: flag_or(args, "dry_run", true),
        },
    )?)
}

const DISCOVERY_MODES: [&str; 3] = ["full", "lazy", "grouped"];

pub(super) fn discovery_set(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let client = required(args, "client")?;
    let mode = match str_trimmed(args, "mode") {
        None => {
            return Err(ToolError::new(
                "invalid_arguments",
                "mode is required: full, lazy, grouped or inherit",
            ))
        }
        Some("inherit") => None,
        Some(mode) if DISCOVERY_MODES.contains(&mode) => Some(mode.to_string()),
        Some(_) => {
            return Err(ToolError::new(
                "invalid_arguments",
                "mode is full, lazy, grouped or inherit",
            ))
        }
    };
    let view = host
        .read_view()
        .map_err(|error| ToolError::new("registry_error", error))?;
    let before = view.as_ref().and_then(|view| view.client_discovery(client));
    let known = host.detect_clients().iter().any(|info| info.id == client);
    if before.is_none() && !known {
        return Err(ToolError::new("not_found", format!("no client '{client}'")));
    }
    let same = before == mode;
    let label = |mode: &Option<String>| mode.clone().unwrap_or_else(|| "inherit".to_string());
    let draft = Draft {
        summary: if same {
            format!("{client} already uses {}", label(&mode))
        } else {
            format!("Set the discovery mode of {client} to {}", label(&mode))
        },
        steps: vec![if same {
            note("nothing to change")
        } else {
            step(match &mode {
                Some(mode) => {
                    format!("{client} gets the {mode} tool listing instead of the global mode")
                }
                None => format!("{client} follows the global discovery mode again"),
            })
        }],
        warnings: Vec::new(),
        undo: format!(
            "client_discovery_set client={client} mode={}",
            label(&before)
        ),
    };
    let (id, pinned) = (client.to_string(), mode.clone());
    let mut data = finish(
        host,
        flag_or(args, "dry_run", true),
        draft,
        !same,
        &mut |tx| tx.set_client_discovery(&id, pinned.as_deref()),
        serde_json::json!({"client": client, "mode": label(&mode), "was": label(&before)}),
    )?;
    data["modes"] = serde_json::json!({
        "full": "the client sees every tool in the list",
        "lazy": "the client finds tools through toolport_search_tools",
        "grouped": "the client browses one server at a time",
        "inherit": "the global mode applies",
    });
    Ok(data)
}
