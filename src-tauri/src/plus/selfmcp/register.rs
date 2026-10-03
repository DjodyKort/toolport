use super::{BINARY_NAME, SERVER_NAME};
use crate::registry::{self, Registry, ServerEntry};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const SELF_SOURCE: &str = "plus:selfmcp";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ensured {
    Created,
    Updated,
    Unchanged,
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

pub fn ensure_self_server() -> Result<(String, Ensured), String> {
    let command = binary_path();
    let (reg, outcome) = registry::update(|reg| Ok(apply_ensure_self_server(reg, &command)))?;
    let id = reg
        .servers
        .iter()
        .find(|s| is_self(s))
        .map(|s| s.id.clone())
        .unwrap_or_default();
    Ok((id, outcome))
}

pub fn ensure_handler(_args: Value) -> Result<Value, String> {
    let (id, outcome) = ensure_self_server()?;
    Ok(json!({"id": id, "action": format!("{outcome:?}").to_lowercase(), "command": binary_path()}))
}
