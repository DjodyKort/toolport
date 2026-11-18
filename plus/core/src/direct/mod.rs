//! Direct client entries: one server given to one client as its own entry, outside the gateway.
//! A stdio server gets a launcher entry (`toolportctl direct run <id>`), so the client file never
//! holds the server's command, env or a secret; a remote server gets its address only. The
//! registry records what was written (`plus.directEntries.<client>.<entry>`), which is how
//! `ls`, `check`, `server uninstall` and `client sync` tell Toolport's entries from the user's.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use toolport_plus_api::{ClientInfo, ClientWriteError, DirectEntry, EntrySpec, Host, ServerRecord};

use crate::context::hash::sha256_hex;
use crate::context::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use crate::op::OpError;

pub mod launcher;

pub const TRADEOFF: &str = "A direct entry bypasses the Toolport gateway: its tools are not \
     covered by access sets, approvals or lazy discovery, and every client with a direct entry \
     starts its own process of the server instead of sharing one.";

pub const REMOTE_TRADEOFF: &str = "A direct entry bypasses the Toolport gateway: its tools are \
     not covered by access sets, approvals or lazy discovery. Toolport writes no credentials \
     into the client: the client signs in to the server by itself.";

const ABSENT: &str = "(absent)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ok,
    Stale,
    Customized,
    Missing,
    Orphan,
    Unrecorded,
    Relocated,
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
            State::Relocated => "relocated",
        }
    }

    pub fn action(self) -> Option<&'static str> {
        match self {
            State::Ok => None,
            State::Stale | State::Customized => Some("rewrite"),
            State::Missing => Some("rewrite or forget"),
            State::Orphan => Some("remove"),
            State::Unrecorded => Some("track"),
            State::Relocated => Some("re-point"),
        }
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
    pub url: Option<String>,
}

impl Row {
    pub fn to_value(&self) -> Value {
        let mut value = json!({
            "client": self.client,
            "clientName": self.client_name,
            "entry": self.entry,
            "server": self.server,
            "serverName": self.server_name,
            "state": self.state.as_str(),
            "path": self.path,
        });
        if let Some(action) = self.state.action() {
            value["action"] = json!(action);
        }
        if let Some(url) = &self.url {
            value["url"] = json!(url);
        }
        value
    }

