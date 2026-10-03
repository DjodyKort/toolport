use super::commands::snapshot;
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{no_args, CtlError, Output};
use crate::clients::{self, DetectedClient, GatewayEntryState};
use crate::registry::{self, Registry};
use serde_json::{json, Value};

const SYNC_USAGE: &str = "usage: client sync [--client <id>]... [--dry-run] [--keep-orphans]";

pub(super) struct Prune {
    pub client_id: String,
    pub path: Option<String>,
    pub removed: Vec<String>,
    pub backup: Option<String>,
    pub error: Option<String>,
}

impl Prune {
    pub fn to_value(&self) -> Value {
        json!({
            "client": self.client_id,
            "path": self.path,
            "removed": self.removed,
            "backup": self.backup,
            "error": self.error,
        })
    }

    pub fn line(&self, dry_run: bool) -> String {
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

fn direct_entries(client: &DetectedClient) -> impl Iterator<Item = &str> {
    client
        .servers
        .iter()
        .filter(|s| !clients::detected_is_gateway(s))
        .map(|s| s.name.as_str())
}

pub(super) fn prune_matching(names: &[String], dry_run: bool) -> Vec<Prune> {
    let mut out = Vec::new();
    for client in clients::detect_clients() {
        if !client.config_exists || client.uses_connectors {
            continue;
        }
        let hits: Vec<String> = direct_entries(&client)
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

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot();
    let reg = snap.registry.unwrap_or_default();
    let rows: Vec<Value> = clients::detect_clients()
        .iter()
        .filter(|c| c.config_exists || c.app_present)
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "path": c.config_path,
                "gateway": gateway_state(c),
                "entries": direct_entries(c).collect::<Vec<_>>(),
                "scope": reg.client_scopes.get(&c.id).filter(|s| !s.is_empty()),
                "managed": reg.client_managed_entry(&c.id).is_some(),
            })
        })
        .collect();
    let human = if rows.is_empty() {
        "No clients detected.".to_string()
    } else {
        rows.iter()
            .map(|r| {
                format!(
                    "{:<16} {:<11} {} direct entries",
                    r["id"].as_str().unwrap_or(""),
                    r["gateway"].as_str().unwrap_or(""),
                    r["entries"].as_array().map_or(0, Vec::len)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(Output::new(json!({"clients": rows}), human))
}

fn gateway_state(client: &DetectedClient) -> &'static str {
    match client.entry_state {
        GatewayEntryState::Managed => "managed",
        GatewayEntryState::Customized => "customized",
        GatewayEntryState::Absent => "absent",
    }
}

const SYNC: Spec = Spec {
    flags: &[
        value("--client").needs("an id"),
        switch("--dry-run"),
        switch("--keep-orphans"),
    ],
    inline: Inline::Off,
    unknown: Unknown::Usage(SYNC_USAGE),
    operands: Operands::Reject,
};

fn is_registered(reg: &Registry, name: &str) -> bool {
    reg.servers
        .iter()
        .any(|s| s.id.eq_ignore_ascii_case(name) || s.name.eq_ignore_ascii_case(name))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    let (ids, dry_run) = (flags.all("--client"), flags.on("--dry-run"));
    let snap = snapshot();
    if let Some(error) = snap.registry_error {
        return Err(CtlError::new("registry_error", error));
    }
    let reg = snap.registry.unwrap_or_default();
    let detected = clients::detect_clients();
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
            return Err(CtlError::new("not_found", format!("unknown client '{id}'")));
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
            for name in direct_entries(client) {
                if is_registered(&reg, name) {
                    removals.push((name.to_string(), "redundant"));
                } else if flags.on("--keep-orphans") {
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
            row["kept"] = json!(kept);
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
        .map_err(|e| CtlError::new("registry_error", e))?;
    }
    let human = if rows.is_empty() {
        "No managed clients to sync.".to_string()
    } else {
        rows.iter()
            .map(sync_line(dry_run))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut output = Output::new(json!({"dryRun": dry_run, "clients": rows}), human);
    output.failed = failed;
    Ok(output)
}

fn sync_line(dry_run: bool) -> impl Fn(&Value) -> String {
    move |row| {
        let removed: Vec<String> = row["removed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                format!(
                    "{} ({})",
                    r["name"].as_str().unwrap_or(""),
                    r["reason"].as_str().unwrap_or("")
                )
            })
            .collect();
        let mut line = format!(
            "{}: gateway {}",
            row["client"].as_str().unwrap_or(""),
            row["gateway"].as_str().unwrap_or("")
        );
        if !removed.is_empty() {
            line.push_str(&format!(
                ", {} {}",
                if dry_run { "would remove" } else { "removed" },
                removed.join(", ")
            ));
        }
        let kept = row["kept"].as_array().map_or(0, Vec::len);
        if kept > 0 {
            line.push_str(&format!(", kept {kept} orphan(s)"));
        }
        if let Some(e) = row["error"].as_str() {
            line.push_str(&format!(", error: {e}"));
        }
        line
    }
}
