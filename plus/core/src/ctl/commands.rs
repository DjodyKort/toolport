use serde_json::Value;
use toolport_plus_api::Host;

use super::output::{no_args, CtlError, Output};
use crate::status::{self, snapshot};

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let snap = snapshot(host);
    let data = status::status(host, &snap);
    let text = |value: &Value, fallback: &str| value.as_str().unwrap_or(fallback).to_string();
    let registry = match (&snap.registry, &snap.registry_error) {
        (Some(_), _) => text(&data["registry"]["path"], ""),
        (None, Some(error)) => format!("error: {error}"),
        (None, None) => "missing (first run)".to_string(),
    };
    let human = format!(
        "Toolport+ {}\nData dir:        {}\nRegistry:        {}\nServers:         {}\nAccess sets:     {}\nDefault access:  {}\nSecrets backend: {}\nGateway binary:  {}",
        text(&data["version"], ""),
        text(&data["dataDir"], "(unresolved)"),
        registry,
        data["serverCount"],
        data["accessSetCount"],
        text(&data["defaultAccess"], "all enabled servers"),
        text(&data["secretsBackend"], ""),
        text(&data["gateway"]["path"], "not found"),
    );
    Ok(Output::new(data, human))
}

pub fn doctor(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    no_args(rest)?;
    let report = status::doctor(host, &snapshot(host));
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
