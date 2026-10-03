//! `toolportctl compression status|presets|run|verify|ledger|proxy|update`. `run` launches
//! `claude` and records the launch; `proxy` and `update` act on the engine; the rest only read.

use super::flags::{switch, value, Dashes, Flag, Inline, Spec};
use super::output::{no_args, CtlError, Output};
use crate::plus::compression::engine::{self, EngineOps};
use crate::plus::compression::launch::{
    plan_launch, run_plan, LaunchOps, LaunchPlan, LedgerEntry, Probe, ProxySpec, SystemOps,
};
use crate::plus::compression::ledger::{self, SavingsEntry};
use crate::plus::compression::manage::{self, Ctx};
use crate::plus::compression::model::{env_for_preset, CompressionConfig, ProviderName};
use crate::plus::compression::store::{self, Loaded, Paths};
use crate::plus::compression::verify::{self, HealthProbe};
use serde_json::{json, Value};

fn load() -> Result<(Paths, Loaded), CtlError> {
    let paths = Paths::from_data_dir()
        .ok_or_else(|| CtlError::failed("no_data_dir", "data directory could not be resolved"))?;
    let loaded = store::read(&paths).map_err(|e| CtlError::failed("config_invalid", e))?;
    Ok((paths, loaded))
}

fn preset_json(name: &str, config: &CompressionConfig) -> Value {
    let p = config.preset_for(Some(name));
    json!({
        "name": name,
        "mode": p.mode.as_str(),
        "savingsProfile": p.savings_profile,
        "port": p.port,
        "knobCount": p.knobs.len(),
        "snapshotVersion": p.snapshot_version,
    })
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let (paths, loaded) = load()?;
    let config = &loaded.config;
    let installed = (config.provider == ProviderName::Headroom)
        .then(|| SystemOps::new().headroom_version())
        .flatten();
    let drift = (config.provider == ProviderName::Headroom)
        .then(|| installed.as_deref() != Some(config.provider_version.pin.as_str()));
    let pv = &config.provider_version;
    let data = json!({
        "configPath": paths.config().to_string_lossy(),
        "configExists": loaded.existed,
        "migrationNotes": loaded.notes,
        "provider": config.provider.as_str(),
        "runtime": config.runtime,
        "preset": preset_json(&config.active_preset, config),
        "scope": config.scope,
        "contexts": config.contexts.len(),
        "pin": {
            "package": pv.package,
            "pin": pv.pin,
            "requirement": pv.requirement(),
            "installed": installed,
            "drift": drift,
        },
        "shims": {"path": paths.shims().to_string_lossy(), "exists": paths.shims().exists()},
    });
    let preset = config.preset_for(None);
    let mut human = format!(
        "provider  {}\nruntime   {}\npreset    {} (mode={}, profile={}, port={})\npin       {} ({})",
        config.provider.as_str(),
        config.runtime.as_str(),
        config.active_preset,
        preset.mode.as_str(),
        preset.savings_profile.as_deref().unwrap_or("-"),
        preset.port,
        pv.pin,
        pv.requirement(),
    );
    if let Some(drift) = drift {
        human.push_str(&match (&installed, drift) {
            (Some(v), false) => format!("\nheadroom  {v} (pinned)"),
            (Some(v), true) => format!("\nheadroom  {v} != pin {} (drift)", pv.pin),
            (None, _) => format!("\nheadroom  not on PATH (pin {})", pv.pin),
        });
    }
    if !loaded.existed {
        human.push_str("\n(no compression.json yet: defaults)");
    }
    Ok(Output::new(data, human))
}

fn preset_rows(config: &CompressionConfig) -> Vec<Value> {
    config
        .presets
        .keys()
        .map(|name| {
            let mut row = preset_json(name, config);
            row["active"] = json!(*name == config.active_preset);
            row
        })
        .collect()
}

