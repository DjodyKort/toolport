//! Engine lifecycle behind the shims: proxy up/down/restart and the pin bump with its
//! re-snapshot. The flows run against [`EngineOps`] so tests never touch a real process;
//! [`SystemOps`] is the implementation that spawns, signals and installs.

use super::launch::SystemOps;
use super::model::{CompressionConfig, OrderedMap};
use super::ops::{apply_snapshot, resolve_update, set_pin, UpdateError, UpdateTarget};
use super::verify::HealthProbe;
use crate::registry;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

pub trait EngineOps {
    fn proxy_health(&self, port: u16) -> Option<Value>;
    fn spawn_proxy(
        &mut self,
        port: u16,
        mode: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<(), String>;
    fn listening_pids(&self, port: u16) -> Vec<u32>;
    fn terminate(&mut self, pid: u32) -> Result<(), String>;
    fn sleep(&mut self, millis: u64);
    fn installed_version(&self) -> Option<String>;
    fn latest_version(&self, package: &str) -> Option<String>;
    /// Installs an exact requirement; returns (version before, version after).
    fn install(&mut self, requirement: &str) -> Result<(Option<String>, Option<String>), String>;
    fn agent_savings(&self, profile: &str) -> Result<Vec<(String, String)>, String>;
    /// Runs one engine subcommand; `Ok` carries the last output line, `Err` the failure.
    fn run_headroom(&mut self, _args: &[&str]) -> Result<String, String> {
        Err("headroom not on PATH".into())
    }
}

fn is_ready(health: &Option<Value>) -> bool {
    health
        .as_ref()
        .and_then(|h| h.get("ready"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

const POLL_MS: u64 = 200;

/// Reuses a healthy proxy (mode is a cold-start setting, so a running one is kept as is);
/// otherwise spawns one and polls `/health` every [`POLL_MS`] for up to `wait` seconds.
pub fn proxy_up(
    ops: &mut dyn EngineOps,
    port: u16,
    env: &OrderedMap<String>,
    wait: u64,
) -> Result<String, String> {
    if is_ready(&ops.proxy_health(port)) {
        return Ok(format!("reusing proxy on :{port}"));
    }
    let mode = env
        .get("HEADROOM_MODE")
        .cloned()
        .unwrap_or_else(|| "cache".into());
    let env: BTreeMap<String, String> = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    ops.spawn_proxy(port, &mode, &env)?;
    for _ in 0..wait.saturating_mul(1000 / POLL_MS) {
        ops.sleep(POLL_MS);
        if is_ready(&ops.proxy_health(port)) {
            return Ok(format!("started proxy on :{port} (mode={mode})"));
        }
    }
    Err(format!("proxy on :{port} did not become ready in {wait}s"))
}

/// Prefers the pid the proxy reports about itself, then falls back to whatever listens.
pub fn proxy_down(ops: &mut dyn EngineOps, port: u16) -> Result<String, String> {
    let reported = ops
        .proxy_health(port)
        .and_then(|h| h.get("config")?.get("pid")?.as_u64())
        .map(|p| p as u32);
    let pids = match reported {
        Some(pid) => vec![pid],
        None => ops.listening_pids(port),
    };
    if pids.is_empty() {
        return Err(format!("no proxy listening on :{port}"));
    }
    let stopped: Vec<String> = pids
        .iter()
        .filter(|pid| ops.terminate(**pid).is_ok())
        .map(u32::to_string)
        .collect();
    if stopped.is_empty() {
        return Err(format!("could not stop pid(s) {pids:?}"));
    }
    Ok(format!("stopped proxy on :{port} ({})", stopped.join(", ")))
}

pub fn resolve_target(
    config: &CompressionConfig,
    ops: &dyn EngineOps,
    to: Option<&str>,
    latest: bool,
) -> Result<UpdateTarget, UpdateError> {
    resolve_update(config, to, latest, || {
        ops.latest_version(&config.provider_version.package)
    })
}

pub fn set_pin_and_save(
    paths: &super::store::Paths,
    config: &mut CompressionConfig,
    target: &str,
) -> Result<(), String> {
    set_pin(config, target);
    super::store::save(paths, config)
}

#[derive(Debug, PartialEq, Eq)]
pub struct UpdateReport {
    pub install_detail: String,
    pub version_changed: bool,
    pub snapshot_notes: Vec<String>,
}

/// Moves the pin, installs the exact requirement, then re-snapshots every preset that has
/// a savings profile. The pin is policy and the install its consequence, so a failed install
/// still leaves the new pin in `config` for the caller to persist.
pub fn apply_update(
    config: &mut CompressionConfig,
    target: &str,
    ops: &mut dyn EngineOps,
) -> Result<UpdateReport, String> {
    set_pin(config, target);
    let (before, after) = ops.install(&config.provider_version.requirement())?;
    let install_detail = if before == after {
        format!("already at {}", after.as_deref().unwrap_or("?"))
    } else {
        format!(
            "{} -> {}",
            before.as_deref().unwrap_or("none"),
            after.as_deref().unwrap_or("?")
        )
    };
    let live = after.as_deref();
    let mut notes = Vec::new();
    for (name, preset) in config.presets.iter_mut() {
        let Some(profile) = preset.savings_profile.clone() else {
            continue;
        };
        match ops.agent_savings(&profile) {
            Ok(env) => {
                let diff = apply_snapshot(preset, &env, live);
                notes.push(format!(
                    "preset '{name}': +{} -{} ~{} (kept {} policy)",
                    diff.added.len(),
                    diff.removed.len(),
                    diff.moved.len(),
                    diff.kept.len()
                ));
            }
            Err(why) => notes.push(format!("preset '{name}': snapshot not refreshed ({why})")),
        }
    }
    Ok(UpdateReport {
        install_detail,
        version_changed: before != after,
        snapshot_notes: notes,
    })
}

fn path_lookup(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let owned;
    let path = match path {
        Some(p) => p,
        None => {
            owned = std::env::var_os("PATH")?;
            owned.as_os_str()
        }
    };
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

pub(crate) fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

pub(super) fn health_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/health")
}

pub(super) fn http_json(url: &str, secs: u64) -> Option<Value> {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(secs))
        .build()
        .get(url)
        .call()
        .ok()?
        .into_json()
        .ok()
}

impl HealthProbe for SystemOps {
    fn binary(&self, name: &str) -> Option<String> {
        path_lookup(name, self.path.as_deref()).map(|p| p.to_string_lossy().into_owned())
    }

    fn installed_version(&self) -> Option<String> {
        EngineOps::installed_version(self)
    }

    fn proxy_health(&self, port: u16) -> Option<Value> {
        EngineOps::proxy_health(self, port)
    }
}

impl EngineOps for SystemOps {
    fn proxy_health(&self, port: u16) -> Option<Value> {
        http_json(&health_url(port), 3)
    }

    fn spawn_proxy(
        &mut self,
        port: u16,
        mode: &str,
        env: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if path_lookup("headroom", self.path.as_deref()).is_none() {
            return Err("headroom not on PATH".into());
        }
        let log_dir = dirs::home_dir()
            .ok_or_else(|| "home directory unknown".to_string())?
            .join(".headroom")
            .join("logs");
        std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
        let log = registry::open_append_private(&log_dir.join(format!("proxy.{port}.out")))
            .map_err(|e| e.to_string())?;
        let err_log = log.try_clone().map_err(|e| e.to_string())?;
        let mut cmd = self.command("headroom");
        cmd.args(["proxy", "--port", &port.to_string(), "--mode", mode])
            .envs(env)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(err_log);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.spawn()
            .map(drop)
            .map_err(|e| format!("spawn failed ({e})"))
    }

    fn listening_pids(&self, port: u16) -> Vec<u32> {
        self.command("lsof")
            .args(["-nP", &format!("-tiTCP:{port}"), "-sTCP:LISTEN"])
            .stdin(Stdio::null())
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .filter_map(|p| p.parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn terminate(&mut self, pid: u32) -> Result<(), String> {
        #[cfg(unix)]
        {
            // SAFETY: kill(2) with a pid and SIGTERM has no memory-safety preconditions.
            if unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error().to_string())
            }
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            Err("stopping a proxy is not supported on this platform".into())
        }
    }

    fn sleep(&mut self, millis: u64) {
        std::thread::sleep(Duration::from_millis(millis));
    }

    fn installed_version(&self) -> Option<String> {
        super::launch::Probe::headroom_version(self)
    }

    fn latest_version(&self, package: &str) -> Option<String> {
        let body = http_json(&format!("https://pypi.org/pypi/{package}/json"), 10)?;
        body.get("info")?
            .get("version")?
            .as_str()
            .map(str::to_string)
    }

    fn install(&mut self, requirement: &str) -> Result<(Option<String>, Option<String>), String> {
        let before = EngineOps::installed_version(self);
        if path_lookup("uv", self.path.as_deref()).is_none() {
            return Err("uv not on PATH: install the engine manually".into());
        }
        let out = self
            .command("uv")
            .args(["tool", "install", requirement, "--force"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("install failed ({e})"))?;
        if !out.status.success() {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            );
            return Err(text
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("uv tool install failed")
                .to_string());
        }
        Ok((before, EngineOps::installed_version(self)))
    }

    fn agent_savings(&self, profile: &str) -> Result<Vec<(String, String)>, String> {
        if path_lookup("headroom", self.path.as_deref()).is_none() {
            return Err("headroom is not on PATH: cannot snapshot profile env. \
                        Install the pinned build: toolportctl compression pin --install"
                .into());
        }
        let out = self
            .command("headroom")
            .args(["agent-savings", "--profile", profile, "--format", "json"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let text = String::from_utf8_lossy(&out.stderr).into_owned();
            let tail = text.lines().last().unwrap_or("(no output)");
            return Err(format!("agent-savings --profile {profile}: {tail}"));
        }
        let map: OrderedMap<Value> =
            serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        Ok(map
            .iter()
            .map(|(k, v)| match v {
                Value::String(s) => (k.clone(), s.clone()),
                other => (k.clone(), other.to_string()),
            })
            .collect())
    }

    fn run_headroom(&mut self, args: &[&str]) -> Result<String, String> {
        if path_lookup("headroom", self.path.as_deref()).is_none() {
            return Err("headroom not on PATH".into());
        }
        let out = self
            .command("headroom")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        let text = match String::from_utf8_lossy(&out.stdout).trim() {
            "" => String::from_utf8_lossy(&out.stderr).trim().to_string(),
            stdout => stdout.to_string(),
        };
        let last = text.lines().last().unwrap_or("ok").to_string();
        if out.status.success() {
            Ok(last)
        } else {
            Err(last)
        }
    }
}
