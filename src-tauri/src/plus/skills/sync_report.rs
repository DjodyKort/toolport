//! What a skills sync reports beyond its counts: what each entry synced and warned about and how
//! each file that shadows a synced skill was handled. One builder serves the selfmcp
//! `skills_sync` tool, the `plus.skills.sync` handler that forwards to it and `toolportctl skills
//! sync`; `plus.skills.resolve` shares the collision rows.

use super::collisions::{Action, CollisionSummary, BACKUP_DIR_NAME};
use super::lock::{LockEntry, LockFile};
use super::state_handlers::display;
use super::sync::SyncResult;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

fn action_name(action: &Action) -> &'static str {
    match action {
        Action::Replaced => "replaced",
        Action::Kept => "kept",
        Action::SkippedDryRun => "skipped-dry-run",
    }
}

pub fn collision_rows(summary: &CollisionSummary) -> Vec<Value> {
    summary
        .resolutions
        .iter()
        .map(|r| {
            json!({
                "skill": r.collision.skill_name,
                "client": r.collision.client_key,
                "collisionPath": display(&r.collision.collision_path),
                "syncedPath": display(&r.collision.synced_path),
                "action": action_name(&r.action),
                "backupPath": r.backup_path.as_deref().map(display),
            })
        })
        .collect()
}

/// Skills first, then rules; a rule that shares a skill's name takes over that row, as mcpm's
/// merged dictionary does.
fn merged(lock: &LockFile) -> Vec<(&str, &'static str, &LockEntry)> {
    let mut rows: Vec<(&str, &'static str, &LockEntry)> = lock
        .skills
        .iter()
        .map(|(name, entry)| (name.as_str(), "skill", entry))
        .collect();
    for (name, entry) in &lock.rules {
        let row = (name.as_str(), "rule", entry);
        match rows.iter_mut().find(|r| r.0 == name) {
            Some(slot) => *slot = row,
            None => rows.push(row),
        }
    }
    rows
}

pub fn sync_report(result: &SyncResult) -> Map<String, Value> {
    let rows = merged(&result.lockfile);
    let clients: BTreeSet<&String> = rows
        .iter()
        .flat_map(|(_, _, entry)| entry.clients_synced.iter())
        .collect();
    let summary = &result.collisions;
    let mut report = Map::new();
    report.insert("clientCount".into(), json!(clients.len()));
    report.insert(
        "entries".into(),
        rows.iter()
            .map(|(name, kind, entry)| {
                json!({
                    "name": name,
                    "type": kind,
                    "clientsSynced": entry.clients_synced,
                    "warnings": entry.warnings,
                })
            })
            .collect(),
    );
    report.insert(
        "backupRoot".into(),
        json!(display(&result.output_root.join(BACKUP_DIR_NAME))),
    );
    report.insert("replaced".into(), json!(summary.replaced().count()));
    report.insert("kept".into(), json!(summary.kept().count()));
    report.insert("collisions".into(), Value::Array(collision_rows(summary)));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::skills::lock::{set_entry, LockEntry};

    fn entry(client: &str) -> LockEntry {
        let mut entry = LockEntry::new(None, "h".into());
        entry.clients_synced.push(client.into());
        entry
    }

    fn result(lock: LockFile) -> SyncResult {
        SyncResult {
            lockfile: lock,
            cleaned: Vec::new(),
            collisions: CollisionSummary::default(),
            output_root: "/out".into(),
        }
    }

    #[test]
    fn a_rule_replaces_the_skill_of_the_same_name_in_place() {
        let mut lock = LockFile::new("t".into());
        set_entry(&mut lock.skills, "a", entry("cursor"));
        set_entry(&mut lock.skills, "b", entry("cursor"));
        set_entry(&mut lock.rules, "a", entry("zed"));
        set_entry(&mut lock.rules, "c", entry("zed"));
        let report = sync_report(&result(lock));
        let rows: Vec<(&str, &str)> = report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["name"].as_str().unwrap(), e["type"].as_str().unwrap()))
            .collect();
        assert_eq!(rows, [("a", "rule"), ("b", "skill"), ("c", "rule")]);
        assert_eq!(report["clientCount"], 2);
        assert_eq!(
            report["backupRoot"],
            json!(std::path::Path::new("/out").join(BACKUP_DIR_NAME).to_string_lossy())
        );
        assert_eq!(report["collisions"], json!([]));
    }
}
