//! Access sets, the client bindings and the folder routes: the writers behind `toolportctl
//! access-set`, `client scope` and `folder route`. The rules are upstream's (`RegistryTx`
//! methods over its own functions); this layer adds the lookups, the checks a command line
//! needs (a set that is still in use is not deleted by accident, a route to a missing set is
//! refused) and the plan every change shows first with the command that undoes it.

use serde_json::{json, Value};
use toolport_plus_api::{AccessSetInfo, Host, RegistryTx, RegistryView, ScopeKind, ScopeReach};

use crate::context::plan::{Effects, PlanV1, ResultV1, Step};
use crate::op::OpError;
use crate::servers;

mod removed;
mod tools;

pub use removed::restore;
pub use tools::{tools_ls, tools_set, ToolChange, ToolsArgs};

const BINDING_RULE: &str = "A folder route outranks a client's own access set: inside a routed folder the gateway uses the folder's set for every client.";

/// What happens to the clients and folders that still name a set being deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orphans {
    /// Remove their bindings; clients fall back to the default access.
    Detach,
    /// Leave them naming the deleted set; they reach nothing until bound again.
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change<'a> {
    Set(&'a str),
    Clear,
}

pub struct AddArgs<'a> {
    /// Empty with `restore`: the restored set keeps its own name.
    pub name: &'a str,
    pub servers: &'a [String],
    /// The id of a removed set (`access-set rm` prints it in its undo line).
    pub restore: Option<&'a str>,
    pub dry_run: bool,
}

pub struct RmArgs<'a> {
    pub set: &'a str,
    pub orphans: Option<Orphans>,
    pub dry_run: bool,
}

fn failed(error: String) -> OpError {
    OpError::failed("registry_error", error)
}

fn read(host: &dyn Host) -> Result<Option<Box<dyn RegistryView>>, OpError> {
    host.read_view().map_err(failed)
}

fn view(host: &dyn Host) -> Result<Box<dyn RegistryView>, OpError> {
    let view =
        read(host)?.ok_or_else(|| OpError::failed("access_sets", "there is no registry yet"))?;
    if view.version() < 3 {
        return Err(OpError::failed(
            "access_sets",
            "access sets need a version 3 registry; open Toolport once to migrate it",
        ));
    }
    Ok(view)
}

fn quote(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@".contains(c))
    {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

fn find_set(sets: &[AccessSetInfo], key: &str) -> Result<AccessSetInfo, OpError> {
    servers::find_access_set(sets, key).cloned().ok_or_else(|| {
        let known: Vec<&str> = sets.iter().map(|set| set.name.as_str()).collect();
        OpError::not_found(format!(
            "no access set called {}; the access sets are: {}",
            key.trim(),
            if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            }
        ))
    })
}

fn server_name(servers: &[toolport_plus_api::ServerRecord], id: &str) -> String {
    servers
        .iter()
        .find(|server| server.id == id)
        .map_or_else(|| id.to_string(), |server| server.name.clone())
}

fn scopes_using<'a>(
    scopes: &'a [ScopeReach],
    kind: ScopeKind,
    set_id: &'a str,
) -> impl Iterator<Item = &'a ScopeReach> {
    scopes.iter().filter(move |scope| {
        scope.kind == kind && !scope.reference.is_empty() && scope.set_id.as_deref() == Some(set_id)
    })
}

pub(crate) fn step(detail: impl Into<String>) -> Step {
    Step {
        op: "registry",
        path: None,
        detail: detail.into(),
        keys: None,
        diff: None,
    }
}

pub(crate) fn note(detail: impl Into<String>) -> Step {
    Step {
        op: "note",
        ..step(detail)
    }
}

pub(crate) struct Draft {
    pub(crate) summary: String,
    pub(crate) steps: Vec<Step>,
    pub(crate) warnings: Vec<String>,
    pub(crate) undo: String,
}

pub(crate) fn finish(
    host: &dyn Host,
    dry_run: bool,
    change: Draft,
    changes: bool,
    run: &mut dyn FnMut(&mut dyn RegistryTx) -> Result<(), String>,
    mut data: Value,
) -> Result<Value, OpError> {
    let plan = PlanV1 {
        summary: change.summary,
        steps: change.steps,
        effects: Effects::default(),
        warnings: change.warnings,
        undo: change.undo.clone(),
    };
    let applied = !dry_run && changes;
    if applied {
        host.transact(run).map_err(failed)?;
    }
    let result = (!dry_run).then(|| ResultV1 {
        applied,
        changed: if applied {
            vec!["registry".to_string()]
        } else {
            Vec::new()
        },
        undo: change.undo,
        backups: Vec::new(),
    });
    data["dryRun"] = json!(dry_run);
    data["plan"] = json!(plan);
    data["result"] = json!(result);
    Ok(data)
}

