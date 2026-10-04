//! Profile management behind `toolportctl profile ls|create|edit|rm`: the shared core of the ctl
//! commands, the `plus.profile.*` handlers and the selfmcp profile tools. Every mutation goes
//! through the registry_controller paths (same validation and locking as the app) and runs on a
//! read-only copy first, so a rejected edit and a dry run write nothing. mcpm's tag profiles map
//! onto registry profiles (`enabledServerIds`); `client.rs` maps a client's profile entries onto
//! its scope.

pub mod client;
pub mod handlers;

#[cfg(test)]
mod tests;

use crate::plus::{registry_ro, servers};
use crate::registry::{self, Profile, Registry, ServerEntry};
use crate::registry_controller;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Invalid,
    NotFound,
    Conflict,
    Failed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    pub message: String,
}

impl Error {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::of(Kind::Invalid, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::of(Kind::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::of(Kind::Conflict, message)
    }

    pub fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self::of(Kind::Failed(code), message)
    }

    fn of(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<Error> for String {
    fn from(error: Error) -> String {
        error.message
    }
}

pub(crate) fn registry_error(message: String) -> Error {
    Error::failed("registry_error", message)
}

/// Plans `apply` on a read-only copy; a rejection or a no-op ends there without a write. Only a
/// real change on a real run repeats `apply` on a fresh copy under the registry lock.
pub(crate) fn mutate<T>(
    dry_run: bool,
    apply: impl Fn(&mut Registry) -> Result<T, Error>,
    changed: impl Fn(&T) -> bool,
) -> Result<(Registry, T), Error> {
    let mut preview = registry_ro::read().map_err(registry_error)?;
    let planned = apply(&mut preview)?;
    if dry_run || !changed(&planned) {
        return Ok((preview, planned));
    }
    let mut rejected = None;
    let saved = registry::update(|reg| {
        apply(reg).map_err(|error| {
            let message = error.message.clone();
            rejected = Some(error);
            message
        })
    });
    match saved {
        Ok(done) => Ok(done),
        Err(message) => Err(rejected.unwrap_or_else(|| registry_error(message))),
    }
}

pub(crate) fn resolve<'a>(reg: &'a Registry, key: &str) -> Result<&'a Profile, Error> {
    let key = key.trim();
    if let Some(profile) = reg.profiles.iter().find(|p| p.id == key) {
        return Ok(profile);
    }
    let mut named = reg
        .profiles
        .iter()
        .filter(|p| p.name.eq_ignore_ascii_case(key));
    match (named.next(), named.next()) {
        (Some(profile), None) => Ok(profile),
        (Some(_), Some(_)) => Err(Error::conflict(format!(
            "Profile '{key}' is ambiguous; use its id"
        ))),
        _ => Err(Error::not_found(format!("Profile '{key}' not found"))),
    }
}

fn find_server<'a>(reg: &'a Registry, key: &str) -> Option<&'a ServerEntry> {
    servers::find(reg, key.trim())
}

fn members<'a>(reg: &'a Registry, profile: &Profile) -> Vec<&'a ServerEntry> {
    reg.servers
        .iter()
        .filter(|s| profile.enabled_server_ids.contains(&s.id))
        .collect()
}

fn target(server: &ServerEntry) -> String {
    match (&server.command, &server.url) {
        (Some(command), _) => std::iter::once(command.as_str())
            .chain(server.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        (None, Some(url)) => url.clone(),
        (None, None) => "Custom".to_string(),
    }
}

pub(crate) fn scoped_clients(reg: &Registry, profile_id: &str) -> Vec<String> {
    let mut ids: Vec<String> = reg
        .client_scopes
        .iter()
        .filter(|(_, scope)| {
            !scope.trim().is_empty()
                && reg.canonical_profile_id(scope).ok().as_deref() == Some(profile_id)
        })
        .map(|(client, _)| client.clone())
        .collect();
    ids.sort();
    ids
}

pub struct Member {
    pub id: String,
    pub name: String,
    pub target: String,
}

pub struct Row {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub servers: Vec<Member>,
    pub clients: Vec<String>,
}

impl Row {
    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "active": self.active,
            "servers": self.servers.iter().map(|m| json!({"id": m.id, "name": m.name, "target": m.target})).collect::<Vec<_>>(),
            "clients": self.clients,
        })
    }
}

