//! The scoped style operations behind `mcpm styles add|sync|apply|remove`, shared by
//! `toolportctl styles`, the `plus.styles.*` handlers and the self-MCP style tools. Scope is
//! user level (outputs under `~/`, the lock beside the registry) or the project one (both in
//! the repository), like `skills sync`.

use super::sync::{apply_style, pick, remove_style, sync_styles, StyleOptions};
use super::transpilers::{all_style_transpilers, STYLE_APPEND_MODE};
use super::{Style, Tier};
use crate::plus::skills::clock::Clock;
use crate::plus::skills::lock::{load_lockfile, save_lockfile, LockFile};
use crate::plus::skills::ops::Scope;
use crate::plus::skills::parser::valid_name;
use crate::plus::skills::pyfs::write_text;
use crate::plus::skills::repo::resolve_path;
use std::path::{Path, PathBuf};

pub fn style_template(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: \"TODO: Describe the tone, verbosity, and persona of this output style.\"\nkeep-coding-instructions: true\n---\n\nTODO: Add style instructions here.\n"
    )
}

pub fn check_style_name(name: &str) -> Result<(), String> {
    valid_name(name).map_err(|e| format!("Invalid style name '{name}': {e}"))
}

#[derive(Debug, PartialEq, Eq)]
pub struct AddReport {
    pub repo: PathBuf,
    pub style_dir: PathBuf,
    pub style_file: PathBuf,
}

/// Writes `styles/<name>/STYLE.md` from the template under `repo`, which must already be a
/// skills repository. Nothing is written on `dry_run`.
pub fn add_style(repo: &Path, name: &str, dry_run: bool) -> Result<AddReport, String> {
    check_style_name(name)?;
    let repo = resolve_path(repo);
    let style_dir = repo.join("styles").join(name);
    if style_dir.exists() {
        return Err(format!(
            "Style '{name}' already exists at {}",
            style_dir.display()
        ));
    }
    let style_file = style_dir.join("STYLE.md");
    if !dry_run {
        write_text(&style_file, &style_template(name))?;
    }
    Ok(AddReport {
        repo,
        style_dir,
        style_file,
    })
}

pub fn is_native_client(key: &str) -> bool {
    all_style_transpilers()
        .iter()
        .any(|t| t.client_key() == key && t.tier() == Tier::Native)
}

fn push_unique(list: &mut Vec<PathBuf>, path: PathBuf) {
    if !list.contains(&path) {
        list.push(path);
    }
}

/// The files `sync_styles` writes for `styles` under `root`, one per client and style (one
/// combined file for the append-mode clients).
pub fn sync_outputs(
    styles: &[Style],
    root: &Path,
    client_keys: &Option<Vec<String>>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for t in pick(Tier::Native, client_keys) {
        let sources: &[Style] = if STYLE_APPEND_MODE.contains(&t.client_key()) {
            styles.get(..1).unwrap_or_default()
        } else {
            styles
        };
        for style in sources {
            push_unique(&mut out, t.get_output_path(style, root));
        }
    }
    out
}

pub struct ScopedSync {
    pub lock: LockFile,
    pub scope: Scope,
    pub outputs: Vec<PathBuf>,
}

/// `mcpm styles sync`: every style to the native-toggle clients, extending the lock the scope
/// already has and saving it unless this is a dry run.
pub fn sync_scoped(
    repo: &Path,
    styles: &[Style],
    global: bool,
    dry_run: bool,
    client_keys: Option<Vec<String>>,
    clock: &dyn Clock,
) -> Result<ScopedSync, String> {
    let scope = Scope::new(global, repo)?;
    let opts = StyleOptions {
        client_keys: client_keys.clone(),
        dry_run,
        clock,
    };
    let lock = sync_styles(styles, &scope.output_root, load_lockfile(&scope.lock_dir), &opts)?;
    if !dry_run {
        save_lockfile(&scope.lock_dir, &lock)?;
    }
    let outputs = sync_outputs(styles, &scope.output_root, &client_keys);
    Ok(ScopedSync {
        lock,
        scope,
        outputs,
    })
}

