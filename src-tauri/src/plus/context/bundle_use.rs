//! `context bundle launch`, `context use` and the auto-apply switch (D-064, D-066). Launch writes
//! the derived `--settings` file and starts nothing; `use` applies a bundle and, when the
//! folder-profile feature is on (D-031), routes the folder to the paired server profile.

use super::bundle::Bundle;
use super::bundle_apply::{self as apply, World};
use super::bundle_io;
use super::bundle_ledger as ledger;
use super::bundle_store::{self, BundleError};
use super::globs::glob_match;
use super::{load_config, save_config};
use crate::plus::plan::ResultV1;
use crate::plus::sources::fsx;
use crate::registry;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

fn failed(message: impl Into<String>) -> BundleError {
    BundleError::new("failed", message)
}

pub fn launch_dir(w: &World) -> PathBuf {
    w.roots.home.join(".config").join("toolport").join("profiles")
}

fn shell_word(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "_./-:@%+=,~".contains(c)) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The bundle as the keys of a `--settings` file; `layers.add` has no settings key.
pub fn settings_value(want: &apply::Desired) -> Value {
    let mut out = Map::new();
    if !want.overrides.is_empty() {
        out.insert(
            "skillOverrides".into(),
            Value::Object(want.overrides.iter().map(|(n, m)| (n.clone(), json!(m))).collect()),
        );
    }
    if !want.plugins.is_empty() {
        out.insert(
            "enabledPlugins".into(),
            Value::Object(want.plugins.iter().map(|id| (id.clone(), json!(false))).collect()),
        );
    }
    if !want.excludes.is_empty() {
        out.insert("claudeMdExcludes".into(), json!(want.excludes));
    }
    if !want.denies.is_empty() {
        out.insert("permissions".into(), json!({ "deny": want.denies }));
    }
    if !want.env.is_empty() {
        out.insert(
            "env".into(),
            Value::Object(want.env.iter().map(|(n, v)| (n.clone(), json!(v))).collect()),
        );
    }
    if !want.deny_servers.is_empty() {
        out.insert(
            "deniedMcpServers".into(),
            Value::Array(want.deny_servers.iter().map(|s| json!({ "serverName": s })).collect()),
        );
    }
    Value::Object(out)
}

pub fn launch(w: &World, name: &str, cwd: Option<&Path>) -> Result<Value, BundleError> {
    let loaded = bundle_store::load(w.roots, name)?;
    let folder = match cwd {
        Some(c) if !fsx::is_dir(c) => return Err(BundleError::new("usage", "cwd is not a folder")),
        Some(c) => Some(fsx::canonical(c)),
        None => None,
    };
    let want = apply::desired(w, &loaded.bundle, folder.as_deref().unwrap_or(&w.roots.home));
    let file = launch_dir(w).join(format!("{name}.settings.json"));
    let text = serde_json::to_string_pretty(&settings_value(&want)).map_err(|e| failed(e.to_string()))? + "\n";
    bundle_io::write_atomic(&file, text.as_bytes()).map_err(|e| failed(format!("{}: {e}", file.display())))?;
    let mut notes = want.warnings.clone();
    if !loaded.bundle.layers_add.is_empty() {
        notes.push(format!(
            "layers.add ({}) rides CLAUDE.local.md, which --settings cannot carry; `context bundle apply` delivers it",
            loaded.bundle.layers_add.join(", ")
        ));
    }
    let shown = fsx::display(&file);
    Ok(json!({
        "bundle": name,
        "settingsFile": shown,
        "command": format!("claude --settings {}", shell_word(&shown)),
        "cwd": folder.as_ref().map(|f| fsx::display(f)),
        "notes": notes,
    }))
}

struct Pair {
    id: String,
    name: String,
}

fn paired_profile(bundle: Option<&Bundle>, name: &str) -> Result<(Option<Pair>, bool), BundleError> {
    let reg = registry::load().map_err(failed)?;
    let wanted = bundle.and_then(|b| b.servers.as_deref()).unwrap_or(name);
    let pair = reg
        .profiles
        .iter()
        .find(|p| p.name == wanted || p.id == wanted)
        .map(|p| Pair { id: p.id.clone(), name: p.name.clone() });
    Ok((pair, reg.folder_profiles_enabled))
}

