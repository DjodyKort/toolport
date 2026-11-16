//! What `toolportctl status`, `toolportctl doctor` and the `where_am_i` and `doctor` self-MCP
//! tools report: a read-only [`Snapshot`] of the data directory and registry, and the JSON built
//! from it. Each surface only renders the result.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toolport_plus_api::{Host, RegistryView, SEAM_MARKER};

use crate::health::Health;
use crate::login_keychain;

const GATEWAY_NAMES: [&str; 2] = ["toolport-gateway", "conduit-gateway"];

pub struct Snapshot {
    pub data_dir: Option<PathBuf>,
    pub registry_path: Option<PathBuf>,
    pub registry: Option<Box<dyn RegistryView>>,
    pub registry_error: Option<String>,
}

/// Reads the registry without any of the loader's recovery or migration writes, so inspection
/// never changes the data directory.
pub fn snapshot(host: &dyn Host) -> Snapshot {
    let mut snap = Snapshot {
        data_dir: host.data_dir(),
        registry_path: host.registry_path(),
        registry: None,
        registry_error: None,
    };
    match host.read_view() {
        Ok(view) => snap.registry = view,
        Err(error) => snap.registry_error = Some(error),
    }
    snap
}

pub fn secrets_backend(host: &dyn Host) -> &'static str {
    let has_key = ["TOOLPORT_SECRET_KEY", "CONDUIT_SECRET_KEY"]
        .iter()
        .any(|name| host.env_var(name).is_some());
    if has_key {
        "encrypted-file"
    } else if login_keychain::enabled(host) {
        "login-keychain"
    } else {
        "os-keychain"
    }
}

pub fn gateway_binary(data_dir: Option<&Path>) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    gateway_binary_for(exe.as_deref(), data_dir)
}

fn gateway_binary_for(exe: Option<&Path>, data_dir: Option<&Path>) -> Option<PathBuf> {
    let suffix = std::env::consts::EXE_SUFFIX;
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(exe) = exe {
        let resolved = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
        dirs.extend(resolved.parent().map(Path::to_path_buf));
    }
    if let Some(dir) = data_dir {
        dirs.push(dir.join("bin"));
    }
    dirs.iter().find_map(|dir| {
        GATEWAY_NAMES
            .iter()
            .map(|name| dir.join(format!("{name}{suffix}")))
            .find(|candidate| candidate.is_file())
    })
}

fn data_dir_writable(dir: &Path) -> bool {
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

/// True when `needle` occurs in the file; reads in blocks, so a large binary stays out of memory.
pub fn file_contains(path: &Path, needle: &[u8]) -> std::io::Result<bool> {
    const BLOCK: usize = 1 << 20;
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0u8; BLOCK + needle.len()];
    let mut carried = 0;
    loop {
        let read = file.read(&mut buffer[carried..carried + BLOCK])?;
        if read == 0 {
            return Ok(false);
        }
        let filled = carried + read;
        if buffer[..filled]
            .windows(needle.len())
            .any(|window| window == needle)
        {
            return Ok(true);
        }
        carried = needle.len().saturating_sub(1).min(filled);
        buffer.copy_within(filled - carried..filled, 0);
    }
}

pub fn status(host: &dyn Host, snap: &Snapshot) -> Value {
    let (servers, access_sets, default_access) = match &snap.registry {
        Some(view) => (
            view.servers().map_or(0, |servers| servers.len()),
            view.access_sets().len(),
            view.default_access(),
        ),
        None => (0, 0, None),
    };
    let gateway = gateway_binary(snap.data_dir.as_deref());
    json!({
        "version": host.app_version(),
        "dataDir": path_str(&snap.data_dir),
        "registry": {
            "path": path_str(&snap.registry_path),
            "exists": snap.registry.is_some() || snap.registry_error.is_some(),
            "readable": snap.registry.is_some(),
            "error": snap.registry_error,
        },
        "serverCount": servers,
        "accessSetCount": access_sets,
        "defaultAccess": default_access,
        "secretsBackend": secrets_backend(host),
        "auth": crate::auth::status_section(host),
        "gateway": {
            "present": gateway.is_some(),
            "path": path_str(&gateway),
        },
    })
}

