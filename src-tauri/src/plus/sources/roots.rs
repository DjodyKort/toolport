//! `sources root ls|add|rm`: the repo roots the `repo`, `client` and `vendored` detectors walk.
//! The defaults come from the config (D-040: the repo holding the clients root, and the clients
//! root); `add` and `rm` edit `sourceRoots` in `context.json` and nothing else.

use super::fsx;
use super::scope::{self, has_dot_git};
use crate::plus::context::{
    backup, load_config, preserve_unreadable, save_config, ContextConfig, Roots,
};
use crate::plus::op::OpError;
use crate::plus::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Add,
    Remove,
}

pub fn list(roots: &Roots, config: &ContextConfig) -> Value {
    let mut rows: Vec<(PathBuf, &str)> = scope::default_roots(roots, config)
        .into_iter()
        .map(|p| (p, "default"))
        .collect();
    for path in scope::config_roots(roots, config) {
        if !rows
            .iter()
            .any(|(p, _)| fsx::canonical(p) == fsx::canonical(&path))
        {
            rows.push((path, "config"));
        }
    }
    json!({"roots": rows.iter().map(|(path, origin)| json!({
        "path": fsx::display(path),
        "origin": origin,
        "exists": fsx::is_dir(path),
        "repo": has_dot_git(path),
    })).collect::<Vec<_>>()})
}

fn shorthand(roots: &Roots, path: &Path) -> String {
    let real_home = fsx::canonical(&roots.home);
    match path
        .strip_prefix(&roots.home)
        .or_else(|_| path.strip_prefix(&real_home))
    {
        Ok(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.to_string_lossy()),
        _ => path.to_string_lossy().into_owned(),
    }
}

fn same(roots: &Roots, raw: &str, target: &Path) -> bool {
    fsx::canonical(&roots.expand_user(raw)) == fsx::canonical(target)
}

fn target(roots: &Roots, change: Change, raw: &str, cwd: &Path) -> Result<PathBuf, OpError> {
    let expanded = roots.expand_user(raw);
    let abs = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    if change == Change::Add && !fsx::is_dir(&abs) {
        return Err(OpError::not_found(format!(
            "not a directory: {}",
            abs.display()
        )));
    }
    Ok(fsx::canonical(&abs))
}

fn edited(
    roots: &Roots,
    config: &ContextConfig,
    change: Change,
    dir: &Path,
) -> Result<Vec<String>, OpError> {
    let mut list = config.source_roots.clone();
    let defaults = scope::default_roots(roots, config);
    match change {
        Change::Add => {
            if defaults.iter().any(|d| fsx::canonical(d) == dir) {
                return Err(OpError::conflict(format!(
                    "{} is already a default source root",
                    dir.display()
                )));
            }
            if list.iter().any(|r| same(roots, r, dir)) {
                return Err(OpError::conflict(format!(
                    "{} is already a source root",
                    dir.display()
                )));
            }
            list.push(shorthand(roots, dir));
        }
        Change::Remove => {
            let before = list.len();
            list.retain(|r| !same(roots, r, dir));
            if list.len() == before {
                let why = if defaults.iter().any(|d| fsx::canonical(d) == dir) {
                    "a default source root comes from clients_root in context.json and cannot be removed here"
                } else {
                    "not a configured source root"
                };
                return Err(OpError::not_found(format!("{}: {why}", dir.display())));
            }
        }
    }
    Ok(list)
}

fn text(list: &[String]) -> String {
    serde_json::to_string(list).unwrap_or_default()
}

fn undo(change: Change, dir: &Path) -> String {
    let verb = if change == Change::Add { "rm" } else { "add" };
    format!("toolportctl sources root {verb} {}", dir.display())
}

fn plan(
    roots: &Roots,
    config: &ContextConfig,
    change: Change,
    dir: &Path,
    after: &[String],
) -> PlanV1 {
    let path = roots.context_config_path();
    let shown = fsx::display(&path);
    let verb = if change == Change::Add {
        "add"
    } else {
        "remove"
    };
    let diff = Diff {
        before: text(&config.source_roots),
        after: text(after),
    };
    let detail = format!(
        "{verb} {} {} sourceRoots",
        dir.display(),
        if change == Change::Add { "to" } else { "from" }
    );
    let step = if path.is_file() {
        Step::merge(&shown, detail, &["sourceRoots"], diff)
    } else {
        Step::create(&shown, detail, &["sourceRoots"], diff)
    };
    PlanV1 {
        summary: format!("{verb} source root {}", dir.display()),
        steps: vec![step],
        effects: Effects::default(),
        warnings: if change == Change::Add && !has_dot_git(dir) {
            vec![format!(
                "{} is not a git checkout; its child checkouts are read, not the folder itself",
                dir.display()
            )]
        } else {
            Vec::new()
        },
        undo: undo(change, dir),
    }
}

/// `{dryRun, plan, result}`; `result` is null for a dry run.
pub fn change(
    roots: &Roots,
    change: Change,
    raw: &str,
    cwd: &Path,
    dry_run: bool,
) -> Result<Value, OpError> {
    let dir = target(roots, change, raw, cwd)?;
    let path = roots.context_config_path();
    if dry_run {
        let config = load_config(&path);
        let after = edited(roots, &config, change, &dir)?;
        let plan = plan(roots, &config, change, &dir, &after);
        return Ok(json!({"dryRun": true, "plan": plan, "result": null}));
    }
    let _lock = crate::registry::lock_at(&path).map_err(|e| OpError::failed("sources", e))?;
    let mut backups: Vec<String> = Vec::new();
    if let Some(saved) = preserve_unreadable(&path) {
        backups.push(fsx::display(&saved));
    }
    let mut config = load_config(&path);
    let after = edited(roots, &config, change, &dir)?;
    let plan = plan(roots, &config, change, &dir, &after);
    if let Some(saved) =
        backup::snapshot(roots, &path).map_err(|e| OpError::failed("sources", e))?
    {
        backups.push(fsx::display(&saved));
    }
    config.source_roots = after;
    save_config(&path, &config).map_err(|e| OpError::failed("sources", e))?;
    let result = ResultV1 {
        applied: true,
        changed: vec![fsx::display(&path)],
        undo: undo(change, &dir),
        backups,
    };
    Ok(json!({"dryRun": false, "plan": plan, "result": result}))
}
