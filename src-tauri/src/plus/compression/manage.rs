//! The command cores behind `toolportctl compression enable|disable|set-provider|use|sync|pin|
//! seal|env|doctor` and the matching `plus.compression.*` handlers. Each core returns the JSON
//! the CLI prints with `--json`; the CLI only adds the human text. Everything outside the
//! policy (engine, registry) is reached through the traits in [`Ctx`], so tests run on fakes.

use super::apply::{apply, legacy_plist_path, ApplyCtx, Report};
use super::capability::{split_unsealed, unsealed_for};
use super::engine::EngineOps;
use super::launch::SystemOps;
use super::legacy::{self, Adoption, Prepared};
use super::mcp_entry::{McpHost, RegistryHost};
use super::model::*;
use super::ops::{self, EnableOpts, SnapshotDiff};
use super::shims::escape_double_quoted;
use super::store::{self, Paths};
use super::verify::{health_checks, HealthProbe};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Usage,
    Failed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdError {
    pub kind: Kind,
    pub message: String,
}

impl CmdError {
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Usage,
            message: message.into(),
        }
    }

    pub fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Failed(code),
            message: message.into(),
        }
    }
}

pub type CmdResult = Result<Value, CmdError>;

pub struct Ctx<'a> {
    pub paths: &'a Paths,
    pub mcpm_root: Option<PathBuf>,
    pub legacy_plist: Option<PathBuf>,
    pub engine: &'a mut dyn EngineOps,
    pub mcp: &'a mut dyn McpHost,
}

/// The real data directory, engine and registry.
pub fn with_system<T, E: From<CmdError>>(
    f: impl FnOnce(&mut Ctx) -> Result<T, E>,
) -> Result<T, E> {
    let paths = Paths::from_data_dir()
        .ok_or_else(|| CmdError::failed("no_data_dir", "data directory could not be resolved"))?;
    let mut engine = SystemOps::new();
    let mut mcp = RegistryHost;
    f(&mut Ctx {
        paths: &paths,
        mcpm_root: legacy::default_root(),
        legacy_plist: legacy_plist_path(),
        engine: &mut engine,
        mcp: &mut mcp,
    })
}

pub fn parse_provider(text: &str) -> Result<ProviderName, CmdError> {
    ProviderName::parse(text).ok_or_else(|| {
        CmdError::usage(format!(
            "unknown provider '{text}' (have: {})",
            ProviderName::ALL.map(ProviderName::as_str).join(", ")
        ))
    })
}

pub fn parse_mode(text: &str) -> Result<CompressionMode, CmdError> {
    match text {
        "cache" => Ok(CompressionMode::Cache),
        "token" => Ok(CompressionMode::Token),
        _ => Err(CmdError::usage(format!(
            "unknown mode '{text}' (have: cache, token)"
        ))),
    }
}

pub fn parse_telemetry(text: &str) -> Result<String, CmdError> {
    match text {
        "off" | "on" => Ok(text.into()),
        _ => Err(CmdError::usage(format!(
            "unknown telemetry '{text}' (have: off, on)"
        ))),
    }
}

fn prepare(cx: &Ctx, root: Option<&Path>, mutating: bool) -> Result<Prepared, CmdError> {
    let invalid = |e: String| CmdError::failed("config_invalid", e);
    if mutating {
        return legacy::prepare(cx.paths, root.or(cx.mcpm_root.as_deref())).map_err(invalid);
    }
    Ok(Prepared {
        config: store::read(cx.paths).map_err(invalid)?.config,
        adoption: None,
        warnings: Vec::new(),
    })
}

fn preset_value(config: &CompressionConfig, name: Option<&str>) -> Value {
    let preset = config.preset_for(name);
    json!({
        "name": name.unwrap_or(&config.active_preset),
        "mode": preset.mode.as_str(),
        "savingsProfile": preset.savings_profile,
        "port": preset.port,
    })
}

struct Reconciled {
    config: CompressionConfig,
    report: Report,
    adoption: Option<Adoption>,
}

impl Reconciled {
    fn data(&self) -> Value {
        let mut data = json!({
            "provider": self.config.provider.as_str(),
            "runtime": self.config.runtime.as_str(),
            "preset": preset_value(&self.config, None),
            "adopted": self.adoption.as_ref().map(Adoption::to_value),
        });
        self.report.put(&mut data);
        data
    }
}

