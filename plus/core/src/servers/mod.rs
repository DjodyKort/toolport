//! Server lookup, the add and edit that `toolportctl server` and the `servers_*` tools share, and
//! the views they print. Add, edit and remove go through upstream's own operations (the adapter
//! implements them); what this layer adds is the plain-env rules, `inherit_env`, access-set
//! membership and the restart through the gateway.

pub mod env;
pub mod restart;

use serde_json::{json, Value};
use toolport_plus_api::{
    AccessSetInfo, CatalogEntryRecord, EnvEntry, Host, RegistryTx, RegistryView, ServerRecord,
    ServerSpec,
};

use self::env::{Change, Refusal};
use crate::op::OpError;

/// Why an add or an edit did not happen. Each surface words these its own way.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerError {
    NotFound(String),
    Exists,
    Refused(Refusal),
    /// Upstream's own validation or the registry refused it.
    Rejected(String),
}

/// A server by id, else by name ignoring case.
pub fn find<'a>(servers: &'a [ServerRecord], key: &str) -> Option<&'a ServerRecord> {
    servers
        .iter()
        .find(|s| s.id == key)
        .or_else(|| named(servers, key))
}

pub fn named<'a>(servers: &'a [ServerRecord], name: &str) -> Option<&'a ServerRecord> {
    servers.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// The value of an `on`/`off` switch given as text.
pub fn parse_switch(raw: &str) -> Result<bool, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(format!("'{raw}' is not on or off")),
    }
}

/// What a surface was asked to set. `None` keeps the base value; `args: Some(vec![])` clears them.
#[derive(Default, Clone)]
pub struct Patch {
    pub name: Option<String>,
    pub transport: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
    pub cwd: Option<String>,
    pub inherit_env: Option<bool>,
    pub declare_client_capabilities: Option<bool>,
    pub forward_instructions: Option<bool>,
}

/// Without an explicit transport, a given command means stdio and a given url means http (sse
/// stays sse); otherwise the base keeps its transport and a new server defaults to stdio.
pub fn spec_from(patch: &Patch, base: Option<&ServerRecord>) -> ServerSpec {
    let base_transport = base.and_then(|b| b.text("transport"));
    let inferred = match (&patch.command, &patch.url, base_transport) {
        (Some(_), _, _) => "stdio",
        (None, Some(_), Some(t @ ("http" | "sse"))) => t,
        (None, Some(_), _) => "http",
        (None, None, Some(t)) => t,
        (None, None, None) => "stdio",
    };
    let from_base = |key: &str| base.and_then(|b| b.text(key)).map(String::from);
    ServerSpec {
        name: patch
            .name
            .clone()
            .or_else(|| base.map(|b| b.name.clone()))
            .unwrap_or_default(),
        transport: patch
            .transport
            .clone()
            .unwrap_or_else(|| inferred.to_string()),
        command: patch.command.clone().or_else(|| from_base("command")),
        args: patch
            .args
            .clone()
            .or_else(|| base.map(|b| b.strings("args")))
            .unwrap_or_default(),
        url: patch.url.clone().or_else(|| from_base("url")),
        cwd: patch.cwd.clone().or_else(|| from_base("cwd")),
        inherit_env: patch.inherit_env,
    }
}

fn typed<T>(
    host: &dyn Host,
    mut run: impl FnMut(&mut dyn RegistryTx) -> Result<T, ServerError>,
) -> Result<T, ServerError> {
    let mut failure: Option<ServerError> = None;
    let mut done: Option<T> = None;
    let result = host.transact(&mut |tx| match run(tx) {
        Ok(value) => {
            done = Some(value);
            Ok(())
        }
        Err(error) => {
            let message = match &error {
                ServerError::Rejected(message) | ServerError::NotFound(message) => message.clone(),
                ServerError::Refused(refusal) => refusal.message.clone(),
                ServerError::Exists => String::new(),
            };
            failure = Some(error);
            Err(message)
        }
    });
    match (result, failure, done) {
        (_, Some(error), _) => Err(error),
        (Err(message), None, _) => Err(ServerError::Rejected(message)),
        (Ok(()), None, Some(value)) => Ok(value),
        (Ok(()), None, None) => Err(ServerError::Rejected("nothing was changed".into())),
    }
}

fn record_in(tx: &dyn RegistryView, server_id: &str) -> Result<ServerRecord, ServerError> {
    let servers = tx.servers().map_err(ServerError::Rejected)?;
    servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| ServerError::NotFound(format!("No server with id '{server_id}'")))
}

