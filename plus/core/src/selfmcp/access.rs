//! The `access_set_*`, `client_scope` and `folder_route_*` tools: what `toolportctl access-set`,
//! `client scope` and `folder route` do, for agents. Writes preview unless told otherwise.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::access::{self, AddArgs, Change, Orphans, RmArgs, ToolChange, ToolsArgs};
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
