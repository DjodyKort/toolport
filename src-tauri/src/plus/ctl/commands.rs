use super::output::{CtlError, Output};
use crate::registry::{self, Registry};
use serde_json::{json, Value};
use std::path::PathBuf;

const GATEWAY_NAMES: [&str; 2] = ["toolport-gateway", "conduit-gateway"];

pub(super) struct Snapshot {
    data_dir: Option<PathBuf>,
    registry_path: Option<PathBuf>,
    pub(super) registry: Option<Registry>,
    pub(super) registry_error: Option<String>,
}

fn no_args(rest: &[String]) -> Result<(), CtlError> {
    match rest.first() {
        Some(extra) => Err(CtlError::usage(format!("unexpected argument: {extra}"))),
        None => Ok(()),
    }
}

/// Reads the registry without any of the loader's recovery or migration
/// writes, so inspection never changes the data directory.
pub(super) fn snapshot() -> Snapshot {
    let data_dir = registry::conduit_dir();
    let registry_path = registry::registry_path();
    let mut snap = Snapshot {
        data_dir,
        registry_path: registry_path.clone(),
        registry: None,
        registry_error: None,
    };
    let Some(path) = registry_path else {
        return snap;
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Registry>(&text) {
            Ok(reg) => snap.registry = Some(reg),
            Err(e) => snap.registry_error = Some(format!("registry is not readable: {e}")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => snap.registry_error = Some(format!("cannot read registry: {e}")),
    }
    snap
}

fn secrets_backend() -> &'static str {
    match crate::brand::env_var("TOOLPORT_SECRET_KEY", "CONDUIT_SECRET_KEY") {
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

fn status_data(snap: &Snapshot) -> Value {
    let (servers, profiles, active) = match &snap.registry {
        Some(reg) => (
            reg.servers.len(),
            reg.profiles.len(),
            Some(reg.active_profile_id()),
        ),
        None => (0, 0, None),
    };
    let gateway = gateway_binary(snap.data_dir.as_ref());
    json!({
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
    })
}

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot();
    let data = status_data(&snap);
    let human = format!(
        "Toolport+ {}\nData dir:        {}\nRegistry:        {}\nServers:         {}\nProfiles:        {}\nActive profile:  {}\nSecrets backend: {}\nGateway binary:  {}",
        data["version"].as_str().unwrap_or(""),
        data["dataDir"].as_str().unwrap_or("(unresolved)"),
        match (&snap.registry, &snap.registry_error) {
            (Some(_), _) => data["registry"]["path"].as_str().unwrap_or("").to_string(),
            (None, Some(e)) => format!("error: {e}"),
            (None, None) => "missing (first run)".to_string(),
        },
        data["serverCount"],
        data["profileCount"],
        data["activeProfile"].as_str().unwrap_or("-"),
        data["secretsBackend"].as_str().unwrap_or(""),
        data["gateway"]["path"].as_str().unwrap_or("not found"),
    );
    Ok(Output::new(data, human))
}

fn check(name: &str, status: &str, detail: String) -> Value {
    json!({"name": name, "status": status, "detail": detail})
}

pub fn doctor(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot();
    let mut checks = Vec::new();

    checks.push(match &snap.data_dir {
        Some(dir) if !dir.exists() => check(
            "dataDir",
            "warn",
            format!("{} does not exist yet", dir.display()),
        ),
        Some(dir) if data_dir_writable(dir) => {
            check("dataDir", "ok", format!("{} is writable", dir.display()))
        }
        Some(dir) => check(
            "dataDir",
            "fail",
            format!("{} is not writable", dir.display()),
        ),
        None => check(
            "dataDir",
            "fail",
            "data directory could not be resolved".into(),
        ),
    });
    checks.push(match (&snap.registry, &snap.registry_error) {
        (Some(reg), _) => check(
            "registry",
            "ok",
            format!(
                "{} servers, {} profiles",
                reg.servers.len(),
                reg.profiles.len()
            ),
        ),
        (None, Some(e)) => check("registry", "fail", e.clone()),
        (None, None) => check("registry", "warn", "no registry file yet".into()),
    });
    checks.push(match &snap.registry {
        Some(reg) => {
            let active = reg.active_profile_id();
            if reg.profiles.iter().any(|p| p.id == active) {
                check("activeProfile", "ok", active)
            } else {
                check(
                    "activeProfile",
                    "warn",
                    format!("{active} is not a defined profile"),
                )
            }
        }
        None => check("activeProfile", "warn", "no registry to inspect".into()),
    });
    checks.push(check("secretsBackend", "ok", secrets_backend().to_string()));
    checks.push(match gateway_binary(snap.data_dir.as_ref()) {
        Some(path) => check("gatewayBinary", "ok", path.display().to_string()),
        None => check("gatewayBinary", "warn", "toolport-gateway not found".into()),
    });

    let failed = checks.iter().any(|c| c["status"] == "fail");
    let human = checks
        .iter()
        .map(|c| {
            format!(
                "[{}] {}: {}",
                c["status"].as_str().unwrap_or(""),
                c["name"].as_str().unwrap_or(""),
                c["detail"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Output::new(json!({"healthy": !failed, "checks": checks}), human);
    output.failed = failed;
    Ok(output)
}

pub fn server_ls(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot();
    if let Some(error) = snap.registry_error {
        return Err(CtlError::new("registry_error", error));
    }
    let reg = snap.registry.unwrap_or_default();
    let active = reg.active_profile_id();
    let servers: Vec<Value> = reg
        .servers
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "transport": s.transport,
                "enabled": reg.is_enabled(&active, &s.id),
            })
        })
        .collect();
    let human = if servers.is_empty() {
        "No servers.".to_string()
    } else {
        reg.servers
            .iter()
            .map(|s| {
                format!(
                    "{:<24} {:<6} {:<9} {}",
                    s.name,
                    s.transport,
                    if reg.is_enabled(&active, &s.id) {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    s.id
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(Output::new(
        json!({"activeProfile": active, "servers": servers}),
        human,
    ))
}
