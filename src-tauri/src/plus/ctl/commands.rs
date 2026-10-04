use super::output::{no_args, CtlError, Output};
use crate::plus::status::{self, snapshot};
use serde_json::{json, Value};

pub fn status(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot();
    let data = status::status(&snap);
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
    let human = match data["directEntries"].as_u64() {
        Some(count) if count > 0 => format!("{human}\nDirect entries:  {count} (client direct ls)"),
        _ => human,
    };
    Ok(Output::new(data, human))
}

pub fn doctor(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let report = status::doctor(&snapshot());
    let human = report
        .checks
        .iter()
        .map(status::Check::line)
        .collect::<Vec<_>>()
        .join("\n");
    let mut output = Output::new(report.to_value(), human);
    output.failed = !report.healthy;
    Ok(output)
}

pub fn server_ls(rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot();
    if let Some(error) = snap.registry_error {
        return Err(CtlError::failed("registry_error", error));
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