fn reconcile(
    cx: &mut Ctx,
    root: Option<&Path>,
    dry_run: bool,
    teardown: bool,
    change: impl FnOnce(&mut CompressionConfig) -> Result<(), CmdError>,
) -> Result<Reconciled, CmdError> {
    let mut prepared = prepare(cx, root, true)?;
    change(&mut prepared.config)?;
    let mut report = Report::new(dry_run);
    if let Some(adoption) = &prepared.adoption {
        let from = adoption.from.display();
        report.did(
            format!("adopted legacy policy from {from}"),
            format!("would adopt legacy policy from {from}"),
        );
        for note in &adoption.notes {
            report.did(format!("migrated {note}"), format!("would migrate {note}"));
        }
    }
    for warning in prepared.warnings {
        report.warn(warning);
    }
    let ctx = ApplyCtx {
        paths: cx.paths,
        legacy_plist: cx.legacy_plist.clone(),
        dry_run,
        teardown,
    };
    apply(
        &mut prepared.config,
        &ctx,
        &mut *cx.engine,
        &mut *cx.mcp,
        &mut report,
    )
    .map_err(|e| CmdError::failed("config_write", e))?;
    Ok(Reconciled {
        config: prepared.config,
        report,
        adoption: prepared.adoption,
    })
}

#[derive(Debug, Default)]
pub struct EnableReq {
    pub provider: Option<ProviderName>,
    pub port: Option<u16>,
    pub telemetry: Option<String>,
    pub preset: Option<String>,
    pub mode: Option<CompressionMode>,
    pub dry_run: bool,
}

fn enable_next_steps(config: &CompressionConfig, paths: &Paths) -> Vec<String> {
    let mut steps = Vec::new();
    if config.provider == ProviderName::Headroom {
        steps.push(format!(
            "add to ~/.zshrc: source {}  (hrclaude/hrup/hrdown \u{2014} replaces headroom-aliases.zsh)",
            paths.shims().display()
        ));
        steps.push(
            "or launch directly: toolportctl compression run -- <claude args>  (resolves the \
             per-dir preset, ensures the proxy, execs claude)"
                .into(),
        );
    }
    steps.push(
        "verify: toolportctl compression status / doctor   (MCP entry already registered)".into(),
    );
    steps
}

pub fn enable(cx: &mut Ctx, req: &EnableReq) -> CmdResult {
    let opts = EnableOpts {
        provider: req.provider,
        port: req.port,
        telemetry: req.telemetry.clone(),
        preset: req.preset.clone(),
        mode: req.mode,
    };
    let done = reconcile(cx, None, req.dry_run, false, |c| {
        ops::enable(c, &opts).map_err(CmdError::usage)
    })?;
    let mut data = done.data();
    data["nextSteps"] = json!(enable_next_steps(&done.config, cx.paths));
    Ok(data)
}

pub fn disable(cx: &mut Ctx, teardown: bool, dry_run: bool) -> CmdResult {
    let done = reconcile(cx, None, dry_run, teardown, |c| {
        ops::disable(c);
        Ok(())
    })?;
    let mut data = done.data();
    data["teardown"] = json!(teardown);
    Ok(data)
}

pub fn set_provider(cx: &mut Ctx, provider: ProviderName, dry_run: bool) -> CmdResult {
    let done = reconcile(cx, None, dry_run, false, |c| {
        ops::set_provider(c, provider);
        Ok(())
    })?;
    Ok(done.data())
}

pub fn use_preset(cx: &mut Ctx, name: &str, dry_run: bool) -> CmdResult {
    let done = reconcile(cx, None, dry_run, false, |c| {
        ops::use_preset(c, name).map_err(CmdError::usage)
    })?;
    Ok(done.data())
}

pub fn sync(cx: &mut Ctx, mcpm_root: Option<&Path>, dry_run: bool) -> CmdResult {
    Ok(reconcile(cx, mcpm_root, dry_run, false, |_| Ok(()))?.data())
}

