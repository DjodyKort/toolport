//! Client config maintenance shared by `toolportctl client sync`, `toolportctl server uninstall`
//! and the `clients_sync` and `servers_uninstall` self-MCP tools: pruning direct entries and
//! reconciling each managed client with the gateway.

use crate::clients::{self, DetectedClient, GatewayEntryState};
use crate::plus::direct::{self, Removing};
use crate::plus::op::OpError;
use crate::plus::status::snapshot;
use crate::registry::{self, Registry};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) struct Prune {
    pub client_id: String,
    pub path: Option<String>,
    pub removed: Vec<String>,
    pub backup: Option<String>,
    pub error: Option<String>,
}

impl Prune {
    pub(crate) fn to_value(&self) -> Value {
        json!({
            "client": self.client_id,
            "path": self.path,
            "removed": self.removed,
            "backup": self.backup,
            "error": self.error,
        })
    }

    pub(crate) fn line(&self, dry_run: bool) -> String {
        match &self.error {
            Some(e) => format!("  {}: error: {e}", self.client_id),
            None => format!(
                "  {}: {} {}",
                self.client_id,
                if dry_run { "would remove" } else { "removed" },
                self.removed.join(", ")
            ),
        }
    }
}

type Claimed = HashSet<(String, String)>;

pub(crate) fn direct_entries<'a>(
    client: &'a DetectedClient,
    claimed: &'a Claimed,
) -> impl Iterator<Item = &'a str> {
    client
        .servers
        .iter()
        .filter(|s| !clients::detected_is_gateway(s))
        .filter(move |s| !claimed.contains(&(client.id.clone(), s.name.clone())))
        .map(|s| s.name.as_str())
}

pub(crate) fn claimed_entries(reg: Option<&Registry>, detected: &[DetectedClient]) -> Claimed {
    reg.map(|reg| direct::claimed(reg, detected))
        .unwrap_or_default()
}

pub(crate) fn prune_matching(names: &[String], dry_run: bool) -> Vec<Prune> {
    let mut out = Vec::new();
    let detected = clients::detect_clients();
    let reg = crate::plus::registry_ro::read_opt();
    let claimed = claimed_entries(reg.as_ref(), &detected);
    for client in &detected {
        if !client.config_exists || client.uses_connectors {
            continue;
        }
        let hits: Vec<String> = direct_entries(client, &claimed)
            .filter(|n| names.iter().any(|m| m.eq_ignore_ascii_case(n)))
            .map(String::from)
            .collect();
        if hits.is_empty() {
            continue;
        }
        out.push(prune_one(&client.id, &hits, dry_run));
    }
    out
}

/// The launcher entries `server uninstall` takes out with the server, folded into the rows of
/// the clients that also had plain direct entries to prune.
pub(crate) fn prune_launchers(server_id: &str, dry_run: bool, plans: &mut Vec<Prune>) {
    for (client_id, result) in direct::remove_recorded(Removing::Server(server_id), dry_run) {
        let (removed, path, backup, error) = match result {
            Ok(outcome) if outcome.changed() => (
                vec![outcome.entry.clone()],
                Some(outcome.path.clone()).filter(|p| !p.is_empty()),
                outcome.backup.clone(),
                None,
            ),
            Ok(_) => continue,
            Err(error) => (Vec::new(), None, None, Some(error.message)),
        };
        match plans.iter_mut().find(|p| p.client_id == client_id) {
            Some(plan) => {
                plan.removed.extend(removed);
                plan.backup = plan.backup.take().or(backup);
                plan.error = plan.error.take().or(error);
            }
            None => plans.push(Prune {
                client_id,
                path,
                removed,
                backup,
                error,
            }),
        }
    }
}

fn prune_one(client_id: &str, names: &[String], dry_run: bool) -> Prune {
    match clients::prune_entries(client_id, names, dry_run) {
        Ok(done) => Prune {
            client_id: client_id.to_string(),
            path: Some(done.path),
            removed: done.removed,
            backup: done.backup,
            error: None,
        },
        Err(error) => Prune {
            client_id: client_id.to_string(),
            path: None,
            removed: Vec::new(),
            backup: None,
            error: Some(error),
        },
    }
}

pub(crate) fn gateway_state(client: &DetectedClient) -> &'static str {
    match client.entry_state {
        GatewayEntryState::Managed => "managed",
        GatewayEntryState::Customized => "customized",
        GatewayEntryState::Absent => "absent",
    }
}

fn is_registered(reg: &Registry, name: &str) -> bool {
    reg.servers
        .iter()
        .any(|s| s.id.eq_ignore_ascii_case(name) || s.name.eq_ignore_ascii_case(name))
}

pub(crate) struct SyncArgs {
    pub clients: Vec<String>,
    pub dry_run: bool,
    pub keep_orphans: bool,
}

pub(crate) struct SyncReport {
    pub dry_run: bool,
    pub rows: Vec<Value>,
    pub failed: bool,
}