fn preset_table(rows: &[Value]) -> String {
    let mut human = String::from("  preset       mode    savings   port  knobs  snapshot");
    for row in rows {
        human.push_str(&format!(
            "\n{} {:<12} {:<7} {:<9} {:<5} {:<6} {}",
            if row["active"] == true { "*" } else { " " },
            row["name"].as_str().unwrap_or(""),
            row["mode"].as_str().unwrap_or(""),
            row["savingsProfile"].as_str().unwrap_or("-"),
            row["port"],
            row["knobCount"],
            row["snapshotVersion"].as_str().unwrap_or("unknown"),
        ));
    }
    human
}

pub fn presets(rest: &[String]) -> Result<Output, CtlError> {
    manage::with_system(|cx| presets_with(cx, rest))
}

pub(super) fn presets_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = super::compression_cfg::PRESETS.parse(rest)?;
    no_args(flags.operands())?;
    let dry_run = flags.on("--dry-run");
    let refreshed = if flags.on("--refresh") {
        Some(manage::refresh_presets(cx, dry_run)?)
    } else {
        None
    };
    let loaded = store::read(cx.paths).map_err(|e| CtlError::failed("config_invalid", e))?;
    let config = &loaded.config;
    let rows = preset_rows(config);
    let mut human = String::new();
    let mut data = json!({"presets": rows, "active": config.active_preset});
    if let Some(refreshed) = refreshed {
        for line in super::compression_cfg::refresh_lines(&refreshed, dry_run) {
            human.push_str(&line);
            human.push('\n');
        }
        data["refresh"] = refreshed;
    }
    human.push_str(&preset_table(&rows));
    Ok(Output::new(data, human))
}

struct RunArgs {
    plan_only: bool,
    force: bool,
    cwd: Option<String>,
    claude_args: Vec<String>,
}

/// Leading `--plan`, `--force`, `--cwd <dir>` belong to us; `--` or the first other token
/// starts the passthrough, so `-h` and friends reach claude untouched.
fn parse_run(rest: &[String]) -> Result<RunArgs, CtlError> {
    let mut out = RunArgs {
        plan_only: false,
        force: false,
        cwd: None,
        claude_args: Vec::new(),
    };
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--plan" => out.plan_only = true,
            "--force" => out.force = true,
            "--cwd" => {
                i += 1;
                out.cwd = Some(
                    rest.get(i)
                        .ok_or_else(|| CtlError::usage("--cwd requires a value"))?
                        .clone(),
                );
            }
            "--" => {
                out.claude_args = rest[i + 1..].to_vec();
                return Ok(out);
            }
            other if other.starts_with("--cwd=") => out.cwd = Some(other[6..].to_string()),
            _ => {
                out.claude_args = rest[i..].to_vec();
                return Ok(out);
            }
        }
        i += 1;
    }
    Ok(out)
}

/// The system launcher plus the append-only launch record `verify` attributes sessions with.
struct LoggedOps {
    inner: SystemOps,
    paths: Paths,
}

impl LaunchOps for LoggedOps {
    fn ensure_proxy(&mut self, spec: &ProxySpec) -> Result<String, String> {
        self.inner.ensure_proxy(spec)
    }

    fn record(&mut self, entry: &LedgerEntry) {
        if let Err(why) = ledger::append_launch(&self.paths, entry, ledger::now_ms()) {
            eprintln!("toolportctl: launch not recorded: {why}");
        }
    }

    fn launch(&mut self, plan: &LaunchPlan) -> Result<i32, String> {
        self.inner.launch(plan)
    }
}

pub fn run(rest: &[String]) -> Result<Output, CtlError> {
    let args = parse_run(rest)?;
    let (paths, loaded) = load()?;
    let cwd = match args.cwd {
        Some(c) => c,
        None => std::env::current_dir()
            .map_err(|e| CtlError::failed("cwd", e.to_string()))?
            .to_string_lossy()
            .into_owned(),
    };
    let mut ops = LoggedOps {
        inner: SystemOps::new(),
        paths: paths.clone(),
    };
    let plan = plan_launch(
        &loaded.config,
        &cwd,
        args.force,
        &args.claude_args,
        &ops.inner,
    );
    if args.plan_only {
        let human = format!(
            "{} {}\n{}",
            plan.program,
            plan.argv.join(" "),
            if plan.routed {
                format!(
                    "routed via {} (preset {}, port {})",
                    plan.provider.as_str(),
                    plan.preset,
                    plan.ledger.port.unwrap_or(0)
                )
            } else {
                format!("plain (provider {})", plan.provider.as_str())
            }
        );
        let data =
            serde_json::to_value(&plan).map_err(|e| CtlError::failed("internal", e.to_string()))?;
        return Ok(Output::new(data, human));
    }
    let outcome = run_plan(plan, &mut ops).map_err(|e| CtlError::failed("launch_failed", e))?;
    for warning in &outcome.plan.warnings {
        eprintln!("toolportctl: {warning}");
    }
    std::process::exit(outcome.exit_code);
}