/// What `eval "$(toolportctl compression env)"` needs: the proxy env of the directory's
/// policy, then the launch hints the wrappers read.
pub fn env(cx: &mut Ctx, cwd: &str) -> CmdResult {
    let config = prepare(cx, None, false)?.config;
    let (provider, preset_name) = config.resolve(cwd);
    if provider != ProviderName::Headroom {
        return Ok(json!({
            "cwd": cwd,
            "provider": provider.as_str(),
            "preset": preset_name,
            "launch": "plain",
            "port": null,
            "env": {},
            "lines": ["HRCOMPRESS_LAUNCH=plain"],
        }));
    }
    let preset = config.preset_for(Some(&preset_name));
    let env = env_for_preset(&config, &preset);
    let mut lines: Vec<String> = env
        .iter()
        .map(|(k, v)| format!("export {k}=\"{}\"", escape_double_quoted(v)))
        .collect();
    lines.push(format!("HRCOMPRESS_PORT={}", preset.port));
    lines.push(format!("HRCOMPRESS_PRESET={preset_name}"));
    lines.push("HRCOMPRESS_LAUNCH=route".into());
    let vars: serde_json::Map<String, Value> =
        env.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    Ok(json!({
        "cwd": cwd,
        "provider": provider.as_str(),
        "preset": preset_name,
        "launch": "route",
        "port": preset.port,
        "env": vars,
        "lines": lines,
    }))
}

fn diff_value(name: &str, diff: &SnapshotDiff) -> Value {
    json!({
        "name": name,
        "changed": diff.changed(),
        "added": diff.added.iter().map(|(k, v)| json!({"knob": k, "value": v})).collect::<Vec<_>>(),
        "removed": diff.removed.iter().map(|(k, v)| json!({"knob": k, "was": v})).collect::<Vec<_>>(),
        "moved": diff.moved.iter().map(|(k, a, b)| json!({"knob": k, "from": a, "to": b})).collect::<Vec<_>>(),
        "kept": diff.kept,
    })
}

/// Re-snapshots every profile-backed preset from the installed build. Nothing is persisted
/// here; a failure part-way leaves the caller's file untouched.
pub fn refresh_snapshots(
    config: &mut CompressionConfig,
    engine: &mut dyn EngineOps,
) -> Result<Value, CmdError> {
    let live = engine.installed_version();
    let mut presets = Vec::new();
    for (name, preset) in config.presets.iter_mut() {
        let Some(profile) = preset.savings_profile.clone() else {
            continue;
        };
        let env = engine.agent_savings(&profile).map_err(|why| {
            CmdError::failed("snapshot_failed", format!("preset '{name}': {why}"))
        })?;
        let diff = ops::apply_snapshot(preset, &env, live.as_deref());
        presets.push(diff_value(name, &diff));
    }
    Ok(json!({"version": live, "presets": presets}))
}

pub fn refresh_presets(cx: &mut Ctx, dry_run: bool) -> CmdResult {
    let mut prepared = prepare(cx, None, true)?;
    let mut refresh = refresh_snapshots(&mut prepared.config, &mut *cx.engine)?;
    if !dry_run {
        store::save(cx.paths, &prepared.config).map_err(|e| CmdError::failed("config_write", e))?;
    }
    refresh["saved"] = json!(!dry_run);
    refresh["dryRun"] = json!(dry_run);
    Ok(refresh)
}

#[derive(Debug, Default)]
pub struct PinReq {
    pub version: Option<String>,
    pub install: bool,
    pub refresh: bool,
    pub dry_run: bool,
}

pub fn pin(cx: &mut Ctx, req: &PinReq) -> CmdResult {
    let mutating = req.version.is_some() || req.install || req.refresh;
    let mut prepared = prepare(cx, None, mutating)?;
    if let Some(version) = &req.version {
        if version_tuple(Some(version)).is_empty() {
            return Err(CmdError::usage(format!(
                "'{version}' is not a parseable X.Y.Z version"
            )));
        }
        ops::set_pin(&mut prepared.config, version);
        if !req.dry_run {
            store::save(cx.paths, &prepared.config)
                .map_err(|e| CmdError::failed("config_write", e))?;
        }
    }
    let pv = prepared.config.provider_version.clone();
    let live = cx.engine.installed_version();
    let mut data = json!({
        "set": req.version.is_some(),
        "pin": pv.pin,
        "requirement": pv.requirement(),
        "installed": live,
        "drift": live.as_deref().is_some_and(|v| v != pv.pin),
        "adopted": prepared.adoption.as_ref().map(Adoption::to_value),
        "install": null,
        "refresh": null,
        "dryRun": req.dry_run,
    });
    if !req.install && !req.refresh {
        return Ok(data);
    }
    let mut changed = false;
    if req.install {
        let requirement = pv.requirement();
        if req.dry_run {
            data["install"] = json!({"requirement": requirement, "dryRun": true});
        } else {
            let (before, after) = cx
                .engine
                .install(&requirement)
                .map_err(|why| CmdError::failed("install_failed", why))?;
            changed = before != after;
            let detail = if before == after {
                format!("already at {}", after.as_deref().unwrap_or("unknown"))
            } else {
                format!(
                    "{} \u{2192} {}",
                    before.as_deref().unwrap_or("none"),
                    after.as_deref().unwrap_or("unknown")
                )
            };
            data["install"] = json!({
                "requirement": requirement,
                "dryRun": false,
                "detail": detail,
                "before": before,
                "after": after,
                "changed": changed,
            });
        }
    }
    if req.refresh {
        let mut refresh = refresh_snapshots(&mut prepared.config, &mut *cx.engine)?;
        if !req.dry_run {
            store::save(cx.paths, &prepared.config)
                .map_err(|e| CmdError::failed("config_write", e))?;
        }
        refresh["saved"] = json!(!req.dry_run);
        data["refresh"] = refresh;
    }
    data["restartProxies"] = json!(changed);
    Ok(data)
}

