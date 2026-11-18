//! `plus.skills.{tapAdd,tapList,tapRemove,tapUpdate,tapLink,tapUnlink,search,install}`: the tap
//! commands over `tap_ops` and `plugins::link`. Arguments use snake_case keys, results camelCase. The typed `*_value` functions are
//! what the selfmcp tools call; the `*_handler` ones adapt them to the `plus_invoke` table.
//! `install` takes `repo_path` literally (default: the working directory), as mcpm's `--path`.

use super::git::SystemGit;
use super::repo::resolve_path;
use super::tap_ops::{
    self, Env, InstallOptions, InstallReport, Kind, Planned, TapError, UpdateRow,
};
use super::taps::redact;
use crate::args::{flag, nonempty_strings, str_nonempty};
use crate::op::{ErrorKind, OpError};
use crate::plugins::{self, link, marketplace};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

type Outcome = Result<Value, TapError>;

fn config_dir() -> Result<PathBuf, TapError> {
    crate::fsx::data_dir()
        .map(|dir| resolve_path(&dir))
        .ok_or_else(|| TapError {
            kind: Kind::Backend,
            message: "data directory unknown".into(),
        })
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_GIT: std::cell::RefCell<Option<std::rc::Rc<dyn super::git::GitRunner>>> =
        const { std::cell::RefCell::new(None) };
}