pub(super) const fn spec(flags: &'static [Flag]) -> Spec {
    Spec {
        flags,
        inline: Inline::Value,
        dashes: Dashes::Long,
        ..Spec::PLAIN
    }
}

const VERIFY: Spec = spec(&[
    switch("--by-pin"),
    value("--limit"),
    value("--min-turns"),
    value("--transcripts"),
]);
const LEDGER_RECORD: Spec = spec(&[
    value("--provider"),
    value("--before"),
    value("--after"),
    value("--source"),
    value("--session"),
]);
const LEDGER_SUMMARY: Spec = spec(&[value("--provider"), value("--since")]);
const UPDATE: Spec = spec(&[switch("--latest"), switch("--accept"), value("--to")]);

fn pct(value: Option<f64>) -> String {
    value.map_or("-".into(), |v| format!("{:.1}%", v * 100.0))
}

fn ratio(value: Option<f64>) -> String {
    value.map_or("-".into(), |v| format!("{v:.2}x"))
}

fn metrics_json(m: &verify::Metrics) -> Value {
    json!({
        "sessions": m.sessions,
        "turns": m.turns,
        "cacheRead": m.cache_read,
        "cacheCreate": m.cache_create,
        "inputTokens": m.input_tokens,
        "outputTokens": m.output_tokens,
        "readRatio": m.read_ratio(),
        "readWrite": m.read_write(),
    })
}

pub fn verify(rest: &[String]) -> Result<Output, CtlError> {
    let flags = VERIFY.parse(rest)?;
    no_args(flags.operands())?;
    let (paths, loaded) = load()?;
    let root = flags
        .one("--transcripts")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(verify::transcript_root);
    verify_with(
        &paths,
        &loaded.config,
        &SystemOps::new(),
        &root,
        flags.number::<usize>("--limit")?,
        flags
            .number::<usize>("--min-turns")?
            .unwrap_or(verify::MIN_TURNS),
        flags.on("--by-pin"),
    )
}

