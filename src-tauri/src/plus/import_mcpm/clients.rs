use super::servers::{MappedServer, McpmInput, Warning};
use crate::registry::{slugify, Profile};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub client_id: String,
    pub servers: Map<String, Value>,
    /// Read from the client's own config because the mcpm root holds no snapshot of it. Such a
    /// client is only switched when it has an entry mcpm launched.
    pub live: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientSkip {
    pub client_id: String,
    pub entry: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MappedEntry {
    pub key: String,
    pub server_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientEntries {
    pub client_id: String,
    pub profile_id: String,
    pub mapped: Vec<MappedEntry>,
    pub orphans: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ClientMapping {
    pub entries: Vec<ClientEntries>,
    pub profiles: Vec<Profile>,
    pub client_scopes: BTreeMap<String, String>,
    pub client_discovery: BTreeMap<String, String>,
    pub skipped: Vec<ClientSkip>,
    pub warnings: Vec<Warning>,
}

enum Resolved {
    Servers(Vec<String>),
    Skip(&'static str),
}

fn profile(id: &str, name: &str, ids: Vec<String>) -> Profile {
    Profile {
        id: id.to_string(),
        name: name.to_string(),
        enabled_server_ids: ids,
        tool_scope: Default::default(),
        instructions: None,
    }
}

fn tag_members(servers: &[MappedServer], tag: &str) -> Vec<String> {
    servers
        .iter()
        .filter(|s| s.enabled && s.tags.iter().any(|t| t == tag))
        .map(|s| s.entry.id.clone())
        .collect()
}

fn by_name(servers: &[MappedServer], name: &str) -> Option<String> {
    servers
        .iter()
        .find(|s| s.mcpm_name == name && s.enabled)
        .map(|s| s.entry.id.clone())
}

fn by_url(servers: &[MappedServer], url: &str) -> Option<String> {
    servers
        .iter()
        .find(|s| s.enabled && s.entry.url.as_deref() == Some(url))
        .map(|s| s.entry.id.clone())
}

fn command_is_mcpm(command: &str) -> bool {
    std::path::Path::new(command)
        .file_name()
        .is_some_and(|name| name == "mcpm")
}

fn launches_mcpm(raw: &Value) -> bool {
    raw.get("command")
        .and_then(Value::as_str)
        .is_some_and(command_is_mcpm)
}

fn resolve(servers: &[MappedServer], obj: &Map<String, Value>) -> Resolved {
    if let Some(url) = obj.get("url").and_then(Value::as_str) {
        return match by_url(servers, url) {
            Some(id) => Resolved::Servers(vec![id]),
            None => Resolved::Skip("unmanaged-url"),
        };
    }
    let command = obj.get("command").and_then(Value::as_str).unwrap_or("");
    let args: Vec<&str> = obj
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if command_is_mcpm(command) {
        return match args.as_slice() {
            ["run", name] => by_name(servers, name)
                .map(|id| Resolved::Servers(vec![id]))
                .unwrap_or(Resolved::Skip("unknown-server")),
            ["profile", "run", tag] => {
                let ids = tag_members(servers, tag);
                if ids.is_empty() {
                    Resolved::Skip("unknown-profile")
                } else {
                    Resolved::Servers(ids)
                }
            }
            _ => Resolved::Skip("unmanaged-command"),
        };
    }
    if args.contains(&"mcp-proxy") {
        if let Some(id) = args
            .iter()
            .filter(|a| a.starts_with("https://") || a.starts_with("http://"))
            .find_map(|u| by_url(servers, u))
        {
            return Resolved::Servers(vec![id]);
        }
    }
    Resolved::Skip("unmanaged-command")
}

pub fn map_clients(
    servers: &[MappedServer],
    _input: &McpmInput,
    clients: &[ClientConfig],
) -> ClientMapping {
    let mut tags: Vec<String> = servers
        .iter()
        .filter(|s| s.enabled)
        .flat_map(|s| s.tags.clone())
        .collect();
    tags.sort();
    tags.dedup();
    let mut profiles: Vec<Profile> = tags
        .iter()
        .map(|t| profile(&slugify(t), t, tag_members(servers, t)))
        .collect();
    let mut client_scopes = BTreeMap::new();
    let mut client_discovery = BTreeMap::new();
    let mut skipped = Vec::new();
    let mut entries = Vec::new();
    for client in clients {
        let mut ids: Vec<String> = Vec::new();
        let mut mapped = Vec::new();
        let mut orphans = Vec::new();
        let mut client_skipped = Vec::new();
        for (key, raw) in &client.servers {
            let resolved = match raw.as_object() {
                Some(obj) => resolve(servers, obj),
                None => Resolved::Skip("invalid"),
            };
            match resolved {
                Resolved::Servers(found) => {
                    mapped.push(MappedEntry {
                        key: key.clone(),
                        server_ids: found.clone(),
                    });
                    for id in found {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
                Resolved::Skip(reason) => {
                    orphans.push(key.clone());
                    client_skipped.push(ClientSkip {
                        client_id: client.client_id.clone(),
                        entry: key.clone(),
                        reason: reason.into(),
                    });
                }
            }
        }
        if client.live && mapped.is_empty() && !client.servers.values().any(launches_mcpm) {
            continue;
        }
        skipped.extend(client_skipped);
        ids.sort();
        let pid = slugify(&client.client_id);
        profiles.push(profile(&pid, &client.client_id, ids));
        client_scopes.insert(client.client_id.clone(), pid.clone());
        let mode = if client.client_id == "claude-code" {
            "full"
        } else {
            "lazy"
        };
        client_discovery.insert(client.client_id.clone(), mode.to_string());
        entries.push(ClientEntries {
            client_id: client.client_id.clone(),
            profile_id: pid,
            mapped,
            orphans,
        });
    }
    ClientMapping {
        entries,
        profiles,
        client_scopes,
        client_discovery,
        skipped,
        warnings: Vec::new(),
    }
}