fn with_env<T>(f: impl FnOnce(&Env) -> Result<T, TapError>) -> Result<T, TapError> {
    let dir = config_dir()?;
    #[cfg(test)]
    if let Some(git) = TEST_GIT.with(|g| g.borrow().clone()) {
        return f(&Env {
            config_dir: &dir,
            git: &*git,
        });
    }
    f(&Env {
        config_dir: &dir,
        git: &SystemGit,
    })
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, TapError> {
    str_nonempty(args, key).ok_or_else(|| TapError {
        kind: Kind::Invalid,
        message: format!("{key} is required"),
    })
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn tap_add_value(args: &Value) -> Outcome {
    let dry_run = flag(args, "dry_run");
    let report = with_env(|env| {
        tap_ops::add(
            env,
            required(args, "repo")?,
            str_nonempty(args, "name"),
            dry_run,
        )
    })?;
    Ok(json!({
        "name": report.tap.name,
        "repo": report.tap.repo,
        "url": redact(&report.tap.url),
        "path": display(&report.path),
        "cloned": report.cloned,
        "head": report.head,
        "dryRun": dry_run,
    }))
}

fn tap_failure(error: OpError) -> TapError {
    let kind = match error.kind {
        ErrorKind::Usage => Kind::Invalid,
        ErrorKind::NotFound => Kind::NotFound,
        ErrorKind::Conflict => Kind::Conflict,
        _ => Kind::Backend,
    };
    TapError {
        kind,
        message: error.message,
    }
}

fn plugin_env() -> Result<plugins::Env, TapError> {
    plugins::Env::host().ok_or_else(|| TapError {
        kind: Kind::Backend,
        message: "home directory could not be resolved".into(),
    })
}

fn refuse_while_linked(host: &dyn Host, name: &str) -> Result<(), TapError> {
    let Some(env) = plugins::Env::host() else {
        return Ok(());
    };
    if link::linked_taps(host, &env, &[name.to_string()]).is_empty() {
        return Ok(());
    }
    Err(TapError {
        kind: Kind::Conflict,
        message: format!(
            "Tap '{name}' is linked to Claude Code; run `toolportctl skills tap unlink {name}` first"
        ),
    })
}

pub fn tap_list_value(host: &dyn Host, _args: &Value) -> Outcome {
    let dir = config_dir()?;
    let rows = with_env(|env| Ok(tap_ops::list(env)))?;
    let plugin_env = rows
        .iter()
        .any(|r| r.cloned && marketplace::is_marketplace(&r.path))
        .then(plugins::Env::host)
        .flatten();
    Ok(json!({
        "tapsRoot": display(&super::taps::taps_root(&dir)),
        "taps": rows
            .iter()
            .map(|r| {
                let mut row = json!({
                    "name": r.tap.name,
                    "repo": r.tap.repo,
                    "url": redact(&r.tap.url),
                    "path": display(&r.path),
                    "cloned": r.cloned,
                });
                if let Some(shown) = plugin_env
                    .as_ref()
                    .and_then(|env| link::describe(host, env, &r.tap.name))
                {
                    row["marketplace"] = shown;
                }
                row
            })
            .collect::<Vec<_>>(),
    }))
}

pub fn tap_remove_value(host: &dyn Host, args: &Value) -> Outcome {
    let dry_run = flag(args, "dry_run");
    let name = required(args, "name")?;
    refuse_while_linked(host, name)?;
    let report = with_env(|env| tap_ops::remove(env, name, dry_run))?;
    Ok(json!({
        "name": report.name,
        "path": display(&report.path),
        "hadClone": report.had_clone,
        "removed": report.removed,
        "dryRun": dry_run,
    }))
}

fn update_row(row: &UpdateRow) -> Value {
    json!({"name": row.name, "ok": row.ok, "head": row.head, "error": row.error})
}

/// Pulls the taps; for the ones linked to Claude Code, also updates the linked plugins whose
/// clone moved on, since Claude Code runs a copy cached per version.
pub fn tap_update_value(host: &dyn Host, args: &Value) -> Outcome {
    let dry_run = flag(args, "dry_run");
    let rows = with_env(|env| tap_ops::update(env, str_nonempty(args, "name"), dry_run))?;
    let pulled: Vec<String> = rows
        .iter()
        .filter(|r| r.ok)
        .map(|r| r.name.clone())
        .collect();
    let refreshed = plugins::Env::host()
        .map(|env| {
            link::refresh(
                host,
                &env,
                &plugins::SystemClaude::from_env(),
                &pulled,
                dry_run,
            )
        })
        .unwrap_or_default();
    let mut out = json!({
        "results": rows.iter().map(update_row).collect::<Vec<_>>(),
        "failed": rows.iter().filter(|r| !r.ok).count(),
        "dryRun": dry_run,
    });
    if !refreshed.is_empty() {
        out["restartRequired"] = json!(refreshed.iter().any(|r| r["outcome"] == "updated"));
        out["plugins"] = json!(refreshed);
    }
    Ok(out)
}

pub fn tap_link_value(host: &dyn Host, args: &Value) -> Outcome {
    let only = nonempty_strings(args, "plugins").unwrap_or_default();
    link::link(
        host,
        &plugin_env()?,
        &plugins::SystemClaude::from_env(),
        required(args, "name")?,
        &only,
        flag(args, "dry_run"),
    )
    .map_err(tap_failure)
}

pub fn tap_unlink_value(host: &dyn Host, args: &Value) -> Outcome {
    link::unlink(
        host,
        &plugin_env()?,
        &plugins::SystemClaude::from_env(),
        required(args, "name")?,
        flag(args, "dry_run"),
    )
    .map_err(tap_failure)
}

pub fn search_value(args: &Value) -> Outcome {
    let query = required(args, "query")?;
    let report = with_env(|env| Ok(tap_ops::search(env, query)))?;
    Ok(json!({
        "query": report.query,
        "tapCount": report.tap_count,
        "results": report
            .hits
            .iter()
            .map(|h| json!({
                "tap": h.tap,
                "repo": h.repo,
                "name": h.name,
                "description": h.description,
                "type": h.kind,
            }))
            .collect::<Vec<_>>(),
        "discoveryWarnings": report.warnings,
    }))
}

fn planned(rows: &[Planned]) -> Vec<Value> {
    rows.iter()
        .map(|p| {
            json!({
                "name": p.name,
                "type": p.kind,
                "path": display(&p.path),
                "files": p.files,
                "status": if p.installed { "installed" } else { "skipped" },
            })
        })
        .collect()
}

fn install_json(report: &InstallReport, dry_run: bool) -> Value {
    let count = |severity: &str| {
        report
            .findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    };
    json!({
        "spec": report.spec,
        "tap": report.tap,
        "tapAdded": report.tap_added,
        "tapMissing": report.tap_missing,
        "cloneUrl": redact(&report.clone_url),
        "version": report.version,
        "versionIgnored": report.version.is_some(),
        "target": display(&report.target),
        "dryRun": dry_run,
        "foundCount": report.found,
        "blocked": report.blocked,
        "audit": {
            "ran": report.audited,
            "high": count("high"),
            "medium": count("medium"),
            "low": count("low"),
            "findings": report
                .findings
                .iter()
                .map(|f| json!({
                    "severity": f.severity,
                    "skill": f.skill_name,
                    "message": f.message,
                    "line": f.line,
                }))
                .collect::<Vec<_>>(),
        },
        "skills": planned(&report.skills),
        "installedCount": report.skills.iter().filter(|p| p.installed).count(),
        "skippedCount": report.skills.iter().filter(|p| !p.installed).count(),
        "symlinksSkipped": report.symlinks_skipped,
        "discoveryWarnings": report.warnings,
    })
}

pub fn install_value(args: &Value) -> Outcome {
    let dry_run = flag(args, "dry_run");
    let target = resolve_path(Path::new(str_nonempty(args, "repo_path").unwrap_or(".")));
    let opts = InstallOptions {
        target: &target,
        no_audit: flag(args, "no_audit"),
        dry_run,
    };
    let report = with_env(|env| tap_ops::install(env, required(args, "spec")?, &opts))?;
    Ok(install_json(&report, dry_run))
}

fn finish(outcome: Outcome) -> Result<Value, String> {
    outcome.map_err(String::from)
}

pub fn tap_add_handler(args: Value) -> Result<Value, String> {
    finish(tap_add_value(&args))
}

pub fn tap_list_handler(host: &dyn Host, args: Value) -> Result<Value, String> {
    finish(tap_list_value(host, &args))
}

pub fn tap_remove_handler(host: &dyn Host, args: Value) -> Result<Value, String> {
    finish(tap_remove_value(host, &args))
}

pub fn tap_update_handler(host: &dyn Host, args: Value) -> Result<Value, String> {
    finish(tap_update_value(host, &args))
}

pub fn tap_link_handler(host: &dyn Host, args: Value) -> Result<Value, String> {
    finish(tap_link_value(host, &args))
}

pub fn tap_unlink_handler(host: &dyn Host, args: Value) -> Result<Value, String> {
    finish(tap_unlink_value(host, &args))
}

pub fn search_handler(args: Value) -> Result<Value, String> {
    finish(search_value(&args))
}

pub fn install_handler(args: Value) -> Result<Value, String> {
    finish(install_value(&args))
}
