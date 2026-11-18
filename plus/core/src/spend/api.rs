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

pub fn status(host: &dyn Host) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    Ok(runtime::status(&ctx.dir, ctx.now))
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
    let inputs = Inputs {
        store: &store,
        transcripts: transcript::scan(&ctx.roots),
        runs: run_spans(&ctx.base),
        now: ctx.now,
    };
    let mut out = report::summary(&inputs, range, project);
    let input_side = ["input", "cacheRead", "cacheCreation"]
        .iter()
        .map(|k| out["totals"]["tokens"][k].as_u64().unwrap_or(0))
        .sum();
    out["mcpPart"] = mcp_part::part(host, report::range_start(ctx.now, range), input_side);
    out["status"] = runtime::status(&ctx.dir, ctx.now);
    Ok(out)
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

pub fn setup_plan(
    host: &dyn Host,
    port: Option<&str>,
    overwrite: &[String],
) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    setup::plan(host, &ctx.dir, port_of(host, port)?, overwrite)
        .map_err(|e| OpError::failed("setup_failed", e))
}

pub fn setup_apply(
    host: &dyn Host,
    port: Option<&str>,
    overwrite: &[String],
    dry_run: bool,
) -> Result<Value, OpError> {
    if dry_run {
        return setup_plan(host, port, overwrite).map(|plan| json!({"dryRun": true, "plan": plan}));
    }
    let ctx = ctx(host)?;
    setup::apply(
        host,
        &ctx.dir,
        port_of(host, port)?,
        overwrite,
        &super::format_ts(ctx.now),
    )
    .map_err(|e| OpError::failed("setup_failed", e))
}

pub fn setup_undo(host: &dyn Host, dry_run: bool) -> Result<Value, OpError> {
    let ctx = ctx(host)?;
    if dry_run {
        return Ok(json!({"dryRun": true, "profilesSetUp": setup::applied_profiles(&ctx.dir)}));
    }
    runtime::stop(&ctx.dir);
    setup::undo(host, &ctx.dir).map_err(|e| OpError::failed("setup_failed", e))
}