    pub fn line(&self) -> String {
        format!(
            "{:<16} {:<24} {:<11} {}",
            self.client,
            self.entry,
            self.state.as_str(),
            self.server
        )
    }
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

fn read_view(host: &dyn Host) -> Result<Option<Box<dyn toolport_plus_api::RegistryView>>, OpError> {
    host.read_view()
        .map_err(|error| OpError::failed("registry_error", error))
}

fn records(host: &dyn Host) -> Result<Vec<(String, String, DirectEntry)>, OpError> {
    let Some(view) = read_view(host)? else {
        return Ok(Vec::new());
    };
    let entries = view
        .direct_entries()
        .map_err(|error| OpError::failed("registry_error", error))?;
    Ok(entries
        .into_iter()
        .flat_map(|(client, entries)| {
            entries
                .into_iter()
                .map(move |(entry, record)| (client.clone(), entry, record))
        })
        .collect())
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
fn launcher_target(entry: &toolport_plus_api::ClientEntryInfo) -> Option<&str> {
    let command = entry.command.as_deref()?;
    match entry.args.as_slice() {
        [direct, run, id]
            if direct == "direct" && run == "run" && basename_stem(command) == "toolportctl" =>
        {
            Some(id)
        }
        _ => None,
    }
}

fn matches_record(entry: &toolport_plus_api::ClientEntryInfo, record: &DirectEntry) -> bool {
    if let Some(url) = &record.url {
        return entry.command.is_none()
            && entry.url.as_deref() == Some(url.as_str())
            && entry.env_keys.is_empty();
    }
    let mut keys = entry.env_keys.clone();
    keys.sort();
    entry.command.as_deref() == Some(record.command.as_str())
        && entry.args == record.args
        && keys.iter().eq(record.env.keys())
}

fn state_of(
    client: Option<&ClientInfo>,
    entry: &str,
    record: &DirectEntry,
    server_exists: bool,
) -> State {
    if !server_exists {
        return State::Orphan;
    }
    if let (Some(client), Some(path)) = (client, &record.path) {
        if client.config_path != *path {
            return State::Relocated;
        }
    }
    let Some(existing) = client.and_then(|c| c.entries.iter().find(|e| e.name == entry)) else {
        let disconnected =
            record.gateway_connected == Some(true) && client.is_some_and(|c| !c.gateway_installed);
        return if disconnected {
            State::Orphan
        } else {
            State::Missing
        };
    };
    if !matches_record(existing, record) {
        return State::Customized;
    }
    let command = std::path::Path::new(&record.command);
    if command.is_absolute() && !command.exists() {
        return State::Stale;
    }
    State::Ok
}

/// Every recorded direct entry with its state, plus launcher-shaped entries nothing recorded.
pub fn assess(host: &dyn Host, only_client: Option<&str>) -> Result<Vec<Row>, OpError> {
    let servers = read_servers(host)?;
    let clients = host.detect_clients();
    let mut rows = Vec::new();
    let mut recorded: Vec<(String, String)> = Vec::new();
    for (client_id, entry, record) in records(host)? {
        if only_client.is_some_and(|only| only != client_id) {
            continue;
        }
        let client = clients.iter().find(|c| c.id == client_id);
        let server = servers.iter().find(|s| s.id == record.server);
        recorded.push((client_id.clone(), entry.clone()));
        rows.push(Row {
            client_name: client.map_or_else(|| client_id.clone(), |c| c.name.clone()),
            path: client.map(|c| c.config_path.clone()).unwrap_or_default(),
            state: state_of(client, &entry, &record, server.is_some()),
            server_name: server.map(|s| s.name.clone()),
            server: record.server.clone(),
            url: record.url.clone(),
            client: client_id,
            entry,
        });
    }
    for client in &clients {
        if only_client.is_some_and(|only| only != client.id) {
            continue;
        }
        for entry in &client.entries {
            let Some(target) = launcher_target(entry) else {
                continue;
            };
            if recorded.contains(&(client.id.clone(), entry.name.clone())) {
                continue;
            }
            rows.push(Row {
                client: client.id.clone(),
                client_name: client.name.clone(),
                entry: entry.name.clone(),
                server: target.to_string(),
                server_name: servers
                    .iter()
                    .find(|s| s.id == target)
                    .map(|s| s.name.clone()),
                state: State::Unrecorded,
                path: client.config_path.clone(),
                url: None,
            });
        }
    }
    rows.sort_by(|a, b| (&a.client, &a.entry).cmp(&(&b.client, &b.entry)));
    Ok(rows)
}

pub fn list(host: &dyn Host, client: Option<&str>) -> Result<Vec<Row>, OpError> {
    if let Some(id) = client {
        find_client(host, id)?;
    }
    assess(host, client)
}

fn read_servers(host: &dyn Host) -> Result<Vec<ServerRecord>, OpError> {
    match read_view(host)? {
        Some(view) => view
            .servers()
            .map_err(|error| OpError::failed("registry_error", error)),
        None => Ok(Vec::new()),
    }
}

fn find_server<'a>(servers: &'a [ServerRecord], key: &str) -> Option<&'a ServerRecord> {
    let key = key.trim();
    servers
        .iter()
        .find(|s| s.id == key)
        .or_else(|| servers.iter().find(|s| s.name.eq_ignore_ascii_case(key)))
}

fn find_client(host: &dyn Host, client_id: &str) -> Result<ClientInfo, OpError> {
    let clients = host.detect_clients();
    clients
        .iter()
        .find(|c| c.id == client_id)
        .cloned()
        .ok_or_else(|| {
            let known: Vec<&str> = clients.iter().map(|c| c.id.as_str()).collect();
            OpError::not_found(format!(
                "client '{client_id}' is not supported; available clients: {}",
                known.join(", ")
            ))
        })
}

fn refused(server: &ServerRecord, reason: &str) -> OpError {
    OpError::failed(
        "refused",
        format!(
            "{} cannot get a direct entry: {reason}. It stays reachable through the gateway.",
            server.name
        ),
    )
}

fn is_remote(server: &ServerRecord) -> bool {
    server.text("command").is_none()
        && server.text("url").is_some()
        && matches!(
            server
                .text("transport")
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "http" | "sse"
        )
}

