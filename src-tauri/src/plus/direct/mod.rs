//! Opt-in direct client entries (D-055). A client normally reaches every server through the one
//! `toolport` gateway entry; `client direct add` additionally gives it one server as its own entry
//! whose command is the stdio launcher `toolportctl direct run <server id>`. The client file never
//! holds the server's command, env or a secret: the launcher resolves them from the registry and
//! the secret store when the client starts it. The registry records what was written
//! (`record.rs`), which is how `client sync`, `client ls`, `server uninstall`, `status` and
//! `doctor` tell a Toolport launcher entry from one the user wrote. The ctl commands, the
//! `plus.client.direct*` handlers and the selfmcp tools are thin adapters over this module.

pub mod handlers;
pub mod launcher;
pub mod record;

#[cfg(test)]
mod tests;

use crate::clients::{self, DetectedClient, McpServer};
use crate::plus::profiles::{registry_error, Error};
use crate::plus::{registry_ro, servers};
use crate::registry::{self, Registry, ServerEntry};
use record::Record;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const TRADEOFF: &str = "A direct entry bypasses the Toolport gateway: its tools are not \
     covered by profile tool scopes, approvals (HITL), receipts or lazy discovery, and every \
     client with a direct entry starts its own process of the server instead of sharing one.";

const ENTRY_SOURCE: &str = "plus:direct";
const BINARY_STEM: &str = "toolportctl";

fn binary_file() -> String {
    format!("{BINARY_STEM}{}", std::env::consts::EXE_SUFFIX)
}

