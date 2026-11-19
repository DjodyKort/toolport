//! `sessions ls|clear|undo|restore|retention`: look at a repository's transcripts, move them to
//! Toolport's trash or archive, bring them back, and read or set Claude Code's one global
//! retention. Transcripts are only ever moved, never copied and never opened.

use super::claude_json::Known;
use super::overview;
use super::resolve::Repo;
use super::sessions::{self, Folder, Session};
use super::store::{self, Dest, Put, Store};
use super::World;
use crate::context::fsx;
use crate::context::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::Path;
use toolport_plus_api::SettingsStore;

const NO_PER_REPO: &str = "Claude Code keeps one retention value for every project and has no per-repository switch for transcripts";

fn repo_of(cwd: &Path) -> Result<Repo, OpError> {
    match fsx::folder_problem(cwd) {
        Some(problem) => Err(OpError::usage(format!("cwd {problem}"))),
        None => Ok(Repo::of(cwd)),
    }
}

fn entry_json(entry: &store::Entry, store: Store) -> Value {
    json!({
        "id": entry.id,
        "created": fsx::zulu(entry.created),
        "expiresAt": entry.expires(store).map(fsx::zulu),
        "sessions": entry.items.len(),
        "bytes": entry.bytes(),
    })
}

fn kept(world: &World, store: Store, repo: &Repo) -> Vec<Value> {
    store::list(world.data_dir, store, world.now)
        .iter()
        .filter(|e| e.kind == "sessions" && e.repo_root == fsx::display(&repo.root))
        .map(|e| entry_json(e, store))
        .collect()
}

pub fn ls(world: &World, cwd: Option<&Path>, all: bool) -> Result<Value, OpError> {
    let retention = sessions::retention(world.claude_dir);
    let known = Known::read(world.claude_json);
    if all {
        let overview = overview::overview(
            world,
            &overview::Options {
                all: true,
                cwd: None,
                sort: "name",
                filter: None,
            },
        )?;
        let rows: Vec<Value> = overview["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                json!({
                    "name": row["name"],
                    "root": row["root"],
                    "key": row["key"],
                    "sessions": row["sessions"],
                })
            })
            .collect();
        return Ok(json!({
            "scope": "all",
            "retention": sessions::retention_json(&retention),
            "repositories": rows,
            "other": overview["other"],
            "totals": overview["totals"],
        }));
    }
    let repo = repo_of(cwd.ok_or_else(|| OpError::usage("--cwd or --all is required"))?)?;
    let folders = sessions::folders_of(world.claude_dir, &repo, &known);
    let mut all_sessions: Vec<&Session> = folders.iter().flat_map(|f| f.sessions.iter()).collect();
    all_sessions.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.id.cmp(&b.id)));
    let totals = sessions::totals(all_sessions.iter().copied(), retention.days, world.now);
    Ok(json!({
        "scope": "repository",
        "cwd": fsx::display(&repo.cwd),
        "name": repo.name(),
        "root": fsx::display(&repo.root),
        "retention": sessions::retention_json(&retention),
        "note": NO_PER_REPO,
        "folders": folders.iter().map(|f| sessions::folder_json(f, retention.days, world.now)).collect::<Vec<_>>(),
        "sessions": all_sessions.iter().map(|s| s.to_json(retention.days, world.now)).collect::<Vec<_>>(),
        "totals": sessions::totals_json(&totals),
        "trash": kept(world, Store::Trash, &repo),
        "archive": kept(world, Store::Archive, &repo),
    }))
}

pub struct ClearArgs<'a> {
    pub cwd: &'a Path,
    pub ids: Vec<String>,
    pub older_than: Option<u64>,
    pub dest: Dest,
    pub confirm: Option<&'a str>,
    pub dry_run: bool,
}

fn select<'a>(
    folders: &'a [Folder],
    args: &ClearArgs,
    now: i64,
) -> Result<Vec<&'a Session>, OpError> {
    let all: Vec<&Session> = folders.iter().flat_map(|f| f.sessions.iter()).collect();
    if let Some(missing) = args.ids.iter().find(|id| !all.iter().any(|s| &s.id == *id)) {
        return Err(OpError::not_found(format!(
            "no session {missing} in this repository"
        )));
    }
    let mut picked: Vec<&Session> = all
        .into_iter()
        .filter(|s| {
            args.ids.contains(&s.id)
                || args
                    .older_than
                    .is_some_and(|days| now - s.modified > days as i64 * 86_400)
        })
        .collect();
    picked.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.id.cmp(&b.id)));
    if picked.is_empty() {
        return Err(OpError::not_found("no session matches"));
    }
    Ok(picked)
}