pub struct Check {
    pub name: &'static str,
    pub status: Health,
    pub detail: String,
}

impl Check {
    pub fn line(&self) -> String {
        format!("[{}] {}: {}", self.status.as_str(), self.name, self.detail)
    }
}

pub struct Doctor {
    pub healthy: bool,
    pub checks: Vec<Check>,
}

impl Doctor {
    pub fn to_value(&self) -> Value {
        let rows: Vec<Value> = self
            .checks
            .iter()
            .map(|c| json!({"name": c.name, "status": c.status.as_str(), "detail": c.detail}))
            .collect();
        json!({"healthy": self.healthy, "checks": rows})
    }
}

fn check(name: &'static str, status: Health, detail: impl Into<String>) -> Check {
    Check {
        name,
        status,
        detail: detail.into(),
    }
}

/// Whether this process and the gateway next to it forward upstream code to the fork logic.
fn plus_built_check(host: &dyn Host, gateway: Option<&Path>) -> Check {
    if !host.seam_live() {
        return check(
            "plusBuilt",
            Health::Fail,
            "this binary was built without the fork seam: upstream code does not reach the fork logic",
        );
    }
    let Some(gateway) = gateway else {
        return check(
            "plusBuilt",
            Health::Ok,
            "this binary carries the fork seam; no gateway found to check",
        );
    };
    match file_contains(gateway, SEAM_MARKER.as_bytes()) {
        Ok(true) => check(
            "plusBuilt",
            Health::Ok,
            format!("this binary and {} carry the fork seam", gateway.display()),
        ),
        Ok(false) => check(
            "plusBuilt",
            Health::Warn,
            format!(
                "{} was built without the fork seam: the egress guard and fork hooks are off in it",
                gateway.display()
            ),
        ),
        Err(error) => check(
            "plusBuilt",
            Health::Warn,
            format!("cannot read {}: {error}", gateway.display()),
        ),
    }
}