fn refuse(server: &ServerRecord) -> Result<(), OpError> {
    if server.name.trim().is_empty() {
        return Err(OpError::failed(
            "refused",
            format!("server '{}' has no name to give the entry", server.id),
        ));
    }
    if server.extra.contains_key("clientCredentials") {
        return Err(refused(
            server,
            "its sign-in is managed by Toolport (OAuth) and only the gateway can refresh it",
        ));
    }
    if is_remote(server) {
        let has_headers = !server
            .env()
            .map_err(|error| OpError::failed("registry_error", error))?
            .is_empty();
        if has_headers {
            return Err(refused(
                server,
                "it sends headers kept in the Toolport vault, and a client entry holds only the address",
            ));
        }
        return Ok(());
    }
    match launcher::refusal(server) {
        Some(reason) => Err(refused(server, &reason)),
        None => Ok(()),
    }
}

fn wanted(
    host: &dyn Host,
    server: &ServerRecord,
    entry: &str,
) -> Result<(EntrySpec, DirectEntry), OpError> {
    if is_remote(server) {
        let url = server.text("url").unwrap_or_default().to_string();
        let transport = if server
            .text("transport")
            .is_some_and(|t| t.eq_ignore_ascii_case("sse"))
        {
            "sse"
        } else {
            "http"
        };
        let spec = EntrySpec {
            name: entry.into(),
            transport: transport.into(),
            url: Some(url.clone()),
            ..Default::default()
        };
        let record = DirectEntry {
            server: server.id.clone(),
            url: Some(url),
            ..Default::default()
        };
        return Ok((spec, record));
    }
    let command = host.launcher_command().ok_or_else(|| {
        OpError::failed(
            "launcher_missing",
            "could not find toolportctl next to this program, in the data directory's bin or on PATH; a direct entry needs it to start the server",
        )
    })?;
    let args = vec!["direct".to_string(), "run".to_string(), server.id.clone()];
    let env = host.launcher_env();
    let spec = EntrySpec {
        name: entry.into(),
        transport: "stdio".into(),
        command: Some(command.clone()),
        args: args.clone(),
        env: env.clone(),
        ..Default::default()
    };
    let record = DirectEntry {
        server: server.id.clone(),
        command,
        args,
        env,
        ..Default::default()
    };
    Ok((spec, record))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Added,
    Updated,
    Adopted,
    Unchanged,
    Removed,
    Forgotten,
    Absent,
}

impl Action {
    fn as_str(self) -> &'static str {
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

    fn changes_anything(self) -> bool {
        !matches!(self, Action::Unchanged | Action::Absent)
    }
}

fn plan_add_action(
    entry: &str,
    client_name: &str,
    existing: Option<&toolport_plus_api::ClientEntryInfo>,
    recorded: Option<&DirectEntry>,
    record: &DirectEntry,
    force: bool,
) -> Result<Action, OpError> {
    let Some(existing) = existing else {
        return Ok(Action::Added);
    };
    if matches_record(existing, record) {
        return Ok(if recorded == Some(record) {
            Action::Unchanged
        } else {
            Action::Adopted
        });
    }
    let ours = launcher_target(existing) == Some(record.server.as_str())
        || recorded.is_some_and(|r| r.server == record.server && matches_record(existing, r));
    if ours || force {
        return Ok(Action::Updated);
    }
    let owned = recorded.is_some_and(|r| r.server == record.server);
    Err(OpError::conflict(if owned {
        format!(
            "{client_name}'s entry '{entry}' was changed after Toolport wrote it; use --force to replace it"
        )
    } else {
        format!(
            "{client_name} already has an entry named '{entry}' that Toolport did not write; remove it first or use --force to replace it"
        )
    }))
}

#[derive(Debug, Clone, Default)]
pub struct Args<'a> {
    pub server: &'a str,
    pub client: &'a str,
    pub force: bool,
    pub dry_run: bool,
    pub plan_hash: Option<&'a str>,
}

fn write_error(error: ClientWriteError) -> OpError {
    match error {
        ClientWriteError::Conflict(message) => OpError::conflict(message),
        ClientWriteError::Failed(message) => OpError::failed("write_failed", message),
    }
}