pub fn clear(world: &World, args: &ClearArgs) -> Result<Value, OpError> {
    if args.ids.is_empty() && args.older_than.is_none() {
        return Err(OpError::usage("give --ids or --older-than"));
    }
    let repo = repo_of(args.cwd)?;
    let known = Known::read(world.claude_json);
    let retention = sessions::retention(world.claude_dir);
    let folders = sessions::folders_of(world.claude_dir, &repo, &known);
    let picked = select(&folders, args, world.now)?;
    let bytes: u64 = picked.iter().map(|s| s.bytes).sum();
    let forever = args.dest == Dest::Forever;
    if forever && !args.dry_run && args.confirm != Some(repo.name().as_str()) {
        return Err(OpError::usage(format!(
            "deleting for good needs the repository name typed with --confirm {}",
            repo.name()
        )));
    }
    let (op, place) = match args.dest {
        Dest::Trash => ("move", "Toolport's trash; kept 30 days".to_string()),
        Dest::Archive => (
            "move",
            "Toolport's archive; kept until you delete them".to_string(),
        ),
        Dest::Forever => ("delete", "nowhere: deleted for good".to_string()),
    };
    let mut steps = Vec::new();
    for folder in &folders {
        let here: Vec<&&Session> = picked.iter().filter(|s| s.folder == folder.path).collect();
        if here.is_empty() {
            continue;
        }
        steps.push(Step {
            op,
            path: folder.dir.as_deref().map(fsx::display),
            detail: format!(
                "{} session(s), {} bytes, to {place}",
                here.len(),
                here.iter().map(|s| s.bytes).sum::<u64>()
            ),
            keys: None,
            diff: None,
        });
    }
    let mut warnings = vec![
        "Sessions are moved, never copied, so --resume <id> still finds each one exactly once".to_string(),
        "Prompt history, the project entry in ~/.claude.json and per-session task and debug folders are left alone".to_string(),
    ];
    if forever {
        warnings.push("This cannot be undone".into());
    }
    let cwd_text = fsx::display(&repo.cwd);
    let mut undo = match args.dest {
        Dest::Trash => format!("toolportctl sessions undo --cwd {cwd_text}"),
        Dest::Archive => "toolportctl sessions restore <id>".to_string(),
        Dest::Forever => String::new(),
    };
    let mut entry_id = None;
    let mut changed = Vec::new();
    if !args.dry_run {
        store::purge_expired(world.data_dir, world.now);
        let paths: Vec<_> = picked
            .iter()
            .flat_map(|s| s.paths.iter().cloned())
            .collect();
        match args.dest.store() {
            Some(target) => {
                let entry = store::put(
                    world.data_dir,
                    &Put {
                        store: target,
                        kind: "sessions",
                        now: world.now,
                        repo_root: &repo.root,
                        repo_name: &repo.name(),
                        froms: &paths,
                        switch_before: None,
                    },
                )?;
                undo = match target {
                    Store::Trash => format!("toolportctl sessions undo {}", entry.id),
                    Store::Archive => format!("toolportctl sessions restore {}", entry.id),
                };
                entry_id = Some(entry.id);
            }
            None => {
                for path in &paths {
                    store::remove_tree(path)
                        .map_err(|e| OpError::failed("memory_failed", e.to_string()))?;
                }
            }
        }
        changed = paths.iter().map(|p| fsx::display(p)).collect();
    }
    let plan = PlanV1 {
        summary: format!(
            "{} {} session(s) of {}",
            match args.dest {
                Dest::Trash => "Clear",
                Dest::Archive => "Archive",
                Dest::Forever => "Delete",
            },
            picked.len(),
            repo.name()
        ),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!args.dry_run).then(|| {
        json!({
            "applied": true,
            "changed": changed,
            "undo": if forever { Value::Null } else { json!(undo) },
            "backups": [],
            "id": entry_id,
        })
    });
    Ok(json!({
        "cwd": cwd_text,
        "root": fsx::display(&repo.root),
        "name": repo.name(),
        "dryRun": args.dry_run,
        "to": match args.dest { Dest::Trash => "trash", Dest::Archive => "archive", Dest::Forever => "forever" },
        "sessions": picked.iter().map(|s| s.to_json(retention.days, world.now)).collect::<Vec<_>>(),
        "totalBytes": bytes,
        "confirm": forever.then(|| repo.name()),
        "plan": plan,
        "result": result,
    }))
}

