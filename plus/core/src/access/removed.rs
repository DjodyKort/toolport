//! The journal behind `access-set rm`: a deleted set is kept whole in the data dir so that
//! `access-set add --restore <id>` puts back everything it carried (its servers, the tool lists
//! per server, its instructions and the bindings that were detached), which no command line of
//! plain `add`, `tools` and `scope` calls can say.

use std::path::PathBuf;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use toolport_plus_api::{Host, RegistryView, ScopeKind, ScopeReach};

use super::{finish, quote, step, view, Draft};
use crate::fsx;
use crate::op::OpError;

const PREFIX: &str = "removed-";
const SCHEMA: u64 = 1;
const KEEP_DAYS: u64 = 30;

/// What a deletion carried: the stored set as the registry file holds it and the bindings the
/// deletion detached.
pub(super) struct Removal {
    pub token: String,
    pub record: Value,
}

impl Removal {
    pub(super) fn of(
        view: &dyn RegistryView,
        set: &toolport_plus_api::AccessSetInfo,
        detached: &[&ScopeReach],
    ) -> Self {
        let profile = view.access_set_record(&set.id).unwrap_or_else(
            || json!({"id": set.id, "name": set.name, "enabledServerIds": set.server_ids}),
        );
        let of_kind = |kind: ScopeKind| -> Vec<Value> {
            detached
                .iter()
                .filter(|scope| scope.kind == kind)
                .map(|scope| json!({"key": scope.key}))
                .collect()
        };
        let record = json!({
            "schema": SCHEMA,
            "accessSet": profile,
            "clients": of_kind(ScopeKind::Client),
            "folders": of_kind(ScopeKind::Folder),
        });
        let digest = Sha256::digest(serde_json::to_string(&record).unwrap_or_default());
        let token = format!("{PREFIX}{}", &format!("{digest:x}")[..16]);
        Self { token, record }
    }

    pub(super) fn undo(&self, name: &str) -> String {
        format!(
            "toolportctl access-set add {} --restore {}",
            quote(name),
            self.token
        )
    }

    /// Writes the record; the same deletion always has the same id, so a second write is a no-op.
    pub(super) fn save(&self, host: &dyn Host) -> Result<(), OpError> {
        let path = file(host, &self.token)?;
        let text = serde_json::to_string_pretty(&self.record).unwrap_or_default() + "\n";
        fsx::atomic_write(&path, &text, fsx::Mode::Private).map_err(|error| {
            OpError::failed(
                "removal_journal",
                format!(
                    "the removed set could not be kept for Undo ({error}); nothing was deleted"
                ),
            )
        })?;
        prune(host, &self.token);
        Ok(())
    }
}

/// Deletes the records older than [`KEEP_DAYS`] days, except `keep`. Best effort: a record that
/// cannot be removed is tried again at the next run.
pub(super) fn prune(host: &dyn Host, keep: &str) {
    let Ok(path) = file(host, keep) else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    let limit = std::time::Duration::from_secs(KEEP_DAYS * 24 * 60 * 60);
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let stale = path.extension().is_some_and(|ext| ext == "json")
            && entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > limit);
        if stale && path != file(host, keep).unwrap_or_default() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn file(host: &dyn Host, token: &str) -> Result<PathBuf, OpError> {
    let valid = token
        .strip_prefix(PREFIX)
        .is_some_and(|rest| rest.len() == 16 && rest.bytes().all(|b| b.is_ascii_hexdigit()));
    if !valid {
        return Err(OpError::usage(format!(
            "{token} is not a removal id; `access-set rm` prints one in its undo line"
        )));
    }
    let dir = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    Ok(dir
        .join("plus")
        .join("access")
        .join("removed")
        .join(format!("{token}.json")))
}

fn load(host: &dyn Host, token: &str) -> Result<Value, OpError> {
    let path = file(host, token)?;
    let text = std::fs::read_to_string(&path).map_err(|_| {
        OpError::not_found(format!(
            "no removed access set {token}; it was restored already or its record is gone"
        ))
    })?;
    serde_json::from_str(&text)
        .map_err(|error| OpError::failed("removal_journal", format!("{token} is damaged: {error}")))
}