pub(super) fn verify_with(
    paths: &Paths,
    config: &CompressionConfig,
    probe: &dyn HealthProbe,
    root: &std::path::Path,
    limit: Option<usize>,
    min_turns: usize,
    by_pin: bool,
) -> Result<Output, CtlError> {
    let checks = verify::health_checks(config, paths, probe);
    let mut failed = checks.iter().any(|c| !c.ok);
    let mut human = String::new();
    for c in &checks {
        human.push_str(&format!(
            "  {} {:<20} {}\n",
            if c.ok { "ok  " } else { "FAIL" },
            c.name,
            c.detail
        ));
    }

    let mut transcripts = verify::iter_transcripts(root);
    if let Some(limit) = limit.filter(|l| *l > 0) {
        transcripts.truncate(limit);
    }
    let mut data = json!({
        "provider": config.provider.as_str(),
        "checks": checks,
        "transcripts": {"root": root.to_string_lossy(), "count": transcripts.len()},
    });
    let unusable = if transcripts.is_empty() {
        Some("  FAIL transcripts           no Claude Code transcripts found - nothing to measure\n")
    } else if !verify::schema_ok(&transcripts) {
        Some(
            "  FAIL transcripts           no cache_* usage fields - the transcript format may \
             have changed; refusing to report a number\n",
        )
    } else {
        None
    };
    if let Some(line) = unusable {
        human.push_str(line);
        data["buckets"] = Value::Null;
        return Ok(Output {
            data,
            human: human.trim_end().into(),
            failed: true,
        });
    }

    let launches = ledger::read_launches(paths);
    let buckets = verify::partition(&transcripts, &launches);
    let proxied = verify::measure(&buckets.proxied, min_turns);
    let plain = verify::measure(&buckets.plain, min_turns);
    let unattributed = verify::measure(&buckets.unattributed, min_turns);
    human.push_str("\n  bucket        sessions  turns   read ratio  read:write\n");
    for (label, m) in [
        ("proxied", &proxied),
        ("plain", &plain),
        ("unattributed", &unattributed),
    ] {
        human.push_str(&format!(
            "  {label:<13} {:<9} {:<7} {:<11} {}\n",
            m.sessions,
            m.turns,
            pct(m.read_ratio()),
            ratio(m.read_write())
        ));
    }
    data["buckets"] = json!({
        "proxied": metrics_json(&proxied),
        "plain": metrics_json(&plain),
        "unattributed": metrics_json(&unattributed),
    });

    let mut verdict = Value::Null;
    if proxied.sessions == 0 {
        human.push_str("\n  no proxied sessions yet - nothing to judge\n");
    } else {
        let (ok, detail) = proxied.verdict();
        failed |= !ok;
        verdict = json!({"pass": ok, "detail": detail});
        human.push_str(&format!(
            "\n  {} proxied - {detail}\n",
            if ok { "pass" } else { "fail" }
        ));
        let mut vs = serde_json::Map::new();
        for (label, base) in [("plain", &plain), ("baseline", &unattributed)] {
            if let Some(delta) = verify::compare(base, &proxied) {
                human.push_str(&format!(
                    "  vs {label}: {delta:+.1} pp read ratio ({} -> {})\n",
                    pct(base.read_ratio()),
                    pct(proxied.read_ratio())
                ));
                vs.insert(label.into(), json!(delta));
            }
        }
        data["versus"] = Value::Object(vs);
    }
    data["verdict"] = verdict;

    if by_pin {
        let groups = verify::partition_by_pin(&buckets.proxied, &launches);
        let mut rows = Vec::new();
        for (pin, files) in &groups {
            let m = verify::measure(files, min_turns);
            if m.sessions == 0 {
                continue;
            }
            let vs_baseline = m
                .read_write()
                .zip(unattributed.read_write())
                .map(|(a, b)| a - b);
            human.push_str(&format!(
                "  pin {pin:<10} {} sessions, {} read ratio, {} read:write\n",
                m.sessions,
                pct(m.read_ratio()),
                ratio(m.read_write())
            ));
            let mut row = metrics_json(&m);
            row["pin"] = json!(pin);
            row["vsBaselineReadWrite"] = json!(vs_baseline);
            rows.push(row);
        }
        if rows.is_empty() {
            human.push_str("  --by-pin: no pin-tagged proxied sessions yet\n");
        }
        data["byPin"] = Value::Array(rows);
    }
    human.push_str(
        "  source: provider-billed usage in Claude Code transcripts, never the provider's own stats",
    );
    Ok(Output {
        data,
        human,
        failed,
    })
}

fn summary_json(s: &ledger::ProviderSummary) -> Value {
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    v["tokensSaved"] = json!(s.saved());
    v["savedPercent"] = json!(s.saved_percent());
    v
}

pub fn ledger_cmd(rest: &[String]) -> Result<Output, CtlError> {
    let (sub, tail) = match rest.first().map(String::as_str) {
        Some("record") => ("record", &rest[1..]),
        Some("summary") => ("summary", &rest[1..]),
        _ => ("summary", rest),
    };
    let (paths, _) = load()?;
    if sub == "record" {
        ledger_record(&paths, tail)
    } else {
        ledger_summary(&paths, tail)
    }
}

