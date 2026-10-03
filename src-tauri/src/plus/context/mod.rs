//! Context engine: layered rules, `CLAUDE.local.md` deploy, cf-dev-tools coexistence (settings
//! permission union, legacy MCP dedupe, shims, tripwires). Port of mcpm-context's deploy side.
//! Launch profiles (CTX-3) are not generated here; a configured profile is reported as a warning.
//!
//! Every operation takes explicit [`Roots`], so tests run in temp dirs and nothing reads `$HOME`.

pub mod backup;
pub mod config;
pub mod dedupe;
pub mod doctor;
pub mod layers;
pub mod roots;
pub mod rules;
pub mod settings;
pub mod shims;

#[cfg(test)]
mod tests;

pub use config::{load_config, save_config, ContextConfig};
pub use roots::Roots;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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

/// Doctor tripwire baseline: hash of cf-dev-tools' shell wrapper. Like mcpm it is only recorded
/// while unset, so a saved config never re-baselines (recorded quirk).
fn record_cf_baseline(roots: &Roots, config: &mut ContextConfig, report: &mut Report) {
    let wrapper = roots.cf_dir.join("claude").join("shell-wrapper.sh");
    let Ok(bytes) = fs::read(wrapper) else {
        return;
    };
    if config.cf_wrapper_hash.is_none() {
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        config.cf_wrapper_hash = Some(digest);
        report.add("recorded cf-dev-tools shell-wrapper baseline");
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
                "orphan profile dir {} (not in config) — `mcpm context profile remove {name} --purge`",
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
        &roots.expand_user(&config.clients_root),
        &mut report,
        dry,
    )?;

    for name in config.profiles.keys() {
        report.warn(format!(
            "profile {name}: generation is not implemented in the context engine yet (launch profiles, MIG-CTX-3)"
        ));
    }
    warn_orphans(roots, config, &mut report);

    if !config.profiles.is_empty() || config.wrap_default_claude {
        let path = shims::write_shims(roots, &config.profiles, config.wrap_default_claude, dry)?;
        report.add(format!("wrote shims: {path}"));
    } else if shims::remove_shims(roots, dry)? {
        report.add("removed shims (no profiles, wrapper disabled)");
    }

    record_cf_baseline(roots, config, &mut report);
    if opts.persist && !dry {
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
    roots.env_claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    Ok(roots)
}

fn config_from_args(roots: &Roots, args: &Value) -> Result<ContextConfig, String> {
    match args.get("config") {
        Some(v) if !v.is_null() => ContextConfig::from_value(v.clone()),
        _ => Ok(load_config(&roots.context_config_path())),
    }
}

fn run(args: &Value, dry_run: bool) -> Result<Value, String> {
    let roots = roots_from_args(args)?;
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
