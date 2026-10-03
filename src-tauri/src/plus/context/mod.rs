//! Context engine: layered rules, `CLAUDE.local.md` deploy, corp-dev-tools coexistence (settings
//! permission union, legacy MCP dedupe, shims, tripwires). Port of mcpm-context's deploy side.
//! Launch profiles (CTX-3): `--strict-mcp-config`/`--settings` argv per profile, no second login.
//!
//! Every operation takes explicit [`Roots`], so tests run in temp dirs and nothing reads `$HOME`.

pub mod backup;
pub mod compact;
pub mod config;
pub mod dedupe;
pub mod doctor;
pub mod folders;
pub mod launch;
pub mod layers;
pub mod loads;
pub mod roots;
pub mod rules;
pub mod settings;
pub mod shims;

#[cfg(test)] mod layers_prop_tests;
#[cfg(test)] mod settings_prop_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_compact;
#[cfg(test)]
mod tests_folders;
#[cfg(test)]
mod tests_hardening;
#[cfg(test)]
mod tests_hooks;

pub use config::{load_config, preserve_unreadable, save_config, ContextConfig};
pub use roots::Roots;

use doctor::sha256_file;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub actions: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn add(&mut self, msg: impl Into<String>) {
        self.actions.push(msg.into());
    }

    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    pub fn to_value(&self) -> Value {
        json!({ "actions": self.actions, "warnings": self.warnings })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ApplyOptions {
    pub persist: bool,
    pub dry_run: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            persist: true,
            dry_run: false,
        }
    }
}

/// Doctor tripwire baseline: hash of corp-dev-tools' shell wrapper. Like mcpm it is only recorded
/// while unset, so a saved config never re-baselines (recorded quirk).
fn record_cf_baseline(roots: &Roots, config: &mut ContextConfig, report: &mut Report) {
    let wrapper = roots.cf_dir.join("claude").join("shell-wrapper.sh");
    let Some(digest) = sha256_file(&wrapper) else {
        return;
    };
    if config.cf_wrapper_hash.is_none() {
        config.cf_wrapper_hash = Some(digest);
        report.add("recorded corp-dev-tools shell-wrapper baseline");
    }
}

fn warn_orphans(roots: &Roots, config: &ContextConfig, report: &mut Report) {
    let Ok(read) = fs::read_dir(roots.profiles_root()) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !config.profiles.contains_key(&name) {
            report.warn(format!(
                "orphan profile dir {} (not in config) — `mcpm context profile remove {name}`",
                dir.display()
            ));
        }
    }
}

/// Makes the world match `config`. Safe to run repeatedly; with `dry_run` nothing is written
/// and the report lists what would happen.
pub fn apply(
    roots: &Roots,
    config: &mut ContextConfig,
    opts: ApplyOptions,
) -> Result<Report, String> {
    config.validate()?;
    let roots = &roots.resolved(config);
    let mut report = Report::default();
    let dry = opts.dry_run;

    if config.dedupe.enabled {
        let removed = dedupe::apply_dedupe(roots, &roots.claude_json, &config.dedupe, true, dry)?;
        if !removed.is_empty() {
            let file = roots
                .claude_json
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            report.add(format!(
                "removed legacy MCP entries from {file}: {}",
                removed.join(", ")
            ));
        }
    }

    let settings_path = roots.claude_home.join("settings.json");
    match settings::ensure_policy(roots, &settings_path, &config.settings, true, dry)? {
        settings::PolicyOutcome::Changed => {
            report.add("re-unioned ensure_allow/ensure_ask into settings.json");
        }
        settings::PolicyOutcome::Skipped(reason) => report.warn(format!("settings.json: {reason}")),
        settings::PolicyOutcome::Unchanged => {}
    }

    layers::deploy_client_locals(
        roots,
        &roots.resolve_clients_root(config),
        &mut report,
        dry,
    )?;

    for (name, spec) in &config.profiles {
        launch::generate_profile(roots, name, spec, &mut report, dry)?;
    }
    warn_orphans(roots, config, &mut report);

    if !config.profiles.is_empty() || config.wrap_default_claude {
        let path = shims::write_shims(roots, &config.profiles, config.wrap_default_claude, dry)?;
        let verb = if dry { "would write" } else { "wrote" };
        report.add(format!("{verb} shims: {path}"));
    } else if shims::remove_shims(roots, dry)? {
        let verb = if dry { "would remove" } else { "removed" };
        report.add(format!("{verb} shims (no profiles, wrapper disabled)"));
    }

    record_cf_baseline(roots, config, &mut report);
    if opts.persist && !dry {
        if let Some(kept) = preserve_unreadable(&roots.context_config_path()) {
            report.add(format!(
                "context.json was unreadable and is replaced by the effective config; kept a copy at {}",
                kept.display()
            ));
        }
        save_config(&roots.context_config_path(), config)?;
        report.add(format!(
            "saved config ({} profile(s))",
            config.profiles.len()
        ));
    }
    Ok(report)
}

