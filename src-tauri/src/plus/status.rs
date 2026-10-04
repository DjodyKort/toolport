//! What `toolportctl status`, `toolportctl doctor` and the `where_am_i` and `doctor` self-MCP
//! tools report: a read-only [`Snapshot`] of the data directory and registry, and the JSON built
//! from it. Each surface only renders the result.

use crate::plus::health::Health;
use crate::plus::registry_ro;
use crate::registry::{self, Registry};
use serde_json::{json, Value};
use std::path::PathBuf;

const GATEWAY_NAMES: [&str; 2] = ["toolport-gateway", "conduit-gateway"];

pub(crate) struct Snapshot {
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) registry_path: Option<PathBuf>,
    pub(crate) registry: Option<Registry>,
    pub(crate) registry_error: Option<String>,
}

/// Reads the registry without any of the loader's recovery or migration
/// writes, so inspection never changes the data directory.
pub(crate) fn snapshot() -> Snapshot {
    let data_dir = registry::conduit_dir();
    let registry_path = registry::resolved_path();
    let mut snap = Snapshot {
        data_dir,
        registry_path: registry_path.clone(),
        registry: None,
        registry_error: None,
    };
    let Some(path) = registry_path else {
        return snap;
    };
    match registry_ro::read_at(&path) {
        Ok(reg) => snap.registry = reg,
        Err(e) => snap.registry_error = Some(e),
    }
    snap
}

fn secrets_backend() -> &'static str {
    match crate::brand::env_var(crate::brand::SECRET_KEY, crate::brand::SECRET_KEY_LEGACY) {
        Some(_) => "encrypted-file",
        None => "os-keychain",
    }
}

fn gateway_binary(data_dir: Option<&PathBuf>) -> Option<PathBuf> {
    let ext = std::env::consts::EXE_SUFFIX;
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    if let Some(dir) = data_dir {
        dirs.push(dir.join("bin"));
    }
    for dir in dirs {
        for name in GATEWAY_NAMES {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn data_dir_writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".toolportctl-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn path_str(path: &Option<PathBuf>) -> Value {
    match path {
        Some(p) => json!(p.to_string_lossy()),
        None => Value::Null,
    }
}

fn auth_summary() -> Value {
    use crate::plus::auth::surfaces;
    use crate::plus::auth::{Clock, SystemClock};
    match surfaces::auth_dir() {
        Some(dir) => surfaces::status_summary(&surfaces::read_status(&dir), SystemClock.now()),
        None => surfaces::status_summary(&Default::default(), SystemClock.now()),
    }
}

pub(crate) fn status(snap: &Snapshot) -> Value {
    let (servers, profiles, active) = match &snap.registry {
        Some(reg) => (
            reg.servers.len(),
            reg.profiles.len(),
            Some(reg.active_profile_id()),
        ),
        None => (0, 0, None),
    };
    let gateway = gateway_binary(snap.data_dir.as_ref());
    let direct_entries = snap.registry.as_ref().map_or(0, crate::plus::direct::count);
    let mut data = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "dataDir": path_str(&snap.data_dir),
        "registry": {
            "path": path_str(&snap.registry_path),
            "exists": snap.registry.is_some() || snap.registry_error.is_some(),
            "readable": snap.registry.is_some(),
            "error": snap.registry_error,
        },
        "serverCount": servers,
        "profileCount": profiles,
        "activeProfile": active,
        "secretsBackend": secrets_backend(),
        "auth": auth_summary(),
        "gateway": {
            "present": gateway.is_some(),
            "path": path_str(&gateway),
        },
    });
    if direct_entries > 0 {
        data["directEntries"] = json!(direct_entries);
    }
    data
}

pub(crate) struct Check {
    pub(crate) name: &'static str,
    pub(crate) status: Health,
    pub(crate) detail: String,
}

impl Check {
    pub(crate) fn line(&self) -> String {
        format!("[{}] {}: {}", self.status.as_str(), self.name, self.detail)
    }
}

pub(crate) struct Doctor {
    pub(crate) healthy: bool,
    pub(crate) checks: Vec<Check>,
}

impl Doctor {
    pub(crate) fn to_value(&self) -> Value {
        let rows: Vec<Value> = self
            .checks
            .iter()
            .map(|c| json!({"name": c.name, "status": c.status, "detail": c.detail}))
            .collect();
        json!({"healthy": self.healthy, "checks": rows})
    }
}

fn check(name: &'static str, status: Health, detail: String) -> Check {
    Check {
        name,
        status,
        detail,
    }
}

fn direct_check(reg: &crate::registry::Registry) -> Check {
    use crate::plus::direct;
    let rows = direct::assess(reg, &crate::clients::detect_clients());
    let bad: Vec<String> = rows
        .iter()
        .filter(|row| !row.state.healthy())
        .map(|row| format!("{}/{} {}", row.client, row.entry, row.state.as_str()))
        .collect();
    if bad.is_empty() {
        check(
            "directEntries",
            Health::Ok,
            format!("{} direct launcher entries", rows.len()),
        )
    } else {
        check(
            "directEntries",
            Health::Warn,
            format!("{} of {} need attention: {}", bad.len(), rows.len(), bad.join(", ")),
        )
    }
}

pub(crate) fn doctor(snap: &Snapshot) -> Doctor {
    let mut checks = Vec::new();

    checks.push(match &snap.data_dir {
        Some(dir) if !dir.exists() => check(
            "dataDir",
            Health::Warn,
            format!("{} does not exist yet", dir.display()),
        ),
        Some(dir) if data_dir_writable(dir) => check(
            "dataDir",
            Health::Ok,
            format!("{} is writable", dir.display()),
        ),
        Some(dir) => check(
            "dataDir",
            Health::Fail,
            format!("{} is not writable", dir.display()),
        ),
        None => check(
            "dataDir",
            Health::Fail,
            "data directory could not be resolved".into(),
        ),
    });
    checks.push(match (&snap.registry, &snap.registry_error) {
        (Some(reg), _) => check(
            "registry",
            Health::Ok,
            format!(
                "{} servers, {} profiles",
                reg.servers.len(),
                reg.profiles.len()
            ),
        ),
        (None, Some(e)) => check("registry", Health::Fail, e.clone()),
        (None, None) => check("registry", Health::Warn, "no registry file yet".into()),
    });
    checks.push(match &snap.registry {
        Some(reg) => {
            let active = reg.active_profile_id();
            if reg.profiles.iter().any(|p| p.id == active) {
                check("activeProfile", Health::Ok, active)
            } else {
                check(
                    "activeProfile",
                    Health::Warn,
                    format!("{active} is not a defined profile"),
                )
            }
        }
        None => check(
            "activeProfile",
            Health::Warn,
            "no registry to inspect".into(),
        ),
    });
    checks.push(check(
        "secretsBackend",
        Health::Ok,
        secrets_backend().to_string(),
    ));
    checks.push(match gateway_binary(snap.data_dir.as_ref()) {
        Some(path) => check("gatewayBinary", Health::Ok, path.display().to_string()),
        None => check(
            "gatewayBinary",
            Health::Warn,
            "toolport-gateway not found".into(),
        ),
    });
    if let Some(reg) = snap.registry.as_ref().filter(|r| crate::plus::direct::count(r) > 0) {
        checks.push(direct_check(reg));
    }

    let healthy = !checks.iter().any(|c| c.status == Health::Fail);
    Doctor { healthy, checks }
}
