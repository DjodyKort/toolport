//! The operations behind `toolportctl context bundle|use` and the `context_bundle_*` tools: one
//! place resolves the host world and maps errors, so the command line and the tools cannot drift.

use super::bundle::{self, Edit, Issue};
use super::bundle_apply::{self as apply, World};
use super::bundle_store::{self, BundleError};
use super::bundle_use;
use super::Roots;
use crate::plus::op::{ErrorKind, OpError};
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn op(e: BundleError) -> OpError {
    match e.code {
        "usage" => OpError::usage(e.message),
        "not_found" => OpError::not_found(e.message),
        "exists" | "applied" | "not_applied" => OpError::conflict(e.message),
        code => OpError {
            kind: ErrorKind::Failed(code),
            message: e.message,
        },
    }
}

fn host() -> Result<(Roots, PathBuf), OpError> {
    let (roots, _) = crate::plus::sources::host_world()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data = crate::registry::conduit_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))?;
    Ok((roots, data))
}

fn with_world<T>(run: impl FnOnce(&World) -> Result<T, BundleError>) -> Result<T, OpError> {
    let (roots, data) = host()?;
    run(&World { roots: &roots, data_dir: &data }).map_err(op)
}

fn folder(cwd: Option<&str>) -> PathBuf {
    match cwd {
        Some(c) => PathBuf::from(c),
        None => std::env::current_dir().unwrap_or_default(),
    }
}

fn issues_value(issues: &[Issue]) -> Value {
    serde_json::to_value(issues).unwrap_or_default()
}

fn counts(b: &bundle::Bundle) -> Value {
    json!({ "off": b.skills_off.len(), "nameOnly": b.skills_name_only.len(), "allow": b.skills_allow.len() })
}

pub fn ls() -> Result<Value, OpError> {
    with_world(|w| {
        let rows: Vec<Value> = bundle_store::list(w.roots)
            .into_iter()
            .map(|l| {
                let issues = bundle::lint(&l.name, &l.text);
                let applied = apply::applied_to(w.data_dir, &l.name);
                match &l.parsed {
                    Ok(b) => json!({
                        "name": l.name, "description": b.description, "servers": b.servers,
                        "skills": counts(b), "plugins": { "off": b.plugins_off },
                        "layers": { "add": b.layers_add, "exclude": b.layers_exclude },
                        "agents": { "off": b.agents_off }, "bind": b.bind,
                        "appliedTo": applied, "path": fsx::display(&l.path), "issues": issues.len(),
                    }),
                    Err(e) => json!({
                        "name": l.name, "description": "", "servers": null,
                        "skills": { "off": 0, "nameOnly": 0, "allow": 0 }, "plugins": { "off": [] },
                        "layers": { "add": [], "exclude": [] }, "agents": { "off": [] }, "bind": [],
                        "appliedTo": applied, "path": fsx::display(&l.path), "issues": issues.len(), "error": e,
                    }),
                }
            })
            .collect();
        Ok(json!({ "bundles": rows, "directory": fsx::display(&bundle_store::dir(w.roots)) }))
    })
}

fn plugins_config_value(config: &[(String, Vec<(String, String)>)]) -> Value {
    let plugins: serde_json::Map<String, Value> = config
        .iter()
        .map(|(plugin, knobs)| {
            let knobs: serde_json::Map<String, Value> = knobs.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
            (plugin.clone(), Value::Object(knobs))
        })
        .collect();
    Value::Object(plugins)
}

pub fn show(name: &str) -> Result<Value, OpError> {
    with_world(|w| {
        let l = bundle_store::load(w.roots, name)?;
        let b = &l.bundle;
        Ok(json!({
            "name": name, "path": fsx::display(&l.path), "description": b.description, "servers": b.servers,
            "skills": { "off": b.skills_off, "nameOnly": b.skills_name_only, "allow": b.skills_allow },
            "plugins": { "off": b.plugins_off, "config": plugins_config_value(&b.plugins_config) },
            "mcp": { "deny": b.mcp_deny },
            "layers": { "add": b.layers_add, "exclude": b.layers_exclude },
            "agents": { "off": b.agents_off }, "bind": b.bind, "legacy": b.legacy_list,
            "yaml": l.text, "issues": issues_value(&bundle::lint(name, &l.text)),
            "appliedTo": apply::applied_to(w.data_dir, name),
        }))
    })
}

fn plan_of(summary: String, step: Value, warnings: Vec<String>, undo: String) -> Value {
    json!({ "summary": summary, "steps": [step], "effects": {}, "warnings": warnings, "undo": undo })
}