fn plan_hash(parts: &Value) -> String {
    sha256_hex(parts.to_string())[..16].to_string()
}

fn check_plan_hash(args: &Args, hash: &str) -> Result<(), OpError> {
    match args.plan_hash {
        Some(given) if given != hash => Err(OpError::conflict(
            "the plan changed since it was shown (the client file or the server differs); run it with --dry-run again",
        )),
        _ => Ok(()),
    }
}

fn step_for(action: Action, path: &str, detail: String, before: String, after: String) -> Step {
    let diff = Diff { before, after };
    match action {
        Action::Added => Step::merge(path, detail, &["entry"], diff),
        _ => Step::merge(path, detail, &["entry"], diff),
    }
}

fn note(detail: impl Into<String>, path: Option<&str>) -> Step {
    Step {
        op: "note",
        path: path.map(str::to_string),
        detail: detail.into(),
        keys: None,
        diff: None,
    }
}

struct Who<'a> {
    server: &'a ServerRecord,
    client: &'a ClientInfo,
    entry: &'a str,
}

struct Planned<'a> {
    action: Action,
    hash: &'a str,
    plan: &'a PlanV1,
    remote: Option<&'a str>,
}

fn outcome_value(who: Who, planned: Planned, result: Option<&ResultV1>) -> Value {
    let Who {
        server,
        client,
        entry,
    } = who;
    let Planned {
        action,
        hash,
        plan,
        remote,
    } = planned;
    let dry_run = result.is_none();
    let mut value = json!({
        "client": client.id,
        "clientName": client.name,
        "server": server.id,
        "serverName": server.name,
        "entry": entry,
        "path": client.config_path,
        "action": action.as_str(),
        "changed": action.changes_anything(),
        "dryRun": dry_run,
        "planHash": hash,
        "plan": plan,
        "result": result,
    });
    if let Some(url) = remote {
        value["remote"] = json!({"url": url});
    }
    value
}

pub fn text_of(value: &Value) -> String {
    let (client, entry, server) = (
        value["clientName"].as_str().unwrap_or(""),
        value["entry"].as_str().unwrap_or(""),
        value["serverName"].as_str().unwrap_or(""),
    );
    let would = value["dryRun"].as_bool().unwrap_or(false);
    let action = value["action"].as_str().unwrap_or("");
    let verb = |done: &str, plan: &str| {
        if would {
            plan.to_string()
        } else {
            done.to_string()
        }
    };
    let mut lines = vec![match action {
        "added" => format!(
            "{} direct entry '{entry}' for {server} to {client}",
            verb("Added", "Would add")
        ),
        "updated" => format!(
            "{} direct entry '{entry}' for {server} in {client}",
            verb("Updated", "Would update")
        ),
        "adopted" => format!(
            "{} direct entry '{entry}' for {server} in {client}",
            verb("Recorded", "Would record")
        ),
        "removed" => format!(
            "{} direct entry '{entry}' for {server} from {client}",
            verb("Removed", "Would remove")
        ),
        "forgotten" => format!(
            "{client} no longer has '{entry}'; {} the record of it",
            verb("dropped", "would drop")
        ),
        "unchanged" => format!("{client} already has the direct entry '{entry}' for {server}"),
        _ => format!("{client} has no direct entry '{entry}' for {server}"),
    }];
    lines.push(format!(
        "Config file: {}",
        value["path"].as_str().unwrap_or("")
    ));
    for warning in value["plan"]["warnings"].as_array().into_iter().flatten() {
        if let Some(text) = warning.as_str() {
            lines.push(text.to_string());
        }
    }
    if would && !matches!(action, "unchanged" | "absent") {
        lines.push(format!(
            "Dry run: nothing was written. Apply with --plan-hash {}.",
            value["planHash"].as_str().unwrap_or("")
        ));
    } else if let Some(backup) = value["result"]["backups"].get(0).and_then(Value::as_str) {
        lines.push(format!("Backup: {backup}"));
        lines.push(format!("Restart {client} for the change to take effect."));
    }
    lines.join("\n")
}

