//! The operations behind `toolportctl spend`, the self-MCP spend tools and the Spend page.

use std::path::PathBuf;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::config::{self, SpendConfig};
use super::report::{self, Inputs, Range, RunSpan};
use super::store::Store;
use super::{mcp_part, runtime, setup, spend_dir, transcript};
use crate::compression::measure_csv::cell;
use crate::op::OpError;
use crate::tasks::{cron, store as task_store};

pub const CSV_HEADER: [&str; 11] = [
    "day",
    "project",
    "model",
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_creation_tokens",
    "cost_usd",
    "cost_basis",
    "requests",
    "source",
];

struct Ctx {
    base: PathBuf,
    dir: PathBuf,
    roots: Vec<PathBuf>,
    now: i64,
}

fn ctx(host: &dyn Host) -> Result<Ctx, OpError> {
    let base = host
        .registry_path()
        .and_then(|p| p.parent().map(PathBuf::from))
        .ok_or_else(|| OpError::failed("no_data_dir", "data directory could not be resolved"))?;
    Ok(Ctx {
        dir: spend_dir(&base),
        roots: runtime::home_roots(host),
        now: host.now_unix() * 1000,
        base,
    })
}

fn run_spans(base: &std::path::Path) -> Vec<RunSpan> {
    task_store::list_runs(base, None)
        .into_iter()
        .filter_map(|run| {
            let start = cron::parse_rfc3339(&run.started_at)? * 1000;
            Some(RunSpan {
                task: run.task,
                start_ms: start,
                end_ms: run
                    .ended_at
                    .as_deref()
                    .and_then(cron::parse_rfc3339)
                    .map(|s| s * 1000),
                session: run
                    .action
                    .as_ref()
                    .and_then(|a| a.get("sessionId")?.as_str().map(str::to_string)),
            })
        })
        .collect()
}

fn range_of(text: Option<&str>) -> Result<Range, OpError> {
    match text {
        None => Ok(Range::Days7),
        Some(t) => Range::parse(t)
            .ok_or_else(|| OpError::usage(format!("unknown range '{t}' (have: today, 7d, 30d)"))),
    }
}

fn transcripts_json(roots: &[PathBuf]) -> Value {
    let (files, done) = transcript::progress(roots);
    json!({"files": files, "done": done})
}

pub fn status(host: &dyn Host) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    let mut out = runtime::status(&ctx.dir, ctx.now);
    out["transcripts"] = transcripts_json(&ctx.roots);
    Ok(out)
}

pub fn receiver_status(host: &dyn Host) -> Result<Value, OpError> {
    let status = status(host)?;
    Ok(json!({
        "state": status["state"],
        "enabled": status["enabled"],
        "receiver": status["receiver"],
        "lastEvent": status["lastEvent"],
    }))
}

pub fn summary(
    host: &dyn Host,
    range: Option<&str>,
    project: Option<&str>,
) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    let range = range_of(range)?;
    let store = Store::new(&ctx.dir);
    let mut status = runtime::status(&ctx.dir, ctx.now);
    let transcripts = transcript::scan(&ctx.roots);
    status["transcripts"] = transcripts_json(&ctx.roots);
    let stale = status["lastEvent"]
        .as_str()
        .and_then(super::parse_ts)
        .filter(|_| status["enabled"] == true)
        .map(|last| {
            let set_up = status["setup"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| super::parse_ts(p["appliedAt"].as_str()?))
                .min();
            let endpoint = format!("http://127.0.0.1:{}", config::load(&ctx.dir).port);
            super::stale::check(&transcripts, last, set_up, &endpoint)
        });
    let inputs = Inputs {
        store: &store,
        transcripts,
        runs: run_spans(&ctx.base),
        now: ctx.now,
    };
    let mut out = report::summary(&inputs, range, project);
    out["stale"] = stale.unwrap_or(Value::Null);
    let input_side = ["input", "cacheRead", "cacheCreation"]
        .iter()
        .map(|k| out["totals"]["tokens"][k].as_u64().unwrap_or(0))
        .sum();
    out["mcpPart"] = mcp_part::part(
        host,
        &ctx.base,
        report::range_start(ctx.now, range),
        input_side,
    );
    out["status"] = status;
    Ok(out)
}

fn flag_value<T: std::str::FromStr>(name: &str, text: Option<&str>) -> Result<Option<T>, OpError> {
    text.map(|t| {
        t.parse::<T>()
            .map_err(|_| OpError::usage(format!("{name}: '{t}' is not valid")))
    })
    .transpose()
}

pub struct ConfigEdit<'a> {
    pub port: Option<&'a str>,
    pub retention_days: Option<&'a str>,
    pub max_mb: Option<&'a str>,
    pub show_cost: Option<&'a str>,
}

