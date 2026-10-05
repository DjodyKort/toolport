//! `tasks_run` (self-MCP tier 2): every run asks the person through the approval broker first. A
//! denial, a timeout and a missing broker run nothing; the answer never carries a captured value.

use super::api;
use super::host::Host;
use super::store;
use crate::approval::{new_correlation_id, ApprovalDecision, ApprovalReason, ApprovalRequest};
use crate::plus::op::OpError;
use serde_json::{json, Value};

pub const TRIGGER: &str = "selfMcp";

pub type Decide<'a> = &'a dyn Fn(ApprovalRequest) -> ApprovalDecision;

fn request(id: &str, plan: &Value, data: &Value) -> ApprovalRequest {
    let steps: Vec<&str> = plan["steps"].as_array().into_iter().flatten().filter_map(|s| s["detail"].as_str()).collect();
    ApprovalRequest {
        token: String::new(),
        id: new_correlation_id(),
        client: Some("self-mcp".into()),
        server: "toolport".into(),
        tool: "tasks_run".into(),
        reason: ApprovalReason::Destructive,
        arguments: json!({"task": id, "steps": steps, "writesSecrets": data["writesSecrets"], "requires": data["requires"]}),
        tool_fingerprint: None,
        url_elicitation: None,
        pii_release: None,
        agent_rule: None,
    }
}

fn refusal(decision: ApprovalDecision) -> OpError {
    match decision {
        ApprovalDecision::Unreachable => OpError::failed("approval_unavailable", "no approval broker is running (open the Toolport app); nothing was started"),
        ApprovalDecision::Timeout => OpError::failed("approval_denied", "nobody approved the run in time; nothing was started"),
        ApprovalDecision::StaleState => OpError::failed("approval_denied", "the approval no longer matches the run; nothing was started"),
        _ => OpError::failed("approval_denied", "the run was denied; nothing was started"),
    }
}

pub fn run(id: &str, dry_run: bool, host: &dyn Host, decide: Decide) -> Result<Value, OpError> {
    let preview = api::run(id, TRIGGER, true, host)?;
    if dry_run {
        return Ok(preview);
    }
    let task = store::load_task(id)?;
    super::triggers::allowed(&task, TRIGGER).map_err(OpError::conflict)?;
    let decision = decide(request(id, &preview["plan"], &preview));
    if !decision.is_approved() {
        return Err(refusal(decision));
    }
    api::run(id, TRIGGER, false, host)
}