#[cfg(test)]
thread_local! {
    static LAUNCHER_OVERRIDE: std::cell::RefCell<Option<Option<String>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[must_use = "the override is reverted when the guard drops, so it must be bound"]
pub(crate) struct LauncherOverride;

#[cfg(test)]
impl LauncherOverride {
    pub(crate) fn set(path: Option<&str>) -> Self {
        LAUNCHER_OVERRIDE.with(|o| *o.borrow_mut() = Some(path.map(String::from)));
        Self
    }
}

#[cfg(test)]
impl Drop for LauncherOverride {
    fn drop(&mut self) {
        LAUNCHER_OVERRIDE.with(|o| *o.borrow_mut() = None);
    }
}

/// The `toolportctl` a client entry should run: this process when it is `toolportctl`, else the
/// one next to it (the gateway and selfmcp ship in the same directory), in the data directory's
/// `bin`, or on PATH. `None` when no installed copy can be found.
pub fn launcher_path() -> Option<String> {
    #[cfg(test)]
    if let Some(forced) = LAUNCHER_OVERRIDE.with(|o| o.borrow().clone()) {
        return forced;
    }
    let file = binary_file();
    let exe = std::env::current_exe().ok();
    if let Some(exe) = &exe {
        let stem = exe.file_stem().and_then(|s| s.to_str());
        if stem == Some(BINARY_STEM) {
            return Some(exe.to_string_lossy().into_owned());
        }
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    dirs.extend(exe.as_ref().and_then(|e| e.parent()).map(PathBuf::from));
    dirs.extend(registry::conduit_dir().map(|d| d.join("bin")));
    dirs.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    dirs.into_iter()
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
        .map(|found| found.to_string_lossy().into_owned())
}

fn launcher_env() -> BTreeMap<String, String> {
    [
        ("TOOLPORT_DATA_DIR", "CONDUIT_DATA_DIR"),
        ("TOOLPORT_REGISTRY", "CONDUIT_REGISTRY"),
    ]
    .into_iter()
    .filter_map(|(new, legacy)| {
        crate::brand::env_var(new, legacy).map(|value| (new.to_string(), value))
    })
    .collect()
}

fn wanted_record(server: &ServerEntry, command: &str) -> Record {
    Record {
        server: server.id.clone(),
        command: command.to_string(),
        args: vec!["direct".into(), "run".into(), server.id.clone()],
        env: launcher_env(),
    }
}

fn client_entry(name: &str, record: &Record) -> ServerEntry {
    serde_json::from_value(json!({
        "id": name,
        "name": name,
        "transport": "stdio",
        "command": record.command,
        "args": record.args,
        "env": record
            .env
            .iter()
            .map(|(key, value)| json!({"key": key, "value": value, "secret": false}))
            .collect::<Vec<_>>(),
        "source": ENTRY_SOURCE,
    }))
    .expect("a launcher entry is a valid server entry")
}

fn basename_stem(command: &str) -> String {
    let base = command
        .trim()
        .trim_matches('"')
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

/// The server id a client entry launches when it has the shape of a Toolport launcher.
fn launcher_target(entry: &McpServer) -> Option<&str> {
    let command = entry.command.as_deref()?;
    match entry.args.as_slice() {
        [direct, run, id]
            if direct == "direct" && run == "run" && basename_stem(command) == BINARY_STEM =>
        {
            Some(id)
        }
        _ => None,
    }
}

fn matches_record(entry: &McpServer, record: &Record) -> bool {
    let mut keys = entry.env_keys.clone();
    keys.sort();
    entry.command.as_deref() == Some(record.command.as_str())
        && entry.args == record.args
        && keys.iter().eq(record.env.keys())
}

fn encrypted_secrets() -> bool {
    crate::brand::env_var("TOOLPORT_SECRET_KEY", "CONDUIT_SECRET_KEY").is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Added,
    Updated,
    Adopted,
    Unchanged,
    Removed,
    Forgotten,
    Absent,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Added => "added",
            Action::Updated => "updated",
            Action::Adopted => "adopted",
            Action::Unchanged => "unchanged",
            Action::Removed => "removed",
            Action::Forgotten => "forgotten",
            Action::Absent => "absent",
        }
    }

    fn writes_client(self) -> bool {
        matches!(self, Action::Added | Action::Updated | Action::Removed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub client: String,
    pub client_name: String,
    pub server: String,
    pub server_name: String,
    pub entry: String,
    pub path: String,
    pub action: Action,
    pub dry_run: bool,
    pub backup: Option<String>,
    pub launcher: Option<Record>,
    pub notes: Vec<String>,
}

impl Outcome {
    pub fn changed(&self) -> bool {
        !matches!(self.action, Action::Unchanged | Action::Absent)
    }

    fn adds(&self) -> bool {
        self.launcher.is_some()
    }

    pub fn to_value(&self) -> Value {
        let mut value = json!({
            "client": self.client,
            "clientName": self.client_name,
            "server": self.server,
            "serverName": self.server_name,
            "entry": self.entry,
            "path": self.path,
            "action": self.action.as_str(),
            "changed": self.changed(),
            "dryRun": self.dry_run,
            "backup": self.backup,
            "notes": self.notes,
        });
        if let Some(launcher) = &self.launcher {
            value["launcher"] = json!({
                "command": launcher.command,
                "args": launcher.args,
                "env": launcher.env,
            });
            value["tradeoff"] = json!(TRADEOFF);
        }
        value
    }

    pub fn text(&self) -> String {
        let (name, entry, client) = (&self.server_name, &self.entry, &self.client_name);
        let would = self.dry_run;
        let headline = match self.action {
            Action::Added => format!(
                "{} direct entry '{entry}' for {name} to {client}",
                if would { "Would add" } else { "Added" }
            ),
            Action::Updated => format!(
                "{} direct entry '{entry}' for {name} in {client}",
                if would { "Would update" } else { "Updated" }
            ),
            Action::Adopted => format!(
                "{} direct entry '{entry}' for {name} in {client} as a Toolport launcher",
                if would { "Would record" } else { "Recorded" }
            ),
            Action::Unchanged => {
                format!("{client} already has the direct entry '{entry}' for {name}")
            }
            Action::Removed => format!(
                "{} direct entry '{entry}' for {name} from {client}",
                if would { "Would remove" } else { "Removed" }
            ),
            Action::Forgotten => format!(
                "{client} no longer has the entry '{entry}'; {} the record of it",
                if would { "would drop" } else { "dropped" }
            ),
            Action::Absent => format!("{client} has no direct entry '{entry}' for {name}"),
        };
        let mut lines = vec![headline];
        if self.action != Action::Absent {
            lines.push(format!("Config file: {}", self.path));
        }
        if let Some(launcher) = &self.launcher {
            lines.push(format!(
                "Launches: {} {}",
                launcher.command,
                launcher.args.join(" ")
            ));
        }
        if let Some(backup) = &self.backup {
            lines.push(format!("Backup: {backup}"));
        }
        if self.adds() {
            lines.push(TRADEOFF.to_string());
        }
        lines.extend(self.notes.iter().cloned());
        if would && self.action != Action::Absent && self.action != Action::Unchanged {
            lines.push("Dry run: nothing was written.".to_string());
        } else if !would && self.action.writes_client() {
            lines.push(format!("Restart {client} for the change to take effect."));
        }
        lines.join("\n")
    }
}

fn find_client(
    detected: Vec<DetectedClient>,
    client_id: &str,
    require_installed: bool,
) -> Result<DetectedClient, Error> {
    let Some(at) = detected.iter().position(|c| c.id == client_id) else {
        let known: Vec<&str> = detected.iter().map(|c| c.id.as_str()).collect();
        return Err(Error::not_found(format!(
            "Client '{client_id}' is not supported.\nAvailable clients: {}",
            known.join(", ")
        )));
    };
    let client = detected.into_iter().nth(at).expect("position is in range");
    if require_installed && !client.config_exists && !client.app_present {
        return Err(Error::not_found(format!(
            "{} installation not detected.",
            client.name
        )));
    }
    if let Some(error) = &client.error {
        return Err(Error::failed(
            "client_config",
            format!("{}: {error}", client.name),
        ));
    }
    Ok(client)
}

fn on_disk<'a>(client: &'a DetectedClient, entry: &str) -> Option<&'a McpServer> {
    client.servers.iter().find(|s| s.name == entry)
}

fn plan_add(
    entry: &str,
    client_name: &str,
    existing: Option<&McpServer>,
    recorded: Option<&Record>,
    wanted: &Record,
    force: bool,
) -> Result<Action, Error> {
    let Some(existing) = existing else {
        return Ok(Action::Added);
    };
    if matches_record(existing, wanted) {
        return Ok(if recorded == Some(wanted) {
            Action::Unchanged
        } else {
            Action::Adopted
        });
    }
    let ours = launcher_target(existing) == Some(wanted.server.as_str())
        || recorded.is_some_and(|r| r.server == wanted.server && matches_record(existing, r));
    if ours || force {
        return Ok(Action::Updated);
    }
    let owned = recorded.is_some_and(|r| r.server == wanted.server);
    Err(Error::conflict(if owned {
        format!(
            "{client_name}'s entry '{entry}' was changed after Toolport wrote it; \
             use --force to replace it"
        )
    } else {
        format!(
            "{client_name} already has an entry named '{entry}' that Toolport did not write; \
             remove it first or use --force to replace it"
        )
    }))
}

fn refuse(server: &ServerEntry) -> Result<(), Error> {
    match launcher::refusal(server) {
        Some(reason) => Err(Error::failed(
            "refused",
            format!(
                "{} cannot get a direct entry: {reason}. It stays reachable through the \
                 gateway.",
                server.name
            ),
        )),
        None if server.name.trim().is_empty() => Err(Error::failed(
            "refused",
            format!("server '{}' has no name to give the entry", server.id),
        )),
        None => Ok(()),
    }
}

fn write_record(client: &str, entry: &str, record: Option<&Record>) -> Result<(), Error> {
    registry::update(|reg| {
        match record {
            Some(record) => record::set(reg, client, entry, record),
            None => {
                record::clear(reg, client, entry);
            }
        }
        Ok(())
    })
    .map(|_| ())
    .map_err(registry_error)
}

fn write_failed(error: String) -> Error {
    Error::failed("write_failed", error)
}

pub fn add(
    server_key: &str,
    client_id: &str,
    force: bool,
    dry_run: bool,
) -> Result<Outcome, Error> {
    let reg = registry_ro::read().map_err(registry_error)?;
    let server = servers::find(&reg, server_key.trim())
        .ok_or_else(|| Error::not_found(format!("Server '{server_key}' not found")))?
        .clone();
    refuse(&server)?;
    let command = launcher_path().ok_or_else(|| {
        Error::failed(
            "launcher_missing",
            format!(
                "could not find {} next to this program, in the data directory's bin or on \
                 PATH; a direct entry needs it to start the server",
                binary_file()
            ),
        )
    })?;
    let client = find_client(clients::detect_clients(), client_id, true)?;
    let entry = server.name.trim().to_string();
    let wanted = wanted_record(&server, &command);
    let recorded = record::get(&reg, client_id, &entry);
    let action = plan_add(
        &entry,
        &client.name,
        on_disk(&client, &entry),
        recorded.as_ref(),
        &wanted,
        force,
    )?;
    let mut notes = Vec::new();
    if encrypted_secrets() && server.env.iter().any(|e| e.secret) {
        notes.push(
            "Secrets come from the encrypted secret file: the client must start the entry with \
             TOOLPORT_SECRET_KEY in its environment (it is never written to the client config)."
                .to_string(),
        );
    }
    let mut outcome = Outcome {
        client: client.id.clone(),
        client_name: client.name.clone(),
        server: server.id.clone(),
        server_name: server.name.clone(),
        entry: entry.clone(),
        path: client.config_path.clone(),
        action,
        dry_run,
        backup: None,
        launcher: Some(wanted.clone()),
        notes,
    };
    if dry_run {
        return Ok(outcome);
    }
    if action.writes_client() {
        let written =
            clients::set_direct_entry(client_id, &entry, Some(&client_entry(&entry, &wanted)))
                .map_err(write_failed)?;
        outcome.path = written.path;
        outcome.backup = written.backup;
    }
    if recorded.as_ref() != Some(&wanted) {
        write_record(client_id, &entry, Some(&wanted))?;
    }
    Ok(outcome)
}

fn remove_in(
    client: &DetectedClient,
    entry: &str,
    server: Option<&ServerEntry>,
    recorded: Option<&Record>,
    force: bool,
    dry_run: bool,
) -> Result<Outcome, Error> {
    let server_id = server
        .map(|s| s.id.clone())
        .or_else(|| recorded.map(|r| r.server.clone()))
        .unwrap_or_default();
    let mut outcome = Outcome {
        client: client.id.clone(),
        client_name: client.name.clone(),
        server_name: server.map_or_else(|| server_id.clone(), |s| s.name.clone()),
        server: server_id.clone(),
        entry: entry.to_string(),
        path: client.config_path.clone(),
        action: Action::Absent,
        dry_run,
        backup: None,
        launcher: None,
        notes: Vec::new(),
    };
    let Some(existing) = on_disk(client, entry) else {
        if recorded.is_some() {
            outcome.action = Action::Forgotten;
            if !dry_run {
                write_record(&client.id, entry, None)?;
            }
        }
        return Ok(outcome);
    };
    let ours = launcher_target(existing) == Some(server_id.as_str())
        || recorded.is_some_and(|r| matches_record(existing, r));
    if !ours && !force {
        return Err(Error::conflict(if recorded.is_some() {
            format!(
                "{}'s entry '{entry}' was changed after Toolport wrote it; use --force to \
                 remove it",
                client.name
            )
        } else {
            format!(
                "{}'s entry '{entry}' is not a Toolport launcher entry; Toolport only removes \
                 entries it wrote (--force overrides that)",
                client.name
            )
        }));
    }
    outcome.action = Action::Removed;
    if dry_run {
        return Ok(outcome);
    }
    let written = clients::set_direct_entry(&client.id, entry, None).map_err(write_failed)?;
    outcome.path = written.path;
    outcome.backup = written.backup;
    if recorded.is_some() {
        write_record(&client.id, entry, None)?;
    }
    Ok(outcome)
}

pub fn remove(
    server_key: &str,
    client_id: &str,
    force: bool,
    dry_run: bool,
) -> Result<Outcome, Error> {
    let reg = registry_ro::read().map_err(registry_error)?;
    let server = servers::find(&reg, server_key.trim());
    let key = server_key.trim();
    let hit = record::all(&reg)
        .into_iter()
        .find(|(client, entry, record)| {
            client == client_id
                && (server.is_some_and(|s| record.server == s.id)
                    || entry.eq_ignore_ascii_case(key)
                    || record.server == key)
        });
    if server.is_none() && hit.is_none() {
        return Err(Error::not_found(format!("Server '{server_key}' not found")));
    }
    let client = find_client(clients::detect_clients(), client_id, false)?;
    let entry = match (&hit, server) {
        (Some((_, entry, _)), _) => entry.clone(),
        (None, Some(server)) => server.name.trim().to_string(),
        (None, None) => unreachable!("one of the two exists"),
    };
    let recorded = hit.map(|(_, _, record)| record);
    let server = server.or_else(|| {
        recorded
            .as_ref()
            .and_then(|r| servers::find(&reg, &r.server))
    });
    remove_in(&client, &entry, server, recorded.as_ref(), force, dry_run)
}

/// What `server uninstall` and `client sync` need: remove the launcher entries recorded for
/// one server, or for every server the registry no longer has, in every client. Unrecorded
/// launcher entries of the uninstalled server go too, so none is left pointing at nothing.
pub fn remove_recorded(
    which: Removing<'_>,
    dry_run: bool,
) -> Vec<(String, Result<Outcome, Error>)> {
    let reg = match registry_ro::read() {
        Ok(reg) => reg,
        Err(error) => return vec![(String::new(), Err(registry_error(error)))],
    };
    let detected = clients::detect_clients();
    let mut candidates: Vec<(String, String, Option<Record>)> = record::all(&reg)
        .into_iter()
        .filter(|(client, _, record)| match which {
            Removing::Server(id) => record.server == id,
            Removing::Orphans(only) => {
                servers::find(&reg, &record.server).is_none()
                    && only.map_or(true, |only| only == client)
            }
        })
        .map(|(client, entry, record)| (client, entry, Some(record)))
        .collect();
    if let Removing::Server(id) = which {
        for client in &detected {
            for entry in &client.servers {
                let known = candidates
                    .iter()
                    .any(|(c, e, _)| *c == client.id && *e == entry.name);
                if !known
                    && launcher_target(entry) == Some(id)
                    && record::get(&reg, &client.id, &entry.name).is_none()
                {
                    candidates.push((client.id.clone(), entry.name.clone(), None));
                }
            }
        }
    }
    let mut results = Vec::new();
    for (client_id, entry, record) in candidates {
        let server = record
            .as_ref()
            .and_then(|r| servers::find(&reg, &r.server))
            .or_else(|| match which {
                Removing::Server(id) => servers::find(&reg, id),
                Removing::Orphans(_) => None,
            });
        let result = match detected.iter().find(|c| c.id == client_id) {
            Some(client) => remove_in(client, &entry, server, record.as_ref(), false, dry_run),
            None => {
                let record = record.expect("a client that is gone only has recorded entries");
                if dry_run {
                    Ok(forgotten(&client_id, &entry, &record))
                } else {
                    write_record(&client_id, &entry, None)
                        .map(|()| forgotten(&client_id, &entry, &record))
                }
            }
        };
        results.push((client_id, result));
    }
    results
}

pub enum Removing<'a> {
    Server(&'a str),
    Orphans(Option<&'a str>),
}

fn forgotten(client: &str, entry: &str, record: &Record) -> Outcome {
    Outcome {
        client: client.to_string(),
        client_name: client.to_string(),
        server: record.server.clone(),
        server_name: record.server.clone(),
        entry: entry.to_string(),
        path: String::new(),
        action: Action::Forgotten,
        dry_run: false,
        backup: None,
        launcher: None,
        notes: Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ok,
    Stale,
    Customized,
    Missing,
    Orphan,
    Unrecorded,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Ok => "ok",
            State::Stale => "stale",
            State::Customized => "customized",
            State::Missing => "missing",
            State::Orphan => "orphan",
            State::Unrecorded => "unrecorded",
        }
    }

    pub fn healthy(self) -> bool {
        self == State::Ok
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub client: String,
    pub client_name: String,
    pub entry: String,
    pub server: String,
    pub server_name: Option<String>,
    pub state: State,
    pub path: String,
}

impl Row {
    pub fn to_value(&self) -> Value {
        json!({
            "client": self.client,
            "clientName": self.client_name,
            "entry": self.entry,
            "server": self.server,
            "serverName": self.server_name,
            "state": self.state.as_str(),
            "path": self.path,
        })
    }

    pub fn line(&self) -> String {
        let server = match &self.server_name {
            Some(name) if *name != self.entry => format!("{} ({name})", self.server),
            _ => self.server.clone(),
        };
        format!(
            "{:<16} {:<24} {:<11} {server}",
            self.client,
            self.entry,
            self.state.as_str()
        )
    }
}

fn state_of(
    client: Option<&DetectedClient>,
    entry: &str,
    record: &Record,
    server_exists: bool,
) -> State {
    if !server_exists {
        return State::Orphan;
    }
    let Some(existing) = client.and_then(|c| on_disk(c, entry)) else {
        return State::Missing;
    };
    if !matches_record(existing, record) {
        return State::Customized;
    }
    if Path::new(&record.command).is_absolute() && !Path::new(&record.command).exists() {
        return State::Stale;
    }
    State::Ok
}

/// Every recorded direct entry with its state, plus launcher-shaped entries nothing recorded.
pub fn assess(reg: &Registry, detected: &[DetectedClient]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut recorded: HashSet<(String, String)> = HashSet::new();
    for (client_id, entry, record) in record::all(reg) {
        let client = detected.iter().find(|c| c.id == client_id);
        let server = servers::find(reg, &record.server);
        recorded.insert((client_id.clone(), entry.clone()));
        rows.push(Row {
            client_name: client.map_or_else(|| client_id.clone(), |c| c.name.clone()),
            path: client.map(|c| c.config_path.clone()).unwrap_or_default(),
            state: state_of(client, &entry, &record, server.is_some()),
            server_name: server.map(|s| s.name.clone()),
            server: record.server.clone(),
            client: client_id,
            entry,
        });
    }
    for client in detected {
        for server in &client.servers {
            let Some(target) = launcher_target(server) else {
                continue;
            };
            if recorded.contains(&(client.id.clone(), server.name.clone())) {
                continue;
            }
            rows.push(Row {
                client: client.id.clone(),
                client_name: client.name.clone(),
                entry: server.name.clone(),
                server: target.to_string(),
                server_name: servers::find(reg, target).map(|s| s.name.clone()),
                state: State::Unrecorded,
                path: client.config_path.clone(),
            });
        }
    }
    rows.sort_by(|a, b| (&a.client, &a.entry).cmp(&(&b.client, &b.entry)));
    rows
}

pub fn list(client_id: Option<&str>) -> Result<Vec<Row>, Error> {
    let reg = registry_ro::read().map_err(registry_error)?;
    let mut detected = clients::detect_clients();
    if let Some(id) = client_id {
        let client = find_client(detected, id, false)?;
        detected = vec![client];
    }
    let mut rows = assess(&reg, &detected);
    if let Some(id) = client_id {
        rows.retain(|row| row.client == id);
    }
    Ok(rows)
}

/// Entries `client sync` and `client import` must treat as Toolport's own.
pub fn owned(reg: &Registry) -> HashSet<(String, String)> {
    record::all(reg)
        .into_iter()
        .map(|(client, entry, _)| (client, entry))
        .collect()
}

pub fn count(reg: &Registry) -> usize {
    record::all(reg).len()
}

pub fn list_value(rows: &[Row]) -> Value {
    json!({"entries": rows.iter().map(Row::to_value).collect::<Vec<_>>()})
}

pub fn list_text(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "No direct client entries.".to_string();
    }
    rows.iter().map(Row::line).collect::<Vec<_>>().join("\n")
}

/// Entries the sync, import and uninstall paths leave to this module: every recorded entry and
/// every launcher-shaped entry of a server the registry has.
pub fn claimed(reg: &Registry, detected: &[DetectedClient]) -> HashSet<(String, String)> {
    let mut set = owned(reg);
    for client in detected {
        for entry in &client.servers {
            if launcher_target(entry).is_some_and(|id| servers::find(reg, id).is_some()) {
                set.insert((client.id.clone(), entry.name.clone()));
            }
        }
    }
    set
}
