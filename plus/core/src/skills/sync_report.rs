//! What a skills sync reports beyond its counts: what each entry synced and warned about and how
//! each file that shadows a synced skill was handled. One builder serves `api::sync`, behind the
//! selfmcp `skills_sync` tool, `plus.skills.sync` and `toolportctl skills sync`;
//! `plus.skills.resolve` shares the collision rows.

use super::collisions::{Action, CollisionSummary, BACKUP_DIR_NAME};
use super::lock::{LockEntry, LockFile};
use super::sync::{Failure, Kind, Op, Plan, SyncResult};
use serde_json::{json, Map, Value};
use std::path::Path;

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
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

/// One step that did not complete; `id` is what `--only` takes to run it again.
fn failed_row(failure: &Failure) -> Value {
    json!({
        "id": failure.kind.scoped(&failure.name),
        "kind": failure.kind.as_str(),
        "name": failure.name,
        "client": failure.client,
        "step": failure.step,
        "message": failure.message,
    })
}

fn plan_json(plan: &Plan) -> Value {
    let ops = [
        Op::Add,
        Op::Update,
        Op::Adopt,
        Op::Unchanged,
        Op::Differs,
        Op::Edited,
        Op::Remove,
        Op::Hook,
        Op::Kept,
    ];
    let counts: Map<String, Value> = ops
        .iter()
        .map(|op| (op.as_str().to_string(), json!(plan.count(*op))))
        .collect();
    json!({
        "counts": counts,
        "held": plan.held().count(),
        "changes": plan
            .changes
            .iter()
            .filter(|c| !matches!(c.op, Op::Unchanged | Op::Kept))
            .map(|c| {
                let mut row = json!({
                    "skill": c.skill,
                    "client": c.client,
                    "path": c.path,
                    "op": c.op.as_str(),
                    "held": c.held,
                });
                if c.kind != Kind::Skill {
                    row["kind"] = json!(c.kind.as_str());
                }
                row
            })
            .collect::<Vec<_>>(),
        "skipped": plan
            .skipped
            .iter()
            .map(|s| json!({"skill": s.skill, "path": s.path, "reason": s.reason}))
            .collect::<Vec<_>>(),
        "failed": plan.failed.iter().map(failed_row).collect::<Vec<_>>(),
    })
}

/// Why a change waits for a decision: the word the screen and the bell show beside it.
fn held_reason(op: Op) -> &'static str {
    match op {
        Op::Remove => "removal",
        Op::Hook => "new-hook",
        Op::Differs => "differs",
        Op::Edited => "edited",
        _ => "held",
    }
}

fn held_rows(plan: &Plan) -> Vec<Value> {
    plan.held()
        .map(|c| {
            let mut row = json!({
                "skill": c.skill,
                "client": c.client,
                "path": c.path,
                "op": c.op.as_str(),
                "reason": held_reason(c.op),
            });
            if c.kind != Kind::Skill {
                row["kind"] = json!(c.kind.as_str());
            }
            row
        })
        .collect()
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
    report.insert("plan".into(), plan_json(&result.plan));
    report.insert("held".into(), Value::Array(held_rows(&result.plan)));
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::lock::{set_entry, LockEntry};

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
            plan: Plan::default(),
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
            json!(std::path::Path::new("/out")
                .join(BACKUP_DIR_NAME)
                .to_string_lossy())
        );
        assert_eq!(report["collisions"], json!([]));
    }
}
