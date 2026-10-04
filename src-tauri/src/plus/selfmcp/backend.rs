use super::catalog::{ResourceDef, ToolDef};
use super::ToolError;
use super::{content, docs};
use crate::plus::args::str_arg;
use crate::plus::profiles;
use crate::plus::registry_ro;
use crate::plus::skills::api;
use crate::plus::status;
use crate::registry::{self, Registry};
use serde_json::{json, Value};
use std::path::PathBuf;

const FLOW: &str = "# Data flow\n\n\
canonical skills repository -> transpilers -> per-client outputs\n\
registry (servers, profiles) -> gateway -> every client\n\
encrypted sync bundle <-> remote (push and pull)\n";

pub(super) fn ctl(path: &[&str]) -> Result<Value, ToolError> {
    let positional: Vec<String> = path.iter().map(|s| s.to_string()).collect();
    let (command, rest) = crate::plus::ctl::find_command(&positional)
        .ok_or_else(|| ToolError::new("internal", "ctl command missing"))?;
    (command.handler)(rest)
        .map(|out| out.data)
        .map_err(ToolError::from)
}

#[cfg(test)]
pub(super) use crate::plus::skills::api::TEST_REPO;

pub(super) fn skills_repo(args: &Value) -> Result<PathBuf, ToolError> {
    let start = str_arg(args, "repo_path").map(PathBuf::from);
    Ok(api::resolve_repo(start.as_deref())?)
}

pub(super) fn read_registry() -> Result<Registry, ToolError> {
    registry_ro::read().map_err(|e| ToolError::new("registry_error", e))
}

fn server_row(reg: &Registry, active: &str, s: &registry::ServerEntry) -> Value {
    json!({
        "id": s.id,
        "name": s.name,
        "transport": s.transport,
        "enabled": reg.is_enabled(active, &s.id),
        "source": s.source,
    })
}

pub fn run_tool(tool: &ToolDef, args: &Value) -> Result<Value, ToolError> {
    if let Some(run) = tool.run {
        return run(args);
    }
    if let Some(outcome) = content::run(tool.name, args) {
        return outcome;
    }
    match tool.name {
        "servers_list" => {
            let reg = read_registry()?;
            let active = reg.active_profile_id();
            Ok(json!({
                "activeProfile": active,
                "servers": reg.servers.iter().map(|s| server_row(&reg, &active, s)).collect::<Vec<_>>(),
            }))
        }
        "servers_get" => {
            let name = str_arg(args, "name").unwrap_or_default();
            let reg = read_registry()?;
            let active = reg.active_profile_id();
            let server = reg
                .servers
                .iter()
                .find(|s| s.name == name || s.id == name)
                .ok_or_else(|| ToolError::new("not_found", format!("server not found: {name}")))?;
            let mut row = server_row(&reg, &active, server);
            row["command"] = json!(server.command);
            row["args"] = json!(server.args);
            row["url"] = json!(server.url);
            row["cwd"] = json!(server.cwd);
            row["disabledTools"] = json!(server.disabled_tools);
            row["declareClientCapabilities"] = json!(server.declare_client_capabilities);
            row["forwardInstructions"] = json!(server.forward_instructions);
            row["env"] = json!(server
                .env
                .iter()
                .map(|e| json!({"key": e.key, "secret": e.secret}))
                .collect::<Vec<_>>());
            Ok(row)
        }
        "servers_list_profiles" => {
            let reg = read_registry()?;
            Ok(json!({
                "activeProfile": reg.active_profile_id(),
                "profiles": profiles::rows(&reg).iter().map(|p| json!({
                    "id": p.id,
                    "name": p.name,
                    "enabledServerIds": p.servers.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            }))
        }
        "clients_list" => Ok(json!({"clients": detected_clients()})),
        "where_am_i" => Ok(status::status(&status::snapshot())),
        "doctor" => Ok(status::doctor(&status::snapshot()).to_value()),
        "flow_diagram" => Ok(json!({"markdown": FLOW})),
        other => Err(ToolError::not_implemented(other)),
    }
}

fn detected_clients() -> Vec<Value> {
    crate::clients::detect_clients()
        .iter()
        .map(|c| json!({"id": c.id, "name": c.name, "appPresent": c.app_present, "gatewayInstalled": c.gateway_installed}))
        .collect()
}

fn as_text(value: Value) -> String {
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

pub fn read_resource(def: &ResourceDef) -> Result<String, ToolError> {
    match def.uri {
        "mcpm://paths" | "mcpm://status" => Ok(as_text(status::status(&status::snapshot()))),
        "mcpm://flow" => Ok(FLOW.to_string()),
        "mcpm://inventory/skills" => content::inventory("skills"),
        "mcpm://inventory/servers" => {
            let reg = read_registry()?;
            Ok(reg
                .servers
                .iter()
                .map(|s| {
                    format!(
                        "{} [{}/{}]",
                        s.name,
                        s.transport,
                        s.source.as_deref().unwrap_or("unknown")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "mcpm://inventory/agents" => content::inventory("agents"),
        "mcpm://inventory/styles" => content::inventory("styles"),
        "mcpm://clients" => Ok(as_text(json!({"clients": detected_clients()}))),
        "mcpm://architecture" => Ok(docs::ARCHITECTURE.to_string()),
        "mcpm://workflows" => Ok(docs::WORKFLOWS.to_string()),
        "mcpm://router/status" => Ok(as_text(docs::router_status())),
        other => Err(ToolError::not_implemented(other)),
    }
}
