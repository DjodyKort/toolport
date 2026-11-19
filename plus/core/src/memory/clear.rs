//! Clearing a repository's auto memory: the folder moves to Toolport's trash for 30 days (undo
//! with `memory undo`) or is deleted for good after typing the repository's name. Instruction
//! files and sessions are never touched here.

use super::inventory;
use super::resolve::Repo;
use super::store::{self, Dest, Put, Store};
use super::switch::Position;
use super::World;
use crate::context::fsx;
use crate::context::leave_out;
use crate::context::loads::{InstructionLoads, LoadSource};
use crate::context::plan::{Effects, PlanV1, Step, TokenEffect};
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Args<'a> {
    pub cwd: &'a Path,
    pub dest: Dest,
    pub confirm: Option<&'a str>,
    pub also_off: bool,
    pub dry_run: bool,
}

/// A folder `autoMemoryDirectory` points at may be anywhere; only one that cannot hold the
/// repository or the home folder is moved.
fn safe_to_clear(dir: &Path, home: &Path, repo: &Repo) -> bool {
    let dir = fsx::canonical(dir);
    dir.parent().is_some()
        && !fsx::canonical(home).starts_with(&dir)
        && !repo.root.starts_with(&dir)
        && !dir.join(".git").exists()
}

pub fn clear(world: &World, args: &Args) -> Result<Value, OpError> {
    if let Some(problem) = fsx::folder_problem(args.cwd) {
        return Err(OpError::usage(format!("cwd {problem}")));
    }
    let repo = Repo::of(args.cwd);
    let layers = world.layers(&repo);
    let (dir, custom) = layers.directory(world.home, world.claude_dir, &repo);
    let files = inventory::files(&dir);
    if files.is_empty() {
        return Err(OpError::not_found(format!(
            "no auto memory is kept for {}",
            repo.name()
        )));
    }
    if custom && !safe_to_clear(&dir, world.home, &repo) {
        return Err(OpError::conflict(format!(
            "{} is set as the auto memory folder but also holds the repository or your home folder; clear it by hand",
            dir.display()
        )));
    }
    if args.also_off && layers.managed.enabled.is_some() {
        return Err(OpError::conflict(
            "managed settings decide auto memory on this Mac; it cannot be turned off here",
        ));
    }
    let summary = inventory::summary(&files);
    let forever = args.dest == Dest::Forever;
    if forever && !args.dry_run && args.confirm != Some(repo.name().as_str()) {
        return Err(OpError::usage(format!(
            "deleting for good needs the repository name typed with --confirm {}",
            repo.name()
        )));
    }
    let held = leave_out::held_in(world.data_dir, &repo.cwd);
    let previous = Position::of(layers.switch());
    let mut steps = vec![Step {
        op: if forever { "delete" } else { "move" },
        path: Some(fsx::display(&dir)),
        detail: if forever {
            format!(
                "delete {} file(s), {} bytes, for good",
                summary.count, summary.bytes
            )
        } else {
            format!(
                "move {} file(s), {} bytes, to Toolport's trash; kept {} days",
                summary.count,
                summary.bytes,
                store::TRASH_DAYS
            )
        },
        keys: None,
        diff: None,
    }];
    let mut warnings = Vec::new();
    if !forever {
        warnings.push(format!(
            "Undo from the trash works for {} days",
            store::TRASH_DAYS
        ));
    } else {
        warnings.push("This cannot be undone".into());
    }
    let sharing: Vec<String> = repo
        .worktrees
        .iter()
        .filter(|w| **w != repo.root)
        .map(|w| fsx::display(w))
        .collect();
    if !sharing.is_empty() {
        warnings.push(format!("{} worktree(s) share this memory", sharing.len()));
    }
    let mut tokens = None;
    if layers.effective().0 && !args.also_off {
        let loads = InstructionLoads {
            home: world.home,
            claude_dir: world.claude_dir,
            policy_file: world.policy_claude_md,
            count_tokens: world.count_tokens,
        };
        if let (Ok(found), Some(index)) = (
            loads.what_loads(&repo.cwd),
            inventory::index(&dir, world.count_tokens),
        ) {
            tokens = Some(TokenEffect {
                before: found.total_tokens,
                after: found.total_tokens.saturating_sub(index.tokens),
                basis: "estimate",
            });
        }
    }
    let cwd_text = fsx::display(&repo.cwd);
    let mut undo = format!("toolportctl memory undo --cwd {cwd_text}");
    if args.also_off {
        let run =
            leave_out::write_state(world.data_dir, &repo.cwd, &held.files, Some(false), true)?;
        steps.extend(run.steps.iter().cloned());
        warnings.extend(run.warnings.iter().cloned());
    }
    let mut changed = Vec::new();
    let mut trash_id = None;
    if !args.dry_run {
        store::purge_expired(world.data_dir, world.now);
        let mut entry = None;
        if forever {
            store::remove_tree(&dir)
                .map_err(|e| OpError::failed("memory_failed", e.to_string()))?;
        } else {
            let put = store::put(
                world.data_dir,
                &Put {
                    store: Store::Trash,
                    kind: "memory",
                    now: world.now,
                    repo_root: &repo.root,
                    repo_name: &repo.name(),
                    froms: std::slice::from_ref(&dir),
                    switch_before: args.also_off.then(|| previous.as_str().to_string()),
                },
            )?;
            undo = format!("toolportctl memory undo {}", put.id);
            entry = Some(put);
        }
        changed.push(fsx::display(&dir));
        if args.also_off {
            // The move comes first: a move that fails leaves the switch alone, and a switch
            // that cannot be written puts the memory back, so a clear is whole or not done.
            match leave_out::write_state(world.data_dir, &repo.cwd, &held.files, Some(false), false)
            {
                Ok(run) => changed.extend(run.changed),
                Err(e) => {
                    // The write may have got as far as the settings file before it failed.
                    let reverted = leave_out::write_state(
                        world.data_dir,
                        &repo.cwd,
                        &held.files,
                        previous.value(),
                        false,
                    );
                    return Err(switch_failed(
                        world,
                        entry.as_ref(),
                        &dir,
                        e,
                        reverted.err(),
                    ));
                }
            }
        }
        trash_id = entry.map(|e| e.id);
    }
    let plan = PlanV1 {
        summary: format!(
            "{} the auto memory of {}",
            if forever { "Delete" } else { "Clear" },
            repo.name()
        ),
        steps,
        effects: Effects { tokens },
        warnings,
        undo: undo.clone(),
    };
    let result = (!args.dry_run).then(|| {
        json!({
            "applied": true,
            "changed": changed,
            "undo": if forever { Value::Null } else { json!(undo) },
            "backups": [],
            "trashId": trash_id,
        })
    });
    Ok(json!({
        "cwd": cwd_text,
        "root": fsx::display(&repo.root),
        "name": repo.name(),
        "directory": fsx::display(&dir),
        "dryRun": args.dry_run,
        "forever": forever,
        "alsoOff": args.also_off,
        "confirm": forever.then(|| repo.name()),
        "files": files.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
        "totals": inventory::summary_json(&summary),
        "sharedBy": sharing,
        "plan": plan,
        "result": result,
    }))
}