fn set_row(set: &AccessSetInfo, view: &dyn RegistryView, scopes: &[ScopeReach]) -> Value {
    let servers = view.servers().unwrap_or_default();
    let ids = |kind| -> Vec<String> {
        scopes_using(scopes, kind, &set.id)
            .map(|scope| scope.key.clone())
            .collect()
    };
    json!({
        "id": set.id,
        "name": set.name,
        "default": view.default_access().as_deref() == Some(set.id.as_str()),
        "servers": set.server_ids.iter().map(|id| json!({"id": id, "name": server_name(&servers, id)})).collect::<Vec<_>>(),
        "reached": set.server_ids.iter().filter(|id| servers.iter().any(|s| &s.id == *id && s.enabled)).count(),
        "clients": ids(ScopeKind::Client),
        "folders": ids(ScopeKind::Folder),
    })
}

pub fn ls(host: &dyn Host) -> Result<Value, OpError> {
    let Some(view) = read(host)? else {
        return Ok(json!({"defaultAccess": null, "accessSets": []}));
    };
    let scopes = view.scope_reach();
    let sets = view.access_sets();
    Ok(json!({
        "defaultAccess": view.default_access(),
        "accessSets": sets.iter().map(|set| set_row(set, view.as_ref(), &scopes)).collect::<Vec<_>>(),
    }))
}

pub fn add(host: &dyn Host, args: &AddArgs) -> Result<Value, OpError> {
    if let Some(token) = args.restore {
        if !args.servers.is_empty() {
            return Err(OpError::usage(
                "--restore brings the set back as it was; it takes no --server",
            ));
        }
        let name = Some(args.name.trim()).filter(|name| !name.is_empty());
        return restore(host, name, token, args.dry_run);
    }
    let name = args.name.trim();
    if name.is_empty() {
        return Err(OpError::usage("give the access set a name"));
    }
    if name.starts_with('@') {
        return Err(OpError::usage(
            "access set names beginning with @ are reserved",
        ));
    }
    let view = view(host)?;
    let sets = view.access_sets();
    if let Some(existing) = sets
        .iter()
        .find(|set| set.name.eq_ignore_ascii_case(name) || set.id == name)
    {
        return Err(OpError::conflict(format!(
            "an access set called {} exists already",
            existing.name
        )));
    }
    let known = view.servers().map_err(failed)?;
    let mut members: Vec<(String, String)> = Vec::new();
    for key in args.servers {
        let server = servers::find(&known, key.trim())
            .ok_or_else(|| OpError::not_found(format!("no server '{}'", key.trim())))?;
        if !members.iter().any(|(id, _)| id == &server.id) {
            members.push((server.id.clone(), server.name.clone()));
        }
    }
    let mut steps = vec![step(format!("create the access set {name}"))];
    for (_, server) in &members {
        steps.push(step(format!("list {server} in {name}")));
    }
    let mut warnings = Vec::new();
    if members.is_empty() {
        warnings.push(format!(
            "{name} lists no server, so a client bound to it reaches nothing"
        ));
    }
    let ids: Vec<String> = members.iter().map(|(id, _)| id.clone()).collect();
    let change = Draft {
        summary: format!("Add access set {name}"),
        steps,
        warnings,
        undo: format!("toolportctl access-set rm {}", quote(name)),
    };
    let created = std::cell::RefCell::new(String::new());
    let data = finish(
        host,
        args.dry_run,
        change,
        true,
        &mut |tx| {
            let id = tx.create_access_set(name)?;
            for server in &ids {
                tx.set_access_server(&id, server, true)?;
            }
            *created.borrow_mut() = id;
            Ok(())
        },
        json!({"name": name}),
    )?;
    let mut data = data;
    if !args.dry_run {
        let id = created.into_inner();
        data["accessSet"] = view_set(host, &id)?;
    }
    Ok(data)
}

fn view_set(host: &dyn Host, id: &str) -> Result<Value, OpError> {
    let view = view(host)?;
    let scopes = view.scope_reach();
    Ok(view
        .access_sets()
        .iter()
        .find(|set| set.id == id)
        .map_or(Value::Null, |set| set_row(set, view.as_ref(), &scopes)))
}

