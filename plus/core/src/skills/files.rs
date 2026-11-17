//! Every file of one library skill with what the next sync does to it, for the Files section of a
//! skill's detail and for `toolportctl skills files`. The statuses come from a dry-run sync, so
//! they are the plan's verdict and cannot drift from what Apply does.

use super::api::{home, resolve_repo, Args};
use super::assets::{deny_list, inventory, rel_string};
use super::client_scope::{default_clients, ensure_supported};
use super::lint::{links_of, LinkState};
use super::lock::{get_entry, load_lockfile, LockFile};
use super::ops::lock_dir;
use super::parser::{discover_skills, Skill, SkillType, DESCRIPTION_LIMIT, SPEC_DESCRIPTION_LIMIT};
use super::sync::{sync_skills, Change, Op, SyncOptions};
use super::transpilers::registry;
use super::SystemClock;
use crate::op::OpError;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

pub const COPIED: &str = "copied";
pub const WOULD_COPY: &str = "would-copy";
pub const DIFFERS: &str = "differs";
pub const HAND_EDITED: &str = "hand-edited";
pub const REMOVED_ON_NEXT_SYNC: &str = "removed-on-next-sync";
pub const NOT_COPIED: &str = "not-copied";
pub const NOT_YOURS: &str = "not-yours";
pub const KEPT: &str = "kept";

const STATUSES: [&str; 8] = [
    COPIED,
    WOULD_COPY,
    DIFFERS,
    HAND_EDITED,
    REMOVED_ON_NEXT_SYNC,
    NOT_COPIED,
    NOT_YOURS,
    KEPT,
];

struct Row {
    path: String,
    client: Option<String>,
    status: &'static str,
    op: Option<Op>,
    size: u64,
    reason: Option<String>,
}

fn status_of(op: Op) -> Option<&'static str> {
    match op {
        Op::Unchanged | Op::Adopt => Some(COPIED),
        Op::Add | Op::Update => Some(WOULD_COPY),
        Op::Differs => Some(DIFFERS),
        Op::Edited => Some(HAND_EDITED),
        Op::Remove => Some(REMOVED_ON_NEXT_SYNC),
        Op::Kept => Some(KEPT),
        Op::Hook => None,
    }
}

fn size_of(path: &Path) -> u64 {
    fs::symlink_metadata(path).map_or(0, |m| m.len())
}

fn installed_files(dir: &Path, base: &Path, out: &mut Vec<String>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            installed_files(&path, base, out);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel_string(rel));
        }
    }
}

fn tracked(lock: Option<&LockFile>, skill: &Skill, client: &str) -> BTreeSet<String> {
    let Some(lock) = lock else {
        return BTreeSet::new();
    };
    let bucket = match skill.skill_type {
        SkillType::Rule => &lock.rules,
        SkillType::Skill => &lock.skills,
    };
    get_entry(bucket, skill.name())
        .and_then(|entry| entry.output_files.iter().find(|(key, _)| key == client))
        .map(|(_, rels)| rels.iter().cloned().collect())
        .unwrap_or_default()
}

fn change_rows(
    changes: &[&Change],
    dst_dir: &Path,
    root: &Path,
    source_dir: &Path,
    skill_file: &Path,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for change in changes {
        let Some(status) = status_of(change.op) else {
            continue;
        };
        let abs = {
            let given = Path::new(&change.path);
            if given.is_absolute() {
                given.to_path_buf()
            } else {
                root.join(given)
            }
        };
        let path = abs
            .strip_prefix(dst_dir)
            .map_or_else(|_| change.path.clone(), rel_string);
        let size = if path == "SKILL.md" {
            size_of(skill_file)
        } else if source_dir.join(&path).exists() {
            size_of(&source_dir.join(&path))
        } else {
            size_of(&abs)
        };
        rows.push(Row {
            path,
            client: Some(change.client.clone()),
            status,
            op: Some(change.op),
            size,
            reason: None,
        });
    }
    rows
}

fn find<'a>(skills: &'a [Skill], name: &str) -> Result<&'a Skill, OpError> {
    skills
        .iter()
        .find(|skill| skill.name() == name)
        .ok_or_else(|| OpError::not_found(format!("skill not found: {name}")))
}