fn bound_here(folder: &str) -> Result<bool, BundleError> {
    let reg = registry::load().map_err(failed)?;
    Ok(reg.folder_profiles.iter().any(|m| m.path.trim() == folder))
}

pub fn use_bundle(w: &World, name: &str, cwd: &Path, dry_run: bool) -> Result<Value, BundleError> {
    let folder = apply::folder_key(cwd);
    let loaded = match bundle_store::load(w.roots, name) {
        Ok(l) => Some(l),
        Err(e) if e.code == "not_found" => None,
        Err(e) => return Err(e),
    };
    let (pair, enabled) = paired_profile(loaded.as_ref().map(|l| &l.bundle), name)?;
    if loaded.is_none() && pair.is_none() {
        return Err(BundleError::new(
            "not_found",
            format!("nothing is called {name}: no bundle in the skills repo's profiles/ and no server profile"),
        ));
    }
    let mut out = match &loaded {
        Some(_) => apply::apply(w, name, cwd, dry_run)?,
        None => {
            if !fsx::is_dir(cwd) {
                return Err(BundleError::new("usage", "cwd is not a folder"));
            }
            json!({
                "dryRun": dry_run, "bundle": name, "cwd": folder,
                "plan": { "summary": "", "steps": [], "effects": {}, "warnings": [], "undo": "" },
                "result": null, "conflicts": [],
            })
        }
    };
    let undo = format!("toolportctl context use --none --cwd {folder}");
    out["plan"]["summary"] = json!(format!("Use {name} in {folder}"));
    out["plan"]["undo"] = json!(undo);
    let mut bound = false;
    let mut changed = Vec::new();
    let steps = out["plan"]["steps"].as_array_mut().expect("steps");
    match (&pair, enabled) {
        (Some(p), true) => {
            steps.push(json!({
                "op": "update", "path": fsx::display(&registry_file()),
                "detail": format!("route {folder} to the server profile {}", p.name),
            }));
            if !dry_run {
                crate::registry_controller::upsert_folder_profile(&folder, &p.id).map_err(failed)?;
                changed.push(fsx::display(&registry_file()));
            }
            bound = true;
        }
        (Some(p), false) => {
            out["plan"]["warnings"].as_array_mut().expect("warnings").push(json!(format!(
                "folder profiles are off, so the server set {} is not routed to this folder; `toolportctl context folders --enable` turns the feature on",
                p.name
            )));
        }
        (None, _) => {}
    }
    if !dry_run {
        let mut result = match out["result"].take() {
            Value::Null => serde_json::to_value(ResultV1 { applied: true, changed: Vec::new(), undo: undo.clone(), backups: Vec::new() })
                .map_err(|e| failed(e.to_string()))?,
            v => v,
        };
        if let Some(list) = result["changed"].as_array_mut() {
            list.extend(changed.into_iter().map(Value::String));
        }
        result["undo"] = json!(undo);
        out["result"] = result;
    }
    out["name"] = json!(name);
    out["bundlePart"] = json!(loaded.is_some());
    out["server"] = json!({ "profile": pair.as_ref().map(|p| p.name.clone()), "foldersEnabled": enabled, "bound": bound });
    Ok(out)
}

pub fn use_none(w: &World, cwd: &Path, dry_run: bool) -> Result<Value, BundleError> {
    let folder = apply::folder_key(cwd);
    let applied = ledger::load(w.data_dir).folders.contains_key(&folder);
    let mapped = bound_here(&folder)?;
    if !applied && !mapped {
        return Err(BundleError::new("not_applied", format!("no bundle and no server routing is set for {folder}")));
    }
    let mut out = if applied {
        apply::undo(w, cwd, dry_run)?
    } else {
        json!({
            "dryRun": dry_run, "bundle": null, "cwd": folder,
            "plan": { "summary": "", "steps": [], "effects": {}, "warnings": [], "undo": "" },
            "result": null, "conflicts": [],
        })
    };
    out["plan"]["summary"] = json!(format!("Stop using a bundle and a server set in {folder}"));
    let mut changed = Vec::new();
    if mapped {
        out["plan"]["steps"].as_array_mut().expect("steps").push(json!({
            "op": "update", "path": fsx::display(&registry_file()),
            "detail": format!("remove the server routing of {folder}"),
        }));
        if !dry_run {
            crate::registry_controller::remove_folder_profile(&folder).map_err(failed)?;
            changed.push(fsx::display(&registry_file()));
        }
    }
    if !dry_run {
        let mut result = match out["result"].take() {
            Value::Null => serde_json::to_value(ResultV1 { applied: true, changed: Vec::new(), undo: String::new(), backups: Vec::new() })
                .map_err(|e| failed(e.to_string()))?,
            v => v,
        };
        if let Some(list) = result["changed"].as_array_mut() {
            list.extend(changed.into_iter().map(Value::String));
        }
        out["result"] = result;
    }
    out["server"] = json!({ "unrouted": mapped });
    Ok(out)
}