fn apply_env(
    host: &dyn Host,
    tx: &mut dyn RegistryTx,
    server_id: &str,
    transport: &str,
    changes: &[Change],
) -> Result<Vec<String>, ServerError> {
    let mut entries = record_in(tx, server_id)?
        .env()
        .map_err(ServerError::Rejected)?;
    let changed =
        env::apply(host, &mut entries, transport, changes).map_err(ServerError::Refused)?;
    if !changed.is_empty() {
        tx.set_server_env(server_id, entries)
            .map_err(ServerError::Rejected)?;
    }
    Ok(changed)
}

/// Refuses to write over a registry that cannot be read: the locked loader would restore it from
/// the backup instead of stopping.
pub fn require_readable(host: &dyn Host) -> Result<(), OpError> {
    host.read_view()
        .map(drop)
        .map_err(|error| OpError::failed("registry_error", error))
}

/// All servers, empty on a first run.
pub fn read_servers(host: &dyn Host) -> Result<Vec<ServerRecord>, OpError> {
    match host
        .read_view()
        .map_err(|error| OpError::failed("registry_error", error))?
    {
        Some(view) => view
            .servers()
            .map_err(|error| OpError::failed("registry_error", error)),
        None => Ok(Vec::new()),
    }
}

/// Adds a server with its env entries and fork switches in one registry write; the name is checked
/// under the registry lock, so two writers cannot both add it. Returns the new id.
pub fn add(host: &dyn Host, patch: &Patch, env_changes: &[Change]) -> Result<String, ServerError> {
    let spec = spec_from(patch, None);
    typed(host, |tx| {
        let exists = tx
            .servers()
            .map_err(ServerError::Rejected)?
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(spec.name.trim()));
        if exists {
            return Err(ServerError::Exists);
        }
        let id = tx.add_server(&spec).map_err(ServerError::Rejected)?;
        apply_env(host, tx, &id, &spec.transport, env_changes)?;
        apply_switches(tx, &id, patch)?;
        Ok(id)
    })
}

fn apply_switches(
    tx: &mut dyn RegistryTx,
    server_id: &str,
    patch: &Patch,
) -> Result<(), ServerError> {
    if let Some(on) = patch.forward_instructions {
        tx.set_forward_instructions(server_id, on)
            .map_err(ServerError::Rejected)?;
    }
    if let Some(on) = patch.declare_client_capabilities {
        tx.set_declare_client_capabilities(server_id, on)
            .map_err(ServerError::Rejected)?;
    }
    Ok(())
}

/// Commits the field update, the env edit and the fork switches together, so a refused env change
/// leaves the server as it was. Returns the env keys whose entry changed.
pub fn update(
    host: &dyn Host,
    server_id: &str,
    patch: &Patch,
    env_changes: &[Change],
) -> Result<Vec<String>, ServerError> {
    typed(host, |tx| {
        let base = record_in(tx, server_id)?;
        let spec = spec_from(patch, Some(&base));
        tx.update_server(server_id, &spec)
            .map_err(ServerError::Rejected)?;
        let changed = apply_env(host, tx, server_id, &spec.transport, env_changes)?;
        apply_switches(tx, server_id, patch)?;
        Ok(changed)
    })
}

/// Adds a curated catalog entry, switched off. Returns the new id.
pub fn add_catalog(host: &dyn Host, entry: &CatalogEntryRecord) -> Result<String, ServerError> {
    typed(host, |tx| {
        let exists = tx
            .servers()
            .map_err(ServerError::Rejected)?
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(entry.name.trim()));
        if exists {
            return Err(ServerError::Exists);
        }
        tx.add_catalog_server(entry).map_err(ServerError::Rejected)
    })
}

/// A curated catalog entry by name, ignoring case.
pub fn find_catalog(host: &dyn Host, name: &str) -> Option<CatalogEntryRecord> {
    host.curated(name)
        .into_iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(name))
}

pub struct UninstallArgs<'a> {
    pub key: &'a str,
    pub dry_run: bool,
    pub keep_clients: bool,
    pub keep_secrets: bool,
}

pub struct Uninstalled {
    pub id: String,
    pub name: String,
    pub dry_run: bool,
    /// Client entries cleaned up with the server. A client reaches every server through the one
    /// gateway entry, so there is nothing per server to remove; direct entries are listed here
    /// once the direct-entry slice brings them.
    pub clients: Vec<Value>,
    pub secrets_removed: Vec<String>,
}

impl Uninstalled {
    pub fn failed(&self) -> bool {
        false
    }

    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "dryRun": self.dry_run,
            "clients": self.clients,
            "secretsRemoved": self.secrets_removed,
        })
    }
}

