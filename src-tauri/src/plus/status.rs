//! What `toolportctl status`, `toolportctl doctor` and the `where_am_i` and `doctor` self-MCP
//! tools report: a read-only [`Snapshot`] of the data directory and registry, and the JSON built
//! from it. Each surface only renders the result.

use crate::plus::gateway_build;
use crate::plus::health::Health;
use crate::plus::op::OpError;
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

pub(crate) fn readable_registry() -> Result<Registry, OpError> {
    let snap = snapshot();
    match snap.registry_error {
        Some(error) => Err(OpError::failed("registry_error", error)),
        None => Ok(snap.registry.unwrap_or_default()),
    }
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
    let builds = snap
        .data_dir
        .as_deref()
        .map(gateway_build::read_live)
        .unwrap_or_default();
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
            "build": builds.first().map_or(Value::Null, |build| json!(build)),
            "builds": builds,
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

/// Only when a user-level skills lock exists: the deployed skill files its clients would reject.
fn skills_check(snap: &Snapshot) -> Option<Check> {
    use crate::plus::skills::ops::check_outputs;
    use crate::plus::skills::transpilers::registry_with_home;
    let lock = crate::plus::skills::load_lockfile(snap.data_dir.as_ref()?)?;
    let home = crate::clients::home()?;
    let found = check_outputs(&lock, &registry_with_home(Some(home.clone())), &home);
    if found.rejected.is_empty() {
        return Some(check(
            "skills",
            Health::Ok,
            format!("{} deployed skill files read cleanly", found.checked),
        ));
    }
    let shown: Vec<String> = found
        .rejected
        .iter()
        .take(5)
        .map(|row| format!("{}/{} ({})", row.name, row.client, row.reason.code()))
        .collect();
    let more = found.rejected.len().saturating_sub(shown.len());
    Some(check(
        "skills",
        Health::Warn,
        format!(
            "{} of {} deployed skill files would be rejected by their client and hidden from the model: {}{}; run toolportctl skills sync",
            found.rejected.len(),
            found.checked,
            shown.join(", "),
            if more > 0 { format!(" and {more} more") } else { String::new() }
        ),
    ))
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
    checks.extend(skills_check(snap));

    let healthy = !checks.iter().any(|c| c.status == Health::Fail);
    Doctor { healthy, checks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::gateway_build::BuildTracker;
    use crate::plus::testutil::DataDirFx;

    fn snapshot_of(fx: &DataDirFx) -> Snapshot {
        Snapshot {
            data_dir: Some(fx.dir.clone()),
            registry_path: None,
            registry: None,
            registry_error: None,
        }
    }

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn status_reports_the_progress_of_a_gateway_build_that_is_running() {
        let fx = DataDirFx::new("status-build", "running").with_secret_key(&"ab".repeat(32));
        let tracker = BuildTracker::new("daemon", Some(fx.dir.clone()));
        tracker.plan(&ids(&["a", "b", "c"]));
        tracker.server_connected("a", 4, 4);
        tracker.server_failed("b");

        let data = status(&snapshot_of(&fx));
        let build = &data["gateway"]["build"];
        assert_eq!(build["building"], true);
        assert_eq!(build["role"], "daemon");
        assert_eq!(build["pid"], std::process::id());
        assert_eq!(build["serversTotal"], 3);
        assert_eq!(build["serversConnected"], 1);
        assert_eq!(build["serversFailed"], 1);
        assert_eq!(build["toolsSoFar"], 4);
        assert_eq!(build["servers"][2]["state"], "connecting");
        assert_eq!(data["gateway"]["builds"].as_array().unwrap().len(), 1);

        tracker.server_connected("c", 2, 6);
        tracker.finish(6);
        let done = status(&snapshot_of(&fx));
        assert_eq!(done["gateway"]["build"]["building"], false);
        assert_eq!(done["gateway"]["build"]["serversConnected"], 2);
        assert_eq!(done["gateway"]["build"]["toolsSoFar"], 6);
    }

    #[test]
    fn status_has_no_build_when_no_gateway_is_running() {
        let fx = DataDirFx::new("status-build", "none").with_secret_key(&"ab".repeat(32));
        let data = status(&snapshot_of(&fx));
        assert!(data["gateway"]["build"].is_null());
        assert_eq!(data["gateway"]["builds"], json!([]));
        assert!(data["gateway"]["present"].is_boolean());
    }
}