fn registry_file() -> PathBuf {
    registry::conduit_dir().unwrap_or_default().join("registry.json")
}

pub fn config(w: &World, auto_apply: Option<bool>) -> Result<Value, BundleError> {
    let path = w.roots.context_config_path();
    let mut cfg = load_config(&path);
    if let Some(on) = auto_apply {
        if cfg.bundle_auto_apply != on {
            super::preserve_unreadable(&path);
            cfg.bundle_auto_apply = on;
            save_config(&path, &cfg).map_err(failed)?;
        }
    }
    Ok(json!({ "autoApply": cfg.bundle_auto_apply, "file": fsx::display(&path) }))
}

fn expand_into(base: &Path, parts: &[&str], depth: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= 200 || depth > 12 {
        return;
    }
    match parts.split_first() {
        None => {
            if fsx::is_dir(base) {
                out.push(base.to_path_buf());
            }
        }
        Some((part, rest)) if part.contains(['*', '?', '[', '{']) => {
            for entry in fsx::list_dir(base) {
                if entry.kind == fsx::Kind::Dir && !entry.name.starts_with('.') && glob_match(part, &entry.name) {
                    expand_into(&entry.path, rest, depth + 1, out);
                }
            }
        }
        Some((part, rest)) => expand_into(&base.join(part), rest, depth + 1, out),
    }
}

/// The folders a `bind` pattern names that exist (`*` stands for one folder name).
pub fn bind_folders(pattern: &str, home: &Path) -> Vec<PathBuf> {
    let expanded = match pattern.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.display()),
        None => pattern.to_string(),
    };
    let parts: Vec<&str> = expanded.trim_end_matches('/').split('/').filter(|p| !p.is_empty()).collect();
    let mut out = Vec::new();
    expand_into(Path::new("/"), &parts, 0, &mut out);
    out
}

/// Folders a bound bundle matches that have no bundle applied yet: what an Attention row offers
/// and what `context sync` applies when `bundle config --auto-apply on` is set.
pub fn unapplied_matches(w: &World) -> Vec<(String, PathBuf)> {
    let applied = ledger::load(w.data_dir).folders;
    let mut out = Vec::new();
    for listed in bundle_store::list(w.roots) {
        let Ok(b) = &listed.parsed else { continue };
        for pattern in &b.bind {
            for folder in bind_folders(pattern, &w.roots.home) {
                let key = apply::folder_key(&folder);
                if !applied.contains_key(&key) && !out.iter().any(|(n, f): &(String, PathBuf)| *n == listed.name && *f == folder) {
                    out.push((listed.name.clone(), folder));
                }
            }
        }
    }
    out
}

pub fn auto_apply(w: &World, dry_run: bool) -> Vec<Value> {
    let mut done: Vec<String> = Vec::new();
    unapplied_matches(w)
        .into_iter()
        .map(|(name, folder)| {
            let key = apply::folder_key(&folder);
            if done.contains(&key) {
                return json!({ "bundle": name, "folder": key, "applied": false, "error": "another bundle matched this folder first" });
            }
            match apply::apply(w, &name, &folder, dry_run) {
                Ok(_) => {
                    done.push(key.clone());
                    json!({ "bundle": name, "folder": key, "applied": !dry_run })
                }
                Err(e) => json!({ "bundle": name, "folder": key, "applied": false, "error": e.message }),
            }
        })
        .collect()
}
