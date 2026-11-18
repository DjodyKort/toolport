//! The operations behind `toolportctl memory` and the `memory_*` tools: one place resolves the
//! host world and the folder, so the command line and the tools cannot drift.

use super::claude_json::Known;
use super::resolve::{Repo, ENV_DISABLE, MANAGED_SETTINGS};
use super::store::Dest;
use super::switch::Position;
use super::{clear as clearing, instructions, inventory, overview, session_ops, sessions, switch};
use super::{store, World};
use crate::context::fsx;
use crate::context::leave_out;
use crate::context::loads::POLICY_CLAUDE_MD;
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

pub fn with_world<T>(
    host: &dyn Host,
    run: impl FnOnce(&World) -> Result<T, OpError>,
) -> Result<T, OpError> {
    let home = host
        .home_dir()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let count = |text: &str| host.count_tokens(text);
    run(&World {
        home: &home,
        data_dir: &data,
        now: host.now_unix(),
        count_tokens: &count,
        managed_settings: Some(Path::new(MANAGED_SETTINGS)),
        policy_claude_md: Some(Path::new(POLICY_CLAUDE_MD)),
        env_disable: std::env::var(ENV_DISABLE)
            .ok()
            .filter(|v| !v.trim().is_empty()),
    })
}

fn folder(cwd: Option<&str>) -> PathBuf {
    match cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_default(),
    }
}

fn existing(cwd: Option<&str>) -> Result<PathBuf, OpError> {
    let path = folder(cwd);
    match fsx::folder_problem(&path) {
        Some(problem) => Err(OpError::usage(format!("cwd {problem}"))),
        None => Ok(path),
    }
}

fn repository_json(repo: &Repo) -> Value {
    json!({
        "name": repo.name(),
        "root": fsx::display(&repo.root),
        "key": repo.slug(),
        "git": repo.git,
        "linked": repo.linked,
        "worktrees": repo.worktrees.iter().map(|w| json!({
            "path": fsx::display(w),
            "exists": fsx::is_dir(w),
            "current": *w == repo.cwd || repo.cwd.starts_with(w),
        })).collect::<Vec<_>>(),
    })
}

pub fn status(host: &dyn Host, cwd: Option<&str>) -> Result<Value, OpError> {
    let cwd = existing(cwd)?;
    with_world(host, |world| {
        let repo = Repo::of(&cwd);
        let layers = world.layers(&repo);
        let (enabled, by) = layers.effective();
        let (dir, custom) = layers.directory(world.home, &repo);
        let files = inventory::files(&dir);
        let summary = inventory::summary(&files);
        let index = inventory::index(&dir, world.count_tokens);
        let held = leave_out::held_in(world.data_dir, &repo.cwd);
        let position = Position::of(layers.switch());
        let known = Known::read(world.home);
        let report = instructions::report(world, &repo, &known)
            .map_err(|e| OpError::failed("context_invalid", e))?;
        let retention = sessions::retention(world.home);
        let folders = sessions::folders_of(world.home, &repo, &known);
        let totals = sessions::totals(
            folders.iter().flat_map(|f| f.sessions.iter()),
            retention.days,
            world.now,
        );
        let mut notes = vec![
            "Auto memory has a real switch; instruction files can only be excluded by path, and sessions have no switch".to_string(),
        ];
        if repo.cwd != repo.root {
            notes.push("The switch is written at the repository root; whether Claude Code reads it from a subfolder or a linked worktree is not verified on Claude Code 2.1.326".into());
        }
        if !repo.git {
            notes.push("Not a git repository: Claude Code keys memory by this folder, and the settings file is not git-ignored".into());
        }
        let unreadable: Vec<String> = [
            (&layers.local_root, repo.local_settings()),
            (&layers.user, layers.user_file.clone()),
        ]
        .into_iter()
        .filter(|(keys, _)| keys.unreadable)
        .map(|(_, path)| fsx::display(&path))
        .collect();
        Ok(json!({
            "cwd": fsx::display(&repo.cwd),
            "repository": repository_json(&repo),
            "state": {
                "enabled": enabled,
                "setBy": by.as_str(),
                "switch": position.as_str(),
                "locked": layers.managed.enabled.is_some(),
                "managed": layers.managed.enabled,
                "environment": layers.env,
                "global": layers.user.enabled,
                "project": layers.project_root.enabled.or(layers.project_cwd.enabled),
                "toolportOwns": held.memory.is_some(),
                "setByHand": layers.switch().is_some() && held.memory != layers.switch(),
                "file": fsx::display(&repo.local_settings()),
                "globalFile": fsx::display(&layers.user_file),
                "unreadable": unreadable,
            },
            "directory": {
                "path": fsx::display(&dir),
                "custom": custom,
                "exists": !files.is_empty(),
            },
            "memory": inventory::summary_json(&summary),
            "index": index.map(|i| json!({
                "lines": i.lines,
                "bytes": i.bytes,
                "loadedLines": i.loaded_lines,
                "truncated": i.truncated,
                "tokens": i.tokens,
                "basis": "estimate",
            })),
            "instructions": {
                "rows": report.rows,
                "totalTokens": report.total_tokens,
                "memoryIndexTokens": report.memory_index_tokens,
                "leftOutTokens": report.left_out_tokens,
                "excluded": report.excluded,
                "localFiles": report.local_files,
                "imports": report.imports,
                "basis": "estimate",
            },
            "sessions": {
                "totals": sessions::totals_json(&totals),
                "retention": sessions::retention_json(&retention),
                "folders": folders.len(),
            },
            "trash": store::list(world.data_dir, store::Store::Trash, world.now)
                .iter()
                .filter(|e| e.kind == "memory" && e.repo_root == fsx::display(&repo.root))
                .map(|e| json!({
                    "id": e.id,
                    "created": fsx::zulu(e.created),
                    "expiresAt": e.expires(store::Store::Trash).map(fsx::zulu),
                    "bytes": e.bytes(),
                }))
                .collect::<Vec<_>>(),
            "notes": notes,
        }))
    })
}

