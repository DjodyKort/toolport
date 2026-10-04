//! `client edit` and `client import`. A Toolport client has one gateway entry that follows one
//! profile (`clientScopes`), where an mcpm client holds one `mcpm_profile_<name>` entry per
//! profile: `client edit` maps the profile options onto that scope, so a client takes at most one
//! profile. Individual server entries do not exist in a client any more, and `client import` reads
//! a client's direct entries into the registry (values of their env are never read).

use super::{mutate, registry_error, resolve, Error, Kind};
use crate::clients::{self, DetectedClient, GatewayEntryState, McpServer};
use crate::plus::registry_ro;
use crate::registry::{self, Registry};
use crate::registry_controller;
use serde_json::{json, Value};

fn detect(reg: &Registry, client_id: &str) -> Result<DetectedClient, Error> {
    let mut detected = clients::detect_clients();
    clients::apply_entry_states(&mut detected, &reg.client_managed_entries);
    let Some(at) = detected.iter().position(|c| c.id == client_id) else {
        let known: Vec<&str> = detected.iter().map(|c| c.id.as_str()).collect();
        return Err(Error::not_found(format!(
            "Client '{client_id}' is not supported.\nAvailable clients: {}",
            known.join(", ")
        )));
    };
    let client = detected.swap_remove(at);
    if !client.config_exists && !client.app_present {
        return Err(Error::not_found(format!(
            "{} installation not detected.",
            client.name
        )));
    }
    Ok(client)
}

fn label(reg: &Registry, scope: &str) -> String {
    resolve(reg, scope).map_or_else(|_| scope.to_string(), |p| p.name.clone())
}

pub enum ProfileOp {
    Add(Vec<String>),
    Remove(Vec<String>),
    Set(Vec<String>),
}

pub struct ClientEdited {
    pub client: String,
    pub name: String,
    pub path: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub scope_before: Option<String>,
    pub scope_after: Option<String>,
    pub not_in_client: Vec<String>,
    pub follows_active: Option<String>,
    pub backup: Option<String>,
    pub dry_run: bool,
}

impl ClientEdited {
    pub fn added(&self) -> Vec<String> {
        self.after
            .iter()
            .filter(|n| !self.before.contains(n))
            .cloned()
            .collect()
    }

    pub fn removed(&self) -> Vec<String> {
        self.before
            .iter()
            .filter(|n| !self.after.contains(n))
            .cloned()
            .collect()
    }

    pub fn changed(&self) -> bool {
        self.scope_before != self.scope_after
    }

    pub fn to_value(&self) -> Value {
        json!({
            "client": self.client,
            "name": self.name,
            "path": self.path,
            "dryRun": self.dry_run,
            "changed": self.changed(),
            "profiles": {
                "before": self.before,
                "after": self.after,
                "added": self.added(),
                "removed": self.removed(),
            },
            "scope": {"before": self.scope_before, "after": self.scope_after},
            "followsActive": self.follows_active,
            "notInClient": self.not_in_client,
            "backup": self.backup,
        })
    }
}

fn profile_ids(reg: &Registry, keys: &[String]) -> Result<Vec<String>, Error> {
    let mut ids = Vec::new();
    let mut unknown = Vec::new();
    for key in keys {
        match resolve(reg, key) {
            Ok(profile) if !ids.contains(&profile.id) => ids.push(profile.id.clone()),
            Ok(_) => {}
            Err(e) if e.kind == Kind::NotFound => unknown.push(key.clone()),
            Err(e) => return Err(e),
        }
    }
    if unknown.is_empty() {
        Ok(ids)
    } else {
        Err(Error::not_found(format!(
            "Profile(s) not found: {}",
            unknown.join(", ")
        )))
    }
}

fn shares_http(reg: &Registry, client_id: &str) -> bool {
    let http_id = format!("client:{client_id}");
    reg.http_clients.iter().any(|c| c.id == http_id)
}

