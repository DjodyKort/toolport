//! The `hrclaude` launcher. `plan_launch` is pure: it resolves the per-directory policy and
//! returns the environment and argv to launch `claude` with, plus what must be running first.
//! `run_plan` walks a plan through an injectable [`LaunchOps`]; [`SystemOps`] is the thin
//! process-spawning wrapper kept out of everything testable.

use super::engine::{health_url, http_json};
use super::model::*;
use super::ops::{pin_guard, PinGuard};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

pub const BASE_URL_VAR: &str = "ANTHROPIC_BASE_URL";

pub trait Probe {
    fn headroom_version(&self) -> Option<String>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProxySpec {
    pub port: u16,
    pub mode: String,
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvPlan {
    pub set: BTreeMap<String, String>,
    pub unset: Vec<String>,
}

/// What a launch did, so `verify` never credits a plain launch to the proxy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LedgerEntry {
    pub cwd: String,
    pub provider: ProviderName,
    pub preset: String,
    pub routed: bool,
    pub port: Option<u16>,
    pub pin: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub cwd: String,
    pub provider: ProviderName,
    pub preset: String,
    pub pin: String,
    pub installed: Option<String>,
    pub routed: bool,
    pub program: String,
    pub argv: Vec<String>,
    pub env: EnvPlan,
    pub proxy: Option<ProxySpec>,
    pub warnings: Vec<String>,
    pub ledger: LedgerEntry,
}

impl LaunchPlan {
    /// The same launch without the proxy; the fallback when routing cannot be trusted.
    pub fn degrade(&self, warning: String) -> LaunchPlan {
        let mut plain = self.clone();
        plain.routed = false;
        plain.proxy = None;
        plain.env = EnvPlan {
            set: BTreeMap::new(),
            unset: vec![BASE_URL_VAR.to_string()],
        };
        plain.ledger.routed = false;
        plain.ledger.port = None;
        plain.warnings.push(warning);
        plain
    }

    /// Applies the plan to a parent environment, as the child would see it.
    #[cfg(test)]
    pub fn child_env(&self, parent: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        let mut env = parent.clone();
        for key in &self.env.unset {
            env.remove(key);
        }
        env.extend(self.env.set.clone());
        env
    }
}

pub fn plan_launch(
    config: &CompressionConfig,
    cwd: &str,
    force: bool,
    claude_args: &[String],
    probe: &dyn Probe,
) -> LaunchPlan {
    let (provider, preset_name) = config.resolve(cwd);
    let preset = config.preset_for(Some(&preset_name));
    let pin = config.provider_version.pin.clone();
    let mut plan = LaunchPlan {
        cwd: cwd.to_string(),
        provider,
        preset: preset_name.clone(),
        pin: pin.clone(),
        installed: None,
        routed: false,
        program: "claude".into(),
        argv: claude_args.to_vec(),
        env: EnvPlan {
            set: BTreeMap::new(),
            unset: vec![BASE_URL_VAR.to_string()],
        },
        proxy: None,
        warnings: Vec::new(),
        ledger: LedgerEntry {
            cwd: cwd.to_string(),
            provider,
            preset: preset_name,
            routed: false,
            port: None,
            pin: pin.clone(),
        },
    };
    // rtk's hook is global and parsec owns its own routing: nothing for a launch to add.
    if provider != ProviderName::Headroom {
        return plan;
    }
    plan.installed = probe.headroom_version();
    match pin_guard(&pin, plan.installed.as_deref(), force) {
        PinGuard::Refused(why) => return plan.degrade(why),
        PinGuard::Forced(why) => plan.warnings.push(why),
        PinGuard::Match => {}
    }
    let env = env_for_preset(config, &preset);
    plan.env = EnvPlan {
        set: env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        unset: Vec::new(),
    };
    plan.proxy = Some(ProxySpec {
        port: preset.port,
        mode: preset.mode.as_str().into(),
        argv: vec![
            "headroom".into(),
            "proxy".into(),
            "--port".into(),
            preset.port.to_string(),
            "--mode".into(),
            preset.mode.as_str().into(),
        ],
    });
    plan.routed = true;
    plan.ledger.routed = true;
    plan.ledger.port = Some(preset.port);
    plan
}

pub trait LaunchOps {
    /// Ensures the proxy is serving; `Err` means it is not, and the launch goes plain.
    fn ensure_proxy(&mut self, spec: &ProxySpec) -> Result<String, String>;
    fn record(&mut self, entry: &LedgerEntry);
    fn launch(&mut self, plan: &LaunchPlan) -> Result<i32, String>;
}

pub struct Outcome {
    pub plan: LaunchPlan,
    pub exit_code: i32,
}

/// A proxy that cannot be confirmed is not routed through: the plan degrades to plain
/// instead of pointing `ANTHROPIC_BASE_URL` at a dead port, and the ledger says so.
pub fn run_plan(plan: LaunchPlan, ops: &mut dyn LaunchOps) -> Result<Outcome, String> {
    let mut plan = plan;
    if let Some(spec) = plan.proxy.clone() {
        if let Err(why) = ops.ensure_proxy(&spec) {
            plan = plan.degrade(format!("{why}; launching plain claude"));
        }
    }
    ops.record(&plan.ledger);
    let exit_code = ops.launch(&plan)?;
    Ok(Outcome { plan, exit_code })
}

pub struct SystemOps {
    /// Overrides PATH for the version probe and the launched process (tests use fakes).
    pub path: Option<OsString>,
}

impl SystemOps {
    pub fn new() -> Self {
        Self { path: None }
    }

    pub(super) fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        if let Some(path) = &self.path {
            cmd.env("PATH", path);
        }
        cmd
    }
}

impl Default for SystemOps {
    fn default() -> Self {
        Self::new()
    }
}

impl Probe for SystemOps {
    fn headroom_version(&self) -> Option<String> {
        let out = self
            .command("headroom")
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .ok()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        static VERSION: OnceLock<Regex> = OnceLock::new();
        let re = VERSION.get_or_init(|| Regex::new(r"(\d+\.\d+\.\d+)").expect("valid pattern"));
        re.captures(&text).map(|c| c[1].to_string())
    }
}

pub fn proxy_ready(port: u16) -> bool {
    http_json(&health_url(port), 3)
        .and_then(|body| body.get("ready").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

impl LaunchOps for SystemOps {
    /// Reuses a healthy proxy. Starting one is proxy control (`hrup`), not the launcher's job.
    fn ensure_proxy(&mut self, spec: &ProxySpec) -> Result<String, String> {
        if proxy_ready(spec.port) {
            Ok(format!("reusing proxy on :{}", spec.port))
        } else {
            Err(format!(
                "no ready proxy on :{} (start it with hrup)",
                spec.port
            ))
        }
    }

    fn record(&mut self, _entry: &LedgerEntry) {}

    fn launch(&mut self, plan: &LaunchPlan) -> Result<i32, String> {
        let mut cmd = self.command(&plan.program);
        cmd.args(&plan.argv);
        for key in &plan.env.unset {
            cmd.env_remove(key);
        }
        for (k, v) in &plan.env.set {
            cmd.env(k, v);
        }
        let status = cmd
            .status()
            .map_err(|e| format!("cannot launch {}: {e}", plan.program))?;
        Ok(status.code().unwrap_or(1))
    }
}