pub fn add(host: &dyn Host, args: &Args) -> Result<Value, OpError> {
    let servers = read_servers(host)?;
    let server = find_server(&servers, args.server)
        .ok_or_else(|| OpError::not_found(format!("Server '{}' not found", args.server)))?
        .clone();
    let client = find_client(host, args.client)?;
    if let Some(reason) = &client.direct_unsupported {
        return Err(OpError::failed("unsupported", reason.clone()));
    }
    if let Some(error) = &client.error {
        return Err(OpError::failed(
            "client_config",
            format!("{}: {error}", client.name),
        ));
    }
    if !client.config_exists && !client.app_present {
        return Err(OpError::not_found(format!(
            "{} installation not detected.",
            client.name
        )));
    }
    refuse(&server)?;
    let entry = server.name.trim().to_string();
    let (spec, mut record) = wanted(host, &server, &entry)?;
    record.path = Some(client.config_path.clone());
    record.gateway_connected = Some(client.gateway_installed);
    let recorded = records(host)?
        .into_iter()
        .find(|(c, e, _)| *c == client.id && *e == entry)
        .map(|(_, _, record)| record);
    if let Some(old) = &recorded {
        let same_but_flag = DirectEntry {
            gateway_connected: old.gateway_connected,
            ..record.clone()
        };
        if same_but_flag == *old {
            record = same_but_flag;
        }
    }
    let existing = client.entries.iter().find(|e| e.name == entry);
    let action = plan_add_action(
        &entry,
        &client.name,
        existing,
        recorded.as_ref(),
        &record,
        args.force,
    )?;
    let revision = host
        .client_revision(&client.id)
        .map_err(|error| OpError::failed("client_config", error))?;
    let rendered = host
        .render_entry(&client.id, &spec)
        .map_err(|error| OpError::failed("client_config", error))?;
    let before = existing.map_or_else(
        || ABSENT.to_string(),
        |e| {
            format!(
                "{} entry '{}' as it stands",
                if e.command.is_some() {
                    "stdio"
                } else {
                    "remote"
                },
                e.name
            )
        },
    );
    let mut steps = Vec::new();
    if action.writes_client() {
        steps.push(step_for(
            action,
            &client.config_path,
            format!(
                "{} the entry '{entry}' and leave every other entry alone",
                if action == Action::Added {
                    "add"
                } else {
                    "replace"
                }
            ),
            before,
            format!("+ {rendered}"),
        ));
    }
    if recorded.as_ref() != Some(&record) {
        steps.push(note(
            format!(
                "record '{entry}' under plus.directEntries.{} in the registry",
                client.id
            ),
            None,
        ));
    }
    steps.push(note(
        "a backup of the client file is made first; the write goes through the client's own lock and conflict check",
        None,
    ));
    let mut warnings = vec![if record.url.is_some() {
        REMOTE_TRADEOFF
    } else {
        TRADEOFF
    }
    .to_string()];
    if record.url.is_none()
        && server
            .env()
            .map(|env| env.iter().any(|e| e.secret))
            .unwrap_or(false)
    {
        warnings.push(
            "Secrets stay in Toolport: the launcher reads them when the client starts the entry; they are never written to the client file."
                .to_string(),
        );
    }
    let plan = PlanV1 {
        summary: format!(
            "Give {} a direct entry '{entry}' for {}",
            client.name, server.name
        ),
        steps,
        effects: Effects::default(),
        warnings,
        undo: format!(
            "toolportctl client direct rm {} --client {}",
            server.id, client.id
        ),
    };
    let hash = plan_hash(&json!({
        "op": "add", "client": client.id, "entry": entry, "action": action.as_str(),
        "spec": spec, "revision": revision, "path": client.config_path,
    }));
    let remote = record.url.clone();
    if args.dry_run {
        return Ok(outcome_value(
            Who {
                server: &server,
                client: &client,
                entry: &entry,
            },
            Planned {
                action,
                hash: &hash,
                plan: &plan,
                remote: remote.as_deref(),
            },
            None,
        ));
    }
    check_plan_hash(args, &hash)?;
    let mut backups = Vec::new();
    let mut changed = Vec::new();
    let mut path = client.config_path.clone();
    if action.writes_client() {
        let written = host
            .write_entry(&client.id, &entry, Some(&spec), Some(&revision))
            .map_err(write_error)?;
        path = written.path.clone();
        changed.push(written.path);
        backups.extend(written.backup);
    }
    if recorded.as_ref() != Some(&record) {
        let mut record = record;
        record.path = Some(path.clone());
        host.set_direct_entry(&client.id, &entry, record)
            .map_err(|error| OpError::failed("registry_error", error))?;
        changed.push("registry: plus.directEntries".to_string());
    }
    let result = ResultV1 {
        applied: true,
        changed,
        undo: plan.undo.clone(),
        backups,
    };
    let mut done = client.clone();
    done.config_path = path;
    Ok(outcome_value(
        Who {
            server: &server,
            client: &done,
            entry: &entry,
        },
        Planned {
            action,
            hash: &hash,
            plan: &plan,
            remote: remote.as_deref(),
        },
        Some(&result),
    ))
}

