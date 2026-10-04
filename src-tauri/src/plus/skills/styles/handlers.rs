//! `plus.styles.*`: the style commands over the shared cores in `styles::*` and `skills::ops`.
//! Arguments use snake_case keys, results camelCase; `global_mode` (default true) selects the
//! user-level scope like `plus.skills.sync`. Every command finds the repository from
//! `repo_path` (default: the working directory) as mcpm does, except a user-level `clean`,
//! which needs none. Style files that fail to parse are skipped and listed as
//! `discoveryWarnings`.

use super::lint::lint_styles;
use super::manage::{
    add_style, apply_scoped, check_style_name, is_native_client, remove_scoped, sync_scoped,
};
use super::{discover_styles_report, Style};
use crate::plus::args::{flag, flag_or, list, str_nonempty};
use crate::plus::skills::clock::SystemClock;
use crate::plus::skills::lock::{get_entry, load_lockfile, save_lockfile, LockFile};
use crate::plus::skills::ops::{self, clean_styles, read_lock, Scope};
use crate::plus::skills::repo::{find_repo, resolve_path};
use crate::plus::skills::state_handlers::{display, literal_repo, paths, scope_name};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn start_of(args: &Value) -> Option<PathBuf> {
    str_nonempty(args, "repo_path").map(|p| resolve_path(Path::new(p)))
}

fn found_repo(args: &Value) -> Result<PathBuf, String> {
    find_repo(start_of(args).as_deref()).ok_or_else(|| "no skills repository found".to_string())
}

fn discovered(args: &Value) -> Result<(PathBuf, Vec<Style>, Vec<String>), String> {
    let repo = found_repo(args)?;
    let (styles, warnings) = discover_styles_report(&repo);
    Ok((repo, styles, warnings))
}

fn pairs(list: &[(String, String)]) -> Vec<Value> {
    list.iter()
        .map(|(client, style)| json!({"client": client, "style": style}))
        .collect()
}

fn client_keys(args: &Value) -> Option<Vec<String>> {
    let keys: Vec<String> = list(args, "client_keys")?
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    (!keys.is_empty()).then_some(keys)
}

fn row(style: &Style, lock: Option<&LockFile>) -> Value {
    let fm = &style.frontmatter;
    let entry = lock.and_then(|l| get_entry(&l.styles, style.name()));
    json!({
        "name": style.name(),
        "description": fm.description,
        "keepCodingInstructions": fm.keep_coding_instructions,
        "path": display(&style.source_path),
        "synced": entry.is_some(),
        "clientsSynced": entry.map_or_else(Vec::new, |e| e.clients_synced.clone()),
    })
}

pub fn list_handler(args: Value) -> Result<Value, String> {
    let (repo, styles, warnings) = discovered(&args)?;
    let lock = read_lock(&repo).map(|(lock, _)| lock);
    Ok(json!({
        "repo": display(&repo),
        "lockfilePresent": lock.is_some(),
        "styles": styles.iter().map(|s| row(s, lock.as_ref())).collect::<Vec<_>>(),
        "active": pairs(lock.as_ref().map_or(&[][..], |l| &l.active_styles)),
        "discoveryWarnings": warnings,
    }))
}

pub fn lint_handler(args: Value) -> Result<Value, String> {
    let (repo, styles, warnings) = discovered(&args)?;
    let result = lint_styles(&styles);
    let (errors, warns) = (result.errors().count(), result.warnings().count());
    Ok(json!({
        "repo": display(&repo),
        "styleCount": styles.len(),
        "errors": errors,
        "warnings": warns,
        "infos": result.messages.len() - errors - warns,
        "messages": result
            .messages
            .iter()
            .map(|m| json!({"level": m.level, "name": m.name, "message": m.message}))
            .collect::<Vec<_>>(),
        "discoveryWarnings": warnings,
    }))
}

pub fn diff_handler(args: Value) -> Result<Value, String> {
    let (repo, styles, warnings) = discovered(&args)?;
    let lock = read_lock(&repo).map(|(lock, _)| lock);
    let report = ops::diff_styles(&styles, lock.as_ref())?;
    Ok(json!({
        "repo": display(&repo),
        "noLockfile": report.no_lockfile,
        "clean": report.is_clean(),
        "new": report.new,
        "modified": report.modified,
        "removed": report.removed,
        "unchanged": report.unchanged,
        "discoveryWarnings": warnings,
    }))
}

pub fn status_handler(args: Value) -> Result<Value, String> {
    let repo = found_repo(&args)?;
    let lock = read_lock(&repo).map(|(lock, _)| lock);
    let status = lock.as_ref().map(ops::styles_status);
    Ok(json!({
        "repo": display(&repo),
        "lockfilePresent": lock.is_some(),
        "native": status.iter().flat_map(|s| &s.native).map(|r| json!({
            "client": r.client,
            "name": r.display_name,
            "styles": r.styles,
        })).collect::<Vec<_>>(),
        "applyRemove": status.iter().flat_map(|s| &s.active).map(|r| json!({
            "client": r.client,
            "name": r.display_name,
            "active": r.active,
        })).collect::<Vec<_>>(),
    }))
}