/// `bundle add` (`create`) and `bundle edit`: writes the yaml in the skills repository and
/// nothing else; committing it is the user's.
pub fn save(name: &str, mut edit: Edit, from_folder: Option<&str>, create: bool, dry_run: bool) -> Result<Value, OpError> {
    with_world(|w| {
        if let Some(dir) = from_folder {
            let base = bundle_store::edit_from_folder(Path::new(dir))?;
            edit = Edit {
                description: edit.description.or(base.description),
                servers: edit.servers.or(base.servers),
                skills_off: edit.skills_off.or(base.skills_off),
                skills_name_only: edit.skills_name_only.or(base.skills_name_only),
                skills_allow: edit.skills_allow.or(base.skills_allow),
                plugins_off: edit.plugins_off.or(base.plugins_off),
                layers_add: edit.layers_add.or(base.layers_add),
                layers_exclude: edit.layers_exclude.or(base.layers_exclude),
                agents_off: edit.agents_off.or(base.agents_off),
                bind: edit.bind.or(base.bind),
            };
        }
        let done = bundle_store::save(w.roots, name, &edit, create, dry_run)?;
        let path = fsx::display(&done.path);
        let issues = bundle::lint(name, &done.after);
        let step = json!({
            "op": if create { "create" } else { "update" }, "path": path,
            "detail": if create { format!("write the new bundle {name}") } else { format!("change bundle {name}") },
            "diff": { "before": done.before.clone().unwrap_or_default(), "after": done.after },
        });
        let undo = if create {
            format!("toolportctl context bundle rm {name}")
        } else {
            format!("git checkout -- {path} (the file is yours to commit)")
        };
        let warnings = issues.iter().map(|i| format!("{}: {}", i.key, i.message)).collect();
        let verb = if create { "Add" } else { "Edit" };
        let result = (!dry_run).then(|| json!({ "applied": true, "changed": [path.clone()], "undo": undo.clone(), "backups": [] }));
        Ok(json!({
            "dryRun": dry_run, "name": name, "path": path, "created": create,
            "plan": plan_of(format!("{verb} bundle {name}"), step, warnings, undo),
            "result": result, "issues": issues_value(&issues),
        }))
    })
}

pub fn rm(name: &str, force: bool, dry_run: bool) -> Result<Value, OpError> {
    with_world(|w| {
        let applied = apply::applied_to(w.data_dir, name);
        if !applied.is_empty() && !force {
            let folders: Vec<&str> = applied.iter().filter_map(|a| a["folder"].as_str()).collect();
            return Err(BundleError::new(
                "applied",
                format!("bundle {name} is applied in {}; undo it there or pass --force", folders.join(", ")),
            ));
        }
        let path = bundle_store::remove(w.roots, name, dry_run)?;
        let path = fsx::display(&path);
        let undo = format!("git checkout -- {path} (the file is yours to commit)");
        let mut warnings = Vec::new();
        if !applied.is_empty() {
            warnings.push(format!("still applied in {} folder(s): their files stay until `context bundle undo`", applied.len()));
        }
        let step = json!({ "op": "delete", "path": path, "detail": format!("delete the definition of bundle {name}") });
        let result = (!dry_run).then(|| json!({ "applied": true, "changed": [path.clone()], "undo": undo.clone(), "backups": [] }));
        Ok(json!({
            "dryRun": dry_run, "name": name, "path": path,
            "plan": plan_of(format!("Remove bundle {name}"), step, warnings, undo), "result": result,
        }))
    })
}

pub fn apply(name: &str, cwd: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    with_world(|w| apply::apply(w, name, &folder(cwd), dry_run))
}

pub fn undo(cwd: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    with_world(|w| apply::undo(w, &folder(cwd), dry_run))
}

pub fn status(cwd: Option<&str>) -> Result<Value, OpError> {
    with_world(|w| apply::status(w, &folder(cwd)))
}

pub fn launch(name: &str, cwd: Option<&str>) -> Result<Value, OpError> {
    with_world(|w| bundle_use::launch(w, name, cwd.map(Path::new)))
}

pub fn config(auto_apply: Option<bool>) -> Result<Value, OpError> {
    with_world(|w| bundle_use::config(w, auto_apply))
}

pub fn use_bundle(name: Option<&str>, cwd: Option<&str>, none: bool, dry_run: bool) -> Result<Value, OpError> {
    with_world(|w| match (name, none) {
        (_, true) => bundle_use::use_none(w, &folder(cwd), dry_run),
        (Some(name), false) => bundle_use::use_bundle(w, name, &folder(cwd), dry_run),
        (None, false) => Err(BundleError::new("usage", "a name or --none is required")),
    })
}

/// What `context sync` adds when the auto-apply switch is on: the bound bundles it applied.
pub fn sync_bundles(dry_run: bool) -> Option<Value> {
    let (roots, data) = host().ok()?;
    let w = World { roots: &roots, data_dir: &data };
    super::load_config(&roots.context_config_path())
        .bundle_auto_apply
        .then(|| Value::Array(bundle_use::auto_apply(&w, dry_run)))
}