pub fn restore(
    host: &dyn Host,
    name: Option<&str>,
    token: &str,
    dry_run: bool,
) -> Result<Value, OpError> {
    let saved = load(host, token)?;
    let mut profile = saved["accessSet"].clone();
    let (Some(id), Some(own_name)) = (
        profile["id"].as_str().map(String::from),
        profile["name"].as_str().map(String::from),
    ) else {
        return Err(OpError::failed(
            "removal_journal",
            format!("{token} holds no access set"),
        ));
    };
    if name.is_some_and(|given| !given.trim().eq_ignore_ascii_case(&own_name)) {
        return Err(OpError::usage(format!(
            "{token} holds the access set {own_name}, not {}",
            name.unwrap_or_default().trim()
        )));
    }
    let view = view(host)?;
    let sets = view.access_sets();
    if let Some(existing) = sets
        .iter()
        .find(|set| set.id == id || set.name.eq_ignore_ascii_case(&own_name))
    {
        return Err(OpError::conflict(format!(
            "an access set called {} exists already",
            existing.name
        )));
    }
    let known = view.servers().map_err(super::failed)?;
    let mut warnings = Vec::new();
    let mut steps = vec![step(format!("restore the access set {own_name}"))];
    let listed: Vec<String> = profile["enabledServerIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(String::from))
        .collect();
    let (kept, gone): (Vec<String>, Vec<String>) = listed
        .into_iter()
        .partition(|id| known.iter().any(|s| &s.id == id));
    profile["enabledServerIds"] = json!(kept);
    for server in &kept {
        steps.push(step(format!(
            "list {} in {own_name}",
            super::server_name(&known, server)
        )));
    }
    if !gone.is_empty() {
        warnings.push(format!(
            "{} no longer exist and are left out of {own_name}",
            gone.join(", ")
        ));
    }
    if let Some(scope) = profile.get_mut("toolScope").and_then(Value::as_object_mut) {
        scope.retain(|server, _| known.iter().any(|s| &s.id == server));
        for (server, tools) in scope.iter() {
            let count = tools.as_array().map_or(0, Vec::len);
            steps.push(step(format!(
                "expose {count} tool(s) of {} in {own_name}, as before",
                super::server_name(&known, server)
            )));
        }
    }
    if profile["instructions"].is_string() {
        steps.push(step(format!("restore the instructions of {own_name}")));
    }
    let scopes = view.scope_reach();
    let mut bind_clients: Vec<String> = Vec::new();
    for entry in saved["clients"].as_array().into_iter().flatten() {
        let Some(client) = entry["key"].as_str() else {
            continue;
        };
        let taken = scopes.iter().any(|scope| {
            scope.kind == ScopeKind::Client && scope.key == client && !scope.reference.is_empty()
        });
        if taken {
            warnings.push(format!(
                "client {client} was bound to another set since; it is left as it is"
            ));
        } else {
            steps.push(step(format!("bind client {client} to {own_name} again")));
            bind_clients.push(client.to_string());
        }
    }
    let mut bind_folders: Vec<String> = Vec::new();
    for entry in saved["folders"].as_array().into_iter().flatten() {
        let Some(path) = entry["key"].as_str() else {
            continue;
        };
        let taken = scopes
            .iter()
            .any(|scope| scope.kind == ScopeKind::Folder && scope.key == path);
        if taken {
            warnings.push(format!(
                "folder {path} was routed to another set since; it is left as it is"
            ));
        } else {
            steps.push(step(format!("route folder {path} to {own_name} again")));
            bind_folders.push(path.to_string());
        }
    }
    let bound = !bind_clients.is_empty() || !bind_folders.is_empty();
    let draft = Draft {
        summary: format!("Restore access set {own_name}"),
        steps,
        warnings,
        undo: format!(
            "toolportctl access-set rm {}{}",
            quote(&own_name),
            if bound { " --detach" } else { "" }
        ),
    };
    let (record, set_id) = (profile.clone(), id.clone());
    let mut data = finish(
        host,
        dry_run,
        draft,
        true,
        &mut |tx| {
            tx.restore_access_set(record.clone())?;
            for client in &bind_clients {
                tx.set_client_scope(client, Some(&set_id))?;
            }
            for path in &bind_folders {
                tx.set_folder_route(path, &set_id)?;
            }
            Ok(())
        },
        json!({"name": own_name, "restored": token}),
    )?;
    if !dry_run {
        let _ = std::fs::remove_file(file(host, token)?);
        prune(host, token);
        let view = super::view(host)?;
        let scopes = view.scope_reach();
        data["accessSet"] = view
            .access_sets()
            .iter()
            .find(|set| set.id == id)
            .map_or(Value::Null, |set| {
                super::set_row(set, view.as_ref(), &scopes)
            });
    }
    Ok(data)
}