pub fn plan(roots: &Roots, config: &mut ContextConfig) -> Result<Report, String> {
    apply(
        roots,
        config,
        ApplyOptions {
            persist: false,
            dry_run: true,
        },
    )
}

fn roots_from_args(args: &Value) -> Result<Roots, String> {
    let home = match args.get("home").and_then(Value::as_str) {
        Some(h) => PathBuf::from(h),
        None => dirs::home_dir().ok_or("cannot resolve the home directory")?,
    };
    let mut roots = Roots::from_home(&home);
    let over = |key: &str| args.get(key).and_then(Value::as_str).map(PathBuf::from);
    if let Some(p) = over("claudeHome") {
        roots.claude_home = p;
    }
    if let Some(p) = over("claudeJson") {
        roots.claude_json = p;
    }
    if let Some(p) = over("configDir") {
        roots.config_dir = p;
    }
    if let Some(p) = over("cacheDir") {
        roots.cache_dir = p;
    }
    if let Some(p) = over("cfDir") {
        roots.cf_dir = p;
    }
    roots.read_env();
    roots.env_claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    compact::apply_env(&mut roots);
    Ok(roots)
}

fn config_from_args(roots: &Roots, args: &Value) -> Result<ContextConfig, String> {
    match args.get("config") {
        Some(v) if !v.is_null() => ContextConfig::from_value(v.clone()),
        _ => Ok(load_config(&roots.context_config_path())),
    }
}

/// Serializes real runs, which the shell wrapper starts once per `claude`, so two of them never
/// interleave their writes. A dry run only reads and stays lock-free. The lock is taken here and
/// not in `apply`: `apply` is also the engine the parity harness drives against a golden home
/// tree, where a `context.json.lock` sibling would be an unexpected file.
fn lock_real_run(
    roots: &Roots,
    dry_run: bool,
) -> Result<Option<crate::registry::FileLock>, String> {
    if dry_run {
        return Ok(None);
    }
    crate::registry::lock_at(&roots.context_config_path())
        .map(Some)
        .map_err(|e| format!("context sync: {e}"))
}

fn run(args: &Value, dry_run: bool) -> Result<Value, String> {
    let roots = roots_from_args(args)?;
    let _lock = lock_real_run(&roots, dry_run)?;
    let mut config = config_from_args(&roots, args)?;
    let persist = args.get("persist").and_then(Value::as_bool).unwrap_or(true);
    let mut report = apply(&roots, &mut config, ApplyOptions { persist, dry_run })?;
    if args.get("rules").and_then(Value::as_bool).unwrap_or(false) {
        for path in rules::deploy_rules_now(&roots, dry_run)? {
            report.add(format!("deployed rule {}", path.display()));
        }
    }
    let mut out = report.to_value();
    out["dryRun"] = json!(dry_run);
    out["checks"] = json!(doctor::run_checks(&roots, &config));
    Ok(out)
}

/// `plus.context.plan`: dry-run listing of what `apply` would change. Takes `home` (default the
/// user's home), optional root overrides, `config` (default the saved one) and `rules: true` to
/// include the layered-rules deploy.
pub fn plan_handler(args: Value) -> Result<Value, String> {
    run(&args, true)
}

/// `plus.context.apply`: same arguments as the plan, writing for real. `persist: false` skips
/// saving `context.json`.
pub fn apply_handler(args: Value) -> Result<Value, String> {
    run(&args, false)
}

/// `plus.context.whatLoads`: takes the `plan` root overrides plus optional `profile` and `cwd`
/// (default the home directory) and returns [`loads::WhatLoads`] as JSON. Read-only.
pub fn what_loads_handler(args: Value) -> Result<Value, String> {
    let roots = roots_from_args(&args)?;
    let config = config_from_args(&roots, &args)?;
    let cwd = args
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| roots.home.clone());
    let profile = args.get("profile").and_then(Value::as_str);
    let report = loads::what_loads(&roots, &config, profile, &cwd)?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}