pub fn rm(host: &dyn Host, args: &RmArgs) -> Result<Value, OpError> {
    let view = view(host)?;
    let sets = view.access_sets();
    let set = find_set(&sets, args.set)?;
    if view.default_access().as_deref() == Some(set.id.as_str()) {
        return Err(OpError::conflict(format!(
            "{} is the default access; choose another with `access-set default <set>` or `access-set default --none` first",
            set.name
        )));
    }
    if sets.len() <= 1 {
        return Err(OpError::conflict(format!(
            "{} is the only access set and cannot be removed",
            set.name
        )));
    }
    let scopes = view.scope_reach();
    let clients: Vec<&ScopeReach> = scopes_using(&scopes, ScopeKind::Client, &set.id).collect();
    let folders: Vec<&ScopeReach> = scopes_using(&scopes, ScopeKind::Folder, &set.id).collect();
    if (!clients.is_empty() || !folders.is_empty()) && args.orphans.is_none() {
        let users = json!({
            "clients": clients.iter().map(|s| s.key.clone()).collect::<Vec<_>>(),
            "folders": folders.iter().map(|s| s.key.clone()).collect::<Vec<_>>(),
        });
        return Err(OpError::conflict(format!(
            "{} is still used by {}. Choose what happens to them: --detach removes their binding (clients follow the default access), --leave deletes the set only (they reach nothing until bound again)",
            set.name,
            describe_users(&clients, &folders)
        ))
        .with_details(users));
    }
    let detach = args.orphans == Some(Orphans::Detach);
    let mut steps = vec![step(format!("delete the access set {}", set.name))];
    let mut warnings = Vec::new();
    let users: Vec<&ScopeReach> = clients.iter().chain(folders.iter()).copied().collect();
    for scope in &users {
        let what = match scope.kind {
            ScopeKind::Client => format!("client {}", scope.key),
            _ => format!("folder {}", scope.key),
        };
        if detach {
            steps.push(step(format!("remove the binding of {what}")));
        } else {
            warnings.push(format!(
                "{what} keeps naming {} and reaches nothing until it is bound again",
                set.name
            ));
        }
    }
    if detach && !clients.is_empty() {
        warnings.push(
            "detached clients follow the default access, which may list more servers".to_string(),
        );
    }
    for client in clients.iter().filter(|s| s.key.starts_with("claude-code-")) {
        warnings.push(format!(
            "{} looks like a launch profile; remove it with `context launch-profile rm {}` instead of leaving it without its set",
            client.key,
            &client.key["claude-code-".len()..]
        ));
    }
    let detached: Vec<&ScopeReach> = if detach { users.clone() } else { Vec::new() };
    let removal = removed::Removal::of(view.as_ref(), &set, &detached);
    let change = Draft {
        summary: format!("Delete access set {}", set.name),
        steps,
        warnings,
        undo: removal.undo(&set.name),
    };
    if !args.dry_run {
        removal.save(host)?;
    }
    let (id, client_keys, folder_keys) = (
        set.id.clone(),
        clients.iter().map(|s| s.key.clone()).collect::<Vec<_>>(),
        folders.iter().map(|s| s.key.clone()).collect::<Vec<_>>(),
    );
    finish(
        host,
        args.dry_run,
        change,
        true,
        &mut |tx| {
            if detach {
                for client in &client_keys {
                    tx.set_client_scope(client, None)?;
                }
                for folder in &folder_keys {
                    tx.remove_folder_route(folder)?;
                }
            }
            tx.delete_access_set(&id)
        },
        json!({"accessSet": {"id": set.id, "name": set.name}}),
    )
}

