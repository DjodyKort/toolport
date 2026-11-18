//! The operations behind `toolportctl context bundle|use` and the `context_bundle_*` tools: one
//! place resolves the host world and maps errors, so the command line and the tools cannot drift.

use super::bundle::{self, Edit, Issue};
use super::bundle_apply::{self as apply, World};
use super::bundle_store::op;
use super::bundle_store::{self, BundleError};
use super::bundle_use;
use super::fsx;
use super::leave_out::{self, Op, Target};
use super::loads::{InstructionLoads, LoadSource, POLICY_CLAUDE_MD};
use super::plugin_adapters::Registry;
use super::plugin_switch::{self, Switch};
use super::Roots;
use crate::op::OpError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

fn with_world<T>(
    host: &dyn Host,
    run: impl FnOnce(&World) -> Result<T, BundleError>,
) -> Result<T, OpError> {
    let home = host
        .home_dir()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let roots = Roots::from_home(&home);
    let count = |text: &str| host.count_tokens(text);
    let loads = InstructionLoads {
        home: &home,
        policy_file: Some(Path::new(POLICY_CLAUDE_MD)),
        count_tokens: &count,
    };
    run(&World {
        roots: &roots,
        data_dir: &data,
        loads: &loads,
    })
    .map_err(op)
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

pub fn ls(host: &dyn Host) -> Result<Value, OpError> {
    with_world(host, |w| {
        let adapters = Registry::load(Some(w.data_dir));
        let rows: Vec<Value> = bundle_store::list(w.roots)
            .into_iter()
            .map(|l| {
                let issues = bundle::lint_with(&l.name, &l.text, &adapters);
                let applied = apply::applied_to(w.data_dir, &l.name);
                match &l.parsed {
                    Ok(b) => {
                        let mut row = json!({
                            "name": l.name, "description": b.description, "servers": b.servers,
                            "skills": counts(b), "plugins": { "off": b.plugins_off },
                            "layers": { "add": b.layers_add, "exclude": b.layers_exclude },
                            "agents": { "off": b.agents_off }, "bind": b.bind,
                            "appliedTo": applied, "path": fsx::display(&l.path), "issues": issues.len(),
                        });
                        if b.memory_off {
                            row["memory"] = json!({ "off": true });
                        }
                        row
                    }
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
            let knobs: serde_json::Map<String, Value> =
                knobs.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
            (plugin.clone(), Value::Object(knobs))
        })
        .collect();
    Value::Object(plugins)
}

pub fn show(host: &dyn Host, name: &str) -> Result<Value, OpError> {
    with_world(host, |w| {
        let adapters = Registry::load(Some(w.data_dir));
        let l = bundle_store::load(w.roots, name)?;
        let b = &l.bundle;
        let mut shown = json!({
            "name": name, "path": fsx::display(&l.path), "description": b.description, "servers": b.servers,
            "skills": { "off": b.skills_off, "nameOnly": b.skills_name_only, "allow": b.skills_allow },
            "plugins": { "off": b.plugins_off, "config": plugins_config_value(&b.plugins_config) },
            "mcp": { "deny": b.mcp_deny },
            "layers": { "add": b.layers_add, "exclude": b.layers_exclude },
            "dirs": { "add": b.dirs_add },
            "agents": { "off": b.agents_off }, "bind": b.bind, "legacy": b.legacy_list,
            "yaml": l.text, "issues": issues_value(&bundle::lint_with(name, &l.text, &adapters)),
            "appliedTo": apply::applied_to(w.data_dir, name),
        });
        if b.memory_off {
            shown["memory"] = json!({ "off": true });
        }
        Ok(shown)
    })
}

fn plan_of(summary: String, step: Value, warnings: Vec<String>, undo: String) -> Value {
    json!({ "summary": summary, "steps": [step], "effects": {}, "warnings": warnings, "undo": undo })
}

/// `bundle add` (`create`) and `bundle edit`: writes the yaml in the skills repository and
/// nothing else; committing it is the user's.
pub fn save(
    host: &dyn Host,
    name: &str,
    mut edit: Edit,
    from_folder: Option<&str>,
    create: bool,
    dry_run: bool,
) -> Result<Value, OpError> {
    with_world(host, |w| {
        let adapters = Registry::load(Some(w.data_dir));
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
        let issues = bundle::lint_with(name, &done.after, &adapters);
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
        let warnings = issues
            .iter()
            .map(|i| format!("{}: {}", i.key, i.message))
            .collect();
        let verb = if create { "Add" } else { "Edit" };
        let result = (!dry_run).then(|| json!({ "applied": true, "changed": [path.clone()], "undo": undo.clone(), "backups": [] }));
        Ok(json!({
            "dryRun": dry_run, "name": name, "path": path, "created": create,
            "plan": plan_of(format!("{verb} bundle {name}"), step, warnings, undo),
            "result": result, "issues": issues_value(&issues),
        }))
    })
}

pub fn rm(host: &dyn Host, name: &str, force: bool, dry_run: bool) -> Result<Value, OpError> {
    with_world(host, |w| {
        let applied = apply::applied_to(w.data_dir, name);
        if !applied.is_empty() && !force {
            let folders: Vec<&str> = applied
                .iter()
                .filter_map(|a| a["folder"].as_str())
                .collect();
            return Err(BundleError::new(
                "applied",
                format!(
                    "bundle {name} is applied in {}; undo it there or pass --force",
                    folders.join(", ")
                ),
            ));
        }
        let path = bundle_store::remove(w.roots, name, dry_run)?;
        let path = fsx::display(&path);
        let undo = format!("git checkout -- {path} (the file is yours to commit)");
        let mut warnings = Vec::new();
        if !applied.is_empty() {
            warnings.push(format!(
                "still applied in {} folder(s): their files stay until `context bundle undo`",
                applied.len()
            ));
        }
        let step = json!({ "op": "delete", "path": path, "detail": format!("delete the definition of bundle {name}") });
        let result = (!dry_run).then(|| json!({ "applied": true, "changed": [path.clone()], "undo": undo.clone(), "backups": [] }));
        Ok(json!({
            "dryRun": dry_run, "name": name, "path": path,
            "plan": plan_of(format!("Remove bundle {name}"), step, warnings, undo), "result": result,
        }))
    })
}

pub fn apply(
    host: &dyn Host,
    name: &str,
    cwd: Option<&str>,
    dry_run: bool,
) -> Result<Value, OpError> {
    with_world(host, |w| apply::apply(w, name, &folder(cwd), dry_run))
}

pub fn undo(host: &dyn Host, cwd: Option<&str>, dry_run: bool) -> Result<Value, OpError> {
    with_world(host, |w| apply::undo(w, &folder(cwd), dry_run))
}

pub fn status(host: &dyn Host, cwd: Option<&str>) -> Result<Value, OpError> {
    with_world(host, |w| apply::status(w, &folder(cwd)))
}

pub fn launch(host: &dyn Host, name: &str, cwd: Option<&str>) -> Result<Value, OpError> {
    with_world(host, |w| bundle_use::launch(w, name, cwd.map(Path::new)))
}

pub fn config(host: &dyn Host, auto_apply: Option<bool>) -> Result<Value, OpError> {
    with_world(host, |w| bundle_use::config(w, auto_apply))
}

pub fn use_bundle(
    host: &dyn Host,
    name: Option<&str>,
    cwd: Option<&str>,
    none: bool,
    dry_run: bool,
) -> Result<Value, OpError> {
    with_world(host, |w| match (name, none) {
        (_, true) => bundle_use::use_none(w, &folder(cwd), dry_run),
        (Some(name), false) => bundle_use::use_bundle(w, name, &folder(cwd), dry_run),
        (None, false) => Err(BundleError::new("usage", "a name or --none is required")),
    })
}

/// What `context sync` adds when the auto-apply switch is on: the bound bundles it applied.
pub fn sync_bundles(host: &dyn Host, dry_run: bool) -> Option<Value> {
    with_world(host, |w| {
        Ok(
            super::config::bundle_auto_apply(&w.roots.context_config_path())
                .then(|| Value::Array(bundle_use::auto_apply(w, dry_run))),
        )
    })
    .ok()
    .flatten()
}

pub fn leave_out(
    host: &dyn Host,
    op_kind: Op,
    target: Target,
    cwd: Option<&str>,
    dry_run: bool,
) -> Result<Value, OpError> {
    let cwd =
        cwd.ok_or_else(|| OpError::usage("cwd is required: a file is left out per folder"))?;
    let home = host
        .home_dir()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let roots = Roots::from_home(&home);
    let count = |text: &str| host.count_tokens(text);
    let loads = InstructionLoads {
        home: &home,
        policy_file: Some(Path::new(POLICY_CLAUDE_MD)),
        count_tokens: &count,
    };
    leave_out::run(
        &leave_out::World {
            roots: &roots,
            loads: &loads,
            data_dir: &data,
        },
        &leave_out::Args {
            op: op_kind,
            target,
            cwd: Path::new(cwd),
            dry_run,
        },
    )
}

pub fn plugin_switch(
    host: &dyn Host,
    op_kind: Switch,
    id: &str,
    cwd: Option<&str>,
    dry_run: bool,
) -> Result<Value, OpError> {
    let cwd = cwd.ok_or_else(|| {
        OpError::usage("cwd is required: a plugin is turned off or on per folder")
    })?;
    let home = host
        .home_dir()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    plugin_switch::run(
        &plugin_switch::World {
            claude_home: &home.join(".claude"),
            data_dir: &data,
        },
        &plugin_switch::Args {
            op: op_kind,
            id,
            cwd: Path::new(cwd),
            dry_run,
        },
    )
}

fn item_value(item: &super::loads::LoadItem) -> Value {
    json!({
        "kind": item.kind,
        "name": item.name,
        "path": item.path,
        "source": item.source,
        "loaded": item.loaded,
        "lazy": item.lazy,
        "reason": item.reason,
        "tokens": item.tokens,
        "leftOutBy": item.left_out_by,
    })
}

fn stack(
    host: &dyn Host,
    cwd: Option<&str>,
) -> Result<(PathBuf, PathBuf, super::loads::WhatLoads), OpError> {
    let home = host
        .home_dir()
        .ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let cwd = folder(cwd);
    if let Some(problem) = fsx::folder_problem(&cwd) {
        return Err(OpError::usage(format!("cwd {problem}")));
    }
    let count = |text: &str| host.count_tokens(text);
    let loads = InstructionLoads {
        home: &home,
        policy_file: Some(Path::new(POLICY_CLAUDE_MD)),
        count_tokens: &count,
    };
    let mut report = loads
        .what_loads(&cwd)
        .map_err(|e| OpError::failed("context_invalid", e))?;
    leave_out::mark(&mut report.items, &data, &cwd);
    Ok((home, cwd, report))
}

/// What a session in a folder loads, as rows with an estimated cost. Skills, plugins, agents,
/// tools and Claude Code's own prompt are not rows, so the total is never presented as a
/// measurement. The folder's calibration factor (from its last measurement) sits next to the
/// estimate; with `measured` the rows the last run's `InstructionsLoaded` log covers are marked.
pub fn loads(host: &dyn Host, cwd: Option<&str>, measured: bool) -> Result<Value, OpError> {
    let (_, cwd, mut report) = stack(host, cwd)?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let calibration = super::measure::calibration(&data, &cwd, None);
    let mut out = json!({
        "cwd": fsx::display(&fsx::canonical(&cwd)),
        "totalTokens": report.total_tokens,
        "basis": "estimate",
        "tokenizer": "cl100k_base",
        "calibration": super::measure::calibration_value(calibration.as_ref(), report.total_tokens),
        "notItemised": "skills, plugins, agents, tool definitions and Claude Code's own prompt are not rows; `context measure` gives the real total",
    });
    if measured {
        let stored = super::measure::stored(&data, &cwd, None);
        out["measured"] = stored.as_ref().map_or(Value::Null, |s| s.summary());
        out["verified"] = stored
            .as_ref()
            .and_then(|s| s.verify(&mut report.items))
            .map_or(Value::Null, |v| json!(v));
    }
    out["items"] = Value::Array(report.items.iter().map(item_value).collect());
    Ok(out)
}

/// Everything Toolport knows about a folder's context, read only: the applied bundle, what is
/// left out and switched off, and the last measurement with its calibration. Nothing is written
/// and no request is made.
pub fn folder_status(host: &dyn Host, cwd: Option<&str>) -> Result<Value, OpError> {
    let (_, cwd, report) = stack(host, cwd)?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let bundle = with_world(host, |w| apply::status(w, &cwd))?;
    let (files, memory_off) = leave_out::owned(&data, &cwd);
    let calibration = super::measure::calibration(&data, &cwd, None);
    Ok(json!({
        "cwd": fsx::display(&fsx::canonical(&cwd)),
        "bundle": bundle,
        "leftOut": { "files": files, "memoryOff": memory_off },
        "pluginsOff": plugin_switch::switched_off(&data, &cwd),
        "estimate": { "totalTokens": report.total_tokens, "basis": "estimate", "tokenizer": "cl100k_base" },
        "measured": super::measure::stored(&data, &cwd, None).map_or(Value::Null, |s| s.summary()),
        "calibration": super::measure::calibration_value(calibration.as_ref(), report.total_tokens),
    }))
}

const COMPOSE_TEXT_CAP: usize = 64 * 1024;

/// The instruction text Claude gets in a folder, part by part in load order.
pub fn compose(host: &dyn Host, cwd: Option<&str>) -> Result<Value, OpError> {
    let (_, cwd, report) = stack(host, cwd)?;
    let parts: Vec<Value> = report
        .items
        .iter()
        .filter(|item| item.loaded || item.lazy)
        .map(|item| {
            let mut part = item_value(item);
            let text = item
                .path
                .as_deref()
                .and_then(|path| fsx::read_text(Path::new(path), COMPOSE_TEXT_CAP))
                .unwrap_or_default();
            part["text"] = Value::String(text);
            part
        })
        .collect();
    Ok(json!({
        "cwd": fsx::display(&fsx::canonical(&cwd)),
        "parts": parts,
        "totalTokens": report.total_tokens,
        "basis": "estimate",
    }))
}

/// `context sync`: the bundle part (apply the bound bundles when auto-apply is on). The layers
/// deploy and the shell shims are not built yet and say so.
pub fn sync(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let bundles = sync_bundles(host, dry_run);
    Ok(json!({
        "dryRun": dry_run,
        "bundles": bundles,
        "autoApply": bundles.is_some(),
        "deploy": { "built": false, "note": "rule layers and shims are not part of this build yet" },
    }))
}

/// What `context measure` is asked for besides the folder.
#[derive(Clone, Debug, Default)]
pub struct MeasureOpts<'a> {
    pub model: Option<&'a str>,
    pub force: bool,
    pub confirm: bool,
    pub without: Vec<String>,
    pub bundle: Option<String>,
}

/// `context measure`: the real size of a folder's stack from Claude Code, as is and once per
/// variant. Without `confirm` and without a cached answer it returns the plan and starts nothing.
pub fn measure(host: &dyn Host, cwd: Option<&str>, opts: &MeasureOpts) -> Result<Value, OpError> {
    let (home, cwd, rows) = stack(host, cwd)?;
    let data = host.data_dir().ok_or_else(|| {
        OpError::failed("no_data_dir", "the data directory could not be resolved")
    })?;
    let launcher = super::measure::ProcessLauncher::from_env();
    let roots = Roots::from_home(&home);
    let env = super::measure::Env {
        roots: &roots,
        data_dir: &data,
        launcher: &launcher,
        rows: &rows,
    };
    let request = super::measure::Request {
        cwd,
        model: opts.model.map(String::from),
        force: opts.force,
        confirm: opts.confirm,
        without: opts.without.clone(),
        bundle: opts.bundle.clone(),
    };
    super::measure::measure(&env, &request).map_err(|e| match e.code {
        "usage" => OpError::usage(e.message),
        code => OpError::failed(code, e.message),
    })
}