pub fn uninstall(host: &dyn Host, args: &UninstallArgs) -> Result<Uninstalled, OpError> {
    let servers = read_servers(host)?;
    let server = find(&servers, args.key)
        .ok_or_else(|| OpError::not_found(format!("no server '{}'", args.key)))?
        .clone();
    let secret_keys: Vec<String> = server
        .env()
        .map_err(|error| OpError::failed("registry_error", error))?
        .into_iter()
        .filter(|entry| entry.secret)
        .map(|entry| entry.key)
        .collect();
    let mut secrets_removed = Vec::new();
    if !args.dry_run {
        host.transact(&mut |tx| tx.remove_server(&server.id))
            .map_err(|error| OpError::failed("uninstall", error))?;
        if !args.keep_secrets {
            for key in &secret_keys {
                if host.delete_server_secret(&server.id, key).is_ok() {
                    secrets_removed.push(key.clone());
                }
            }
        }
    }
    Ok(Uninstalled {
        id: server.id,
        name: server.name,
        dry_run: args.dry_run,
        clients: Vec::new(),
        secrets_removed,
    })
}

// ---- access sets (what 1.x called profiles) ----

/// An access set by id, else by name ignoring case.
pub fn find_access_set<'a>(sets: &'a [AccessSetInfo], key: &str) -> Option<&'a AccessSetInfo> {
    sets.iter().find(|set| set.id == key).or_else(|| {
        sets.iter()
            .find(|set| set.name.eq_ignore_ascii_case(key.trim()))
    })
}

/// Ids of the access sets that list the server, in the registry's order. A set lists a server
/// whether or not the server is switched on; the global switch decides whether it is reachable.
pub fn access_sets_of(sets: &[AccessSetInfo], server_id: &str) -> Vec<String> {
    sets.iter()
        .filter(|set| set.server_ids.iter().any(|id| id == server_id))
        .map(|set| set.id.clone())
        .collect()
}

/// Lists or unlists a server in an access set. Returns the ids of the sets listing it afterwards.
pub fn set_member(
    host: &dyn Host,
    server_id: &str,
    set_key: &str,
    included: bool,
) -> Result<Vec<String>, ServerError> {
    typed(host, |tx| {
        let sets = tx.access_sets();
        let set = find_access_set(&sets, set_key)
            .ok_or_else(|| ServerError::NotFound(format!("access set not found: {set_key}")))?
            .id
            .clone();
        tx.set_access_server(&set, server_id, included)
            .map_err(ServerError::Rejected)?;
        Ok(access_sets_of(&tx.access_sets(), server_id))
    })
}

// ---- views ----

pub fn list_value(view: Option<&dyn RegistryView>, with_source: bool) -> Result<Value, String> {
    let (servers, default_access) = match view {
        Some(view) => (view.servers()?, view.default_access()),
        None => (Vec::new(), None),
    };
    let rows: Vec<Value> = servers
        .iter()
        .map(|s| {
            let mut row = json!({
                "id": s.id,
                "name": s.name,
                "transport": s.text("transport").unwrap_or("stdio"),
                "enabled": s.enabled,
            });
            if with_source {
                row["source"] = json!(s.text("source"));
            }
            row
        })
        .collect();
    Ok(json!({"defaultAccess": default_access, "servers": rows}))
}

/// One server without any secret value, and the access sets that list it.
pub fn info_value(server: &ServerRecord, access_sets: &[String]) -> Result<Value, String> {
    let env: Vec<Value> = server
        .env()?
        .iter()
        .map(|e| json!({"key": e.key, "secret": e.secret}))
        .collect();
    Ok(json!({
        "id": server.id,
        "name": server.name,
        "enabled": server.enabled,
        "transport": server.text("transport").unwrap_or("stdio"),
        "command": server.text("command"),
        "args": server.strings("args"),
        "url": server.text("url"),
        "cwd": server.text("cwd"),
        "source": server.text("source"),
        "env": env,
        "inheritEnv": server.switch("inheritEnv"),
        "disabledTools": server.strings("disabledTools"),
        "declareClientCapabilities": server.fork.declare_client_capabilities.unwrap_or(false),
        "forwardInstructions": server.fork.forward_instructions.unwrap_or(false),
        "accessSets": access_sets,
    }))
}

/// What a stdio server's process sees of the shell's environment, said in words.
pub fn child_environment(server: &ServerRecord) -> &'static str {
    if server.text("transport").unwrap_or("stdio") != "stdio" {
        "not a local process"
    } else if server.switch("inheritEnv") {
        "the whole shell environment, plus its own entries"
    } else {
        "only a fixed set (PATH, HOME, locale, certificates), plus its own entries"
    }
}

pub fn env_entries(server: &ServerRecord) -> Vec<EnvEntry> {
    server.env().unwrap_or_default()
}

#[cfg(test)]
mod tests;
