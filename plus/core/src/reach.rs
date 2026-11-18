//! Who reaches a server: the default access, each client's own access set, folder routing and the
//! direct entries Toolport wrote. The access sets are resolved by upstream's own functions behind
//! `RegistryView::scope_reach`; this module only joins them with the servers, the clients and the
//! direct-entry records. Read-only: `toolportctl client reach`, `clients_reach` and the Reach row
//! of the desktop app render `assess`, and `access_rows` is what the Access check of Attention
//! reads.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use toolport_plus_api::{Host, ScopeKind, ScopeReach, ServerRecord};

use crate::direct;
use crate::op::OpError;

/// The precedence the gateway applies, as `effective_profile` in `toolport-gateway.rs` does:
/// a folder mapping first, then the client's own access set, then `TOOLPORT_PROFILE`, then
/// Default access.
pub const FOLDER_RULE: &str = "A folder mapping outranks a client's own access set: inside a \
     mapped folder, and below it, the gateway uses the folder's set for every client. Order: \
     folder mapping, the client's own set, TOOLPORT_PROFILE, Default access.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub set_id: Option<String>,
    pub set_name: Option<String>,
    pub dangling: bool,
    pub server_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRow {
    pub id: String,
    pub name: String,
    /// The client holds Toolport's gateway entry.
    pub gateway: bool,
    /// The client has an access set of its own; otherwise it follows the default access.
    pub own_set: bool,
    pub scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRow {
    pub path: String,
    pub scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetRow {
    pub id: String,
    pub name: String,
    pub listed: Vec<String>,
    pub reached: Vec<String>,
    pub default: bool,
    pub clients: Vec<String>,
    pub folders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectReach {
    pub client: String,
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerReach {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub listed_in: Vec<String>,
    pub default: bool,
    pub clients: Vec<String>,
    pub folders: Vec<String>,
    pub direct: Vec<DirectReach>,
    /// Switched on and reached by no access set: not by the default access, not by a client's own
    /// set and not by a folder mapping.
    pub unreached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    Unreached,
    FolderOutranks,
    MissingSet,
    /// A direct entry starts a server that was removed (`server uninstall --keep-clients`).
    DirectOrphan,
}

impl FindingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreached => "unreached",
            Self::FolderOutranks => "folder-outranks",
            Self::MissingSet => "missing-set",
            Self::DirectOrphan => "direct-orphan",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: FindingKind,
    pub server: Option<String>,
    pub client: Option<String>,
    pub folder: Option<String>,
    /// Server ids the finding is about.
    pub servers: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub default_access: Option<Scope>,
    pub sets: Vec<SetRow>,
    pub clients: Vec<ClientRow>,
    pub folders: Vec<FolderRow>,
    pub servers: Vec<ServerReach>,
    pub findings: Vec<Finding>,
    /// The rows of the Access check in Attention, from the whole report even when it is narrowed.
    pub access_rows: Vec<AccessRow>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Filter<'a> {
    pub server: Option<&'a str>,
    pub client: Option<&'a str>,
}

fn names_of(servers: &[ServerRecord], ids: &[String]) -> String {
    ids.iter()
        .map(|id| {
            servers
                .iter()
                .find(|server| &server.id == id)
                .map_or(id.as_str(), |server| server.name.as_str())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

pub(crate) fn scope_of(scope: &ScopeReach, set_names: &BTreeMap<String, String>) -> Scope {
    Scope {
        set_name: scope
            .set_id
            .as_ref()
            .and_then(|id| set_names.get(id).cloned()),
        set_id: scope.set_id.clone(),
        dangling: scope.dangling,
        server_ids: scope.server_ids.clone(),
    }
}

/// Joins the scopes with the servers, the clients that hold the gateway entry and the direct
/// entries. `direct_rows` are `direct::list` rows.
pub fn build(
    servers: &[ServerRecord],
    sets: &[toolport_plus_api::AccessSetInfo],
    scopes: &[ScopeReach],
    clients: &[toolport_plus_api::ClientInfo],
    direct_rows: &[direct::Row],
) -> Report {
    let set_names: BTreeMap<String, String> = sets
        .iter()
        .map(|set| (set.id.clone(), set.name.clone()))
        .collect();
    let default_scope = scopes.iter().find(|s| s.kind == ScopeKind::Default);
    let default_reach = default_scope.map(|scope| scope_of(scope, &set_names));

    let mut client_rows: Vec<ClientRow> = Vec::new();
    for scope in scopes.iter().filter(|s| s.kind == ScopeKind::Client) {
        let known = clients.iter().find(|client| client.id == scope.key);
        client_rows.push(ClientRow {
            id: scope.key.clone(),
            name: known.map_or_else(|| scope.key.clone(), |client| client.name.clone()),
            gateway: known.is_some_and(|client| client.gateway_installed),
            own_set: !scope.reference.is_empty(),
            scope: scope_of(scope, &set_names),
        });
    }
    if let Some(default) = &default_reach {
        for client in clients.iter().filter(|c| c.gateway_installed) {
            if client_rows.iter().all(|row| row.id != client.id) {
                client_rows.push(ClientRow {
                    id: client.id.clone(),
                    name: client.name.clone(),
                    gateway: true,
                    own_set: false,
                    scope: default.clone(),
                });
            }
        }
    }
    client_rows.sort_by(|a, b| a.id.cmp(&b.id));

    let folder_rows: Vec<FolderRow> = scopes
        .iter()
        .filter(|s| s.kind == ScopeKind::Folder)
        .map(|scope| FolderRow {
            path: scope.key.clone(),
            scope: scope_of(scope, &set_names),
        })
        .collect();

    let set_rows: Vec<SetRow> = sets
        .iter()
        .map(|set| SetRow {
            id: set.id.clone(),
            name: set.name.clone(),
            listed: set.server_ids.clone(),
            reached: servers
                .iter()
                .filter(|server| server.enabled && set.server_ids.contains(&server.id))
                .map(|server| server.id.clone())
                .collect(),
            default: default_reach
                .as_ref()
                .is_some_and(|d| d.set_id.as_deref() == Some(&set.id)),
            clients: client_rows
                .iter()
                .filter(|c| c.scope.set_id.as_deref() == Some(&set.id))
                .map(|c| c.id.clone())
                .collect(),
            folders: folder_rows
                .iter()
                .filter(|f| f.scope.set_id.as_deref() == Some(&set.id))
                .map(|f| f.path.clone())
                .collect(),
        })
        .collect();

    let server_rows: Vec<ServerReach> = servers
        .iter()
        .map(|server| {
            let id = &server.id;
            let default = default_reach
                .as_ref()
                .is_some_and(|d| d.server_ids.contains(id));
            let clients: Vec<String> = client_rows
                .iter()
                .filter(|c| c.scope.server_ids.contains(id))
                .map(|c| c.id.clone())
                .collect();
            let folders: Vec<String> = folder_rows
                .iter()
                .filter(|f| f.scope.server_ids.contains(id))
                .map(|f| f.path.clone())
                .collect();
            let direct: Vec<DirectReach> = direct_rows
                .iter()
                .filter(|row| &row.server == id)
                .map(|row| DirectReach {
                    client: row.client.clone(),
                    state: row.state.as_str(),
                })
                .collect();
            let own_set_reaches = client_rows
                .iter()
                .any(|c| c.own_set && c.scope.server_ids.contains(id));
            let unreached = server.enabled && !default && !own_set_reaches && folders.is_empty();
            ServerReach {
                id: id.clone(),
                name: server.name.clone(),
                enabled: server.enabled,
                listed_in: sets
                    .iter()
                    .filter(|set| set.server_ids.contains(id))
                    .map(|set| set.id.clone())
                    .collect(),
                default,
                clients,
                folders,
                direct,
                unreached,
            }
        })
        .collect();

    let findings = findings_of(
        servers,
        &server_rows,
        &client_rows,
        &folder_rows,
        direct_rows,
    );
    let mut report = Report {
        default_access: default_reach,
        sets: set_rows,
        clients: client_rows,
        folders: folder_rows,
        servers: server_rows,
        findings,
        access_rows: Vec::new(),
    };
    report.access_rows = access_rows(&report);
    report
}

fn findings_of(
    servers: &[ServerRecord],
    server_rows: &[ServerReach],
    clients: &[ClientRow],
    folders: &[FolderRow],
    direct_rows: &[direct::Row],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for row in server_rows
        .iter()
        .filter(|row| row.unreached && !row.direct.iter().any(|d| d.state == "ok"))
    {
        findings.push(Finding {
            kind: FindingKind::Unreached,
            server: Some(row.id.clone()),
            client: None,
            folder: None,
            servers: vec![row.id.clone()],
            text: format!(
                "No access set reaches {}: the default access, the clients' own sets and the \
                 folder mappings all leave it out.",
                row.name
            ),
        });
    }
    for row in direct_rows
        .iter()
        .filter(|row| row.state == direct::State::Orphan)
    {
        findings.push(Finding {
            kind: FindingKind::DirectOrphan,
            server: None,
            client: Some(row.client.clone()),
            folder: None,
            servers: Vec::new(),
            text: format!(
                "{} still has a direct entry, {}, for a server that was removed, so it reaches \
                 nothing through any access set. Remove it with `client direct rm {} --client {}`.",
                row.client_name, row.entry, row.server, row.client
            ),
        });
    }
    for client in clients.iter().filter(|c| c.scope.dangling) {
        findings.push(Finding {
            kind: FindingKind::MissingSet,
            server: None,
            client: Some(client.id.clone()),
            folder: None,
            servers: Vec::new(),
            text: format!(
                "{} is bound to an access set that no longer exists, so it sees no servers.",
                client.name
            ),
        });
    }
    for folder in folders.iter().filter(|f| f.scope.dangling) {
        findings.push(Finding {
            kind: FindingKind::MissingSet,
            server: None,
            client: None,
            folder: Some(folder.path.clone()),
            servers: Vec::new(),
            text: format!(
                "The folder mapping for {} names an access set that no longer exists, so \
                 clients started there see no servers.",
                folder.path
            ),
        });
    }
    for folder in folders.iter().filter(|f| !f.scope.dangling) {
        for client in clients
            .iter()
            .filter(|c| c.own_set && !c.scope.dangling && c.scope.set_id != folder.scope.set_id)
        {
            findings.extend(folder_outranks(
                servers,
                &client.id,
                &client.name,
                &client.scope,
                folder,
            ));
        }
    }
    findings
}

/// The finding for a client with its own set when a folder mapping decides instead; `None` when
/// the folder's set reaches the same servers.
pub fn folder_outranks(
    servers: &[ServerRecord],
    client_id: &str,
    client_name: &str,
    own_scope: &Scope,
    folder: &FolderRow,
) -> Option<Finding> {
    let folder_set: BTreeSet<&String> = folder.scope.server_ids.iter().collect();
    let set_name = folder
        .scope
        .set_name
        .clone()
        .unwrap_or_else(|| "its access set".to_string());
    let own: BTreeSet<&String> = own_scope.server_ids.iter().collect();
    let hidden: Vec<String> = own
        .difference(&folder_set)
        .map(|id| (*id).clone())
        .collect();
    let added: Vec<String> = folder_set
        .difference(&own)
        .map(|id| (*id).clone())
        .collect();
    if hidden.is_empty() && added.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !hidden.is_empty() {
        parts.push(format!(
            "hides {} its own set lists ({})",
            plural(hidden.len(), "server", "servers"),
            names_of(servers, &hidden)
        ));
    }
    if !added.is_empty() {
        parts.push(format!(
            "adds {} its own set leaves out ({})",
            plural(added.len(), "server", "servers"),
            names_of(servers, &added)
        ));
    }
    let mut touched = hidden;
    touched.extend(added);
    Some(Finding {
        kind: FindingKind::FolderOutranks,
        server: None,
        client: Some(client_id.to_string()),
        folder: Some(folder.path.clone()),
        servers: touched,
        text: format!(
            "In {} the gateway uses {set_name}, not {client_name}'s own set: it {}. {FOLDER_RULE}",
            folder.path,
            parts.join(" and ")
        ),
    })
}

fn scope_value(scope: &Scope) -> Value {
    json!({
        "accessSet": scope.set_id,
        "accessSetName": scope.set_name,
        "missing": scope.dangling,
        "servers": scope.server_ids,
    })
}

impl Report {
    pub fn to_value(&self) -> Value {
        json!({
            "defaultAccess": self.default_access.as_ref().map(scope_value),
            "accessSets": self.sets.iter().map(|set| json!({
                "id": set.id,
                "name": set.name,
                "listed": set.listed,
                "reached": set.reached,
                "default": set.default,
                "clients": set.clients,
                "folders": set.folders,
            })).collect::<Vec<_>>(),
            "clients": self.clients.iter().map(|client| {
                let mut value = scope_value(&client.scope);
                value["id"] = json!(client.id);
                value["name"] = json!(client.name);
                value["gateway"] = json!(client.gateway);
                value["ownSet"] = json!(client.own_set);
                value
            }).collect::<Vec<_>>(),
            "folders": self.folders.iter().map(|folder| {
                let mut value = scope_value(&folder.scope);
                value["path"] = json!(folder.path);
                value
            }).collect::<Vec<_>>(),
            "servers": self.servers.iter().map(|server| json!({
                "id": server.id,
                "name": server.name,
                "enabled": server.enabled,
                "listedIn": server.listed_in,
                "default": server.default,
                "clients": server.clients,
                "folders": server.folders,
                "direct": server.direct.iter().map(|d| json!({
                    "client": d.client,
                    "state": d.state,
                })).collect::<Vec<_>>(),
                "unreached": server.unreached,
            })).collect::<Vec<_>>(),
            "findings": self.findings.iter().map(|finding| json!({
                "kind": finding.kind.as_str(),
                "server": finding.server,
                "client": finding.client,
                "folder": finding.folder,
                "servers": finding.servers,
                "text": finding.text,
            })).collect::<Vec<_>>(),
            "accessRows": self.access_rows.iter().map(|row| json!({
                "id": row.id,
                "title": row.title,
                "detail": row.detail,
                "action": {"label": row.action_label, "command": row.command},
            })).collect::<Vec<_>>(),
        })
    }

    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();
        for server in &self.servers {
            let mut by = Vec::new();
            if server.default {
                by.push("default access".to_string());
            }
            by.extend(server.clients.iter().map(|c| format!("client {c}")));
            by.extend(server.folders.iter().map(|f| format!("folder {f}")));
            by.extend(
                server
                    .direct
                    .iter()
                    .map(|d| format!("direct entry in {} ({})", d.client, d.state)),
            );
            let state = if !server.enabled {
                "off".to_string()
            } else if by.is_empty() {
                "reached by nobody".to_string()
            } else {
                by.join(", ")
            };
            lines.push(format!("{:<24} {state}", server.name));
        }
        if self.servers.is_empty() {
            lines.push("No servers.".into());
        }
        for finding in &self.findings {
            lines.push(format!("! {}", finding.text));
        }
        lines.join("\n")
    }
}

fn read(host: &dyn Host) -> Result<Report, OpError> {
    let Some(view) = host
        .read_view()
        .map_err(|error| OpError::failed("registry_error", error))?
    else {
        return Ok(Report::default());
    };
    let servers = view
        .servers()
        .map_err(|error| OpError::failed("registry_error", error))?;
    let direct_rows = direct::list(host, None).unwrap_or_default();
    Ok(build(
        &servers,
        &view.access_sets(),
        &view.scope_reach(),
        &host.detect_clients(),
        &direct_rows,
    ))
}

/// The whole report narrowed to one server and/or one client. Unknown keys are `not_found`.
pub fn assess(host: &dyn Host, filter: Filter) -> Result<Report, OpError> {
    let mut report = read(host)?;
    let server_id = match filter.server {
        Some(key) => {
            let key = key.trim();
            let found = report
                .servers
                .iter()
                .find(|s| s.id == key)
                .or_else(|| {
                    report
                        .servers
                        .iter()
                        .find(|s| s.name.eq_ignore_ascii_case(key))
                })
                .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?;
            Some(found.id.clone())
        }
        None => None,
    };
    if let Some(key) = filter.client {
        let id = report
            .clients
            .iter()
            .find(|c| c.id == key || c.name.eq_ignore_ascii_case(key))
            .map(|c| c.id.clone())
            .ok_or_else(|| OpError::not_found(format!("no client '{key}' reaches the gateway")))?;
        report.clients.retain(|c| c.id == id);
        report
            .findings
            .retain(|f| f.client.as_deref().is_none_or(|c| c == id));
        for server in &mut report.servers {
            server.clients.retain(|c| *c == id);
            server.direct.retain(|d| d.client == id);
        }
    }
    if let Some(id) = server_id {
        report.servers.retain(|s| s.id == id);
        report
            .findings
            .retain(|f| f.servers.contains(&id) || f.servers.is_empty() && f.server.is_none());
    }
    Ok(report)
}

/// One row of the Access check in Attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRow {
    /// Carries what the row is about, so a row dismissed for a deliberate choice comes back when
    /// another server or client is affected.
    pub id: String,
    pub title: String,
    pub detail: String,
    pub action_label: &'static str,
    pub command: Vec<String>,
}

/// The hook the Access check of Attention calls: "N servers no access set reaches" as one row and
/// one row per mapped folder that outranks the own sets of clients. The attention snapshot lives
/// on its own branch, so this returns plain rows for it to wrap in its `Item`.
pub fn access_rows(report: &Report) -> Vec<AccessRow> {
    let command = vec!["toolportctl".into(), "client".into(), "reach".into()];
    let mut rows = Vec::new();
    let unreached: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|f| f.kind == FindingKind::Unreached)
        .collect();
    if !unreached.is_empty() {
        let mut ids: Vec<&str> = unreached
            .iter()
            .filter_map(|f| f.server.as_deref())
            .collect();
        ids.sort_unstable();
        let names: Vec<&str> = ids
            .iter()
            .map(|id| {
                report
                    .servers
                    .iter()
                    .find(|s| s.id == *id)
                    .map_or(*id, |s| s.name.as_str())
            })
            .collect();
        rows.push(AccessRow {
            id: format!("access:unreached:{}", ids.join(",")),
            title: format!(
                "{} no access set reaches",
                plural(ids.len(), "server", "servers")
            ),
            detail: format!(
                "{} {} on, but neither the default access, a client's own set nor a folder \
                 mapping lists {}. Add {} to a set, or switch {} off.",
                names.join(", "),
                if ids.len() == 1 { "is" } else { "are" },
                if ids.len() == 1 { "it" } else { "them" },
                if ids.len() == 1 { "it" } else { "them" },
                if ids.len() == 1 { "it" } else { "them" },
            ),
            action_label: "Review",
            command: command.clone(),
        });
    }
    let mut by_folder: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for finding in report
        .findings
        .iter()
        .filter(|f| f.kind == FindingKind::FolderOutranks)
    {
        if let Some(folder) = &finding.folder {
            by_folder.entry(folder).or_default().push(finding);
        }
    }
    for (folder, findings) in by_folder {
        let mut clients: Vec<&str> = findings
            .iter()
            .filter_map(|f| f.client.as_deref())
            .collect();
        clients.sort_unstable();
        rows.push(AccessRow {
            id: format!("access:folder:{folder}:{}", clients.join(",")),
            title: format!(
                "Folder {folder} uses its own access set for {}",
                plural(clients.len(), "client", "clients")
            ),
            detail: findings
                .iter()
                .map(|f| f.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            action_label: "Review",
            command: command.clone(),
        });
    }
    rows
}

/// Reads the registry and the clients and returns the rows of the Access check.
pub fn access_rows_of(host: &dyn Host) -> Result<Vec<AccessRow>, OpError> {
    Ok(read(host)?.access_rows)
}

#[cfg(test)]
mod tests;
