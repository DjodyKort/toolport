//! The launch wiring: the shell file in the data directory and the one marked line that makes the
//! shell read it. `init` and `disable` show their plan first, copy the shell startup file before
//! they touch it and change nothing in it but that line. `status` is the health card of the tab.

use super::bundle_io::{self, Action};
use super::fsx;
use super::hash::sha256_hex;
use super::launch_profile;
use super::layer_deploy::{self, Env};
use super::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use super::presync;
use super::shell_rc::{self, Edit};
use super::shims::{self, ProfileShim, SHIMS_FILE};
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

const RC_NAME: &str = ".zshrc";
const RC_CAP: usize = 8 * 1024 * 1024;
const KNOWN_ORG_ASSETS: [&str; 5] = [
    "CLAUDE.md",
    "CLAUDE.uncompressed.md",
    "settings.json",
    "shell-wrapper.sh",
    "commands",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RcRec {
    path: String,
    /// Adding the line also added the newline the file did not end with.
    added_newline: bool,
    created_file: bool,
    applied_at: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ledger {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rc: Option<RcRec>,
    /// Copies of the shell startup file, newest last; never removed by Toolport.
    #[serde(default)]
    backups: Vec<String>,
}

fn ledger_path(env: &Env) -> PathBuf {
    env.data_dir.join("plus").join("context-wiring.json")
}

fn load_ledger(env: &Env) -> Ledger {
    bundle_io::read_exact(&ledger_path(env))
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_ledger(env: &Env, ledger: &Ledger) -> Result<(), String> {
    let text = serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())? + "\n";
    crate::fsx::atomic_write(&ledger_path(env), &text, crate::fsx::Mode::KeepOrDefault)
        .map_err(|e| format!("{}: {e}", ledger_path(env).display()))
}

fn shims_path(env: &Env) -> PathBuf {
    env.data_dir.join(SHIMS_FILE)
}

fn rc_path(env: &Env) -> PathBuf {
    env.home.join(RC_NAME)
}

fn backup_dir(env: &Env) -> PathBuf {
    env.data_dir
        .join("plus")
        .join("context-backups")
        .join("shell")
}

fn profiles(env: &Env) -> Vec<ProfileShim> {
    launch_profile::profile_files(&env.home)
        .into_iter()
        .map(|(name, file)| ProfileShim {
            name,
            mcp_file: fsx::display(&file),
        })
        .collect()
}

fn generated(env: &Env) -> String {
    shims::snippet(&profiles(env))
}

struct Rc {
    path: PathBuf,
    text: Option<String>,
    /// Where a symlinked file really lives.
    target: Option<PathBuf>,
}

fn read_rc(env: &Env) -> Result<Rc, OpError> {
    let path = rc_path(env);
    let target = std::fs::symlink_metadata(&path)
        .ok()
        .filter(|meta| meta.file_type().is_symlink())
        .map(|_| fsx::canonical(&path));
    let text = match fsx::read_bytes(&path, RC_CAP) {
        None => None,
        Some(bytes) => Some(String::from_utf8(bytes).map_err(|_| {
            OpError::failed(
                "rc_unreadable",
                format!(
                    "{} is not valid UTF-8; Toolport leaves it alone",
                    path.display()
                ),
            )
        })?),
    };
    Ok(Rc { path, text, target })
}

fn step(op: &'static str, path: &Path, detail: impl Into<String>, diff: Option<Diff>) -> Step {
    Step {
        op,
        path: Some(fsx::display(path)),
        detail: detail.into(),
        keys: None,
        diff,
    }
}

fn plan_value(plan: PlanV1) -> Value {
    serde_json::to_value(plan).unwrap_or_default()
}

fn result_value(result: ResultV1) -> Value {
    json!({
        "applied": result.applied, "changed": result.changed,
        "undo": result.undo, "backups": result.backups,
    })
}

fn backup_rc(env: &Env, text: &str) -> Result<String, OpError> {
    let path = backup_dir(env).join(format!("zshrc-{}.bak", &sha256_hex(text)[..12]));
    crate::fsx::atomic_write(&path, text, crate::fsx::Mode::Private)
        .map_err(|e| OpError::failed("backup", format!("{}: {e}", path.display())))?;
    Ok(fsx::display(&path))
}

/// Changes the shell startup file through `edit`, after a copy of it, and only if it is still
/// what `edit` read. Returns what the edit reported and the copy's path.
fn change_rc<T>(
    env: &Env,
    mut edit: impl FnMut(&str) -> Result<(Option<String>, T), OpError>,
) -> Result<(T, Option<String>), OpError> {
    let path = rc_path(env);
    let mut backup = None;
    let mut failure: Option<OpError> = None;
    let out = bundle_io::update(&path, 5, |before| {
        let text = before.unwrap_or_default();
        match edit(text) {
            Ok((after, info)) => {
                if before.is_some() {
                    match backup_rc(env, text) {
                        Ok(made) => backup = Some(made),
                        Err(e) => {
                            let message = e.message.clone();
                            failure = Some(e);
                            return Err(message);
                        }
                    }
                }
                Ok((after.map_or(Action::Keep, Action::Write), info))
            }
            Err(e) => {
                let message = e.message.clone();
                failure = Some(e);
                Err(message)
            }
        }
    });
    match out {
        Ok(info) => Ok((info, backup)),
        Err(message) => Err(failure.unwrap_or_else(|| OpError::failed("rc_write", message))),
    }
}

pub fn init(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let env = layer_deploy::env(host)?;
    let rc = read_rc(&env)?;
    let line = shell_rc::source_line(&env.home, &shims_path(&env));
    let rc_text = rc.text.clone().unwrap_or_default();
    let (_, edit, _) = shell_rc::wire(&rc_text, &line);
    let shims_now = fsx::read_text(&shims_path(&env), RC_CAP);
    let shims_after = generated(&env);
    let found = shell_rc::scan(&rc_text);

    let mut steps = Vec::new();
    let mut warnings = Vec::new();
    if shims_now.as_deref() != Some(shims_after.as_str()) {
        steps.push(step(
            if shims_now.is_some() { "update" } else { "create" },
            &shims_path(&env),
            "The shell file Toolport owns: claude() runs the presync first, one function per launch profile",
            Some(Diff {
                before: shims_now.clone().unwrap_or_default(),
                after: shims_after.clone(),
            }),
        ));
    }
    if edit != Edit::Unchanged {
        let detail = match edit {
            Edit::Moved => format!(
                "Moves our line to the end of the file, below {}; no other line changes",
                found.sourced_after.join(" and ")
            ),
            _ => "Adds one line at the end of the file, marked toolport:context-shims; no other line changes".to_string(),
        };
        let before = rc_text
            .lines()
            .filter(|l| l.contains(shell_rc::MARKER) && !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        steps.push(step(
            if rc.text.is_some() {
                "update"
            } else {
                "create"
            },
            &rc.path,
            detail,
            Some(Diff {
                before,
                after: line.clone(),
            }),
        ));
        if rc.text.is_some() {
            steps.push(step(
                "note",
                &backup_dir(&env),
                "A copy of the file is saved here first",
                None,
            ));
        }
    }
    if let Some(target) = &rc.target {
        warnings.push(format!(
            "{} is a link to {}; the line goes into that file",
            rc.path.display(),
            target.display()
        ));
    }
    if !found.by_hand.is_empty() && found.marked.is_empty() {
        warnings.push(format!(
            "{} already sources context-shims.zsh on line {} (not added by Toolport); Toolport leaves that line as it is",
            rc.path.display(),
            found.by_hand[0]
        ));
    }
    let summary = match (steps.is_empty(), edit) {
        (true, _) => "The launch wiring is already set up".to_string(),
        (false, Edit::Moved) => "Fix the source order of the launch wiring".to_string(),
        (false, _) => "Set up the launch wiring".to_string(),
    };
    let plan = PlanV1 {
        summary,
        steps,
        effects: Effects::default(),
        warnings,
        undo: "toolportctl context wiring disable".into(),
    };
    if dry_run || plan.steps.is_empty() {
        return Ok(json!({ "dryRun": dry_run, "plan": plan_value(plan), "result": Value::Null }));
    }

    let mut changed = Vec::new();
    let mut backups = Vec::new();
    if shims_now.as_deref() != Some(shims_after.as_str()) {
        crate::fsx::atomic_write(
            &shims_path(&env),
            &shims_after,
            crate::fsx::Mode::KeepOrDefault,
        )
        .map_err(|e| OpError::failed("write", format!("{}: {e}", shims_path(&env).display())))?;
        changed.push(fsx::display(&shims_path(&env)));
    }
    let mut newline = false;
    if edit != Edit::Unchanged {
        let (added, backup) = change_rc(&env, |current| {
            let (after, _, added) = shell_rc::wire(current, &line);
            Ok((Some(after), added))
        })?;
        newline = added;
        backups.extend(backup);
        changed.push(fsx::display(&rc.path));
    }
    let mut ledger = load_ledger(&env);
    if edit != Edit::Unchanged {
        let keep_created = ledger.rc.as_ref().is_some_and(|r| r.created_file);
        ledger.rc = Some(RcRec {
            path: fsx::display(&rc.path),
            added_newline: newline,
            created_file: rc.text.is_none() || keep_created,
            applied_at: fsx::zulu(host.now_unix()),
        });
    }
    ledger.backups.extend(backups.clone());
    save_ledger(&env, &ledger).map_err(|e| OpError::failed("ledger", e))?;
    let result = ResultV1 {
        applied: true,
        changed,
        undo: plan.undo.clone(),
        backups,
    };
    Ok(json!({ "dryRun": false, "plan": plan_value(plan), "result": result_value(result) }))
}

pub fn disable(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let env = layer_deploy::env(host)?;
    let rc = read_rc(&env)?;
    let ledger = load_ledger(&env);
    let rc_text = rc.text.clone().unwrap_or_default();
    let found = shell_rc::scan(&rc_text);
    let added_newline = ledger.rc.as_ref().is_some_and(|r| r.added_newline);
    let (rc_after, removes) = shell_rc::unwire(&rc_text, added_newline);
    let delete_rc =
        removes && rc_after.is_empty() && ledger.rc.as_ref().is_some_and(|r| r.created_file);
    let shims_exists = fsx::is_file(&shims_path(&env));

    let mut steps = Vec::new();
    let mut warnings = Vec::new();
    if removes {
        let removed = rc_text
            .lines()
            .filter(|l| l.contains(shell_rc::MARKER) && !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        steps.push(step(
            if delete_rc { "delete" } else { "update" },
            &rc.path,
            "Removes the line marked toolport:context-shims; no other line changes",
            Some(Diff {
                before: removed,
                after: String::new(),
            }),
        ));
        steps.push(step(
            "note",
            &backup_dir(&env),
            "A copy of the file is saved here first",
            None,
        ));
    }
    if shims_exists {
        steps.push(step(
            "delete",
            &shims_path(&env),
            "The shell file Toolport owns; it is generated again by init",
            None,
        ));
    }
    if !found.by_hand.is_empty() {
        warnings.push(format!(
            "{} sources context-shims.zsh on line {} without Toolport's marker; remove that line yourself",
            rc.path.display(),
            found.by_hand[0]
        ));
    }
    let plan = PlanV1 {
        summary: if steps.is_empty() {
            "The launch wiring is not set up".to_string()
        } else {
            "Turn off the launch wiring".to_string()
        },
        steps,
        effects: Effects::default(),
        warnings,
        undo: "toolportctl context wiring init".into(),
    };
    if dry_run || plan.steps.is_empty() {
        return Ok(json!({ "dryRun": dry_run, "plan": plan_value(plan), "result": Value::Null }));
    }

    let mut changed = Vec::new();
    let mut backups = Vec::new();
    if removes {
        let ((), backup) = change_rc(&env, |current| {
            let (after, _) = shell_rc::unwire(current, added_newline);
            Ok((Some(after), ()))
        })?;
        backups.extend(backup);
        if delete_rc {
            let _ = std::fs::remove_file(&rc.path);
        }
        changed.push(fsx::display(&rc.path));
    }
    if shims_exists {
        std::fs::remove_file(shims_path(&env)).map_err(|e| {
            OpError::failed("write", format!("{}: {e}", shims_path(&env).display()))
        })?;
        changed.push(fsx::display(&shims_path(&env)));
    }
    let mut ledger = ledger;
    ledger.rc = None;
    ledger.backups.extend(backups.clone());
    save_ledger(&env, &ledger).map_err(|e| OpError::failed("ledger", e))?;
    let result = ResultV1 {
        applied: true,
        changed,
        undo: plan.undo.clone(),
        backups,
    };
    Ok(json!({ "dryRun": false, "plan": plan_value(plan), "result": result_value(result) }))
}

/// Part of `context sync`: the shell file is rewritten when it is out of date, and only once the
/// wiring has been set up; a sync never creates it and never touches the startup file.
pub fn refresh_shims(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let env = layer_deploy::env(host)?;
    let path = shims_path(&env);
    let Some(current) = fsx::read_text(&path, RC_CAP) else {
        return Ok(
            json!({ "action": "skipped", "reason": "the launch wiring is not set up", "file": fsx::display(&path) }),
        );
    };
    let wanted = generated(&env);
    if current == wanted {
        return Ok(json!({ "action": "unchanged", "file": fsx::display(&path) }));
    }
    if !dry_run {
        crate::fsx::atomic_write(&path, &wanted, crate::fsx::Mode::KeepOrDefault)
            .map_err(|e| OpError::failed("write", format!("{}: {e}", path.display())))?;
    }
    Ok(json!({
        "action": if dry_run { "would rewrite" } else { "rewritten" },
        "file": fsx::display(&path),
    }))
}

fn check(id: &str, level: &str, title: impl Into<String>, detail: impl Into<String>) -> Value {
    json!({ "id": id, "level": level, "title": title.into(), "detail": detail.into() })
}

fn pull_status(config: &crate::sources::config::Config, config_file: &Path) -> Value {
    let (source, command, display, error) = match presync::OrgCommand::from_config(config) {
        Ok(presync::OrgCommand::BuiltIn) => (
            "default",
            None,
            "git pull, then the org tool's own sync (built in)".to_string(),
            None,
        ),
        Ok(presync::OrgCommand::Argv(argv)) => {
            let text = presync::command_text(&argv);
            ("configured", Some(argv), text, None)
        }
        Err(why) => (
            "invalid",
            None,
            "not run until context.json is fixed".to_string(),
            Some(why),
        ),
    };
    json!({
        "source": source,
        "command": command,
        "display": display,
        "error": error,
        "configFile": fsx::display(config_file),
        "key": format!("{}.command", crate::sources::config::ORG_PULL_KEY),
    })
}

fn org_status(
    host: &dyn Host,
    places: &crate::sources::Places,
    config: &crate::sources::config::Config,
    config_file: &Path,
) -> Value {
    let dir = &places.corp_tools_dir;
    let claude_src = dir.join("claude");
    let installed = fsx::is_dir(&claude_src);
    let user_md = places.claude_home.join("CLAUDE.md");
    let user_text = fsx::read_text(&user_md, RC_CAP);
    let clone_hashes: Vec<String> = ["CLAUDE.md", "CLAUDE.uncompressed.md"]
        .iter()
        .filter_map(|f| fsx::read_bytes(&claude_src.join(f), RC_CAP))
        .map(sha256_hex)
        .collect();
    let matches = match (&user_text, clone_hashes.is_empty()) {
        (Some(_), false) => {
            let current = fsx::read_bytes(&user_md, RC_CAP).map(sha256_hex);
            Some(current.is_some_and(|h| clone_hashes.contains(&h)))
        }
        _ => None,
    };
    let last = fsx::read_text(&places.claude_home.join(".last_auto_sync"), 64)
        .and_then(|t| t.trim().parse::<i64>().ok());
    json!({
        "installed": installed,
        "name": dir.file_name().map(|n| n.to_string_lossy().into_owned()),
        "folder": fsx::display(dir),
        "file": fsx::display(&user_md),
        "fileTokens": user_text.as_deref().map(|t| host.count_tokens(t)),
        "tokenBasis": "estimate",
        "intervalSecs": presync::interval_secs(config),
        "pull": pull_status(config, config_file),
        "lastSyncedAt": last.map(fsx::zulu),
        "matchesClone": matches,
    })
}

fn org_checks(env: &Env, places: &crate::sources::Places, org: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(why) = org["pull"]["error"].as_str() {
        out.push(check(
            "org-pull",
            "look",
            "The org pull command cannot be used",
            format!("{why}. The org file is not pulled until it is fixed."),
        ));
    }
    if org["installed"] != json!(true) {
        out.push(check(
            "org-tool",
            "ok",
            "The org tool is not installed",
            "No org file loads and no coexistence rules apply. Layers still deploy.",
        ));
        return out;
    }
    let src = places.corp_tools_dir.join("claude");
    let config = crate::sources::config::load(&env.roots.context_config_path());
    let wrapper = fsx::read_bytes(&src.join("shell-wrapper.sh"), RC_CAP).map(sha256_hex);
    let unexpected: Vec<String> = fsx::list_dir(&src)
        .into_iter()
        .map(|e| e.name)
        .filter(|n| !KNOWN_ORG_ASSETS.contains(&n.as_str()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    match (config.cf_wrapper_hash.as_deref().filter(|h| !h.is_empty()), &wrapper) {
        (Some(base), Some(now)) if base != now => out.push(check(
            "org-tool",
            "look",
            "The org tool's shell wrapper changed",
            "Its sync may now touch more than before. Check that it still leaves your rules and servers alone. To accept it, delete cf_wrapper_hash from context.json; the next presync records the new wrapper.",
        )),
        _ if !unexpected.is_empty() => out.push(check(
            "org-tool",
            "look",
            "The org tool syncs files it did not sync before",
            format!("New in its claude folder: {}.", unexpected.join(", ")),
        )),
        _ => out.push(check(
            "org-tool",
            "ok",
            "The org tool is installed: no coexistence constraints are violated",
            "",
        )),
    }
    match org["matchesClone"].as_bool() {
        Some(false) => out.push(check(
            "org-file",
            "look",
            "The org file differs from the org tool's copy",
            "It was not synced yet, or it was edited by hand. Toolport never edits it.",
        )),
        Some(true) => out.push(check(
            "org-file",
            "ok",
            "The org file is read-only and unchanged by Toolport",
            "",
        )),
        None => {}
    }
    out
}

pub fn status(host: &dyn Host) -> Result<Value, OpError> {
    let env = layer_deploy::env(host)?;
    let now = host.now_unix();
    let rc = read_rc(&env)?;
    let rc_text = rc.text.clone().unwrap_or_default();
    let found = shell_rc::scan(&rc_text);
    let shims_file = shims_path(&env);
    let shims_text = fsx::read_text(&shims_file, RC_CAP);
    let shims_exists = shims_text.is_some();
    let shims_current = shims_text.as_deref() == Some(generated(&env).as_str());
    let set_up = shims_exists || found.sourced();
    let config_file = env.roots.context_config_path();
    let config = crate::sources::config::load(&config_file);
    let places = crate::sources::Places::from_env(&env.roots, &config);
    let org = org_status(host, &places, &config, &config_file);
    let record = presync::read(&env.data_dir);
    let interval = presync::interval_secs(&config);

    let mut checks = Vec::new();
    if set_up {
        if !found.sourced() {
            checks.push(check(
                "source-order",
                "look",
                "The shell does not read the shell file",
                format!(
                    "context-shims.zsh is not sourced from {}. Claude starts without the presync.",
                    rc.path.display()
                ),
            ));
        } else if !found.sourced_after.is_empty() {
            checks.push(check(
                "source-order",
                "look",
                format!(
                    "context-shims sourced BEFORE {} in ~/.zshrc",
                    found.sourced_after.join(" and ")
                ),
                "The other wrapper wins, so layers are not refreshed before a launch. Move our source line below it; Toolport copies the file first.",
            ));
        } else {
            checks.push(check(
                "source-order",
                "ok",
                "context-shims is sourced last",
                "",
            ));
        }
        if !shims_exists {
            checks.push(check(
                "shims-file",
                "look",
                "The shell file is missing",
                "The shell startup file reads a file that is not there. Set the wiring up again.",
            ));
        } else if !shims_current {
            checks.push(check(
                "shims-file",
                "look",
                "The shell file is out of date",
                "A sync rewrites it (for example after a launch profile was added).",
            ));
        } else {
            checks.push(check(
                "shims-file",
                "ok",
                "The shell file is written and current",
                "",
            ));
        }
    }
    checks.extend(org_checks(&env, &places, &org));
    if let Some(record) = &record {
        if !record.ok {
            checks.push(check(
                "presync",
                "look",
                "The last presync failed",
                record
                    .warnings
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "see the output".into()),
            ));
        }
    }
    let failed = record.as_ref().is_some_and(|r| !r.ok);
    let looks = checks
        .iter()
        .any(|c| c["level"] == "look" || c["level"] == "fail");
    let state = if !set_up {
        "not_set_up"
    } else if failed {
        "failed"
    } else if looks {
        "look"
    } else {
        "healthy"
    };
    let presync_value = record.as_ref().map(|r| {
        json!({
            "lastAttemptAt": fsx::zulu(r.last_attempt_at),
            "lastOkAt": r.last_ok_at.map(fsx::zulu),
            "durationMs": r.duration_ms,
            "ok": r.ok,
            "warnings": r.warnings,
            "output": r.output,
            "nextAfter": r.last_ok_at.map(|at| fsx::zulu(at + interval as i64)),
            "due": r.last_ok_at.is_none_or(|at| now - at >= interval as i64),
        })
    });
    let shim_functions: Vec<Value> = shims::functions(&profiles(&env))
        .into_iter()
        .map(|f| json!({ "name": f.name, "summary": f.summary }))
        .collect();
    Ok(json!({
        "state": state,
        "home": fsx::display(&env.home),
        "setUp": set_up,
        "shims": {
            "file": fsx::display(&shims_file),
            "exists": shims_exists,
            "current": shims_current,
            "functions": shim_functions,
        },
        "shell": {
            "file": fsx::display(&rc.path),
            "exists": rc.text.is_some(),
            "linkTarget": rc.target.map(|t| fsx::display(&t)),
            "sourced": found.sourced(),
            "addedByToolport": !found.marked.is_empty(),
            "line": found.line(),
            "sourcedBefore": found.sourced_after,
        },
        "presync": presync_value,
        "intervalSecs": interval,
        "org": org,
        "checks": checks,
        "shells": ["zsh"],
    }))
}