pub(super) fn ledger_record(paths: &Paths, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LEDGER_RECORD.parse(rest)?;
    no_args(flags.operands())?;
    let provider = flags
        .one("--provider")
        .map(String::from)
        .ok_or_else(|| CtlError::usage("--provider is required"))?;
    if ProviderName::parse(&provider).is_none() {
        return Err(CtlError::usage(format!(
            "unknown provider {provider:?} (have: {})",
            ProviderName::ALL.map(ProviderName::as_str).join(", ")
        )));
    }
    let entry = SavingsEntry {
        ts: ledger::format_ts(ledger::now_ms()),
        provider,
        source: flags.one("--source").unwrap_or_default().to_string(),
        session: flags.one("--session").map(String::from),
        tokens_before: flags
            .number::<u64>("--before")?
            .ok_or_else(|| CtlError::usage("--before is required"))?,
        tokens_after: flags
            .number::<u64>("--after")?
            .ok_or_else(|| CtlError::usage("--after is required"))?,
    };
    ledger::append_savings(paths, &entry).map_err(|e| CtlError::failed("ledger_write", e))?;
    let human = format!(
        "recorded {} saved {} tokens ({} -> {})",
        entry.provider,
        entry.saved(),
        entry.tokens_before,
        entry.tokens_after
    );
    let data = json!({
        "recorded": serde_json::to_value(&entry).unwrap_or(Value::Null),
        "tokensSaved": entry.saved(),
        "path": paths.savings().to_string_lossy(),
    });
    Ok(Output::new(data, human))
}

pub(super) fn ledger_summary(paths: &Paths, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LEDGER_SUMMARY.parse(rest)?;
    no_args(flags.operands())?;
    let since = match flags.one("--since") {
        Some(text) => Some(ledger::parse_ts(text).ok_or_else(|| {
            CtlError::usage("--since needs an RFC 3339 timestamp, e.g. 2026-01-31T00:00:00Z")
        })?),
        None => None,
    };
    let provider = flags.one("--provider");
    let rows = ledger::summarize(
        &ledger::read_launches(paths),
        &ledger::read_savings(paths),
        provider,
        since,
    );
    let mut human = String::from(
        "provider    launches  routed  plain  entries  before      after       saved       saved%",
    );
    for r in &rows {
        human.push_str(&format!(
            "\n{:<11} {:<9} {:<7} {:<6} {:<8} {:<11} {:<11} {:<11} {}",
            r.provider,
            r.launches,
            r.routed,
            r.plain,
            r.savings_entries,
            r.tokens_before,
            r.tokens_after,
            r.saved(),
            r.saved_percent().map_or("-".into(), |p| format!("{p:.1}%")),
        ));
    }
    if rows.is_empty() {
        human.push_str("\n(ledger is empty)");
    }
    let total: i64 = rows.iter().map(|r| r.saved()).sum();
    Ok(Output::new(
        json!({
            "providers": rows.iter().map(summary_json).collect::<Vec<_>>(),
            "tokensSaved": total,
            "launchesPath": paths.launches().to_string_lossy(),
            "savingsPath": paths.savings().to_string_lossy(),
        }),
        human,
    ))
}

pub fn proxy(rest: &[String]) -> Result<Output, CtlError> {
    let action = match rest {
        [a] if matches!(a.as_str(), "up" | "down" | "restart") => a.as_str(),
        [] => return Err(CtlError::usage("proxy needs up, down or restart")),
        [other, ..] => {
            return Err(CtlError::usage(format!(
                "unknown proxy action: {other} (up, down, restart)"
            )))
        }
    };
    let (_, loaded) = load()?;
    proxy_with(&loaded.config, &mut SystemOps::new(), action)
}

pub(super) fn proxy_with(
    config: &CompressionConfig,
    ops: &mut dyn EngineOps,
    action: &str,
) -> Result<Output, CtlError> {
    let preset = config.preset_for(None);
    let port = preset.port;
    let env = env_for_preset(config, &preset);
    let mut steps = Vec::new();
    if action != "up" {
        match engine::proxy_down(ops, port) {
            Ok(detail) => steps.push(detail),
            Err(why) if action == "down" => return Err(CtlError::failed("proxy_down", why)),
            Err(why) => steps.push(why),
        }
    }
    if action != "down" {
        match engine::proxy_up(ops, port, &env, 30) {
            Ok(detail) => steps.push(detail),
            Err(why) => return Err(CtlError::failed("proxy_up", why)),
        }
    }
    Ok(Output::new(
        json!({"action": action, "port": port, "steps": steps}),
        steps.join("\n"),
    ))
}

