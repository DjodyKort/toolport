use super::{BINARY_NAME, SERVER_NAME};
use crate::registry::{self, Registry, ServerEntry};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub const SELF_SOURCE: &str = "plus:selfmcp";

const PLUS_KEY: &str = "plus";
const RECORD_KEY: &str = "selfmcp";
const ENABLED_IN: &str = "enabledIn";
const OPT_OUT: &str = "optOut";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    Created,
    Updated,
    Unchanged,
    OptedOut,
}

impl Ensured {
    pub fn as_str(self) -> &'static str {
        match self {
            Ensured::Created => "created",
            Ensured::Updated => "updated",
            Ensured::Unchanged => "unchanged",
            Ensured::OptedOut => "opted-out",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub outcome: Ensured,
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Missing,
    OptedOut,
    NotEnabled,
    Enabled,
    Disabled,
}

impl Standing {
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::Missing => "missing",
            Standing::OptedOut => "opted-out",
            Standing::NotEnabled => "not-enabled",
            Standing::Enabled => "enabled",
            Standing::Disabled => "disabled",
        }
    }

    pub fn healthy(self) -> bool {
        !matches!(self, Standing::Missing | Standing::NotEnabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileState {
    pub id: String,
    pub enabled: bool,
    pub opted_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub standing: Standing,
    pub active: Option<ProfileState>,
    pub clients: Vec<ProfileState>,
}

fn binary_file() -> String {
    format!("{BINARY_NAME}{}", std::env::consts::EXE_SUFFIX)
}

/// Absolute path of `toolport-selfmcp` next to the running binary (the gateway ships in the
/// same directory), else in the data directory's `bin`, else the bare name for PATH lookup.
pub fn binary_path() -> String {
    let file = binary_file();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        dirs.extend(exe.parent().map(PathBuf::from));
    }
    if let Some(dir) = registry::conduit_dir() {
        dirs.push(dir.join("bin"));
    }
    for dir in &dirs {
        let candidate = dir.join(&file);
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    match dirs.first() {
        Some(dir) => dir.join(&file).to_string_lossy().into_owned(),
        None => file,
    }
}

fn is_self(entry: &ServerEntry) -> bool {
    entry.source.as_deref() == Some(SELF_SOURCE)
}

fn self_entry(command: &str) -> ServerEntry {
    ServerEntry {
        id: String::new(),
        name: SERVER_NAME.to_string(),
        transport: "stdio".into(),
        command: Some(command.to_string()),
        args: Vec::new(),
        env: Vec::new(),
        url: None,
        cwd: None,
        source: Some(SELF_SOURCE.into()),
        disabled_tools: Vec::new(),
        client_credentials: None,
        request_timeout_ms: None,
        max_request_timeout_ms: None,
        initialize_timeout_ms: None,
        launch: None,
        unknown_fields: serde_json::Map::new(),
    }
}

/// Idempotent: adds the self server when missing, repoints a stale command, and leaves every
/// other field (profile membership, disabled tools, timeouts) alone.
pub fn apply_ensure_self_server(reg: &mut Registry, command: &str) -> Ensured {
    if let Some(existing) = reg.servers.iter_mut().find(|s| is_self(s)) {
        let wanted = Some(command.to_string());
        if existing.command == wanted && existing.transport == "stdio" && existing.url.is_none() {
            return Ensured::Unchanged;
        }
        existing.command = wanted;
        existing.transport = "stdio".into();
        existing.url = None;
        return Ensured::Updated;
    }
    if reg.servers.iter().any(|s| s.name == SERVER_NAME) {
        return Ensured::Unchanged;
    }
    reg.add_server(self_entry(command));
    Ensured::Created
}

pub fn find_self(reg: &Registry) -> Option<&ServerEntry> {
    reg.servers.iter().find(|s| is_self(s))
}

/// `Ensure` is what import and `plus.selfmcp.ensure` run unattended and never overrides an
/// opt-out. `Install` is `toolportctl mcp install`, an explicit request that brings back an
/// uninstalled server; it still leaves profiles the user switched it off in alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Ensure,
    Install,
}

fn record(reg: &Registry) -> Option<&Map<String, Value>> {
    reg.unknown_fields
        .get(PLUS_KEY)?
        .get(RECORD_KEY)?
        .as_object()
}

fn record_mut(reg: &mut Registry) -> &mut Map<String, Value> {
    let plus = reg
        .unknown_fields
        .entry(PLUS_KEY.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !plus.is_object() {
        *plus = Value::Object(Map::new());
    }
    let slot = plus
        .as_object_mut()
        .expect("just made an object")
        .entry(RECORD_KEY.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    slot.as_object_mut().expect("just made an object")
}

fn clear_record(reg: &mut Registry) {
    if let Some(plus) = reg
        .unknown_fields
        .get_mut(PLUS_KEY)
        .and_then(Value::as_object_mut)
    {
        plus.remove(RECORD_KEY);
        if plus.is_empty() {
            reg.unknown_fields.remove(PLUS_KEY);
        }
    }
}

/// Profiles Toolport has already switched the self server on in. Once a profile is listed it
/// is never switched on again by import or `ensure`, so turning it off there sticks.
pub fn enabled_in(reg: &Registry) -> Vec<String> {
    record(reg)
        .and_then(|r| r.get(ENABLED_IN))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// A recorded self server that is no longer in the registry was removed on purpose.
fn removed(reg: &Registry) -> bool {
    find_self(reg).is_none() && record(reg).is_some()
}

fn client_profiles(reg: &Registry) -> Vec<String> {
    let mut ids: Vec<String> = reg
        .client_scopes
        .values()
        .filter(|id| reg.profiles.iter().any(|p| &p.id == *id))
        .cloned()
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn auto_targets(reg: &Registry) -> Vec<String> {
    let mut ids = vec![reg.active_profile_id()];
    for id in client_profiles(reg) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.retain(|id| reg.profiles.iter().any(|p| &p.id == id));
    ids
}

fn enroll(reg: &mut Registry, id: &str, forced: Option<&str>) -> Result<Vec<String>, String> {
    let before = enabled_in(reg);
    let mut recorded = before.clone();
    let mut enabled = Vec::new();
    for target in auto_targets(reg) {
        if recorded.contains(&target) {
            continue;
        }
        reg.set_server_enabled(&target, id, true)?;
        recorded.push(target.clone());
        enabled.push(target);
    }
    if let Some(forced) = forced {
        if !reg.is_enabled(forced, id) {
            reg.set_server_enabled(forced, id, true)?;
            if !enabled.iter().any(|e| e == forced) {
                enabled.push(forced.to_string());
            }
        }
        if !recorded.iter().any(|r| r == forced) {
            recorded.push(forced.to_string());
        }
    }
    if recorded != before || record(reg).is_none() {
        record_mut(reg).insert(ENABLED_IN.into(), json!(recorded));
    }
    Ok(enabled)
}

/// Registers the self server and switches it on in the active profile and in every profile a
/// connected client is scoped to, once per profile. `profile` is switched on regardless of
/// earlier choices and must exist: an unknown one fails before anything is written.
pub fn apply_install(
    reg: &mut Registry,
    command: &str,
    intent: Intent,
    profile: Option<&str>,
) -> Result<Installed, String> {
    if removed(reg) {
        if intent == Intent::Ensure {
            return Ok(Installed {
                id: String::new(),
                outcome: Ensured::OptedOut,
                enabled: Vec::new(),
            });
        }
        clear_record(reg);
    }
    let outcome = apply_ensure_self_server(reg, command);
    let id = find_self(reg).map(|s| s.id.clone()).unwrap_or_default();
    if id.is_empty() {
        if profile.is_some() {
            return Err(format!(
                "a server named '{SERVER_NAME}' already exists and is not the self server"
            ));
        }
        return Ok(Installed {
            id,
            outcome,
            enabled: Vec::new(),
        });
    }
    let enabled = enroll(reg, &id, profile)?;
    let outcome = match outcome {
        Ensured::Unchanged if !enabled.is_empty() => Ensured::Updated,
        other => other,
    };
    Ok(Installed {
        id,
        outcome,
        enabled,
    })
}

pub fn status(reg: &Registry) -> Status {
    let Some(entry) = find_self(reg) else {
        return Status {
            standing: if record(reg).is_some() {
                Standing::OptedOut
            } else {
                Standing::Missing
            },
            active: None,
            clients: Vec::new(),
        };
    };
    let recorded = enabled_in(reg);
    let state = |id: &str| {
        let enabled = reg.is_enabled(id, &entry.id);
        ProfileState {
            id: id.to_string(),
            enabled,
            opted_out: !enabled && recorded.iter().any(|r| r == id),
        }
    };
    let active = state(&reg.active_profile_id());
    let clients: Vec<ProfileState> = client_profiles(reg).iter().map(|id| state(id)).collect();
    let all = || std::iter::once(&active).chain(clients.iter());
    let standing = if all().any(|p| !p.enabled && !p.opted_out) {
        Standing::NotEnabled
    } else if all().any(|p| p.opted_out) {
        Standing::Disabled
    } else {
        Standing::Enabled
    };
    Status {
        standing,
        active: Some(active),
        clients,
    }
}

pub fn install(profile: Option<&str>, intent: Intent) -> Result<Installed, String> {
    let command = binary_path();
    let (_, installed) = registry::update(|reg| apply_install(reg, &command, intent, profile))?;
    Ok(installed)
}

pub fn ensure_self_server() -> Result<(String, Ensured), String> {
    install(None, Intent::Ensure).map(|i| (i.id, i.outcome))
}

/// With a profile, the registry write is all-or-nothing: an unknown profile registers nothing.
pub fn install_self_server(profile: Option<&str>) -> Result<(String, Ensured), String> {
    install(profile, Intent::Install).map(|i| (i.id, i.outcome))
}

/// Removes the entry and records the opt-out, so import and `ensure` leave it out.
pub fn uninstall_self_server() -> Result<Option<String>, String> {
    let (_, removed) = registry::update(|reg| {
        let removed = match find_self(reg).map(|s| s.id.clone()) {
            Some(id) => {
                reg.remove_server(&id)?;
                Some(id)
            }
            None => None,
        };
        record_mut(reg).insert(OPT_OUT.into(), json!(true));
        Ok(removed)
    })?;
    Ok(removed)
}

pub fn ensure_handler(_args: Value) -> Result<Value, String> {
    let installed = install(None, Intent::Ensure)?;
    Ok(json!({
        "id": installed.id,
        "action": installed.outcome.as_str(),
        "enabled": installed.enabled,
        "command": binary_path(),
    }))
}
