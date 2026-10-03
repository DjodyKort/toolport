//! The registry entry for the engine's own MCP server. mcpm pushed `headroom mcp serve` into
//! each client config; here it is one registry server, enabled in the active profile, so every
//! client reaches it through the gateway. Providers without an MCP server remove it.

use super::model::ProviderName;
use super::provider::mcp_server_config;
use crate::plus::registry_ro;
use crate::registry::{self, Registry, ServerEntry};
use serde_json::Value;

pub const MCP_NAME: &str = "headroom";
pub const MCP_SOURCE: &str = "plus:compression";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub id: String,
    pub profile: String,
    pub created: bool,
    pub changed: bool,
}

/// Where the MCP entry lives. `dry_run` reports what would change and writes nothing.
pub trait McpHost {
    fn register(&mut self, dry_run: bool) -> Result<Registration, String>;
    /// The id of the entry that was (or would be) removed.
    fn unregister(&mut self, dry_run: bool) -> Result<Option<String>, String>;
}

fn desired() -> ServerEntry {
    let config = mcp_server_config(ProviderName::Headroom).unwrap_or(Value::Null);
    let text = |key: &str| config.get(key).and_then(Value::as_str).map(str::to_string);
    ServerEntry {
        id: String::new(),
        name: text("name").unwrap_or_else(|| MCP_NAME.into()),
        transport: "stdio".into(),
        command: text("command"),
        args: config
            .get("args")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        launch: None,
        env: Vec::new(),
        url: None,
        cwd: None,
        source: Some(MCP_SOURCE.into()),
        disabled_tools: Vec::new(),
        client_credentials: None,
        request_timeout_ms: None,
        initialize_timeout_ms: None,
        unknown_fields: Default::default(),
    }
}

/// An entry this provider created, or one imported under the same name and command.
pub fn find_entry(reg: &Registry) -> Option<&ServerEntry> {
    let want = desired();
    reg.servers.iter().find(|s| {
        s.source.as_deref() == Some(MCP_SOURCE)
            || (s.name == want.name && s.command == want.command)
    })
}

pub fn apply_register(reg: &mut Registry) -> Result<Registration, String> {
    let want = desired();
    let profile = reg.active_profile_id();
    let (id, created, mut changed) = match find_entry(reg).cloned() {
        Some(current) => {
            let mut entry = current.clone();
            entry.transport = want.transport;
            entry.command = want.command;
            entry.args = want.args;
            entry.url = None;
            entry.source = current.source.clone().or(want.source);
            let changed = entry != current;
            reg.update_server(entry)?;
            (current.id, false, changed)
        }
        None => (reg.add_server(want), true, true),
    };
    if !reg.is_enabled(&profile, &id) {
        reg.set_server_enabled(&profile, &id, true)?;
        changed = true;
    }
    Ok(Registration {
        id,
        profile,
        created,
        changed,
    })
}

pub fn apply_unregister(reg: &mut Registry) -> Result<Option<String>, String> {
    let Some(id) = find_entry(reg).map(|e| e.id.clone()) else {
        return Ok(None);
    };
    reg.remove_server(&id)?;
    Ok(Some(id))
}

/// The real registry, locked and written only when the entry actually changes.
pub struct RegistryHost;

impl McpHost for RegistryHost {
    fn register(&mut self, dry_run: bool) -> Result<Registration, String> {
        let mut preview = registry_ro::read()?;
        let plan = apply_register(&mut preview)?;
        if dry_run || !plan.changed {
            return Ok(plan);
        }
        let (_, done) = registry::update(apply_register)?;
        Ok(done)
    }

    fn unregister(&mut self, dry_run: bool) -> Result<Option<String>, String> {
        let mut preview = registry_ro::read()?;
        let plan = apply_unregister(&mut preview)?;
        if dry_run || plan.is_none() {
            return Ok(plan);
        }
        let (_, done) = registry::update(apply_unregister)?;
        Ok(done)
    }
}