pub struct ScopedApply {
    pub lock: LockFile,
    pub scope: Scope,
    /// The clients that had another active style, with that style.
    pub replaced: Vec<(String, String)>,
    /// The clients that now have the style active and the file written for each.
    pub applied: Vec<(String, PathBuf)>,
}

/// `mcpm styles apply`: the style as the always-on rule of the apply-remove clients.
pub fn apply_scoped(
    repo: &Path,
    style: &Style,
    global: bool,
    dry_run: bool,
    client_keys: Option<Vec<String>>,
    clock: &dyn Clock,
) -> Result<ScopedApply, String> {
    let scope = Scope::new(global, repo)?;
    let before = load_lockfile(&scope.lock_dir);
    let replaced: Vec<(String, String)> = before
        .iter()
        .flat_map(|lock| lock.active_styles.iter())
        .filter(|(client, active)| {
            active != style.name()
                && client_keys
                    .as_ref()
                    .is_none_or(|keys| keys.is_empty() || keys.contains(client))
        })
        .cloned()
        .collect();
    let opts = StyleOptions {
        client_keys: client_keys.clone(),
        dry_run,
        clock,
    };
    let lock = apply_style(style, &scope.output_root, before, &opts)?;
    if !dry_run {
        save_lockfile(&scope.lock_dir, &lock)?;
    }
    let applied = pick(Tier::ApplyRemove, &client_keys)
        .iter()
        .filter(|t| {
            lock.active_styles
                .iter()
                .any(|(client, active)| client == t.client_key() && active == style.name())
        })
        .map(|t| {
            (
                t.client_key().to_string(),
                t.get_output_path(style, &scope.output_root),
            )
        })
        .collect();
    Ok(ScopedApply {
        lock,
        scope,
        replaced,
        applied,
    })
}

pub struct ScopedRemove {
    pub lock: Option<LockFile>,
    pub scope: Scope,
    /// Whether the lock has any active style at all.
    pub had_active: bool,
    /// The active styles this call removes: client, style and the rule file of the client.
    pub targets: Vec<(String, String, PathBuf)>,
}

/// `mcpm styles remove`: the active style leaves the apply-remove clients (all, or the given
/// ones). Nothing is written when there is no active style or none on the selected clients.
pub fn remove_scoped(
    repo: &Path,
    global: bool,
    dry_run: bool,
    client_keys: Option<Vec<String>>,
    clock: &dyn Clock,
) -> Result<ScopedRemove, String> {
    let scope = Scope::new(global, repo)?;
    let loaded = load_lockfile(&scope.lock_dir);
    let active = loaded.as_ref().map(|l| l.active_styles.clone()).unwrap_or_default();
    let selected = |client: &str| {
        client_keys
            .as_ref()
            .is_none_or(|keys| keys.is_empty() || keys.iter().any(|k| k == client))
    };
    let rule_files = super::sync::tier2_output_paths(&scope.output_root);
    let targets: Vec<(String, String, PathBuf)> = active
        .iter()
        .filter(|(client, _)| selected(client))
        .map(|(client, style)| {
            let path = rule_files
                .iter()
                .find(|(key, _)| key == client)
                .map(|(_, path)| path.clone())
                .unwrap_or_default();
            (client.clone(), style.clone(), path)
        })
        .collect();
    let had_active = !active.is_empty();
    if targets.is_empty() {
        return Ok(ScopedRemove {
            lock: loaded,
            scope,
            had_active,
            targets,
        });
    }
    let opts = StyleOptions {
        client_keys,
        dry_run,
        clock,
    };
    let lock = remove_style(&scope.output_root, loaded, &opts)?;
    if !dry_run {
        save_lockfile(&scope.lock_dir, &lock)?;
    }
    Ok(ScopedRemove {
        lock: Some(lock),
        scope,
        had_active,
        targets,
    })
}