fn put_back(
    world: &World,
    entry: store::Entry,
    from: Store,
    dry_run: bool,
    undo: String,
) -> Result<Value, OpError> {
    let steps: Vec<Step> = entry
        .items
        .iter()
        .map(|item| Step {
            op: "move",
            path: Some(item.from.clone()),
            detail: format!("move {} bytes back", item.bytes),
            keys: None,
            diff: None,
        })
        .collect();
    let plan = PlanV1 {
        summary: format!(
            "Put back {} session item(s) of {}",
            entry.items.len(),
            entry.repo_name
        ),
        steps,
        effects: Effects::default(),
        warnings: Vec::new(),
        undo,
    };
    let restored = if dry_run {
        Vec::new()
    } else {
        store::restore(world.data_dir, from, &entry)?
    };
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed: restored,
        undo: plan.undo.clone(),
        backups: Vec::new(),
    });
    Ok(json!({
        "id": entry.id,
        "name": entry.repo_name,
        "root": entry.repo_root,
        "dryRun": dry_run,
        "plan": plan,
        "result": result,
    }))
}

pub fn undo(
    world: &World,
    id: Option<&str>,
    cwd: Option<&Path>,
    dry_run: bool,
) -> Result<Value, OpError> {
    let root = cwd.map(|c| Repo::of(c).root);
    let entry = store::find(
        world.data_dir,
        Store::Trash,
        "sessions",
        id,
        if id.is_some() { None } else { root.as_deref() },
        world.now,
    )?;
    let undo = format!("toolportctl sessions clear --cwd {}", entry.repo_root);
    put_back(world, entry, Store::Trash, dry_run, undo)
}

pub fn restore(world: &World, id: &str, dry_run: bool) -> Result<Value, OpError> {
    let entry = store::find(
        world.data_dir,
        Store::Archive,
        "sessions",
        Some(id),
        None,
        world.now,
    )?;
    let undo = format!(
        "toolportctl sessions clear --cwd {} --to archive",
        entry.repo_root
    );
    put_back(world, entry, Store::Archive, dry_run, undo)
}

pub fn retention(
    world: &World,
    store: &dyn SettingsStore,
    set: Option<u64>,
    dry_run: bool,
) -> Result<Value, OpError> {
    let current = sessions::retention(world.claude_dir);
    let Some(days) = set else {
        return Ok(json!({
            "retention": sessions::retention_json(&current),
            "note": NO_PER_REPO,
        }));
    };
    if days < 1 {
        return Err(OpError::usage(
            "cleanupPeriodDays must be at least 1; Claude Code rejects 0",
        ));
    }
    let (mut root, original) = store
        .read_settings(&current.file)
        .map_err(|e| OpError::failed("settings_unreadable", e))?;
    if !root.is_object() {
        return Err(OpError::failed(
            "settings_unreadable",
            format!("{} is not a JSON object", current.file.display()),
        ));
    }
    let before = root
        .get("cleanupPeriodDays")
        .map_or_else(|| "(absent)".to_string(), |v| v.to_string());
    root["cleanupPeriodDays"] = json!(days);
    let path = fsx::display(&current.file);
    let diff = Diff {
        before: format!("cleanupPeriodDays: {before}\n"),
        after: format!("cleanupPeriodDays: {days}\n"),
    };
    let unchanged = current.from_settings && current.days == days;
    let steps = if unchanged {
        Vec::new()
    } else if original.is_some() {
        vec![Step::merge(
            &path,
            "cleanupPeriodDays in the user settings",
            &["cleanupPeriodDays"],
            diff,
        )]
    } else {
        vec![Step::create(
            &path,
            "cleanupPeriodDays in the user settings",
            &["cleanupPeriodDays"],
            diff,
        )]
    };
    let undo = if current.from_settings {
        format!("toolportctl sessions retention --set {}", current.days)
    } else {
        format!(
            "toolportctl sessions retention --set {}",
            sessions::DEFAULT_RETENTION_DAYS
        )
    };
    let mut warnings = vec![
        "One value for every project: Claude Code deletes transcripts older than this at its next cleanup".to_string(),
        "Claude Code does not say which settings file applies when the cleanup runs over another project's transcripts; the user settings are the one written here".to_string(),
    ];
    if days < current.days {
        warnings.push(
            "A shorter period deletes sessions sooner; Move to archive keeps the ones you want"
                .into(),
        );
    }
    let mut changed = Vec::new();
    if !dry_run && !unchanged {
        store
            .write_key(
                &current.file,
                original.as_deref(),
                &root,
                "cleanupPeriodDays",
            )
            .map_err(|e| OpError::failed("settings_unwritable", e))?;
        changed.push(path);
    }
    let plan = PlanV1 {
        summary: format!("Keep sessions {days} days"),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed,
        undo,
        backups: Vec::new(),
    });
    Ok(json!({
        "retention": sessions::retention_json(&current),
        "set": days,
        "dryRun": dry_run,
        "plan": plan,
        "result": result,
    }))
}