pub fn update(rest: &[String]) -> Result<Output, CtlError> {
    let flags = UPDATE.parse(rest)?;
    no_args(flags.operands())?;
    let (paths, loaded) = load()?;
    update_with(
        &paths,
        loaded.config,
        &mut SystemOps::new(),
        flags.one("--to"),
        flags.on("--latest"),
        flags.on("--accept"),
    )
}

pub(super) fn update_with(
    paths: &Paths,
    mut config: CompressionConfig,
    ops: &mut dyn EngineOps,
    to: Option<&str>,
    latest: bool,
    accept: bool,
) -> Result<Output, CtlError> {
    let target = engine::resolve_target(&config, ops, to, latest).map_err(|e| match e {
        crate::plus::compression::ops::UpdateError::Unresolvable => {
            CtlError::failed("update_unresolved", e.message())
        }
        _ => CtlError::usage(e.message()),
    })?;
    let mut human = format!(
        "update pin  {}\n  this build is unverified against the recorded contract - run \
         `toolportctl compression verify` after",
        if target.same {
            format!("already pinned at {}", target.target)
        } else {
            format!("{} -> {}", target.current, target.target)
        }
    );
    let mut data = json!({
        "current": target.current,
        "target": target.target,
        "same": target.same,
        "accepted": accept,
    });
    if !accept {
        human.push_str(
            "\n  preview only: re-run with --accept to move the pin, install and re-snapshot",
        );
        return Ok(Output::new(data, human));
    }
    engine::set_pin_and_save(paths, &mut config, &target.target)
        .map_err(|e| CtlError::failed("config_write", e))?;
    let report = engine::apply_update(&mut config, &target.target, ops)
        .map_err(|e| CtlError::failed("install_failed", e))?;
    store::save(paths, &config).map_err(|e| CtlError::failed("config_write", e))?;
    human.push_str(&format!("\n  installed: {}", report.install_detail));
    for note in &report.snapshot_notes {
        human.push_str(&format!("\n  {note}"));
    }
    if report.version_changed {
        human.push_str(
            "\n  restart proxies to run the new build: toolportctl compression proxy restart",
        );
    }
    data["installed"] = json!(report.install_detail);
    data["versionChanged"] = json!(report.version_changed);
    data["snapshots"] = json!(report.snapshot_notes);
    Ok(Output::new(data, human))
}

fn handler_value(result: Result<Output, CtlError>) -> Result<Value, String> {
    result.map(|o| o.data).map_err(|e| e.message)
}

fn handler_argv(args: &Value, keys: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    for (key, flag) in keys {
        match args.get(*key) {
            Some(Value::Bool(true)) => out.push(format!("--{flag}")),
            Some(Value::String(v)) => out.push(format!("--{flag}={v}")),
            Some(Value::Number(v)) => out.push(format!("--{flag}={v}")),
            _ => {}
        }
    }
    out
}

pub fn status_handler(_args: Value) -> Result<Value, String> {
    handler_value(status(&[]))
}

pub fn verify_handler(args: Value) -> Result<Value, String> {
    let argv = handler_argv(
        &args,
        &[
            ("limit", "limit"),
            ("minTurns", "min-turns"),
            ("transcripts", "transcripts"),
            ("byPin", "by-pin"),
        ],
    );
    handler_value(verify(&argv))
}

pub fn ledger_handler(args: Value) -> Result<Value, String> {
    let mut argv = vec!["summary".to_string()];
    argv.extend(handler_argv(&args, &[("provider", "provider"), ("since", "since")]));
    handler_value(ledger_cmd(&argv))
}

pub fn plan_handler(args: Value) -> Result<Value, String> {
    let mut argv = vec!["--plan".to_string()];
    argv.extend(handler_argv(&args, &[("cwd", "cwd")]));
    handler_value(run(&argv))
}