pub fn remove(host: &dyn Host, args: &Args) -> Result<Value, OpError> {
    let servers = read_servers(host)?;
    let key = args.server.trim();
    let server = find_server(&servers, key).cloned();
    let all = records(host)?;
    let hit = all.iter().find(|(client, entry, record)| {
        client == args.client
            && (server.as_ref().is_some_and(|s| record.server == s.id)
                || entry.eq_ignore_ascii_case(key)
                || record.server == key)
    });
    if server.is_none() && hit.is_none() {
        return Err(OpError::not_found(format!(
            "Server '{}' not found",
            args.server
        )));
    }
    let client = find_client(host, args.client)?;
    let entry = match (hit, &server) {
        (Some((_, entry, _)), _) => entry.clone(),
        (None, Some(server)) => server.name.trim().to_string(),
        (None, None) => unreachable!("one of the two exists"),
    };
    let recorded = hit.map(|(_, _, record)| record.clone());
    let server = server.or_else(|| {
        recorded
            .as_ref()
            .and_then(|r| servers.iter().find(|s| s.id == r.server).cloned())
    });
    remove_in(
        host,
        &client,
        &entry,
        server.as_ref(),
        recorded.as_ref(),
        args,
    )
}

fn remove_in(
    host: &dyn Host,
    client: &ClientInfo,
    entry: &str,
    server: Option<&ServerRecord>,
    recorded: Option<&DirectEntry>,
    args: &Args,
) -> Result<Value, OpError> {
    let server_id = server
        .map(|s| s.id.clone())
        .or_else(|| recorded.map(|r| r.server.clone()))
        .unwrap_or_default();
    let shown = server.cloned().unwrap_or_else(|| ServerRecord {
        id: server_id.clone(),
        name: server_id.clone(),
        ..Default::default()
    });
    let existing = client.entries.iter().find(|e| e.name == entry);
    let action = match existing {
        None if recorded.is_some() => Action::Forgotten,
        None => Action::Absent,
        Some(existing) => {
            let ours = launcher_target(existing) == Some(server_id.as_str())
                || recorded.is_some_and(|r| matches_record(existing, r));
            if !ours && !args.force {
                return Err(OpError::conflict(if recorded.is_some() {
                    format!("{}'s entry '{entry}' was changed after Toolport wrote it; use --force to remove it", client.name)
                } else {
                    format!("{}'s entry '{entry}' is not a Toolport launcher entry; Toolport only removes entries it wrote (--force overrides that)", client.name)
                }));
            }
            Action::Removed
        }
    };
    let revision = host
        .client_revision(&client.id)
        .map_err(|error| OpError::failed("client_config", error))?;
    let mut steps = Vec::new();
    if action == Action::Removed {
        steps.push(step_for(
            action,
            &client.config_path,
            format!("remove the entry '{entry}' and leave every other entry alone"),
            format!("entry '{entry}'"),
            ABSENT.to_string(),
        ));
    }
    if recorded.is_some() {
        steps.push(note(
            format!(
                "drop the record of '{entry}' under plus.directEntries.{}",
                client.id
            ),
            None,
        ));
    }
    let plan = PlanV1 {
        summary: format!("Remove the direct entry '{entry}' from {}", client.name),
        steps,
        effects: Effects::default(),
        warnings: Vec::new(),
        undo: format!(
            "toolportctl client direct add {} --client {}",
            server_id, client.id
        ),
    };
    let hash = plan_hash(&json!({
        "op": "rm", "client": client.id, "entry": entry, "action": action.as_str(),
        "revision": revision, "path": client.config_path,
    }));
    if args.dry_run {
        return Ok(outcome_value(
            Who {
                server: &shown,
                client,
                entry,
            },
            Planned {
                action,
                hash: &hash,
                plan: &plan,
                remote: None,
            },
            None,
        ));
    }
    check_plan_hash(args, &hash)?;
    let mut changed = Vec::new();
    let mut backups = Vec::new();
    let mut path = client.config_path.clone();
    if action == Action::Removed {
        let written = host
            .write_entry(&client.id, entry, None, Some(&revision))
            .map_err(write_error)?;
        path = written.path.clone();
        changed.push(written.path);
        backups.extend(written.backup);
    }
    if recorded.is_some() {
        host.clear_direct_entry(&client.id, entry)
            .map_err(|error| OpError::failed("registry_error", error))?;
        changed.push("registry: plus.directEntries".to_string());
    }
    let result = ResultV1 {
        applied: true,
        changed,
        undo: plan.undo.clone(),
        backups,
    };
    let mut done = client.clone();
    done.config_path = path;
    Ok(outcome_value(
        Who {
            server: &shown,
            client: &done,
            entry,
        },
        Planned {
            action,
            hash: &hash,
            plan: &plan,
            remote: None,
        },
        Some(&result),
    ))
}

