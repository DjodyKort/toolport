use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::catalog::{ResourceDef, ToolDef};
use super::ToolError;
use crate::status;

const FLOW: &str = "# Data flow\n\n\
canonical skills repository -> transpilers -> per-client outputs\n\
registry (servers, profiles) -> gateway -> every client\n\
encrypted sync bundle <-> remote (push and pull)\n";

pub fn run_tool(host: &dyn Host, tool: &ToolDef, args: &Value) -> Result<Value, ToolError> {
    (tool.run)(host, args)
}

pub(super) fn where_am_i(host: &dyn Host, _: &Value) -> Result<Value, ToolError> {
    Ok(status::status(host, &status::snapshot(host)))
}

pub(super) fn doctor(host: &dyn Host, _: &Value) -> Result<Value, ToolError> {
    Ok(status::doctor(host, &status::snapshot(host)).to_value())
}

pub(super) fn flow_diagram(_: &dyn Host, _: &Value) -> Result<Value, ToolError> {
    Ok(json!({"markdown": FLOW}))
}

fn as_text(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

fn inventory_servers(host: &dyn Host) -> Result<String, ToolError> {
    let view = host
        .read_view()
        .map_err(|error| ToolError::new("registry_error", error))?;
    let servers = match view {
        Some(view) => view
            .servers()
            .map_err(|error| ToolError::new("registry_error", error))?,
        None => Vec::new(),
    };
    Ok(servers
        .iter()
        .map(|s| {
            format!(
                "{} [{}/{}]",
                s.name,
                s.text("transport").unwrap_or("stdio"),
                s.text("source").unwrap_or("unknown")
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

pub fn read_resource(host: &dyn Host, def: &ResourceDef) -> Result<String, ToolError> {
    match def.uri {
        "mcpm://paths" | "mcpm://status" => {
            Ok(as_text(status::status(host, &status::snapshot(host))))
        }
        "mcpm://flow" => Ok(FLOW.to_string()),
        "mcpm://inventory/servers" => inventory_servers(host),
        other => Err(ToolError::not_implemented(other)),
    }
}