pub fn config_set(host: &dyn Host, edit: &ConfigEdit) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    let mut cfg = config::load(&ctx.dir);
    let before = cfg.clone();
    if let Some(v) = flag_value("--port", edit.port)? {
        cfg.port = v;
    }
    if let Some(v) = flag_value("--retention-days", edit.retention_days)? {
        cfg.retention_days = v;
    }
    if let Some(v) = flag_value("--max-mb", edit.max_mb)? {
        cfg.max_mb = v;
    }
    if let Some(text) = edit.show_cost {
        cfg.show_cost = match text {
            "on" | "true" => true,
            "off" | "false" => false,
            _ => return Err(OpError::usage("--show-cost: use on or off")),
        };
    }
    cfg.validate().map_err(OpError::usage)?;
    if cfg != before {
        config::save(&ctx.dir, &cfg).map_err(|e| OpError::failed("config_failed", e))?;
    }
    Ok(json!({
        "port": cfg.port,
        "retentionDays": cfg.retention_days,
        "maxMb": cfg.max_mb,
        "showCost": cfg.show_cost,
        "enabled": cfg.enabled,
        "portChanged": cfg.port != before.port,
        "changed": cfg != before,
    }))
}

pub fn export_csv(host: &dyn Host, range: Option<&str>) -> Result<(String, usize), OpError> {
    let ctx = ctx(host)?;
    let range = range_of(range)?;
    let store = Store::new(&ctx.dir);
    let inputs = Inputs {
        store: &store,
        transcripts: transcript::scan(&ctx.roots),
        runs: Vec::new(),
        now: ctx.now,
    };
    let rows = report::rows_for_csv(&inputs, range);
    let mut text = CSV_HEADER.join(",") + "\n";
    for row in &rows {
        text.push_str(&row.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
        text.push('\n');
    }
    Ok((text, rows.len()))
}

pub fn prune(host: &dyn Host, dry_run: bool, totals: bool) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    let cfg = config::load(&ctx.dir);
    let report = runtime::prune_with(&ctx.dir, &ctx.roots, &cfg, ctx.now, dry_run)
        .map_err(|e| OpError::failed("prune_failed", e))?;
    let mut out = serde_json::to_value(&report).unwrap_or_default();
    out["retentionDays"] = json!(cfg.retention_days);
    out["maxBytes"] = json!(cfg.max_bytes());
    out["totalsDeleted"] = json!(false);
    if totals && !dry_run {
        out["totalsDeleted"] = json!(Store::new(&ctx.dir)
            .delete_daily()
            .map_err(|e| OpError::failed("prune_failed", e))?);
    }
    Ok(out)
}

fn port_of(host: &dyn Host, port: Option<&str>) -> Result<u16, OpError> {
    let ctx = ctx(host)?;
    match port {
        None => Ok(config::load(&ctx.dir).port),
        Some(text) => {
            let port: u16 = text
                .parse()
                .map_err(|_| OpError::usage(format!("--port: '{text}' is not a port")))?;
            SpendConfig {
                port,
                ..SpendConfig::default()
            }
            .validate()
            .map_err(OpError::usage)?;
            Ok(port)
        }
    }
}

fn setup_error(message: String) -> OpError {
    if message.starts_with(setup::UNKNOWN_PROFILE) {
        OpError::usage(message)
    } else {
        OpError::failed("setup_failed", message)
    }
}

#[derive(Default)]
pub struct SetupChoice {
    pub port: Option<String>,
    pub overwrite: Vec<String>,
    pub profiles: Vec<String>,
}

pub fn setup_plan(host: &dyn Host, choice: &SetupChoice) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    setup::plan(
        host,
        &ctx.dir,
        port_of(host, choice.port.as_deref())?,
        &choice.overwrite,
        &choice.profiles,
    )
    .map_err(setup_error)
}

pub fn setup_apply(host: &dyn Host, choice: &SetupChoice, dry_run: bool) -> Result<Value, OpError> {
    if dry_run {
        return setup_plan(host, choice).map(|plan| json!({"dryRun": true, "plan": plan}));
    }
    let ctx = ctx(host)?;
    setup::apply(
        host,
        &ctx.dir,
        port_of(host, choice.port.as_deref())?,
        &choice.overwrite,
        &choice.profiles,
        &super::format_ts(ctx.now),
    )
    .map_err(setup_error)
}

fn stats_json(stats: &super::store::Stats) -> Value {
    json!({"events": stats.events, "bytes": stats.bytes, "dailyLines": stats.daily_lines})
}

/// With `delete_data` the collected events and daily totals go too, which only makes sense once
/// no profile is left exporting.
pub fn setup_undo(
    host: &dyn Host,
    profiles: &[String],
    delete_data: bool,
    dry_run: bool,
) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    let remaining = setup::remaining_after_undo(&ctx.dir, profiles).map_err(setup_error)?;
    if delete_data && remaining > 0 {
        return Err(OpError::usage(
            "--delete-data needs every profile undone: drop --profile or undo the rest first",
        ));
    }
    let store = Store::new(&ctx.dir);
    if dry_run {
        let mut out = json!({
            "dryRun": true,
            "profilesSetUp": setup::applied_profiles(&ctx.dir),
            "remaining": remaining,
        });
        if delete_data {
            out["deleteData"] = stats_json(&store.stats());
        }
        return Ok(out);
    }
    let mut out = setup::undo(host, &ctx.dir, profiles).map_err(setup_error)?;
    if remaining == 0 {
        runtime::stop(&ctx.dir);
    }
    if delete_data {
        let stats = store
            .delete_all()
            .map_err(|e| OpError::failed("setup_failed", e))?;
        out["deleted"] = stats_json(&stats);
    }
    Ok(out)
}