pub fn rows(reg: &Registry) -> Vec<Row> {
    let active = reg.active_profile_id();
    reg.profiles
        .iter()
        .map(|profile| Row {
            id: profile.id.clone(),
            name: profile.name.clone(),
            active: profile.id == active,
            servers: members(reg, profile)
                .into_iter()
                .map(|s| Member {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    target: target(s),
                })
                .collect(),
            clients: scoped_clients(reg, &profile.id),
        })
        .collect()
}

pub fn list() -> Result<Vec<Row>, Error> {
    registry_ro::read()
        .map(|reg| rows(&reg))
        .map_err(registry_error)
}

pub fn list_value(rows: &[Row]) -> Value {
    json!({
        "activeProfile": rows.iter().find(|r| r.active).map(|r| r.id.clone()),
        "profiles": rows.iter().map(Row::to_value).collect::<Vec<_>>(),
    })
}

pub struct Created {
    pub id: String,
    pub name: String,
    pub created: bool,
    pub dry_run: bool,
}

impl Created {
    pub fn to_value(&self) -> Value {
        json!({"id": self.id, "name": self.name, "created": self.created, "dryRun": self.dry_run})
    }
}

pub fn create(name: &str, force: bool, dry_run: bool) -> Result<Created, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("give the profile a name"));
    }
    let (_, mut created) = mutate(
        dry_run,
        |reg| {
            if let Some(existing) = reg
                .profiles
                .iter()
                .find(|p| p.id == name || p.name.eq_ignore_ascii_case(name))
            {
                if !force {
                    return Err(Error::conflict(format!("Profile '{name}' already exists.")));
                }
                return Ok(Created {
                    id: existing.id.clone(),
                    name: existing.name.clone(),
                    created: false,
                    dry_run,
                });
            }
            registry_controller::apply_create_profile(reg, name);
            let made = reg.profiles.last().expect("a profile was just added");
            Ok(Created {
                id: made.id.clone(),
                name: made.name.clone(),
                created: true,
                dry_run,
            })
        },
        |created| created.created,
    )?;
    created.dry_run = dry_run;
    Ok(created)
}

pub enum ServerOp {
    Set(Vec<String>),
    Add(Vec<String>),
    Remove(Vec<String>),
}

#[derive(Default)]
pub struct EditSpec {
    pub name: Option<String>,
    pub servers: Option<ServerOp>,
}

pub struct Edited {
    pub id: String,
    pub old_name: String,
    pub name: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
    pub not_in_profile: Vec<String>,
    pub dry_run: bool,
}

impl Edited {
    pub fn renamed(&self) -> bool {
        self.name != self.old_name
    }

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
        self.renamed() || !self.added().is_empty() || !self.removed().is_empty()
    }

    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "oldName": self.old_name,
            "renamed": self.renamed(),
            "changed": self.changed(),
            "dryRun": self.dry_run,
            "servers": {
                "before": self.before,
                "after": self.after,
                "added": self.added(),
                "removed": self.removed(),
            },
            "notInProfile": self.not_in_profile,
        })
    }
}

fn names_of(reg: &Registry, profile_id: &str) -> Vec<String> {
    reg.profiles
        .iter()
        .find(|p| p.id == profile_id)
        .map(|p| members(reg, p).iter().map(|s| s.name.clone()).collect())
        .unwrap_or_default()
}

