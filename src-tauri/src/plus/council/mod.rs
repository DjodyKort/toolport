//! Council as a downstream MCP server (MIG-CNC-1). The Python server stays an
//! external command; this module owns its registry definition and a static
//! manifest of what it exposes.

use crate::registry::{self, EnvVar, Registry, ServerEntry};
use serde_json::{json, Value};

pub const COUNCIL_ID: &str = "council";
pub const COUNCIL_SOURCE: &str = "plus:council";
pub const API_KEY_ENV: &str = "OPENROUTER_API_KEY";

const COMMAND: &str = "uvx";
const ARGS: &[&str] = &[
    "--no-project",
    "--from",
    "mcpm-council",
    "python",
    "-m",
    "mcpm_council",
];

pub struct ToolInfo {
    pub name: &'static str,
    pub tier: u8,
    pub summary: &'static str,
}

pub const TOOLS: &[ToolInfo] = &[
    ToolInfo {
        name: "council_models_list",
        tier: 1,
        summary: "Inspect the configured council members and chairman",
    },
    ToolInfo {
        name: "council_ask",
        tier: 1,
        summary: "Fan a question out to the members and synthesise a verdict",
    },
    ToolInfo {
        name: "council_review",
        tier: 2,
        summary: "Code-review framing with security, performance and maintainability roles",
    },
    ToolInfo {
        name: "council_config_set",
        tier: 3,
        summary: "Persist council defaults (refuses without confirm=true)",
    },
];

pub const RESOURCES: &[(&str, &str)] = &[
    (
        "council://config",
        "Wired-up members, chairman and key state",
    ),
    (
        "council://how-to-use",
        "Guidance on when to invoke a council",
    ),
];

pub fn server_entry() -> ServerEntry {
    ServerEntry {
        id: COUNCIL_ID.into(),
        name: COUNCIL_ID.into(),
        transport: "stdio".into(),
        command: Some(COMMAND.into()),
        args: ARGS.iter().map(|s| s.to_string()).collect(),
        launch: None,
        env: vec![EnvVar {
            key: API_KEY_ENV.into(),
            value: None,
            secret: true,
        }],
        url: None,
        cwd: None,
        source: Some(COUNCIL_SOURCE.into()),
        disabled_tools: Vec::new(),
        client_credentials: None,
        request_timeout_ms: Some(300_000),
        initialize_timeout_ms: Some(120_000),
        unknown_fields: Default::default(),
    }
}

pub fn find_entry(reg: &Registry) -> Option<&ServerEntry> {
    reg.servers
        .iter()
        .find(|s| s.source.as_deref() == Some(COUNCIL_SOURCE))
}

pub fn manifest() -> Value {
    json!({
        "tools": TOOLS.iter().map(|t| json!({
            "name": t.name, "tier": t.tier, "summary": t.summary,
        })).collect::<Vec<_>>(),
        "resources": RESOURCES.iter().map(|(uri, summary)| json!({
            "uri": uri, "summary": summary,
        })).collect::<Vec<_>>(),
    })
}

pub struct Installed {
    pub id: String,
    pub created: bool,
    pub key_stored: bool,
}

pub fn install(api_key: Option<&str>) -> Result<Installed, String> {
    let mut key_stored = false;
    let (_, (id, created)) = registry::update(|reg| {
        let (id, created) = match find_entry(reg).map(|e| e.id.clone()) {
            Some(id) => {
                let mut entry = server_entry();
                entry.id = id.clone();
                let current = reg.servers.iter().find(|s| s.id == id).cloned();
                if let Some(current) = current {
                    entry.disabled_tools = current.disabled_tools;
                    entry.cwd = current.cwd;
                }
                reg.update_server(entry)?;
                (id, false)
            }
            None => (reg.add_server(server_entry()), true),
        };
        let profile = reg.active_profile_id();
        reg.set_server_enabled(&profile, &id, true)?;
        if let Some(key) = api_key {
            let stored = crate::secrets::get_secret_result(&id, API_KEY_ENV)
                .ok()
                .flatten();
            if stored.as_deref() != Some(key) {
                crate::secrets::set_secret(&id, API_KEY_ENV, key)
                    .map_err(|e| format!("vault write failed for {id}::{API_KEY_ENV}: {e}"))?;
                reg.secrets_generation += 1;
            }
            key_stored = true;
        }
        Ok((id, created))
    })?;
    Ok(Installed {
        id,
        created,
        key_stored,
    })
}

pub fn uninstall(purge_key: bool) -> Result<Option<String>, String> {
    let (_, removed) = registry::update(|reg| {
        let Some(id) = find_entry(reg).map(|e| e.id.clone()) else {
            return Ok(None);
        };
        reg.remove_server(&id)?;
        if purge_key {
            crate::secrets::delete_secret(&id, API_KEY_ENV)
                .map_err(|e| format!("vault delete failed for {id}::{API_KEY_ENV}: {e}"))?;
            reg.secrets_generation += 1;
        }
        Ok(Some(id))
    })?;
    Ok(removed)
}

pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn on_path(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(command);
        candidate.is_file() || candidate.with_extension("exe").is_file()
    })
}

pub fn doctor(reg: Option<&Registry>) -> Vec<Check> {
    let entry = reg.and_then(find_entry);
    let mut checks = vec![Check {
        name: "registry_entry",
        ok: entry.is_some(),
        detail: entry
            .map(|e| e.id.clone())
            .unwrap_or_else(|| "run `toolportctl council install`".into()),
    }];
    let command = entry
        .and_then(|e| e.command.clone())
        .unwrap_or_else(|| COMMAND.into());
    checks.push(Check {
        name: "launcher_on_path",
        ok: on_path(&command),
        detail: command,
    });
    let enabled = match (reg, entry) {
        (Some(reg), Some(e)) => reg.is_enabled(&reg.active_profile_id(), &e.id),
        _ => false,
    };
    checks.push(Check {
        name: "enabled_in_active_profile",
        ok: enabled,
        detail: reg
            .map(|r| r.active_profile_id())
            .unwrap_or_else(|| "-".into()),
    });
    let declared = entry
        .map(|e| e.env.iter().any(|v| v.key == API_KEY_ENV && v.secret))
        .unwrap_or(false);
    checks.push(Check {
        name: "key_declared_secret",
        ok: declared,
        detail: API_KEY_ENV.into(),
    });
    let stored = entry
        .and_then(|e| crate::secrets::get_secret(&e.id, API_KEY_ENV))
        .is_some_and(|v| !v.is_empty());
    checks.push(Check {
        name: "key_in_vault",
        ok: stored,
        detail: match entry {
            Some(e) => format!("{}::{API_KEY_ENV}", e.id),
            None => format!("{COUNCIL_ID}::{API_KEY_ENV}"),
        },
    });
    checks
}

#[cfg(test)]
mod tests;