#[derive(Debug, Default)]
pub struct SealReq {
    pub preset: Option<String>,
    pub apply: bool,
    pub dry_run: bool,
}

pub fn seal(cx: &mut Ctx, req: &SealReq) -> CmdResult {
    let mut prepared = prepare(cx, None, req.apply)?;
    let config = &mut prepared.config;
    let name = req
        .preset
        .clone()
        .unwrap_or_else(|| config.active_preset.clone());
    if !config.presets.contains_key(&name) {
        let have: Vec<&str> = config.presets.keys().map(String::as_str).collect();
        return Err(CmdError::usage(format!(
            "unknown preset '{name}' (have: {})",
            have.join(", ")
        )));
    }
    let preset = config.presets.get_mut(&name).expect("checked above");
    let port = preset.port;
    let effective = cx
        .engine
        .proxy_health(port)
        .and_then(|h| h.get("config").and_then(Value::as_object).cloned())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| {
            CmdError::failed(
                "no_proxy",
                format!(
                    "no proxy on :{port} \u{2014} start one first (toolportctl compression proxy up). \
                     /health is the only truthful source for what a build actually runs."
                ),
            )
        })?;
    let (declarable, unset) = split_unsealed(&unsealed_for(&preset.knobs, &effective));
    let mut data = json!({
        "preset": name,
        "port": port,
        "version": cx.engine.installed_version(),
        "complete": declarable.is_empty() && unset.is_empty(),
        "declarable": declarable.iter().map(|(k, v)| json!({"knob": k, "value": v})).collect::<Vec<_>>(),
        "unset": unset,
        "apply": req.apply,
        "dryRun": req.dry_run,
        "sealed": 0,
    });
    if !req.apply || req.dry_run || declarable.is_empty() {
        return Ok(data);
    }
    data["sealed"] = json!(ops::seal(preset, &declarable));
    store::save(cx.paths, config).map_err(|e| CmdError::failed("config_write", e))?;
    Ok(data)
}

pub fn doctor(cx: &mut Ctx, probe: &dyn HealthProbe) -> CmdResult {
    let loaded = store::read(cx.paths).map_err(|e| CmdError::failed("config_invalid", e))?;
    let checks = health_checks(&loaded.config, cx.paths, probe);
    Ok(json!({
        "provider": loaded.config.provider.as_str(),
        "migrated": loaded.notes,
        "healthy": checks.iter().all(|c| c.ok),
        "checks": checks,
    }))
}

pub fn preset_json(name: &str, config: &CompressionConfig) -> Value {
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

/// The status JSON plus the pieces a surface needs to word it.
pub struct StatusReport {
    pub data: Value,
    pub config: CompressionConfig,
    pub installed: Option<String>,
    pub drift: Option<bool>,
    pub existed: bool,
}

pub fn status(cx: &Ctx) -> Result<StatusReport, CmdError> {
    let loaded = store::read(cx.paths).map_err(|e| CmdError::failed("config_invalid", e))?;
    let config = &loaded.config;
    let installed = (config.provider == ProviderName::Headroom)
        .then(|| cx.engine.installed_version())
        .flatten();
    let drift = (config.provider == ProviderName::Headroom)
        .then(|| installed.as_deref() != Some(config.provider_version.pin.as_str()));
    let pv = &config.provider_version;
    let data = json!({
        "configPath": cx.paths.config().to_string_lossy(),
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
        "shims": {"path": cx.paths.shims().to_string_lossy(), "exists": cx.paths.shims().exists()},
    });
    Ok(StatusReport {
        data,
        existed: loaded.existed,
        config: loaded.config,
        installed,
        drift,
    })
}
