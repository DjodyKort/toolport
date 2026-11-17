//! `tasks_run` (self-MCP tier 2): every run asks the person through the approval broker first. A
//! denial, a timeout and a missing broker run nothing; the answer never carries a captured value.

use super::{api, store, triggers};
use crate::op::OpError;
use serde_json::Value;
use toolport_plus_api::{RunApproval, RunDecision, TaskHost};

pub const TRIGGER: &str = "selfMcp";

fn request(id: &str, plan: &Value, data: &Value) -> RunApproval {
    RunApproval {
        task: id.into(),
        steps: plan["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["detail"].as_str().map(String::from))
            .collect(),
        writes_secrets: data["writesSecrets"].clone(),
        requires: data["requires"].clone(),
    }
}

fn refusal(decision: RunDecision) -> OpError {
    match decision {
        RunDecision::Unreachable => OpError::failed(
            "approval_unavailable",
            "no approval broker is running (open the Toolport app); nothing was started",
        ),
        RunDecision::Timeout => OpError::failed(
            "approval_denied",
            "nobody approved the run in time; nothing was started",
        ),
        RunDecision::Stale => OpError::failed(
            "approval_denied",
            "the approval no longer matches the run; nothing was started",
        ),
        _ => OpError::failed("approval_denied", "the run was denied; nothing was started"),
    }
}

pub fn run(id: &str, dry_run: bool, host: &dyn TaskHost) -> Result<Value, OpError> {
    let preview = api::run(id, TRIGGER, true, host)?;
    if dry_run {
        return Ok(preview);
    }
    let base = triggers::base_of(host)?;
    let task = store::load_task(&base, id)?;
    triggers::allowed(&task, TRIGGER).map_err(OpError::conflict)?;
    let decision = host.task_approve_run(&request(id, &preview["plan"], &preview));
    if !decision.is_approved() {
        return Err(refusal(decision));
    }
    api::run(id, TRIGGER, false, host)
}
