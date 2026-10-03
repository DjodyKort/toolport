use super::catalog::{ResourceDef, ToolDef};
use super::ToolError;
use super::{content, docs, servers};
use crate::plus::args::{list, str_arg};
use crate::plus::registry_ro;
use crate::plus::skills::lint::{lint_skills, LintResult};
use crate::plus::skills::ops::find_skills_repo;
use crate::plus::skills::parser::{discover_skills, Skill};
use crate::plus::skills::transpiler::TranspilerRegistry;
use crate::plus::skills::transpilers;
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
        .map_err(|e| {
            let kind = match e.code.as_str() {
                "not_found" => "not_found",
                "conflict" => "conflict",
                "usage" => "invalid_arguments",
                _ => "backend_error",
            };
            ToolError::new(kind, e.message)
        })
}

#[cfg(test)]
thread_local! {
    pub(super) static TEST_REPO: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn skills_repo(args: &Value) -> Result<PathBuf, ToolError> {
    let start = str_arg(args, "repo_path").map(PathBuf::from);
    #[cfg(test)]
    if start.is_none() {
        if let Some(repo) = TEST_REPO.with(|r| r.borrow().clone()) {
            return Ok(repo);
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let config = registry::conduit_dir().unwrap_or_else(|| cwd.clone());
    find_skills_repo(start.as_deref(), &cwd, &config)
        .ok_or_else(|| ToolError::new("not_found", "no skills repository found"))
}

fn load_skills(args: &Value) -> Result<(PathBuf, Vec<Skill>), ToolError> {
    let repo = skills_repo(args)?;
    let skills = discover_skills(&repo);
    Ok((repo, skills))
}

pub(super) fn skill_row(skill: &Skill) -> Value {
    json!({
        "name": skill.name(),
        "description": skill.frontmatter.description,
        "activation": skill.frontmatter.activation.as_str(),
        "type": format!("{:?}", skill.skill_type).to_lowercase(),
        "path": skill.source_path.to_string_lossy(),
    })
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

fn transpiler_keys() -> Vec<String> {
    let mut reg = TranspilerRegistry::new();
    transpilers::register_all(&mut reg);
    reg.all().map(|t| t.client_key().to_string()).collect()
}

pub(super) fn lint_value(result: &LintResult) -> Value {
    json!({
        "errors": result.errors().count(),
        "warnings": result.warnings().count(),
        "messages": result.messages.iter().map(|m| json!({"level": m.level, "name": m.name, "message": m.message})).collect::<Vec<_>>(),
    })
}

pub fn run_tool(tool: &ToolDef, args: &Value) -> Result<Value, ToolError> {
    if let Some(outcome) = content::run(tool.name, args).or_else(|| servers::run(tool.name, args)) {
        return outcome;
    }
    match tool.name {
        "skills_list" => {
            let (repo, skills) = load_skills(args)?;
            Ok(
                json!({"repo": repo.to_string_lossy(), "skills": skills.iter().map(skill_row).collect::<Vec<_>>()}),
            )
        }
        "skills_get" => {
            let name = str_arg(args, "name").unwrap_or_default();
            let (_, skills) = load_skills(args)?;
            let skill = skills
                .iter()
                .find(|s| s.name() == name)
                .ok_or_else(|| ToolError::new("not_found", format!("skill not found: {name}")))?;
            let mut row = skill_row(skill);
            row["body"] = json!(skill.body);
            Ok(row)
        }
        "skills_lint" => {
            let (_, mut skills) = load_skills(args)?;
            if let Some(names) = list(args, "names") {
                skills.retain(|s| names.iter().any(|n| n.as_str() == Some(s.name())));
            }
            Ok(lint_value(&lint_skills(&skills)))
        }
        "skills_list_transpilers" => Ok(json!({"transpilers": transpiler_keys()})),
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
                "profiles": reg.profiles.iter().map(|p| json!({"id": p.id, "name": p.name, "enabledServerIds": p.enabled_server_ids})).collect::<Vec<_>>(),
            }))
        }
        "clients_list" => Ok(json!({"clients": detected_clients()})),
        "where_am_i" => ctl(&["status"]),
        "doctor" => ctl(&["doctor"]),
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

fn skills_inventory() -> Result<String, ToolError> {
    match load_skills(&json!({})) {
        Ok((_, skills)) => Ok(skills
            .iter()
            .map(|s| format!("{} - {}", s.name(), s.frontmatter.description))
            .collect::<Vec<_>>()
            .join("\n")),
        Err(e) if e.kind == "not_found" => Ok(String::new()),
        Err(e) => Err(e),
    }
}

pub fn read_resource(def: &ResourceDef) -> Result<String, ToolError> {
    match def.uri {
        "mcpm://paths" | "mcpm://status" => ctl(&["status"]).map(as_text),
        "mcpm://flow" => Ok(FLOW.to_string()),
        "mcpm://inventory/skills" => skills_inventory(),
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