/// `args.name` is the skill; `args.clients` narrows the clients as for `sync`.
pub fn skill_files(args: &Args) -> Result<Value, OpError> {
    let repo = resolve_repo(args.repo.as_deref())?;
    let skills = discover_skills(&repo);
    let skill = find(&skills, &args.name)?;
    let registry = registry();
    ensure_supported(args.clients.iter().flatten().map(String::as_str), &registry)
        .map_err(OpError::usage)?;
    let global = args.global;
    let root = if global { home()? } else { repo.clone() };
    let dir = lock_dir(global, &repo);
    let lock = load_lockfile(&dir);
    let targeted = match &args.clients {
        Some(keys) => keys.clone(),
        None => default_clients(lock.as_ref(), &registry).0,
    };
    let opts = SyncOptions {
        output_root: root.clone(),
        lock_dir: dir,
        global_mode: global,
        dry_run: true,
        migrate: None,
        client_keys: Some(targeted.clone()),
        apply_held: false,
        allow_tap_hooks: false,
        clock: &SystemClock,
        settings: None,
        unattended: false,
        journal: None,
        take: Vec::new(),
        only: None,
    };
    let plan = sync_skills(&skills, &registry, &opts)
        .map_err(|message| OpError::failed("skills", message))?
        .plan;
    let changes: Vec<&Change> = plan
        .changes
        .iter()
        .filter(|change| change.skill == skill.name())
        .collect();

    let mut rows = Vec::new();
    let mut installed = Vec::new();
    for client in &targeted {
        let Some(transpiler) = registry.get(client) else {
            continue;
        };
        if transpiler.capabilities().append_mode {
            continue;
        }
        let Ok(out) = transpiler.transpile(skill, &root) else {
            continue;
        };
        let Some(dst_dir) = out.output_path.parent().map(Path::to_path_buf) else {
            continue;
        };
        let mine: Vec<&Change> = changes
            .iter()
            .copied()
            .filter(|c| &c.client == client)
            .collect();
        let mut client_rows = change_rows(
            &mine,
            &dst_dir,
            &root,
            skill.source_dir(),
            &skill.source_path,
        );
        let known: BTreeSet<String> = client_rows.iter().map(|r| r.path.clone()).collect();
        let owned = tracked(lock.as_ref(), skill, client);
        if skill.skill_type == SkillType::Skill {
            let mut on_disk = Vec::new();
            installed_files(&dst_dir, &dst_dir, &mut on_disk);
            for path in on_disk {
                let abs_rel = rel_string(
                    &dst_dir
                        .join(&path)
                        .strip_prefix(&root)
                        .map_or_else(|_| dst_dir.join(&path), Path::to_path_buf),
                );
                if known.contains(&path) || owned.contains(&abs_rel) {
                    continue;
                }
                client_rows.push(Row {
                    size: size_of(&dst_dir.join(&path)),
                    path,
                    client: Some(client.clone()),
                    status: NOT_YOURS,
                    op: None,
                    reason: None,
                });
            }
        }
        installed.push(json!({
            "client": client,
            "dir": dst_dir.to_string_lossy(),
            "present": dst_dir.is_dir(),
        }));
        rows.extend(client_rows);
    }
    for skipped in plan.skipped.iter().filter(|s| s.skill == skill.name()) {
        let path = skipped.path.clone();
        rows.push(Row {
            size: size_of(&skill.source_dir().join(path.trim_end_matches('/'))),
            path,
            client: None,
            status: NOT_COPIED,
            op: None,
            reason: Some(skipped.reason.clone()),
        });
    }
    let order = |row: &Row| {
        (
            row.path != "SKILL.md",
            matches!(row.status, NOT_COPIED | NOT_YOURS),
            row.path.clone(),
            row.client.clone(),
        )
    };
    rows.sort_by_key(order);

    let mut counts: BTreeMap<&str, usize> = STATUSES.iter().map(|s| (*s, 0)).collect();
    for row in &rows {
        *counts.entry(row.status).or_default() += 1;
    }
    let found = inventory(skill.source_dir());
    let links: Vec<Value> = links_of(skill, &found)
        .into_iter()
        .map(|link| {
            let (state, reason) = match link.state {
                LinkState::Installed => ("installed", None),
                LinkState::OutsideFolder => ("outside-folder", None),
                LinkState::Missing => ("missing", None),
                LinkState::NotInstalled(why) => ("not-installed", Some(why)),
            };
            json!({"target": link.target, "line": link.line, "state": state, "reason": reason})
        })
        .collect();
    Ok(json!({
        "repo": repo.to_string_lossy(),
        "skill": skill.name(),
        "type": skill.skill_type.as_str(),
        "source": skill.source_dir().to_string_lossy(),
        "lockfilePresent": lock.is_some(),
        "targetedClients": targeted,
        "installed": installed,
        "denyList": deny_list(),
        "description": {
            "length": skill.frontmatter.description.chars().count(),
            "specLimit": SPEC_DESCRIPTION_LIMIT,
            "limit": DESCRIPTION_LIMIT,
        },
        "counts": counts,
        "files": rows.iter().map(|row| json!({
            "path": row.path,
            "client": row.client,
            "status": row.status,
            "op": row.op.map(Op::as_str),
            "size": row.size,
            "reason": row.reason,
        })).collect::<Vec<_>>(),
        "links": links,
    }))
}
