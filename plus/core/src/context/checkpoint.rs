//! The checkpoint gauge of Rules and launch: how full the context of the newest Claude session is,
//! against the window it compacts at. The usage comes from the transcripts Spend already reads
//! (the last request of the newest session), so nothing new is collected. The checkpoint is a
//! number the person sets: the tokens before the compaction point at which the handoff is due.

use super::config;
use super::fsx;
use super::layer_deploy;
use super::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use crate::op::OpError;
use crate::spend::transcript;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toolport_plus_api::Host;

const SMALL_WINDOW: u64 = 200_000;
const LARGE_WINDOW: u64 = 1_000_000;
const MIN_SETTING: u64 = 100_000;
const ENV_WINDOW: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
const STALE_AFTER_SECS: i64 = 24 * 3600;

fn model_window(model: Option<&str>, used: u64) -> (u64, &'static str) {
    let named = model.is_some_and(|m| {
        let lower = m.to_ascii_lowercase();
        lower.contains("[1m]") || lower.ends_with("-1m")
    });
    if named {
        (LARGE_WINDOW, "model name")
    } else if used > SMALL_WINDOW {
        (LARGE_WINDOW, "usage above 200,000")
    } else {
        (SMALL_WINDOW, "model name")
    }
}

fn setting_window(settings: &Value, model: Option<&str>) -> Option<u64> {
    let number = |v: &Value| {
        v.as_u64()
            .filter(|n| (MIN_SETTING..=LARGE_WINDOW).contains(n))
    };
    model
        .and_then(|m| settings["modelSettings"][m]["autoCompactWindow"].as_u64())
        .and_then(|n| number(&json!(n)))
        .or_else(|| number(&settings["autoCompactWindow"]))
}

/// The window the session compacts at: the environment beats the settings, the settings beat the
/// model's own window, and none is larger than the model's.
fn effective_window(
    host: &dyn Host,
    claude_dir: &Path,
    model: Option<&str>,
    used: u64,
    env_value: Option<&str>,
) -> (u64, String) {
    let (model_tokens, how) = model_window(model, used);
    let from_env = env_value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| (MIN_SETTING..=LARGE_WINDOW).contains(n));
    if let Some(tokens) = from_env {
        return (tokens.min(model_tokens), "environment".into());
    }
    let settings = host
        .read_settings(&claude_dir.join("settings.json"))
        .map(|(value, _)| value)
        .unwrap_or_default();
    match setting_window(&settings, model) {
        Some(tokens) => (tokens.min(model_tokens), "setting".into()),
        None => (model_tokens, how.into()),
    }
}

pub fn status(host: &dyn Host, cwd: Option<&str>) -> Result<Value, OpError> {
    status_with(host, cwd, std::env::var(ENV_WINDOW).ok().as_deref())
}

pub(super) fn status_with(
    host: &dyn Host,
    cwd: Option<&str>,
    env_window: Option<&str>,
) -> Result<Value, OpError> {
    let env = layer_deploy::env(host)?;
    let claude_dir = crate::fsx::claude_dir_in(&env.home);
    let at = config::checkpoint_at(&env.roots.context_config_path());
    let roots = transcript::roots_under(&env.home, Some(&claude_dir));
    let folder = cwd.map(PathBuf::from);
    let Some(session) = transcript::latest(&roots, folder.as_deref()) else {
        return Ok(json!({
            "session": Value::Null,
            "checkpointAt": at,
            "note": "No Claude session with usage was found yet.",
        }));
    };
    let last = session.samples.iter().max_by_key(|s| s.ts);
    let used = last.map_or(0, |s| s.input + s.cache_read + s.cache_creation);
    let model = last.and_then(|s| s.model.clone());
    let (window, source) = effective_window(host, &claude_dir, model.as_deref(), used, env_window);
    let point = at.filter(|n| *n < window).map(|n| window - n);
    let last_at = session.last_ts / 1000;
    let now = host.now_unix();
    Ok(json!({
        "session": {
            "id": session.session,
            "cwd": session.cwd,
            "model": model,
            "lastActivityAt": fsx::zulu(last_at),
            "stale": now - last_at > STALE_AFTER_SECS,
        },
        "usedTokens": used,
        "usedBasis": "the input, cache read and cache creation tokens of the session's last request",
        "window": { "tokens": window, "source": source },
        "compactionAt": window,
        "checkpointAt": at,
        "checkpoint": point.map(|p| json!({ "point": p, "reached": used >= p, "remaining": p.saturating_sub(used) })),
        "remainingToCompaction": window.saturating_sub(used),
    }))
}

/// `context checkpoint-config`: the tokens before compaction at which the handoff is due, or off.
pub fn set(host: &dyn Host, at: Option<u64>, dry_run: bool) -> Result<Value, OpError> {
    let env = layer_deploy::env(host)?;
    let path = env.roots.context_config_path();
    if let Some(tokens) = at {
        if !(1..LARGE_WINDOW).contains(&tokens) {
            return Err(OpError::usage(format!(
                "--at must be between 1 and {} tokens before compaction",
                LARGE_WINDOW - 1
            )));
        }
    }
    let before = config::checkpoint_at(&path);
    let describe = |n: Option<u64>| {
        n.map_or("off".to_string(), |t| {
            format!("{t} tokens before compaction")
        })
    };
    let plan = PlanV1 {
        summary: format!("Checkpoint: {}", describe(at)),
        steps: vec![Step {
            op: "update",
            path: Some(fsx::display(&path)),
            detail: "Sets checkpointAt in context.json; every other key stays".into(),
            keys: Some(vec!["checkpointAt".into()]),
            diff: Some(Diff {
                before: describe(before),
                after: describe(at),
            }),
        }],
        effects: Effects::default(),
        warnings: Vec::new(),
        undo: match before {
            Some(n) => format!("toolportctl context checkpoint-config --at {n}"),
            None => "toolportctl context checkpoint-config --off".into(),
        },
    };
    if dry_run || before == at {
        return Ok(json!({
            "dryRun": dry_run,
            "plan": serde_json::to_value(plan).unwrap_or_default(),
            "result": Value::Null,
        }));
    }
    config::set_checkpoint_at(&path, at).map_err(|e| OpError::failed("write", e))?;
    let result = ResultV1 {
        applied: true,
        changed: vec![fsx::display(&path)],
        undo: plan.undo.clone(),
        backups: Vec::new(),
    };
    Ok(json!({
        "dryRun": false,
        "plan": serde_json::to_value(&plan).unwrap_or_default(),
        "result": { "applied": true, "changed": result.changed, "undo": result.undo, "backups": result.backups },
    }))
}
