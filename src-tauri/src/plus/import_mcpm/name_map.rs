use super::{
    exposed_prefix, load_input, map_servers, MappedServer, RunOptions, MAX_TOOL_NAME_LEN,
    TOOL_NAME_PREFIX,
};
use crate::router::sanitize_segment;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub type ToolManifest = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Offender {
    pub server: String,
    pub id: String,
    pub tool: String,
    pub exposed: String,
    pub length: usize,
    pub suggested_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Collision {
    pub exposed: String,
    pub old_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct NameMapError {
    pub too_long: Vec<Offender>,
    pub collisions: Vec<Collision>,
    pub unknown_servers: Vec<String>,
}

impl NameMapError {
    fn is_empty(&self) -> bool {
        self.too_long.is_empty() && self.collisions.is_empty() && self.unknown_servers.is_empty()
    }
}

impl std::fmt::Display for NameMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for s in &self.unknown_servers {
            writeln!(f, "tool manifest names unknown server '{s}'")?;
        }
        for o in &self.too_long {
            let hint = match &o.suggested_id {
                Some(id) => format!("suggested short id: {id}"),
                None => "no short id can fit this tool name".to_string(),
            };
            writeln!(
                f,
                "{} is {} chars (max {MAX_TOOL_NAME_LEN}), server '{}' id '{}': {hint}",
                o.exposed, o.length, o.server, o.id
            )?;
        }
        for c in &self.collisions {
            writeln!(f, "{} collides: {}", c.exposed, c.old_names.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for NameMapError {}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NameMap {
    pub map: BTreeMap<String, String>,
    pub servers: BTreeMap<String, String>,
    /// Every server of the import, with or without a tool manifest entry, so a reference to a
    /// server that exists is never reported as one that is gone.
    #[serde(skip)]
    pub imported: BTreeSet<String>,
}

impl NameMap {
    pub fn to_value(&self) -> Value {
        json!({ "map": self.map, "servers": self.servers, "count": self.map.len() })
    }
}

pub(super) fn old_tool_name(mcpm_server: &str, tool: &str) -> String {
    format!("mcp__mcpm_{mcpm_server}__{tool}")
}

pub(super) fn exposed_tool_name(id: &str, tool: &str) -> String {
    format!("{}{}", exposed_prefix(id), sanitize_segment(tool))
}

fn suggest_id(id: &str, tool: &str) -> Option<String> {
    let overhead = TOOL_NAME_PREFIX.len() + 2 + sanitize_segment(tool).len();
    let room = MAX_TOOL_NAME_LEN.checked_sub(overhead).filter(|n| *n > 0)?;
    let cut: String = id.chars().take(room).collect();
    let cut = cut.trim_end_matches(['-', '_']);
    (!cut.is_empty()).then(|| cut.to_string())
}

pub fn build_name_map(
    servers: &[MappedServer],
    manifest: &ToolManifest,
) -> Result<NameMap, NameMapError> {
    let mut err = NameMapError::default();
    let mut map = BTreeMap::new();
    let mut ids = BTreeMap::new();
    let mut by_new: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, tools) in manifest {
        let Some(server) = servers.iter().find(|s| &s.mcpm_name == name) else {
            err.unknown_servers.push(name.clone());
            continue;
        };
        let id = &server.entry.id;
        ids.insert(name.clone(), id.clone());
        let mut seen = std::collections::BTreeSet::new();
        for tool in tools {
            if !seen.insert(tool.as_str()) {
                continue;
            }
            let exposed = exposed_tool_name(id, tool);
            let old = old_tool_name(name, tool);
            if exposed.len() > MAX_TOOL_NAME_LEN {
                err.too_long.push(Offender {
                    server: name.clone(),
                    id: id.clone(),
                    tool: tool.clone(),
                    length: exposed.len(),
                    suggested_id: suggest_id(id, tool),
                    exposed: exposed.clone(),
                });
            }
            by_new.entry(exposed.clone()).or_default().push(old.clone());
            map.insert(old, exposed);
        }
    }
    for (exposed, old_names) in by_new {
        if old_names.len() > 1 {
            err.collisions.push(Collision { exposed, old_names });
        }
    }
    if err.is_empty() {
        let imported = servers.iter().map(|s| s.mcpm_name.clone()).collect();
        Ok(NameMap {
            map,
            servers: ids,
            imported,
        })
    } else {
        Err(err)
    }
}

pub fn load_manifest(path: &Path) -> Result<ToolManifest, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "{} must map server name to a list of tool names: {e}",
            path.display()
        )
    })
}

pub fn name_map(opts: &RunOptions, tools: &Path) -> Result<NameMap, String> {
    let (input, _) = load_input(opts)?;
    let (servers, _) = map_servers(&input);
    let manifest = load_manifest(tools)?;
    build_name_map(&servers, &manifest).map_err(|e| e.to_string().trim_end().to_string())
}

pub fn name_map_handler(args: Value) -> Result<Value, String> {
    let root = args
        .get("root")
        .and_then(Value::as_str)
        .ok_or("root is required")?;
    let tools = args
        .get("tools")
        .and_then(Value::as_str)
        .ok_or("tools is required")?;
    let opts = RunOptions {
        root: PathBuf::from(root),
        short_ids_path: args
            .get("shortIds")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        home: args.get("home").and_then(Value::as_str).map(String::from),
        ..RunOptions::default()
    };
    name_map(&opts, Path::new(tools)).map(|m| m.to_value())
}
