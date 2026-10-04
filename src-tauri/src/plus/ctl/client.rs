use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{no_args, CtlError, Output};
use crate::clients;
use crate::plus::client_sync::{
    claimed_entries, direct_entries, gateway_state, sync as sync_clients, SyncArgs,
};
use crate::plus::direct;
use crate::plus::status::snapshot;
use serde_json::{json, Value};

const SYNC_USAGE: &str = "usage: client sync [--client <id>]... [--dry-run] [--keep-orphans]";

pub fn ls(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot();
    let reg = snap.registry.unwrap_or_default();
    let detected = clients::detect_clients();
    let claimed = claimed_entries(Some(&reg), &detected);
    let launchers = direct::assess(&reg, &detected);
    let rows: Vec<Value> = detected
        .iter()
        .filter(|c| c.config_exists || c.app_present)
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "path": c.config_path,
                "gateway": gateway_state(c),
                "entries": direct_entries(c, &claimed).collect::<Vec<_>>(),
                "launchers": launchers
                    .iter()
                    .filter(|l| l.client == c.id)
                    .map(direct::Row::to_value)
                    .collect::<Vec<_>>(),
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
                let mut line = format!(
                    "{:<16} {:<11} {} direct entries",
                    r["id"].as_str().unwrap_or(""),
                    r["gateway"].as_str().unwrap_or(""),
                    r["entries"].as_array().map_or(0, Vec::len)
                );
                let launchers = r["launchers"].as_array().map_or(0, Vec::len);
                if launchers > 0 {
                    line.push_str(&format!(", {launchers} direct launcher entries"));
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(Output::new(json!({"clients": rows}), human))
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
    ..Spec::PLAIN
};

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    let report = sync_clients(&SyncArgs {
        clients: flags.all("--client"),
        dry_run: flags.on("--dry-run"),
        keep_orphans: flags.on("--keep-orphans"),
    })?;
    let human = if report.rows.is_empty() {
        "No managed clients to sync.".to_string()
    } else {
        report
            .rows
            .iter()
            .map(sync_line(report.dry_run))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut output = Output::new(report.to_value(), human);
    output.failed = report.failed;
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
        let direct = row["direct"].as_array().map_or(0, Vec::len);
        if direct > 0 {
            line.push_str(&format!(", left {direct} direct launcher entr(ies) alone"));
        }
        if let Some(e) = row["error"].as_str() {
            line.push_str(&format!(", error: {e}"));
        }
        line
    }
}