/// Stdio and Google logins only: a remote server's sign-in state is upstream's, shown in its row.
fn logins_check(host: &dyn Host) -> Check {
    let section = crate::auth::status_section(host);
    let count = |key: &str| section["counts"][key].as_u64().unwrap_or(0);
    let needing: Vec<String> = section["servers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row["fix"].is_object())
        .filter_map(|row| row["server"].as_str().map(String::from))
        .collect();
    let known = count("ok") + needing.len() as u64;
    if !needing.is_empty() {
        return check(
            "logins",
            Health::Warn,
            format!(
                "{} of {known} logins need attention: {}; run `toolportctl auth probe --force`",
                needing.len(),
                needing.join(", ")
            ),
        );
    }
    check(
        "logins",
        Health::Ok,
        if known == 0 {
            "no stdio or Google logins checked yet".to_string()
        } else {
            format!("{known} logins ok")
        },
    )
}

pub fn doctor(host: &dyn Host, snap: &Snapshot) -> Doctor {
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
            "data directory could not be resolved",
        ),
    });
    checks.push(match (&snap.registry, &snap.registry_error) {
        (Some(view), _) => check(
            "registry",
            Health::Ok,
            format!(
                "{} servers, {} access sets",
                view.servers().map_or(0, |servers| servers.len()),
                view.access_sets().len()
            ),
        ),
        (None, Some(error)) => check("registry", Health::Fail, error.clone()),
        (None, None) => check("registry", Health::Warn, "no registry file yet"),
    });
    checks.push(match &snap.registry {
        Some(view) => match view.default_access() {
            Some(id) => check("defaultAccess", Health::Ok, id),
            None => check("defaultAccess", Health::Ok, "all enabled servers"),
        },
        None => check("defaultAccess", Health::Warn, "no registry to inspect"),
    });
    checks.push(check("secretsBackend", Health::Ok, secrets_backend(host)));
    let gateway = gateway_binary(snap.data_dir.as_deref());
    checks.push(match &gateway {
        Some(path) => check("gatewayBinary", Health::Ok, path.display().to_string()),
        None => check("gatewayBinary", Health::Warn, "toolport-gateway not found"),
    });
    checks.push(plus_built_check(host, gateway.as_deref()));
    checks.push(logins_check(host));

    let healthy = !checks.iter().any(|c| c.status == Health::Fail);
    Doctor { healthy, checks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolport_plus_api::{MemoryHost, MemoryRegistry, ServerRecord};

    fn host(dir: &Path) -> MemoryHost {
        let record = |id: &str, enabled: bool| ServerRecord {
            id: id.into(),
            name: id.into(),
            enabled,
            ..Default::default()
        };
        MemoryHost {
            registry: MemoryRegistry::with_servers(vec![record("a", true), record("b", false)]),
            secrets: toolport_plus_api::MemorySecrets {
                data_dir: Some(dir.to_path_buf()),
                ..Default::default()
            },
            seam: true,
            ..Default::default()
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("plus-status-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn status_counts_servers_and_access_sets_from_the_read_view() {
        let dir = scratch("counts");
        let host = host(&dir);
        let data = status(&host, &snapshot(&host));
        assert_eq!(data["serverCount"], 2);
        assert_eq!(data["accessSetCount"], 0);
        assert_eq!(data["defaultAccess"], Value::Null);
        assert_eq!(data["secretsBackend"], "os-keychain");
        assert_eq!(data["registry"]["exists"], true);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_secret_key_in_the_environment_selects_the_encrypted_file() {
        let dir = scratch("backend");
        let host = host(&dir);
        host.secrets.set_env("TOOLPORT_SECRET_KEY", "ab");
        assert_eq!(secrets_backend(&host), "encrypted-file");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn doctor_fails_only_when_the_seam_is_missing_and_warns_without_a_gateway() {
        let dir = scratch("doctor");
        let mut host = host(&dir);
        let report = doctor(&host, &snapshot(&host));
        assert!(report.healthy);
        let plus = report
            .checks
            .iter()
            .find(|c| c.name == "plusBuilt")
            .unwrap();
        assert_eq!(plus.status, Health::Ok);
        host.seam = false;
        let report = doctor(&host, &snapshot(&host));
        assert!(!report.healthy);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_gateway_without_the_marker_is_a_warning_and_with_it_is_ok() {
        let dir = scratch("marker");
        let host = host(&dir);
        let gateway = dir.join("toolport-gateway");
        std::fs::write(&gateway, vec![7u8; 3_000_000]).unwrap();
        assert_eq!(plus_built_check(&host, Some(&gateway)).status, Health::Warn);
        let mut bytes = vec![7u8; 1_048_570];
        bytes.extend_from_slice(SEAM_MARKER.as_bytes());
        bytes.extend_from_slice(&[7u8; 100]);
        std::fs::write(&gateway, bytes).unwrap();
        assert_eq!(plus_built_check(&host, Some(&gateway)).status, Health::Ok);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_marker_is_found_across_a_block_boundary_at_any_offset() {
        let dir = scratch("boundary");
        let file = dir.join("blob");
        for shift in [0usize, 1, 5, 19, 20] {
            let mut bytes = vec![1u8; (1 << 20) - shift];
            bytes.extend_from_slice(b"needle-123456789012345");
            bytes.extend_from_slice(&[1u8; 40]);
            std::fs::write(&file, &bytes).unwrap();
            assert!(
                file_contains(&file, b"needle-123456789012345").unwrap(),
                "{shift}"
            );
        }
        std::fs::write(&file, vec![1u8; 2_000_000]).unwrap();
        assert!(!file_contains(&file, b"needle").unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }
}