fn resolve_servers(reg: &Registry, keys: &[String]) -> Result<Vec<String>, Error> {
    let mut ids = Vec::new();
    let mut unknown = Vec::new();
    for key in keys {
        match find_server(reg, key) {
            Some(server) if !ids.contains(&server.id) => ids.push(server.id.clone()),
            Some(_) => {}
            None => unknown.push(key.clone()),
        }
    }
    if unknown.is_empty() {
        return Ok(ids);
    }
    let available: Vec<String> = reg
        .servers
        .iter()
        .map(|s| format!("  • {}", s.name))
        .collect();
    Err(Error::not_found(format!(
        "Server(s) not found: {}\n\nAvailable servers:\n{}",
        unknown.join(", "),
        available.join("\n")
    )))
}

fn apply_edit(reg: &mut Registry, key: &str, spec: &EditSpec) -> Result<Edited, Error> {
    let profile = resolve(reg, key)?;
    let (id, old_name) = (profile.id.clone(), profile.name.clone());
    let name = match spec.name.as_deref().map(str::trim) {
        Some("") => return Err(Error::invalid("give the profile a name")),
        Some(new) => new.to_string(),
        None => old_name.clone(),
    };
    if name != old_name
        && reg
            .profiles
            .iter()
            .any(|p| p.id != id && (p.name.eq_ignore_ascii_case(&name) || p.id == name))
    {
        return Err(Error::conflict(format!("Profile '{name}' already exists")));
    }
    let before = names_of(reg, &id);
    let mut not_in_profile = Vec::new();
    let mut toggles: Vec<(String, bool)> = Vec::new();
    if let Some(op) = &spec.servers {
        if reg.servers.is_empty() {
            return Err(Error::not_found(
                "No servers found in the registry\nInstall servers first with `toolportctl server install <name>`",
            ));
        }
        let current: Vec<String> = reg
            .profiles
            .iter()
            .find(|p| p.id == id)
            .map(|p| members(reg, p).iter().map(|s| s.id.clone()).collect())
            .unwrap_or_default();
        match op {
            ServerOp::Set(keys) => {
                let wanted = resolve_servers(reg, keys)?;
                toggles.extend(
                    current
                        .iter()
                        .filter(|s| !wanted.contains(s))
                        .map(|s| (s.clone(), false)),
                );
                toggles.extend(
                    wanted
                        .into_iter()
                        .filter(|s| !current.contains(s))
                        .map(|s| (s, true)),
                );
            }
            ServerOp::Add(keys) => {
                let wanted = resolve_servers(reg, keys)?;
                toggles.extend(
                    wanted
                        .into_iter()
                        .filter(|s| !current.contains(s))
                        .map(|s| (s, true)),
                );
            }
            ServerOp::Remove(keys) => {
                for key in keys {
                    match find_server(reg, key) {
                        Some(server) if current.contains(&server.id) => {
                            if !toggles.iter().any(|(id, _)| *id == server.id) {
                                toggles.push((server.id.clone(), false));
                            }
                        }
                        _ => not_in_profile.push(key.clone()),
                    }
                }
            }
        }
    }
    for (server_id, enabled) in &toggles {
        registry_controller::apply_server_enabled(reg, &id, server_id, *enabled, false)
            .map_err(|e| Error::failed("registry_error", e))?;
    }
    if matches!(spec.servers, Some(ServerOp::Set(_))) {
        let known: Vec<String> = reg.servers.iter().map(|s| s.id.clone()).collect();
        if let Some(profile) = reg.profiles.iter_mut().find(|p| p.id == id) {
            profile.enabled_server_ids.retain(|s| known.contains(s));
        }
    }
    if let Some(profile) = reg.profiles.iter_mut().find(|p| p.id == id) {
        profile.name = name.clone();
    }
    Ok(Edited {
        after: names_of(reg, &id),
        id,
        old_name,
        name,
        before,
        not_in_profile,
        dry_run: false,
    })
}

pub fn edit(key: &str, spec: &EditSpec, dry_run: bool) -> Result<Edited, Error> {
    let (_, mut edited) = mutate(dry_run, |reg| apply_edit(reg, key, spec), Edited::changed)?;
    edited.dry_run = dry_run;
    Ok(edited)
}

