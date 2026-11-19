//! The person's yes for one call of a tool that must never run on the model's say-so alone. It
//! goes through the same broker as `tasks_run`, which never remembers an answer; a denial, a
//! timeout and a missing broker change nothing.

use serde_json::Value;
use toolport_plus_api::{Host, RunApproval, RunDecision};

use super::ToolError;

pub(super) fn ask(
    host: &dyn Host,
    label: &str,
    steps: Vec<String>,
    writes_secrets: Value,
) -> Result<(), ToolError> {
    let decision = host.task_approve_run(&RunApproval {
        task: label.into(),
        steps,
        writes_secrets,
        requires: serde_json::json!({}),
    });
    match decision {
        RunDecision::Approved => Ok(()),
        RunDecision::Unreachable => Err(ToolError::new(
            "approval_unavailable",
            "no approval broker is running (open the Toolport app); nothing was changed",
        )),
        RunDecision::Timeout => Err(ToolError::new(
            "approval_denied",
            "nobody approved the call in time; nothing was changed",
        )),
        RunDecision::Stale => Err(ToolError::new(
            "approval_denied",
            "the approval no longer matches the call; nothing was changed",
        )),
        _ => Err(ToolError::new(
            "approval_denied",
            "the call was denied; nothing was changed",
        )),
    }
}