fn switch_failed(
    world: &World,
    entry: Option<&store::Entry>,
    dir: &Path,
    cause: OpError,
    not_reverted: Option<OpError>,
) -> OpError {
    let mut message = match entry {
        Some(entry) => match store::restore(world.data_dir, Store::Trash, entry) {
            Ok(_) => format!(
                "auto memory could not be switched off ({}); it was put back, nothing changed",
                cause.message
            ),
            Err(e) => format!(
                "auto memory could not be switched off ({}) and could not be put back ({}); it is in the trash as {}, `toolportctl memory undo {}` restores it",
                cause.message, e.message, entry.id, entry.id
            ),
        },
        None => format!(
            "{} was deleted but auto memory could not be switched off: {}",
            dir.display(),
            cause.message
        ),
    };
    if let Some(e) = not_reverted {
        message.push_str(&format!(
            "; its previous switch could not be restored either: {}",
            e.message
        ));
    }
    OpError::failed("memory_failed", message)
}

/// `memory undo`: the newest trashed memory, or the one called `id`, goes back where it was.
pub fn undo(
    world: &World,
    id: Option<&str>,
    cwd: Option<&Path>,
    dry_run: bool,
) -> Result<Value, OpError> {
    let root: Option<PathBuf> = cwd.map(|c| Repo::of(c).root);
    let entry = store::find(
        world.data_dir,
        Store::Trash,
        "memory",
        id,
        if id.is_some() { None } else { root.as_deref() },
        world.now,
    )?;
    let steps: Vec<Step> = entry
        .items
        .iter()
        .map(|item| Step {
            op: "move",
            path: Some(item.from.clone()),
            detail: format!("move {} bytes back from Toolport's trash", item.bytes),
            keys: None,
            diff: None,
        })
        .collect();
    let mut warnings = Vec::new();
    if entry.switch_before.is_some() {
        warnings.push("The switch the clear turned off is put back too".into());
    }
    let plan = PlanV1 {
        summary: format!("Put back the auto memory of {}", entry.repo_name),
        steps,
        effects: Effects::default(),
        warnings,
        undo: format!("toolportctl memory clear --cwd {}", entry.repo_root),
    };
    let mut restored = Vec::new();
    if !dry_run {
        restored = store::restore(world.data_dir, Store::Trash, &entry)?;
        if let Some(before) = entry.switch_before.as_deref().and_then(Position::parse) {
            let held = leave_out::held_in(world.data_dir, Path::new(&entry.repo_root));
            let value = before.value();
            if Path::new(&entry.repo_root).is_dir() {
                leave_out::write_state(
                    world.data_dir,
                    Path::new(&entry.repo_root),
                    &held.files,
                    value,
                    false,
                )?;
            }
        }
    }
    Ok(json!({
        "id": entry.id,
        "name": entry.repo_name,
        "root": entry.repo_root,
        "dryRun": dry_run,
        "plan": plan,
        "result": (!dry_run).then(|| json!({
            "applied": true,
            "changed": restored,
            "undo": format!("toolportctl memory clear --cwd {}", entry.repo_root),
            "backups": [],
        })),
    }))
}