pub fn ls(host: &dyn Host, cwd: Option<&str>, all: bool) -> Result<Value, OpError> {
    with_world(host, |world| {
        if all {
            return Ok(overview::memory_all(world));
        }
        let cwd = existing(cwd)?;
        let repo = Repo::of(&cwd);
        let layers = world.layers(&repo);
        let (dir, custom) = layers.directory(world.home, &repo);
        let files = inventory::files(&dir);
        let summary = inventory::summary(&files);
        Ok(json!({
            "cwd": fsx::display(&repo.cwd),
            "repository": repository_json(&repo),
            "directory": { "path": fsx::display(&dir), "custom": custom, "exists": !files.is_empty() },
            "files": files.iter().map(|f| f.to_json()).collect::<Vec<_>>(),
            "totals": inventory::summary_json(&summary),
            "sharedBy": repo.worktrees.iter().filter(|w| **w != repo.root).map(|w| fsx::display(w)).collect::<Vec<_>>(),
        }))
    })
}

pub fn set(
    host: &dyn Host,
    cwd: Option<&str>,
    global: bool,
    position: Position,
    dry_run: bool,
) -> Result<Value, OpError> {
    with_world(host, |world| {
        if global {
            switch::set_global(world, host, position, dry_run)
        } else {
            switch::set_repository(world, &existing(cwd)?, position, dry_run)
        }
    })
}

pub struct ClearOpts<'a> {
    pub forever: bool,
    pub confirm: Option<&'a str>,
    pub also_off: bool,
    pub dry_run: bool,
}

pub fn clear(host: &dyn Host, cwd: Option<&str>, opts: &ClearOpts) -> Result<Value, OpError> {
    let cwd = existing(cwd)?;
    with_world(host, |world| {
        clearing::clear(
            world,
            &clearing::Args {
                cwd: &cwd,
                dest: if opts.forever {
                    Dest::Forever
                } else {
                    Dest::Trash
                },
                confirm: opts.confirm,
                also_off: opts.also_off,
                dry_run: opts.dry_run,
            },
        )
    })
}

pub fn undo(
    host: &dyn Host,
    id: Option<&str>,
    cwd: Option<&str>,
    dry_run: bool,
) -> Result<Value, OpError> {
    with_world(host, |world| {
        clearing::undo(world, id, cwd.map(Path::new), dry_run)
    })
}

pub fn overview(
    host: &dyn Host,
    cwd: Option<&str>,
    all: bool,
    sort: &str,
    filter: Option<&str>,
) -> Result<Value, OpError> {
    let cwd = if all { None } else { Some(existing(cwd)?) };
    with_world(host, |world| {
        overview::overview(
            world,
            &overview::Options {
                all,
                cwd: cwd.as_deref(),
                sort,
                filter,
            },
        )
    })
}

pub fn sessions_ls(host: &dyn Host, cwd: Option<&str>, all: bool) -> Result<Value, OpError> {
    let cwd = if all { None } else { Some(existing(cwd)?) };
    with_world(host, |world| session_ops::ls(world, cwd.as_deref(), all))
}

pub struct SessionsClear<'a> {
    pub ids: Vec<String>,
    pub older_than: Option<u64>,
    pub dest: Dest,
    pub confirm: Option<&'a str>,
    pub dry_run: bool,
}

pub fn sessions_clear(
    host: &dyn Host,
    cwd: Option<&str>,
    opts: &SessionsClear,
) -> Result<Value, OpError> {
    let cwd = existing(cwd)?;
    with_world(host, |world| {
        session_ops::clear(
            world,
            &session_ops::ClearArgs {
                cwd: &cwd,
                ids: opts.ids.clone(),
                older_than: opts.older_than,
                dest: opts.dest,
                confirm: opts.confirm,
                dry_run: opts.dry_run,
            },
        )
    })
}

pub fn sessions_undo(
    host: &dyn Host,
    id: Option<&str>,
    cwd: Option<&str>,
    dry_run: bool,
) -> Result<Value, OpError> {
    with_world(host, |world| {
        session_ops::undo(world, id, cwd.map(Path::new), dry_run)
    })
}

pub fn sessions_restore(host: &dyn Host, id: &str, dry_run: bool) -> Result<Value, OpError> {
    with_world(host, |world| session_ops::restore(world, id, dry_run))
}

pub fn sessions_retention(
    host: &dyn Host,
    set: Option<u64>,
    dry_run: bool,
) -> Result<Value, OpError> {
    with_world(host, |world| {
        session_ops::retention(world, host, set, dry_run)
    })
}