pub struct Cleanup {
    pub client: String,
    pub name: String,
    pub backup: Option<String>,
    pub error: Option<String>,
}

impl Cleanup {
    pub fn to_value(&self) -> Value {
        json!({"client": self.client, "name": self.name, "backup": self.backup, "error": self.error})
    }
}

pub struct Removed {
    pub id: String,
    pub name: String,
    pub servers: usize,
    pub cleanups: Vec<Cleanup>,
    pub left: Vec<String>,
    pub dry_run: bool,
}

impl Removed {
    pub fn failed(&self) -> bool {
        self.cleanups.iter().any(|c| c.error.is_some())
    }

    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "dryRun": self.dry_run,
            "servers": self.servers,
            "clients": self.cleanups.iter().map(Cleanup::to_value).collect::<Vec<_>>(),
            "left": self.left,
        })
    }
}

fn client_name(id: &str) -> String {
    crate::clients::detect_clients()
        .into_iter()
        .find(|c| c.id == id)
        .map_or_else(|| id.to_string(), |c| c.name)
}

pub fn remove(key: &str, keep_clients: bool, dry_run: bool) -> Result<Removed, Error> {
    let (_, mut removed) = mutate(
        dry_run,
        |reg| {
            let profile = resolve(reg, key)?;
            let (id, name) = (profile.id.clone(), profile.name.clone());
            let servers = members(reg, profile).len();
            let scoped = scoped_clients(reg, &id);
            registry_controller::apply_delete_profile(reg, &id).map_err(Error::conflict)?;
            Ok(Removed {
                id,
                name,
                servers,
                cleanups: Vec::new(),
                left: scoped,
                dry_run,
            })
        },
        |_| true,
    )?;
    removed.dry_run = dry_run;
    if keep_clients {
        return Ok(removed);
    }
    for client in std::mem::take(&mut removed.left) {
        let name = client_name(&client);
        let outcome = if dry_run {
            Ok(None)
        } else {
            registry_controller::disconnect_client(&client).map(|done| done.outcome.backup)
        };
        removed.cleanups.push(match outcome {
            Ok(backup) => Cleanup {
                client,
                name,
                backup,
                error: None,
            },
            Err(error) => Cleanup {
                client,
                name,
                backup: None,
                error: Some(error),
            },
        });
    }
    Ok(removed)
}

/// Enables or disables one server in a profile, creating the profile when `create` is set and the
/// reference matches none. The selfmcp `servers_add_profile_tag` and `servers_remove_profile_tag`
/// tools use it; `profile edit` shares `apply_server_enabled`.
pub fn set_member(
    profile_ref: &str,
    server_id: &str,
    enabled: bool,
    create: bool,
) -> Result<Registry, Error> {
    let (reg, _) = mutate(
        false,
        |reg| {
            let mut created = false;
            let id = match resolve(reg, profile_ref) {
                Ok(profile) => profile.id.clone(),
                Err(e) if e.kind == Kind::NotFound && create => {
                    let name = profile_ref.trim();
                    if name.is_empty() {
                        return Err(Error::invalid("give the profile a name"));
                    }
                    registry_controller::apply_create_profile(reg, name);
                    created = true;
                    reg.profiles
                        .last()
                        .expect("a profile was just added")
                        .id
                        .clone()
                }
                Err(e) => return Err(e),
            };
            let was = reg.is_enabled(&id, server_id);
            registry_controller::apply_server_enabled(reg, &id, server_id, enabled, false)
                .map_err(|e| Error::failed("registry_error", e))?;
            Ok(created || was != enabled)
        },
        |changed| *changed,
    )?;
    Ok(reg)
}

pub fn tags_of(reg: &Registry, server_id: &str) -> Vec<String> {
    reg.profiles
        .iter()
        .filter(|p| p.enabled_server_ids.iter().any(|s| s == server_id))
        .map(|p| p.name.clone())
        .collect()
}