fn describe_users(clients: &[&ScopeReach], folders: &[&ScopeReach]) -> String {
    let mut parts = Vec::new();
    if !clients.is_empty() {
        parts.push(format!(
            "{} ({})",
            plural(clients.len(), "client"),
            clients
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !folders.is_empty() {
        parts.push(format!(
            "{} ({})",
            plural(folders.len(), "folder route"),
            folders
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    parts.join(" and ")
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

pub fn default(host: &dyn Host, change: Change, dry_run: bool) -> Result<Value, OpError> {
    let view = view(host)?;
    let sets = view.access_sets();
    let current = view
        .default_access()
        .and_then(|id| sets.iter().find(|set| set.id == id).cloned());
    let target = match change {
        Change::Set(key) => Some(find_set(&sets, key)?),
        Change::Clear => None,
    };
    let undo = match &current {
        Some(set) => format!("toolportctl access-set default {}", quote(&set.name)),
        None => "toolportctl access-set default --none".to_string(),
    };
    let same = target.as_ref().map(|set| &set.id) == current.as_ref().map(|set| &set.id);
    let (summary, detail) = match &target {
        Some(set) => (
            format!("Make {} the default access", set.name),
            format!(
                "clients without their own set reach the enabled servers {} lists",
                set.name
            ),
        ),
        None => (
            "Return the default access to every enabled server".to_string(),
            "clients without their own set reach every enabled server".to_string(),
        ),
    };
    let steps = vec![if same {
        note("the default access is already this")
    } else {
        step(detail)
    }];
    let id = target.as_ref().map(|set| set.id.clone());
    let mut data = finish(
        host,
        dry_run,
        Draft {
            summary: if same {
                "The default access is up to date".to_string()
            } else {
                summary
            },
            steps,
            warnings: Vec::new(),
            undo,
        },
        !same,
        &mut |tx| tx.set_default_access(id.as_deref()),
        json!({}),
    )?;
    data["defaultAccess"] = json!(target.map(|set| json!({"id": set.id, "name": set.name})));
    Ok(data)
}

pub fn scope(
    host: &dyn Host,
    client: &str,
    change: Change,
    dry_run: bool,
) -> Result<Value, OpError> {
    let view = view(host)?;
    let sets = view.access_sets();
    let scopes = view.scope_reach();
    let client = client.trim();
    let existing = scopes
        .iter()
        .find(|scope| scope.kind == ScopeKind::Client && scope.key == client);
    let known = host.detect_clients().iter().any(|info| info.id == client);
    if existing.is_none() && !known {
        let ids: Vec<String> = host
            .detect_clients()
            .into_iter()
            .map(|info| info.id)
            .collect();
        return Err(OpError::not_found(format!(
            "no client '{client}'; the clients are: {}",
            ids.join(", ")
        )));
    }
    let current = existing
        .filter(|scope| !scope.reference.is_empty())
        .map(|scope| {
            (
                scope.reference.clone(),
                scope
                    .set_id
                    .as_ref()
                    .and_then(|id| sets.iter().find(|set| &set.id == id))
                    .map(|set| set.name.clone()),
            )
        });
    let target = match change {
        Change::Set(key) => Some(find_set(&sets, key)?),
        Change::Clear => None,
    };
    let undo = match &current {
        Some((reference, name)) => format!(
            "toolportctl client scope {} {}",
            quote(client),
            quote(name.as_deref().unwrap_or(reference))
        ),
        None => format!("toolportctl client scope {} --none", quote(client)),
    };
    let current_id = existing
        .filter(|scope| !scope.reference.is_empty())
        .and_then(|scope| scope.set_id.as_deref());
    let same = match (&target, &current) {
        (Some(set), Some(_)) => current_id == Some(set.id.as_str()),
        (None, None) => true,
        _ => false,
    };
    let mut warnings = Vec::new();
    let routed: Vec<&str> = scopes
        .iter()
        .filter(|scope| scope.kind == ScopeKind::Folder)
        .map(|scope| scope.key.as_str())
        .collect();
    if target.is_some() && !routed.is_empty() {
        warnings.push(format!(
            "{BINDING_RULE} Routed folders: {}.",
            routed.join(", ")
        ));
    }
    let (summary, detail) = match &target {
        Some(set) => (
            format!("Bind {client} to the access set {}", set.name),
            format!("{client} reaches the enabled servers {} lists", set.name),
        ),
        None => (
            format!("Unbind {client} from its access set"),
            format!("{client} follows the default access"),
        ),
    };
    let id = target.as_ref().map(|set| set.id.clone());
    let owned = client.to_string();
    let mut data = finish(
        host,
        dry_run,
        Draft {
            summary: if same {
                format!("{client} is up to date")
            } else {
                summary
            },
            steps: vec![if same {
                note("nothing to change")
            } else {
                step(detail)
            }],
            warnings,
            undo,
        },
        !same,
        &mut |tx| tx.set_client_scope(&owned, id.as_deref()),
        json!({"client": client}),
    )?;
    data["accessSet"] = json!(target.map(|set| json!({"id": set.id, "name": set.name})));
    Ok(data)
}

fn normalized(path: &str) -> String {
    let mut text = path.trim().replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    text
}

fn absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("//")
        || path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'/')
}

pub fn routes(host: &dyn Host) -> Result<Value, OpError> {
    let Some(view) = read(host)? else {
        return Ok(json!({"rule": BINDING_RULE, "routes": []}));
    };
    let servers = view.servers().unwrap_or_default();
    let sets = view.access_sets();
    let rows: Vec<Value> = view
        .scope_reach()
        .iter()
        .filter(|scope| scope.kind == ScopeKind::Folder)
        .map(|scope| {
            let set = scope
                .set_id
                .as_ref()
                .and_then(|id| sets.iter().find(|set| &set.id == id));
            json!({
                "path": scope.key,
                "accessSet": set.map(|set| json!({"id": set.id, "name": set.name})),
                "missing": scope.dangling,
                "reference": scope.reference,
                "servers": scope.server_ids.iter().map(|id| json!({"id": id, "name": server_name(&servers, id)})).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({"rule": BINDING_RULE, "routes": rows}))
}

pub fn route_add(
    host: &dyn Host,
    path: &str,
    set_key: &str,
    dry_run: bool,
) -> Result<Value, OpError> {
    let path = normalized(path);
    if path.is_empty() {
        return Err(OpError::usage("choose a project folder"));
    }
    if !absolute(&path) {
        return Err(OpError::usage(format!(
            "{path} is not an absolute folder; the gateway matches the absolute folder a client reports"
        )));
    }
    let view = view(host)?;
    let sets = view.access_sets();
    let set = find_set(&sets, set_key)?;
    let scopes = view.scope_reach();
    let existing = scopes
        .iter()
        .find(|scope| scope.kind == ScopeKind::Folder && normalized(&scope.key) == path);
    let same = existing.is_some_and(|scope| scope.set_id.as_deref() == Some(set.id.as_str()));
    let undo = match existing {
        Some(scope) => {
            let name = scope
                .set_id
                .as_ref()
                .and_then(|id| sets.iter().find(|set| &set.id == id))
                .map_or(scope.reference.clone(), |set| set.name.clone());
            format!(
                "toolportctl folder route add {} {}",
                quote(&path),
                quote(&name)
            )
        }
        None => format!("toolportctl folder route rm {}", quote(&path)),
    };
    let mut warnings = Vec::new();
    let owners: Vec<&str> = scopes
        .iter()
        .filter(|scope| scope.kind == ScopeKind::Client && !scope.reference.is_empty())
        .map(|scope| scope.key.as_str())
        .collect();
    if !owners.is_empty() {
        warnings.push(format!(
            "{BINDING_RULE} Clients with their own set: {}.",
            owners.join(", ")
        ));
    }
    let stored = existing.map(|scope| scope.key.clone());
    let (owned_path, set_id) = (path.clone(), set.id.clone());
    let mut data = finish(
        host,
        dry_run,
        Draft {
            summary: if same {
                format!("{path} is already routed to {}", set.name)
            } else {
                format!("Route {path} to the access set {}", set.name)
            },
            steps: vec![if same {
                note("nothing to change")
            } else {
                step(format!("{path} and the folders below it use {}", set.name))
            }],
            warnings,
            undo,
        },
        !same,
        &mut |tx| {
            if let Some(old) = stored.as_deref().filter(|old| *old != owned_path) {
                tx.remove_folder_route(old)?;
            }
            tx.set_folder_route(&owned_path, &set_id)
        },
        json!({"path": path}),
    )?;
    data["accessSet"] = json!({"id": set.id, "name": set.name});
    Ok(data)
}

pub fn route_rm(host: &dyn Host, path: &str, dry_run: bool) -> Result<Value, OpError> {
    let wanted = normalized(path);
    let view = view(host)?;
    let sets = view.access_sets();
    let scopes = view.scope_reach();
    let existing = scopes
        .iter()
        .find(|scope| scope.kind == ScopeKind::Folder && normalized(&scope.key) == wanted)
        .ok_or_else(|| OpError::not_found(format!("no folder route for {wanted}")))?;
    let name = existing
        .set_id
        .as_ref()
        .and_then(|id| sets.iter().find(|set| &set.id == id))
        .map_or(existing.reference.clone(), |set| set.name.clone());
    let key = existing.key.clone();
    finish(
        host,
        dry_run,
        Draft {
            summary: format!("Remove the folder route for {wanted}"),
            steps: vec![step(format!(
                "{wanted} and the folders below it stop using {name}"
            ))],
            warnings: Vec::new(),
            undo: format!(
                "toolportctl folder route add {} {}",
                quote(&wanted),
                quote(&name)
            ),
        },
        true,
        &mut |tx| tx.remove_folder_route(&key),
        json!({"path": wanted, "accessSet": {"name": name}}),
    )
}

#[cfg(test)]
mod tests;
