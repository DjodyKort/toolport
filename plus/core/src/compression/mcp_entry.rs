//! The registry entry for the engine's own MCP server. mcpm pushed `headroom mcp serve` into
//! each client config; here it is one registry server, enabled in the active profile, so every
//! client reaches it through the gateway. Providers without an MCP server remove it.

use super::model::ProviderName;
use super::provider::mcp_server_config;
use serde_json::Value;
use toolport_plus_api::{Host, RegistryTx, RegistryView, ServerRecord, ServerSpec};

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

struct Wanted {
    name: String,
    command: Option<String>,
    args: Vec<String>,
}

fn desired() -> Wanted {
    let config = mcp_server_config(ProviderName::Headroom).unwrap_or(Value::Null);
    let text = |key: &str| config.get(key).and_then(Value::as_str).map(str::to_string);
    Wanted {
        name: text("name").unwrap_or_else(|| MCP_NAME.into()),
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
    }
}

/// An entry this provider created, or one imported under the same name and command.
pub fn find_entry(servers: &[ServerRecord]) -> Option<&ServerRecord> {
    let want = desired();
    servers.iter().find(|s| {
        s.text("source") == Some(MCP_SOURCE)
            || (s.name == want.name && s.text("command") == want.command.as_deref())
    })
}

fn profile_of(view: &dyn RegistryView) -> String {
    view.default_access()
        .or_else(|| view.access_sets().into_iter().next().map(|set| set.id))
        .unwrap_or_else(|| "default".to_string())
}

fn plan_register(view: &dyn RegistryView) -> Result<Registration, String> {
    let want = desired();
    let servers = view.servers()?;
    let profile = profile_of(view);
    Ok(match find_entry(&servers) {
        Some(current) => Registration {
            id: current.id.clone(),
            profile,
            created: false,
            changed: !current.enabled
                || current.text("command") != want.command.as_deref()
                || current.text("transport") != Some("stdio"),
        },
        None => Registration {
            id: String::new(),
            profile,
            created: true,
            changed: true,
        },
    })
}

fn apply_register(tx: &mut dyn RegistryTx) -> Result<Registration, String> {
    let want = desired();
    let servers = tx.servers()?;
    let profile = profile_of(tx);
    let spec = ServerSpec {
        name: want.name.clone(),
        transport: "stdio".into(),
        command: want.command.clone(),
        args: want.args.clone(),
        ..ServerSpec::default()
    };
    let (id, created) = match find_entry(&servers).cloned() {
        Some(current) => {
            tx.update_server(&current.id, &spec)?;
            (current.id, false)
        }
        None => {
            let command = want.command.clone().unwrap_or_default();
            let id = tx.add_stdio_server(&want.name, &command, MCP_SOURCE)?;
            tx.update_server(&id, &spec)?;
            (id, true)
        }
    };
    tx.set_server_enabled(&id, true)?;
    if let Some(default) = tx.default_access() {
        tx.set_access_server(&default, &id, true)?;
    }
    let changed = match servers.iter().find(|s| s.id == id) {
        Some(before) => {
            !before.enabled
                || before.text("command") != want.command.as_deref()
                || before.text("transport") != Some("stdio")
        }
        None => true,
    };
    Ok(Registration {
        id,
        profile,
        created,
        changed,
    })
}

/// The registry behind a host, written only when the entry actually changes.
pub struct HostRegistry<'a>(pub &'a dyn Host);

impl McpHost for HostRegistry<'_> {
    fn register(&mut self, dry_run: bool) -> Result<Registration, String> {
        let plan = match self.0.read_view()? {
            Some(view) => plan_register(view.as_ref())?,
            None => return Err("no registry to register the headroom server in".into()),
        };
        if dry_run || !plan.changed {
            return Ok(plan);
        }
        let mut done = None;
        self.0.transact(&mut |tx| {
            done = Some(apply_register(tx)?);
            Ok(())
        })?;
        done.ok_or_else(|| "registry edit did not run".to_string())
    }

    fn unregister(&mut self, dry_run: bool) -> Result<Option<String>, String> {
        let Some(view) = self.0.read_view()? else {
            return Ok(None);
        };
        let found = find_entry(&view.servers()?).map(|s| s.id.clone());
        drop(view);
        let Some(id) = found else {
            return Ok(None);
        };
        if !dry_run {
            self.0.transact(&mut |tx| tx.remove_server(&id))?;
        }
        Ok(Some(id))
    }
}