impl SyncReport {
    pub(crate) fn to_value(&self) -> Value {
        json!({"dryRun": self.dry_run, "clients": self.rows})
    }
}

pub(crate) fn sync(args: &SyncArgs) -> Result<SyncReport, OpError> {
    let (ids, dry_run) = (args.clients.clone(), args.dry_run);
    let snap = snapshot();
    if let Some(error) = snap.registry_error {
        return Err(OpError::failed("registry_error", error));
    }
    let reg = snap.registry.unwrap_or_default();
    let detected = clients::detect_clients();
    let claimed = claimed_entries(Some(&reg), &detected);
    let launchers = direct::assess(&reg, &detected);
    let mut targets: Vec<String> = if ids.is_empty() {
        let mut ids: Vec<String> = reg.client_managed_entries.keys().cloned().collect();
        ids.sort();
        ids
    } else {
        ids.clone()
    };
    targets.dedup();
    for id in &ids {
        if !detected.iter().any(|c| &c.id == id) {
            return Err(OpError::not_found(format!("unknown client '{id}'")));
        }
    }
    let mut rows = Vec::new();
    let mut records = Vec::new();
    let mut failed = false;
    for id in &targets {
        let Some(client) = detected.iter().find(|c| &c.id == id) else {
            continue;
        };
        let mut row = json!({
            "client": id,
            "gateway": gateway_state(client),
            "removed": [],
            "kept": [],
            "direct": [],
            "backups": [],
            "error": null,
        });
        if !client.config_exists && !client.app_present {
            row["gateway"] = json!("not-installed");
            rows.push(row);
            continue;
        }
        let mut backups: Vec<String> = Vec::new();
        let mut error: Option<String> = None;
        match client.entry_state {
            GatewayEntryState::Absent => {
                if dry_run {
                    row["gateway"] = json!("would-install");
                } else {
                    let scope = reg.client_scopes.get(id).filter(|s| !s.is_empty());
                    match clients::install_gateway(id, scope.map(String::as_str)) {
                        Ok(outcome) => {
                            row["gateway"] = json!("installed");
                            backups.extend(outcome.backup);
                            if let Some(managed) = outcome.managed {
                                records.push((id.clone(), managed));
                            }
                        }
                        Err(e) => error = Some(e),
                    }
                }
            }
            GatewayEntryState::Customized => {
                row["gateway"] = json!("customized");
            }
            GatewayEntryState::Managed => {}
        }
        if error.is_none() {
            let mut removals: Vec<(String, &str)> = Vec::new();
            let mut kept: Vec<String> = Vec::new();
            for name in direct_entries(client, &claimed) {
                if is_registered(&reg, name) {
                    removals.push((name.to_string(), "redundant"));
                } else if args.keep_orphans {
                    kept.push(name.to_string());
                } else {
                    removals.push((name.to_string(), "orphan"));
                }
            }
            let names: Vec<String> = removals.iter().map(|(n, _)| n.clone()).collect();
            if !names.is_empty() {
                let done = prune_one(id, &names, dry_run);
                backups.extend(done.backup);
                error = done.error;
                if error.is_none() {
                    row["removed"] = json!(removals
                        .iter()
                        .map(|(n, why)| json!({"name": n, "reason": why}))
                        .collect::<Vec<_>>());
                }
            }
            let ours = launchers.iter().filter(|l| l.client == *id);
            let mut left_alone: Vec<String> = Vec::new();
            for launcher in ours {
                match launcher.state {
                    direct::State::Orphan if args.keep_orphans => {
                        kept.push(launcher.entry.clone())
                    }
                    direct::State::Orphan => {}
                    direct::State::Unrecorded if launcher.server_name.is_none() => {}
                    _ => left_alone.push(launcher.entry.clone()),
                }
            }
            if error.is_none() && !args.keep_orphans {
                let mut orphans = Vec::new();
                for (_, result) in direct::remove_recorded(Removing::Orphans(Some(id)), dry_run) {
                    match result {
                        Ok(done) => {
                            backups.extend(done.backup.clone());
                            orphans.push(json!({"name": done.entry, "reason": "orphan"}));
                        }
                        Err(e) => error = Some(e.message),
                    }
                }
                if !orphans.is_empty() {
                    let mut listed = row["removed"].as_array().cloned().unwrap_or_default();
                    listed.extend(orphans);
                    row["removed"] = json!(listed);
                }
            }
            row["kept"] = json!(kept);
            row["direct"] = json!(left_alone);
        }
        row["backups"] = json!(backups);
        if let Some(e) = error {
            failed = true;
            row["error"] = json!(e);
        }
        rows.push(row);
    }
    if !records.is_empty() {
        registry::update(|reg| {
            for (id, managed) in records {
                reg.set_client_managed_entry(&id, managed);
            }
            Ok(())
        })
        .map_err(|e| OpError::failed("registry_error", e))?;
    }
    Ok(SyncReport {
        dry_run,
        rows,
        failed,
    })
}