pub fn edit(
    client_id: &str,
    op: Option<&ProfileOp>,
    force: bool,
    dry_run: bool,
) -> Result<ClientEdited, Error> {
    let reg = registry_ro::read().map_err(registry_error)?;
    let client = detect(&reg, client_id)?;
    let installed = client.entry_state != GatewayEntryState::Absent;
    let current: Option<String> = reg
        .client_scopes
        .get(client_id)
        .map(|s| s.trim().to_string())
        .filter(|s| installed && !s.is_empty());
    let mut not_in_client = Vec::new();
    let target: Option<String> = match op {
        None => current.clone(),
        Some(ProfileOp::Add(keys)) => {
            let ids = profile_ids(&reg, keys)?;
            match (&current, ids.as_slice()) {
                (Some(have), [one]) if have != one => {
                    return Err(Error::conflict(format!(
                        "{} already follows profile '{}'; a client follows one profile, so use --set-profiles to switch",
                        client.name,
                        label(&reg, have)
                    )))
                }
                (_, []) => current.clone(),
                (_, [one]) => Some(one.clone()),
                _ => return Err(one_profile()),
            }
        }
        Some(ProfileOp::Remove(keys)) => {
            let mut keep = current.clone();
            for key in keys {
                let id = resolve(&reg, key).map_or_else(|_| key.clone(), |p| p.id.clone());
                if keep.as_deref() == Some(id.as_str()) {
                    keep = None;
                } else {
                    not_in_client.push(key.clone());
                }
            }
            keep
        }
        Some(ProfileOp::Set(keys)) => {
            let ids = profile_ids(&reg, keys)?;
            match ids.as_slice() {
                [] => None,
                [one] => Some(one.clone()),
                _ => return Err(one_profile()),
            }
        }
    };
    let backup = if target != current && !dry_run {
        if shares_http(&reg, client_id) {
            return Err(Error::conflict(format!(
                "{} uses Toolport Shared HTTP; change its profile in the app",
                client.name
            )));
        }
        if client.entry_state == GatewayEntryState::Customized && !force {
            return Err(Error::conflict(format!(
                "{}'s Toolport entry has a custom configuration; use --force to replace it with the default gateway",
                client.name
            )));
        }
        registry_controller::connect_client_stdio(client_id, target.as_deref(), force)
            .map_err(|e| Error::failed("client_config", e))?
            .outcome
            .backup
    } else {
        None
    };
    let names =
        |scope: &Option<String>| -> Vec<String> { scope.iter().map(|s| label(&reg, s)).collect() };
    let follows_active =
        (target.is_none() && installed).then(|| label(&reg, &reg.active_profile_id()));
    Ok(ClientEdited {
        client: client.id.clone(),
        name: client.name.clone(),
        path: client.config_path.clone(),
        before: names(&current),
        after: names(&target),
        scope_before: current,
        scope_after: target,
        not_in_client,
        follows_active,
        backup,
        dry_run,
    })
}

fn one_profile() -> Error {
    Error::invalid("A client follows one profile; give one profile name")
}

#[derive(Default)]
pub struct ImportSpec {
    pub select: Vec<String>,
    pub all: bool,
    pub profile: Option<String>,
}

pub struct Direct {
    pub name: String,
    pub transport: String,
    pub target: String,
    pub status: &'static str,
}

pub struct ProfileResult {
    pub id: String,
    pub name: String,
    pub created: bool,
    pub servers: Vec<String>,
}

