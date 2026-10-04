//! `context_measure`: the same measurement as `toolportctl context measure`, started from an
//! agent. Calling a tier 2 tool is the approval, so the requests it makes need no extra yes.

use super::ToolError;
use crate::plus::args::{flag, nonempty_strings, str_nonempty};
use crate::plus::context::measure::{measure, Env, MeasureError, ProcessLauncher, Request};
use serde_json::Value;
use std::path::PathBuf;

pub(super) fn measure_tool(args: &Value) -> Result<Value, ToolError> {
    let cwd = str_nonempty(args, "cwd")
        .map(PathBuf::from)
        .ok_or_else(|| ToolError::new("invalid_arguments", "cwd is required"))?;
    let (roots, config) = crate::plus::sources::host_world()
        .ok_or_else(|| ToolError::new("not_found", "home directory unknown"))?;
    let launcher = ProcessLauncher::from_env();
    let data_dir = crate::registry::conduit_dir();
    let env = Env {
        roots: &roots,
        config: &config,
        data_dir: data_dir.as_deref(),
        launcher: &launcher,
    };
    let request = Request {
        cwd,
        without: nonempty_strings(args, "without").unwrap_or_default(),
        bundle: str_nonempty(args, "bundle").map(String::from),
        model: str_nonempty(args, "model").map(String::from),
        force: flag(args, "force"),
    };
    let report = measure(&env, &request, &mut |_| true).map_err(|e| match e {
        MeasureError::Usage(message) => ToolError::new("invalid_arguments", message),
        MeasureError::Failed { code: "bundle_unreadable", message } => {
            ToolError::new("not_found", message)
        }
        MeasureError::Failed { code, message } => ToolError::backend(format!("{code}: {message}")),
        MeasureError::Declined(_) => ToolError::new("refused", "the measurement was not approved"),
    })?;
    serde_json::to_value(&report).map_err(|e| ToolError::backend(e.to_string()))
}
