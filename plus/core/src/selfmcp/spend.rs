//! `spend_status`, `spend_receiver_status`, `spend_summary`, `spend_export_csv` and
//! `spend_setup_plan` (tier 1), `spend_config_set`, `spend_setup_apply` and `spend_setup_undo`
//! (tier 3) and `spend_prune` (tier 4), all preview first: the same operations as
//! `toolportctl spend`, over `crate::spend::api`. Overwriting a key another tool set, writing a
//! file to a path and deleting the daily totals stay with the command line and the Spend page.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use serde_json::json;

use crate::args::{flag_or, str_trimmed};
use crate::spend::api;
use crate::spend::report::Filter;

pub(super) fn status(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(api::status(host)?)
}

pub(super) fn summary(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let filter = Filter {
        project: str_trimmed(args, "project"),
        model: str_trimmed(args, "model"),
        task: str_trimmed(args, "task"),
    };
    Ok(api::summary(host, str_trimmed(args, "range"), &filter)?)
}

pub(super) fn setup_plan(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::setup_plan(
        host,
        &api::SetupChoice {
            port: str_trimmed(args, "port").map(str::to_string),
            ..api::SetupChoice::default()
        },
    )?)
}

pub(super) fn receiver_status(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(api::receiver_status(host)?)
}

pub(super) fn export_csv(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let (csv, rows) = api::export_csv(host, str_trimmed(args, "range"))?;
    Ok(json!({"rows": rows, "csv": csv}))
}

fn number(args: &Value, key: &str) -> Result<Option<String>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(n) => Ok(Some(n.to_string())),
            None => Err(ToolError::new(
                "invalid_arguments",
                format!("{key} must be a whole number"),
            )),
        },
    }
}

pub(super) fn config_set(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let port = str_trimmed(args, "port").map(str::to_string);
    let retention_days = number(args, "retention_days")?;
    let max_mb = number(args, "max_mb")?;
    let show_cost =
        args.get("show_cost")
            .and_then(Value::as_bool)
            .map(|on| if on { "on" } else { "off" });
    if port.is_none() && retention_days.is_none() && max_mb.is_none() && show_cost.is_none() {
        return Err(ToolError::new(
            "invalid_arguments",
            "nothing to set: pass port, retention_days, max_mb or show_cost",
        ));
    }
    Ok(api::config_edit(
        host,
        &api::ConfigEdit {
            port: port.as_deref(),
            retention_days: retention_days.as_deref(),
            max_mb: max_mb.as_deref(),
            show_cost,
        },
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn setup_apply(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::setup_apply(
        host,
        &api::SetupChoice {
            port: str_trimmed(args, "port").map(str::to_string),
            ..api::SetupChoice::default()
        },
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn setup_undo(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::setup_undo(
        host,
        &[],
        false,
        flag_or(args, "dry_run", true),
    )?)
}

pub(super) fn prune(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let dry_run = flag_or(args, "dry_run", true);
    let mut report = api::prune(host, dry_run, false)?;
    report["summary"] = json!(format!(
        "{} {} events ({} bytes) of {} days; the daily totals of those days stay, {} events are kept",
        if dry_run { "Would remove" } else { "Removed" },
        report["removedEvents"],
        report["removedBytes"],
        report["foldedDays"],
        report["keptEvents"],
    ));
    Ok(report)
}