pub struct Imported {
    pub client: String,
    pub name: String,
    pub path: String,
    pub gateway: Vec<String>,
    pub direct: Vec<Direct>,
    pub selected: bool,
    pub imported: Vec<(String, String)>,
    pub skipped: Vec<(String, &'static str)>,
    pub profile: Option<ProfileResult>,
    pub secrets: Vec<(String, String)>,
    pub dry_run: bool,
}

impl Imported {
    pub fn to_value(&self) -> Value {
        json!({
            "client": self.client,
            "name": self.name,
            "path": self.path,
            "dryRun": self.dry_run,
            "selected": self.selected,
            "gateway": self.gateway,
            "direct": self.direct.iter().map(|d| json!({
                "name": d.name, "transport": d.transport, "target": d.target, "status": d.status,
            })).collect::<Vec<_>>(),
            "imported": self.imported.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
            "skipped": self.skipped.iter().map(|(name, why)| json!({"name": name, "reason": why})).collect::<Vec<_>>(),
            "profile": self.profile.as_ref().map(|p| json!({
                "id": p.id, "name": p.name, "created": p.created, "servers": p.servers,
            })),
            "secrets": self.secrets.iter().map(|(server, key)| json!({"server": server, "key": key})).collect::<Vec<_>>(),
        })
    }
}

pub const CREDENTIAL_HINT: &str = "Servers with an inline credential are not imported; add them with `toolportctl server new` and store the value with `toolportctl secret set`.";

const ALREADY: &str = "already in the registry";
const TAKEN: &str = "a different server with this name is installed";
pub const CREDENTIAL: &str = "it holds an inline credential";

/// What a direct entry runs, with every credential-looking argument and URL part masked, and
/// whether it held one. An inline credential is never printed or copied into the registry.
fn shown(server: &McpServer) -> (String, bool) {
    let mask = registry::secret_arg_mask(&server.args);
    let mut credential = mask.iter().any(|m| *m);
    let target = match (&server.command, &server.url) {
        (Some(command), _) => std::iter::once(command.as_str())
            .chain(server.args.iter().zip(&mask).map(|(arg, hidden)| {
                if *hidden {
                    "<redacted>"
                } else {
                    arg.as_str()
                }
            }))
            .collect::<Vec<_>>()
            .join(" "),
        (None, Some(url)) => {
            let clean = registry::redact_url_userinfo(url);
            credential |= clean != *url;
            let authority_at = clean.find("://").map_or(0, |at| at + 3);
            let rest = &clean[authority_at..];
            let path_at = authority_at + rest.find(['/', '?', '#']).unwrap_or(rest.len());
            if registry::arg_looks_secret(&clean[path_at..]) {
                credential = true;
                format!("{}/<redacted>", &clean[..path_at])
            } else {
                clean
            }
        }
        (None, None) => String::new(),
    };
    (target, credential)
}

fn registered<'a>(
    reg: &'a Registry,
    server: &McpServer,
) -> Option<&'a crate::registry::ServerEntry> {
    let key = clients::import_dedupe_key(&server.name, server.command.as_deref(), &server.args);
    reg.servers
        .iter()
        .find(|s| clients::import_dedupe_key(&s.name, s.command.as_deref(), &s.args) == key)
}

fn status_of(reg: &Registry, server: &McpServer) -> &'static str {
    if registered(reg, server).is_some() {
        "already"
    } else if reg
        .servers
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&server.name))
    {
        "name-taken"
    } else if shown(server).1 {
        "credential"
    } else {
        "importable"
    }
}

fn pick<'a>(
    client: &str,
    direct: &'a [McpServer],
    spec: &ImportSpec,
) -> Result<Vec<&'a McpServer>, Error> {
    if spec.all {
        return Ok(direct.iter().collect());
    }
    let mut picked = Vec::new();
    let mut unknown = Vec::new();
    for key in &spec.select {
        match direct
            .iter()
            .find(|s| s.name == *key)
            .or_else(|| direct.iter().find(|s| s.name.eq_ignore_ascii_case(key)))
        {
            Some(server) if !picked.iter().any(|p: &&McpServer| p.name == server.name) => {
                picked.push(server)
            }
            Some(_) => {}
            None => unknown.push(key.clone()),
        }
    }
    if unknown.is_empty() {
        Ok(picked)
    } else {
        Err(Error::not_found(format!(
            "Server(s) not found in {client}: {}",
            unknown.join(", ")
        )))
    }
}

