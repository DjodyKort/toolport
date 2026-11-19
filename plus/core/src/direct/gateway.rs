//! `clients_sync`: refreshes the gateway entry a client already holds when it no longer points at
//! the installed gateway program. It never adds the entry to a client that has none (connecting a
//! client is the person's choice, in the app), never touches a scoped install or an entry with
//! other settings, and writes only the one entry named `toolport`, through the same writer and
//! conflict check as a direct entry (D-016: one gateway entry per client).

use serde_json::{json, Value};
use toolport_plus_api::{ClientInfo, EntrySpec, Host};

use super::{plan_hash, write_error};
use crate::context::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use crate::op::OpError;

const GATEWAY_ENTRY: &str = "toolport";
const CLIENT_ID_ENV: &str = "TOOLPORT_CLIENT_ID";
const PROFILE_ENV: &str = "TOOLPORT_PROFILE";

#[derive(Debug, Clone, Default)]
pub struct SyncArgs<'a> {
    pub client: Option<&'a str>,
    pub dry_run: bool,
    pub plan_hash: Option<&'a str>,
}

enum State {
    Ok,
    Stale { revision: Option<String> },
    Skipped(String),
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            State::Ok => "ok",
            State::Stale { .. } => "stale",
            State::Skipped(_) => "skipped",
        }
    }
}

fn is_codex(client_id: &str) -> bool {
    client_id == "codex" || client_id.starts_with("codex@")
}

fn classify(host: &dyn Host, client: &ClientInfo, gateway: &str) -> Result<State, OpError> {
    let skip = |why: &str| Ok(State::Skipped(why.to_string()));
    if is_codex(&client.id) {
        return skip("Codex keeps per-entry settings that only the app's Connect writes");
    }
    if client.uses_connectors {
        return skip("the client lists its servers through an account, not a file");
    }
    if let Some(reason) = &client.direct_unsupported {
        return Ok(State::Skipped(format!(
            "Toolport cannot write an entry here: {reason}"
        )));
    }
    if !client.gateway_installed {
        return skip("the client has no gateway entry; connect it in the Toolport app");
    }
    let Some(entry) = client.entries.iter().find(|e| e.name == GATEWAY_ENTRY) else {
        return skip("the gateway entry has another name; reconnect the client in the app");
    };
    if entry.env_keys.iter().any(|key| key == PROFILE_ENV) {
        return skip("a scoped install: the app writes its initial profile");
    }
    if entry.env_keys.iter().any(|key| key != CLIENT_ID_ENV) {
        return skip("the entry carries environment variables Toolport did not write");
    }
    let current = entry.command.as_deref() == Some(gateway)
        && entry.args.is_empty()
        && entry.env_keys.iter().any(|key| key == CLIENT_ID_ENV);
    if current {
        return Ok(State::Ok);
    }
    let revision = host
        .client_revision(&client.id)
        .map_err(|error| OpError::failed("client_error", error))?;
    Ok(State::Stale { revision })
}

fn spec_for(client: &ClientInfo, gateway: &str) -> EntrySpec {
    EntrySpec {
        name: GATEWAY_ENTRY.into(),
        transport: "stdio".into(),
        command: Some(gateway.to_string()),
        env: [(CLIENT_ID_ENV.to_string(), client.id.clone())].into(),
        ..Default::default()
    }
}

pub fn sync(host: &dyn Host, args: &SyncArgs) -> Result<Value, OpError> {
    let gateway = host.gateway_command().ok_or_else(|| {
        OpError::failed(
            "no_gateway",
            "the toolport-gateway program could not be found",
        )
    })?;
    let all = host.detect_clients();
    let targets: Vec<&ClientInfo> = match args.client.map(str::trim).filter(|c| !c.is_empty()) {
        Some(id) => vec![all
            .iter()
            .find(|client| client.id == id)
            .ok_or_else(|| OpError::not_found(format!("unknown client '{id}'")))?],
        None => all
            .iter()
            .filter(|client| client.gateway_installed)
            .collect(),
    };
    let mut rows = Vec::new();
    for client in targets {
        rows.push((client, classify(host, client, &gateway)?));
    }
    let mut steps = Vec::new();
    let mut hashed = Vec::new();
    for (client, state) in &rows {
        let State::Stale { revision } = state else {
            continue;
        };
        let spec = spec_for(client, &gateway);
        let rendered = host
            .render_entry(&client.id, &spec)
            .map_err(|error| OpError::failed("render_failed", error))?;
        let before = client
            .entries
            .iter()
            .find(|e| e.name == GATEWAY_ENTRY)
            .and_then(|e| e.command.clone())
            .unwrap_or_default();
        steps.push(Step::merge(
            &client.config_path,
            format!(
                "point {}'s '{GATEWAY_ENTRY}' entry at the installed gateway and leave every other entry alone",
                client.name
            ),
            &["entry"],
            Diff {
                before: format!("command: {before}"),
                after: format!("+ {rendered}"),
            },
        ));
        hashed.push(json!({
            "client": client.id, "path": client.config_path, "revision": revision,
            "before": before, "spec": spec,
        }));
    }
    let plan = PlanV1 {
        summary: format!(
            "Refresh the gateway entry of {} client(s)",
            hashed.len()
        ),
        steps,
        effects: Effects::default(),
        warnings: vec![
            "Only a gateway entry a client already holds is rewritten; a backup of each client file is made first".into(),
        ],
        undo: "restore the backup the write reports for each client file".into(),
    };
    let hash = plan_hash(&json!({"op": "clients_sync", "gateway": gateway, "rows": hashed}));
    let clients: Vec<Value> = rows
        .iter()
        .map(|(client, state)| {
            let mut row = json!({
                "client": client.id,
                "clientName": client.name,
                "path": client.config_path,
                "state": state.name(),
            });
            if let State::Skipped(why) = state {
                row["reason"] = json!(why);
            }
            row
        })
        .collect();
    if args.dry_run {
        return Ok(json!({
            "dryRun": true,
            "changed": !hashed.is_empty(),
            "planHash": hash,
            "clients": clients,
            "plan": plan,
            "result": null,
        }));
    }
    if let Some(given) = args.plan_hash {
        if given != hash {
            return Err(OpError::conflict(
                "the plan changed since it was shown (a client file or the gateway differs); run it with dry_run again",
            ));
        }
    }
    let mut changed = Vec::new();
    let mut backups = Vec::new();
    for (client, state) in &rows {
        let State::Stale { revision } = state else {
            continue;
        };
        let written = host
            .write_entry(
                &client.id,
                GATEWAY_ENTRY,
                Some(&spec_for(client, &gateway)),
                Some(revision),
            )
            .map_err(write_error)?;
        changed.push(written.path);
        backups.extend(written.backup);
    }
    let result = ResultV1 {
        applied: true,
        changed,
        undo: plan.undo.clone(),
        backups,
    };
    Ok(json!({
        "dryRun": false,
        "changed": !result.changed.is_empty(),
        "planHash": hash,
        "clients": clients,
        "plan": plan,
        "result": result,
    }))
}