pub fn add_handler(args: Value) -> Result<Value, String> {
    let dry_run = flag(&args, "dry_run");
    let name = str_nonempty(&args, "name").ok_or("name is required")?;
    check_style_name(name)?;
    let repo = found_repo(&args)?;
    let report = add_style(&repo, name, dry_run)?;
    Ok(json!({
        "repo": display(&report.repo),
        "name": name,
        "path": display(&report.style_file),
        "dryRun": dry_run,
    }))
}

pub fn sync_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let (repo, styles, warnings) = discovered(&args)?;
    let scope = Scope::new(global, &repo)?;
    let mut data = json!({
        "repo": display(&repo),
        "scope": scope_name(global),
        "outputRoot": display(&scope.output_root),
        "lockDir": display(&scope.lock_dir),
        "dryRun": dry_run,
        "foundCount": styles.len(),
        "styleCount": 0,
        "clientCount": 0,
        "styles": [],
        "outputs": [],
        "discoveryWarnings": warnings,
    });
    if styles.is_empty() {
        return Ok(data);
    }
    let synced = sync_scoped(&repo, &styles, global, dry_run, client_keys(&args), &SystemClock)?;
    let lock = &synced.lock;
    let clients: BTreeSet<&String> = lock
        .styles
        .iter()
        .flat_map(|(_, e)| e.clients_synced.iter())
        .collect();
    data["syncedAt"] = json!(lock.synced_at);
    data["styleCount"] = json!(styles.len());
    data["clientCount"] = json!(clients.len());
    data["styles"] = styles
        .iter()
        .map(|style| {
            let entry = get_entry(&lock.styles, style.name());
            json!({
                "name": style.name(),
                "description": style.frontmatter.description,
                "clientsSynced": entry.map_or_else(Vec::new, |e| e.clients_synced.clone()),
                "warnings": entry.map_or_else(Vec::new, |e| e.warnings.clone()),
            })
        })
        .collect();
    data["outputs"] = json!(paths(&synced.outputs));
    Ok(data)
}

pub fn apply_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let name = str_nonempty(&args, "name").ok_or("name is required")?;
    let (repo, styles, warnings) = discovered(&args)?;
    let style = styles.iter().find(|s| s.name() == name).ok_or_else(|| {
        let available: Vec<&str> = styles.iter().map(Style::name).collect();
        let available = if available.is_empty() {
            "none".to_string()
        } else {
            available.join(", ")
        };
        format!("Style '{name}' not found. Available: {available}")
    })?;
    let keys = client_keys(&args);
    let native: Vec<&String> = keys
        .iter()
        .flatten()
        .filter(|k| is_native_client(k))
        .collect();
    let native = json!(native);
    let applied = apply_scoped(&repo, style, global, dry_run, keys, &SystemClock)?;
    let lock = &applied.lock;
    Ok(json!({
        "repo": display(&repo),
        "name": name,
        "scope": scope_name(global),
        "outputRoot": display(&applied.scope.output_root),
        "lockDir": display(&applied.scope.lock_dir),
        "dryRun": dry_run,
        "nativeClients": native,
        "replaced": pairs(&applied.replaced),
        "appliedCount": lock.active_styles.iter().filter(|(_, s)| s == name).count(),
        "applied": applied
            .applied
            .iter()
            .map(|(client, path)| json!({"client": client, "path": display(path)}))
            .collect::<Vec<_>>(),
        "active": pairs(&lock.active_styles),
        "discoveryWarnings": warnings,
    }))
}

pub fn remove_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let repo = found_repo(&args)?;
    let keys = client_keys(&args);
    let removed = remove_scoped(&repo, global, dry_run, keys.clone(), &SystemClock)?;
    Ok(json!({
        "repo": display(&repo),
        "scope": scope_name(global),
        "outputRoot": display(&removed.scope.output_root),
        "lockDir": display(&removed.scope.lock_dir),
        "dryRun": dry_run,
        "clientKeys": keys.unwrap_or_default(),
        "hadActive": removed.had_active,
        "removed": removed
            .targets
            .iter()
            .map(|(client, style, path)| json!({
                "client": client,
                "style": style,
                "path": display(path),
            }))
            .collect::<Vec<_>>(),
        "active": pairs(removed.lock.as_ref().map_or(&[][..], |l| &l.active_styles)),
    }))
}

pub fn clean_handler(args: Value) -> Result<Value, String> {
    let global = flag_or(&args, "global_mode", true);
    let dry_run = flag(&args, "dry_run");
    let repo = if global {
        literal_repo(&args)
    } else {
        found_repo(&args)?
    };
    let scope = Scope::new(global, &repo)?;
    let mut lock = load_lockfile(&scope.lock_dir);
    let out = clean_styles(&scope.output_root, lock.as_mut(), dry_run);
    let lock_updated = match &lock {
        Some(lock) if !dry_run => {
            save_lockfile(&scope.lock_dir, lock)?;
            true
        }
        _ => false,
    };
    Ok(json!({
        "scope": scope_name(global),
        "lockDir": display(&scope.lock_dir),
        "cleanRoot": display(&scope.output_root),
        "dryRun": dry_run,
        "lockfilePresent": lock.is_some(),
        "lockUpdated": lock_updated,
        "managed": out.managed,
        "removed": paths(&out.removed),
        "skipped": out
            .skipped
            .iter()
            .map(|(client, error)| json!({"client": client, "error": error}))
            .collect::<Vec<_>>(),
        "ignored": out.ignored,
    }))
}