/// The direct entries `server uninstall` takes with a server: every recorded entry of it, and
/// launcher-shaped entries of it nobody recorded, so none is left pointing at nothing. A plan
/// with `dry_run`; nothing is read from the registry afterwards, so call it before the server
/// is removed.
pub fn remove_for_server(
    host: &dyn Host,
    server: &ServerRecord,
    dry_run: bool,
) -> Result<Vec<Value>, OpError> {
    let clients = host.detect_clients();
    let mut candidates: Vec<(String, String, Option<DirectEntry>)> = records(host)?
        .into_iter()
        .filter(|(_, _, record)| record.server == server.id)
        .map(|(client, entry, record)| (client, entry, Some(record)))
        .collect();
    for client in &clients {
        for entry in &client.entries {
            let known = candidates
                .iter()
                .any(|(c, e, _)| *c == client.id && *e == entry.name);
            if !known && launcher_target(entry) == Some(server.id.as_str()) {
                candidates.push((client.id.clone(), entry.name.clone(), None));
            }
        }
    }
    let mut results = Vec::new();
    for (client_id, entry, record) in candidates {
        let args = Args {
            server: &server.id,
            client: &client_id,
            dry_run,
            ..Default::default()
        };
        let done = match clients.iter().find(|c| c.id == client_id) {
            Some(client) => remove_in(host, client, &entry, Some(server), record.as_ref(), &args),
            None => {
                if !dry_run {
                    host.clear_direct_entry(&client_id, &entry)
                        .map_err(|error| OpError::failed("registry_error", error))?;
                }
                Ok(
                    json!({"client": client_id, "entry": entry, "action": "forgotten", "dryRun": dry_run}),
                )
            }
        };
        results.push(match done {
            Ok(value) => json!({
                "client": client_id,
                "entry": entry,
                "action": value["action"],
                "path": value["path"],
            }),
            Err(error) => json!({
                "client": client_id,
                "entry": entry,
                "action": "kept",
                "reason": error.message,
            }),
        });
    }
    Ok(results)
}

/// Rows `check` reports and whether all are healthy.
pub fn check(host: &dyn Host, client: Option<&str>) -> Result<(Vec<Row>, bool), OpError> {
    let rows = list(host, client)?;
    let healthy = rows.iter().all(|row| row.state == State::Ok);
    Ok((rows, healthy))
}

pub fn is_recorded(host: &dyn Host, client: &str, entry: &str) -> bool {
    records(host).is_ok_and(|all| all.iter().any(|(c, e, _)| c == client && e == entry))
}

pub fn recorded_by_client(host: &dyn Host) -> Result<BTreeMap<String, Vec<String>>, OpError> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (client, entry, _) in records(host)? {
        map.entry(client).or_default().push(entry);
    }
    Ok(map)
}

#[cfg(test)]
mod tests;
