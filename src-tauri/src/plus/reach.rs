//! How each client reaches each server: through the gateway, through a Toolport launcher entry,
//! or through an entry the client holds on its own (a remote server the client signs in to itself,
//! a hand-written command). Read-only: it reads the registry and the client configs and changes
//! neither. `toolportctl client reach`, the `clients_reach` self-MCP tool and the server detail
//! of the desktop app are renderers over `assess`.

use crate::clients::{self, DetectedClient, GatewayEntryState, McpServer};
use claude_code::Held;
use crate::plus::direct::{self, launcher_target};
use crate::plus::op::OpError;
use crate::plus::servers;
use crate::registry::{Registry, ServerEntry};
use serde_json::{json, Value};

pub mod claude_code;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Gateway,
    Launcher,
    OwnEntry,
}

impl How {
    pub fn as_str(self) -> &'static str {
        match self {
            How::Gateway => "gateway",
            How::Launcher => "launcher",
            How::OwnEntry => "own-entry",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub client: String,
    pub client_name: String,
    pub how: How,
    pub entry: Option<String>,
    pub transport: Option<String>,
    pub target: Option<String>,
    pub state: Option<String>,
    pub basis: Option<&'static str>,
    pub recorded: bool,
    pub note: Option<String>,
    pub held: Option<Held>,
}

impl Route {
    fn to_value(&self) -> Value {
        json!({
            "client": self.client,
            "clientName": self.client_name,
            "how": self.how.as_str(),
            "entry": self.entry,
            "transport": self.transport,
            "target": self.target,
            "state": self.state,
            "basis": self.basis,
            "recorded": self.recorded,
            "note": self.note,
            "held": self.held.as_ref().map(|h| json!({
                "enabled": h.enabled(),
                "disabledIn": h.disabled_in,
                "live": h.live,
            })),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerReach {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub profiles: Vec<String>,
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unregistered {
    pub client: String,
    pub client_name: String,
    pub entry: String,
    pub transport: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: &'static str,
    pub kind: &'static str,
    pub server: Option<String>,
    pub client: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRow {
    pub id: String,
    pub name: String,
    pub gateway: &'static str,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Report {
    pub clients: Vec<ClientRow>,
    pub servers: Vec<ServerReach>,
    pub unregistered: Vec<Unregistered>,
    pub findings: Vec<Finding>,
}

fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn same_url(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim().trim_end_matches('/').to_ascii_lowercase();
    trim(a) == trim(b)
}

/// How an entry a client holds on its own matches a registered server, strongest first.
fn basis_of(entry: &McpServer, server: &ServerEntry) -> Option<&'static str> {
    let name = squash(&entry.name);
    if !name.is_empty() && (name == squash(&server.name) || name == squash(&server.id)) {
        return Some("name");
    }
    if let (Some(a), Some(b)) = (&entry.url, &server.url) {
        if same_url(a, b) {
            return Some("url");
        }
    }
    if let (Some(a), Some(b)) = (&entry.command, &server.command) {
        if a == b && entry.args == server.args {
            return Some("command");
        }
    }
    None
}

/// What the client starts or calls, without anything that can carry a credential: a url loses its
/// user info and query, a command loses its arguments.
fn target_of(entry: &McpServer) -> Option<String> {
    if let Some(url) = &entry.url {
        let bare = url.split(['?', '#']).next().unwrap_or(url);
        return Some(match bare.split_once("://") {
            Some((scheme, rest)) => {
                let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
                format!("{scheme}://{rest}")
            }
            None => bare.to_string(),
        });
    }
    let command = entry.command.as_deref()?;
    let base = command.rsplit(['/', '\\']).next().unwrap_or(command);
    Some(match entry.args.len() {
        0 => base.to_string(),
        1 => format!("{base} (1 argument)"),
        n => format!("{base} ({n} arguments)"),
    })
}

fn profile_name(reg: &Registry, id: &str) -> Option<String> {
    reg.profiles
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
}

fn gateway_label(client: &DetectedClient) -> &'static str {
    match client.entry_state {
        GatewayEntryState::Managed => "managed",
        GatewayEntryState::Customized => "customized",
        GatewayEntryState::Absent => "absent",
    }
}

pub fn assess(reg: &Registry, detected: &[DetectedClient]) -> Report {
    let launchers = direct::assess(reg, detected);
    let present: Vec<&DetectedClient> = detected
        .iter()
        .filter(|c| (c.config_exists || c.app_present) && c.error.is_none())
        .collect();
    let mut report = Report::default();
    let mut reached: Vec<Vec<Route>> = vec![Vec::new(); reg.servers.len()];
    let active = reg.active_profile_id();
    let mut left_out: Vec<Vec<(&DetectedClient, String)>> = vec![Vec::new(); reg.servers.len()];
    for client in &present {
        let scope = reg.client_scopes.get(&client.id).map_or("", String::as_str);
        let profile_id = reg.resolve_profile_id(scope);
        let gateway = client.entry_state != GatewayEntryState::Absent;
        report.clients.push(ClientRow {
            id: client.id.clone(),
            name: client.name.clone(),
            gateway: gateway_label(client),
            profile: if gateway { profile_name(reg, &profile_id) } else { None },
        });
        if gateway {
            for (at, server) in reg.servers.iter().enumerate() {
                if reg.is_enabled(&profile_id, &server.id) {
                    reached[at].push(Route {
                        client: client.id.clone(),
                        client_name: client.name.clone(),
                        how: How::Gateway,
                        entry: None,
                        transport: None,
                        target: None,
                        state: None,
                        basis: None,
                        recorded: false,
                        note: (client.entry_state == GatewayEntryState::Customized)
                            .then(|| "the gateway entry was edited by hand".to_string()),
                        held: None,
                    });
                } else if profile_id != active
                    && reg.is_enabled(&active, &server.id)
                    && !clients::is_gateway_server(server)
                {
                    let profile = profile_name(reg, &profile_id).unwrap_or_else(|| profile_id.clone());
                    left_out[at].push((client, profile));
                }
            }
        }
        for entry in client.servers.iter().filter(|s| !clients::detected_is_gateway(s)) {
            let row = launchers
                .iter()
                .find(|l| l.client == client.id && l.entry == entry.name);
            let transport = Some(entry.transport.clone());
            let target = target_of(entry);
            if let Some(id) = launcher_target(entry) {
                let at = servers::find(reg, id)
                    .and_then(|s| reg.servers.iter().position(|r| r.id == s.id));
                let state = row.map(|r| r.state.as_str().to_string());
                match at {
                    Some(at) => reached[at].push(Route {
                        client: client.id.clone(),
                        client_name: client.name.clone(),
                        how: How::Launcher,
                        entry: Some(entry.name.clone()),
                        transport,
                        target,
                        state,
                        basis: None,
                        recorded: row.is_some_and(|r| r.state != direct::State::Unrecorded),
                        note: None,
                        held: None,
                    }),
                    None => report.findings.push(Finding {
                        level: "look",
                        kind: "orphan-launcher",
                        server: Some(id.to_string()),
                        client: Some(client.id.clone()),
                        text: format!(
                            "{} has the launcher entry '{}' for '{id}', which is not in the registry.",
                            client.name, entry.name
                        ),
                    }),
                }
                continue;
            }
            let recorded_server = row.and_then(|r| servers::find(reg, &r.server));
            let (matched, basis) = match recorded_server {
                Some(server) => (Some(server), Some("record")),
                None => reg
                    .servers
                    .iter()
                    .find_map(|s| basis_of(entry, s).map(|b| (s, b)))
                    .map_or((None, None), |(s, b)| (Some(s), Some(b))),
            };
            match matched.and_then(|s| reg.servers.iter().position(|r| r.id == s.id)) {
                Some(at) => reached[at].push(Route {
                    client: client.id.clone(),
                    client_name: client.name.clone(),
                    how: How::OwnEntry,
                    entry: Some(entry.name.clone()),
                    transport,
                    target,
                    state: row.map(|r| r.state.as_str().to_string()),
                    basis,
                    recorded: row.is_some(),
                    note: row.map(|_| {
                        format!(
                            "Toolport recorded this as a launcher it wrote, but it was edited since and is now a {} entry",
                            entry.transport
                        )
                    }),
                    held: None,
                }),
                None => report.unregistered.push(Unregistered {
                    client: client.id.clone(),
                    client_name: client.name.clone(),
                    entry: entry.name.clone(),
                    transport: entry.transport.clone(),
                    target,
                }),
            }
        }
    }
    let active_name = profile_name(reg, &active).unwrap_or_else(|| active.clone());
    for ((server, routes), left_out) in reg.servers.iter().zip(reached).zip(left_out) {
        let profiles: Vec<String> = reg
            .profiles
            .iter()
            .filter(|p| p.enabled_server_ids.iter().any(|id| *id == server.id))
            .map(|p| p.name.clone())
            .collect();
        for (client, profile) in left_out {
            if routes.iter().any(|r| r.client == client.id) {
                continue;
            }
            report.findings.push(Finding {
                level: "look",
                kind: "left-out-by-profile",
                server: Some(server.id.clone()),
                client: Some(client.id.clone()),
                text: format!(
                    "{} is on in {active_name}, but the profile {profile} of {} leaves it out, \
                     so {} does not reach it.",
                    server.name, client.name, client.name
                ),
            });
        }
        for route in routes.iter().filter(|r| r.how == How::OwnEntry) {
            let also_gateway = routes
                .iter()
                .any(|r| r.client == route.client && r.how == How::Gateway);
            let entry = route.entry.as_deref().unwrap_or("");
            if also_gateway {
                report.findings.push(Finding {
                    level: "look",
                    kind: "twice",
                    server: Some(server.id.clone()),
                    client: Some(route.client.clone()),
                    text: format!(
                        "{} reaches {} twice: through the gateway and through its own entry '{entry}'.",
                        route.client_name, server.name
                    ),
                });
            }
            if profiles.is_empty() {
                report.findings.push(Finding {
                    level: "fyi",
                    kind: "direct-only",
                    server: Some(server.id.clone()),
                    client: Some(route.client.clone()),
                    text: format!(
                        "{} is in no profile, so the gateway does not serve it; {} reaches it \
                         through its own entry '{entry}', which Toolport does not manage.",
                        server.name, route.client_name
                    ),
                });
            }
        }
        report.servers.push(ServerReach {
            id: server.id.clone(),
            name: server.name.clone(),
            transport: server.transport.clone(),
            profiles,
            routes,
        });
    }
    report
}

impl Report {
    pub fn only(mut self, client: Option<&str>, server: Option<&str>) -> Result<Report, OpError> {
        if let Some(id) = client {
            if !self.clients.iter().any(|c| c.id == id) {
                let known: Vec<&str> = self.clients.iter().map(|c| c.id.as_str()).collect();
                return Err(OpError::not_found(format!(
                    "Client '{id}' is not installed here.\nInstalled clients: {}",
                    known.join(", ")
                )));
            }
            self.clients.retain(|c| c.id == id);
            self.unregistered.retain(|u| u.client == id);
            self.findings
                .retain(|f| f.client.as_deref().is_none_or(|c| c == id));
            for entry in &mut self.servers {
                entry.routes.retain(|r| r.client == id);
            }
        }
        if let Some(key) = server {
            let wanted = self
                .servers
                .iter()
                .find(|s| s.id == key)
                .or_else(|| self.servers.iter().find(|s| s.name.eq_ignore_ascii_case(key)))
                .map(|s| s.id.clone())
                .ok_or_else(|| OpError::not_found(format!("No server '{key}' in the registry.")))?;
            self.servers.retain(|s| s.id == wanted);
            self.unregistered.clear();
            self.findings
                .retain(|f| f.server.as_deref() == Some(wanted.as_str()));
        }
        Ok(self)
    }

    pub fn to_value(&self) -> Value {
        json!({
            "clients": self.clients.iter().map(|c| json!({
                "id": c.id,
                "name": c.name,
                "gateway": c.gateway,
                "profile": c.profile,
            })).collect::<Vec<_>>(),
            "servers": self.servers.iter().map(|s| json!({
                "id": s.id,
                "name": s.name,
                "transport": s.transport,
                "profiles": s.profiles,
                "routes": s.routes.iter().map(Route::to_value).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "unregistered": self.unregistered.iter().map(|u| json!({
                "client": u.client,
                "clientName": u.client_name,
                "entry": u.entry,
                "transport": u.transport,
                "target": u.target,
            })).collect::<Vec<_>>(),
            "findings": self.findings.iter().map(|f| json!({
                "level": f.level,
                "kind": f.kind,
                "server": f.server,
                "client": f.client,
                "text": f.text,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn text(&self) -> String {
        if self.servers.is_empty() && self.unregistered.is_empty() {
            return "No servers in the registry and no client entries to show.".to_string();
        }
        let mut lines = Vec::new();
        for server in &self.servers {
            let scope = if server.profiles.is_empty() {
                "in no profile".to_string()
            } else {
                format!("in {}", server.profiles.join(", "))
            };
            lines.push(format!("{} ({}, {scope})", server.name, server.transport));
            if server.routes.is_empty() {
                lines.push("  no client reaches it".to_string());
            }
            for route in &server.routes {
                lines.push(format!("  {}", route_line(route)));
            }
        }
        if !self.unregistered.is_empty() {
            lines.push(String::new());
            lines.push("Entries in clients that match no server of Toolport:".to_string());
            for entry in &self.unregistered {
                lines.push(format!(
                    "  {}: '{}' ({}{})",
                    entry.client,
                    entry.entry,
                    entry.transport,
                    entry.target.as_ref().map_or(String::new(), |t| format!(", {t}"))
                ));
            }
        }
        if !self.findings.is_empty() {
            lines.push(String::new());
            lines.extend(self.findings.iter().map(|f| format!("{}: {}", f.level, f.text)));
        }
        lines.join("\n")
    }
}

fn route_line(route: &Route) -> String {
    let mut line = match route.how {
        How::Gateway => format!("{:<16} through the gateway", route.client),
        How::Launcher => format!(
            "{:<16} through a Toolport launcher entry '{}'{}",
            route.client,
            route.entry.as_deref().unwrap_or(""),
            route.state.as_ref().map_or(String::new(), |s| format!(" ({s})"))
        ),
        How::OwnEntry => format!(
            "{:<16} through its own entry '{}' ({}{}), not through the gateway",
            route.client,
            route.entry.as_deref().unwrap_or(""),
            route.transport.as_deref().unwrap_or(""),
            route.target.as_ref().map_or(String::new(), |t| format!(", {t}"))
        ),
    };
    if let Some(basis) = route.basis {
        if basis != "record" {
            line.push_str(&format!("; matched by {basis}"));
        }
    }
    if let Some(note) = &route.note {
        line.push_str(&format!("; {note}"));
    }
    if let Some(held) = &route.held {
        line.push_str(&format!("; {}", held_line(held)));
    }
    line
}

fn held_line(held: &Held) -> String {
    let switch = match held.disabled_in.len() {
        0 => "switched on in Claude Code".to_string(),
        1 => "switched off in 1 project of Claude Code".to_string(),
        n => format!("switched off in {n} projects of Claude Code"),
    };
    match held.live.as_deref() {
        Some("connected") => format!("{switch}, connects"),
        Some("needs-auth") => format!("{switch}, needs a sign-in"),
        Some("failed") => format!("{switch}, fails to connect"),
        _ => switch,
    }
}

/// With `live`, Claude Code is also asked which servers it connects to; that starts them and takes
/// a few seconds, so it is opt-in.
pub fn report(client: Option<&str>, server: Option<&str>, live: bool) -> Result<Report, OpError> {
    let reg = crate::plus::registry_ro::read().map_err(|e| OpError::failed("registry_error", e))?;
    let detected = clients::detect_clients();
    let mut report = assess(&reg, &detected);
    let probed = live.then(|| claude_code::probe(&claude_code::runner()));
    claude_code::annotate(&mut report, &detected, probed.as_ref());
    report.only(client, server)
}

#[cfg(test)]
mod tests;