struct Applied {
    added: Vec<(String, String)>,
    skipped: Vec<(String, &'static str)>,
    profile: Option<ProfileResult>,
    secrets: Vec<(String, String)>,
    changed: bool,
}

fn apply_import(
    reg: &mut Registry,
    client: &DetectedClient,
    chosen: &[&McpServer],
    spec: &ImportSpec,
) -> Result<Applied, Error> {
    let mut done = Applied {
        added: Vec::new(),
        skipped: Vec::new(),
        profile: None,
        secrets: Vec::new(),
        changed: false,
    };
    let mut matched: Vec<String> = Vec::new();
    for server in chosen {
        match status_of(reg, server) {
            "already" => {
                done.skipped.push((server.name.clone(), ALREADY));
                if let Some(existing) = registered(reg, server) {
                    matched.push(existing.id.clone());
                }
            }
            "name-taken" => done.skipped.push((server.name.clone(), TAKEN)),
            "credential" => done.skipped.push((server.name.clone(), CREDENTIAL)),
            _ => {
                let entry = registry_controller::server_from_detected(server, &client.id);
                let keys: Vec<String> = entry.env.iter().map(|e| e.key.clone()).collect();
                let id = registry_controller::apply_add_entry(reg, entry);
                done.secrets
                    .extend(keys.into_iter().map(|key| (id.clone(), key)));
                done.added.push((id.clone(), server.name.clone()));
                matched.push(id);
                done.changed = true;
            }
        }
    }
    let Some(wanted) = spec.profile.as_deref().map(str::trim) else {
        return Ok(done);
    };
    if wanted.is_empty() {
        return Err(Error::invalid("give the profile a name"));
    }
    let (id, created) = match resolve(reg, wanted) {
        Ok(profile) => (profile.id.clone(), false),
        Err(e) if e.kind == Kind::NotFound => {
            registry_controller::apply_create_profile(reg, wanted);
            let made = reg.profiles.last().expect("a profile was just added");
            (made.id.clone(), true)
        }
        Err(e) => return Err(e),
    };
    done.changed |= created;
    for server_id in &matched {
        done.changed |= !reg.is_enabled(&id, server_id);
        registry_controller::apply_server_enabled(reg, &id, server_id, true, false)
            .map_err(|e| Error::failed("registry_error", e))?;
    }
    let name = reg
        .profiles
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    done.profile = Some(ProfileResult {
        servers: matched
            .iter()
            .filter_map(|sid| reg.servers.iter().find(|s| &s.id == sid))
            .map(|s| s.name.clone())
            .collect(),
        id,
        name,
        created,
    });
    Ok(done)
}

pub fn import(client_id: &str, spec: &ImportSpec, dry_run: bool) -> Result<Imported, Error> {
    let reg = registry_ro::read().map_err(registry_error)?;
    let client = detect(&reg, client_id)?;
    if !client.config_exists {
        return Err(Error::not_found(format!(
            "No configuration found for {}.",
            client.name
        )));
    }
    let (gateway, direct): (Vec<&McpServer>, Vec<&McpServer>) = client
        .servers
        .iter()
        .partition(|s| clients::detected_is_gateway(s));
    let direct: Vec<McpServer> = direct.into_iter().cloned().collect();
    let mut out = Imported {
        client: client.id.clone(),
        name: client.name.clone(),
        path: client.config_path.clone(),
        gateway: gateway.iter().map(|s| s.name.clone()).collect(),
        direct: direct
            .iter()
            .map(|s| Direct {
                name: s.name.clone(),
                transport: s.transport.clone(),
                target: shown(s).0,
                status: status_of(&reg, s),
            })
            .collect(),
        selected: spec.all || !spec.select.is_empty(),
        imported: Vec::new(),
        skipped: Vec::new(),
        profile: None,
        secrets: Vec::new(),
        dry_run,
    };
    if !out.selected {
        return Ok(out);
    }
    let chosen = pick(&client.name, &direct, spec)?;
    let (_, done) = mutate(
        dry_run,
        |reg| apply_import(reg, &client, &chosen, spec),
        |done| done.changed,
    )?;
    out.imported = done.added;
    out.skipped = done.skipped;
    out.profile = done.profile;
    out.secrets = done.secrets;
    Ok(out)
}
