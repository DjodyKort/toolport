//! The policy re-check and human approval of a client's `tasks/*` request: the checks a
//! `tools/call` of the task's tool goes through in `execute_call`, asked again for the request
//! that reads or changes the task.

use super::*;

struct Context<'a> {
    reg: &'a Registry,
    router: &'a Router,
    cached: &'a dyn ToolCatalog,
    live_router: Option<&'a Arc<Mutex<Arc<Router>>>>,
    cancel: Option<&'a downstream::CancelContext>,
    client: Option<&'a str>,
}

/// Answers a legacy client's `tasks/*` request with the JSON-RPC response for `id`.
#[allow(clippy::too_many_arguments)]
pub(super) fn request(
    id: Value,
    req: &Value,
    reg: &Registry,
    router: &Router,
    cached: &dyn ToolCatalog,
    live_router: Option<&Arc<Mutex<Arc<Router>>>>,
    cancel: Option<downstream::CancelContext>,
    client: Option<&str>,
    allowed: Option<&HashSet<String>>,
) -> Option<Value> {
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let cx = Context {
        reg,
        router,
        cached,
        live_router,
        cancel: cancel.as_ref(),
        client,
    };
    let shape = &mut |owner: &str, result| {
        defend_and_shape(reg, owner, method, client, result, "", true).result
    };
    let admit = &mut |method: &str, server: &str, tool: Option<&str>, task: Option<&str>| {
        admit(&cx, method, server, tool, task)
    };
    Some(conduit_lib::seam::tasks_request(
        id,
        req,
        router,
        allowed,
        cancel.clone(),
        shape,
        admit,
    ))
}

/// `tasks/cancel` is never refused, like a modern client's: it only stops work. `tasks/result` is
/// the request that hands the tool's output to the client, so it asks a human where the call
/// itself would have. `tasks/get` and `tasks/list` only re-check policy.
fn admit(
    cx: &Context<'_>,
    method: &str,
    server: &str,
    tool: Option<&str>,
    task: Option<&str>,
) -> Result<(), String> {
    if method == "tasks/cancel" {
        return Ok(());
    }
    let fresh = clone_live_router(cx.live_router);
    let router = fresh.as_deref().unwrap_or(cx.router);
    let name = match tool {
        Some(tool) => Some(
            router
                .exposed_tool_name(server, tool)
                .ok_or_else(|| {
                    "Toolport: the tool of this task is no longer available".to_string()
                })?
                .to_string(),
        ),
        None => None,
    };
    let target = match &name {
        Some(name) => DispatchTarget::Tool(name),
        None => DispatchTarget::Server(server),
    };
    let recheck = || {
        prepare_dispatch(router, cx.live_router, target, cx.cancel, false)
            .map_err(|message| format!("Toolport: {message}"))
    };
    recheck()?;
    let (Some(name), Some(tool), "tasks/result") = (name.as_deref(), tool, method) else {
        return Ok(());
    };
    approve(cx, router, server, tool, name, task)?;
    recheck()
}

fn approve(
    cx: &Context<'_>,
    router: &Router,
    server: &str,
    tool: &str,
    name: &str,
    task: Option<&str>,
) -> Result<(), String> {
    let reg = cx.reg;
    if !reg.human_approval_effective() {
        return Ok(());
    }
    let is_dest = tool_is_destructive_fail_closed(name, cx.cached, router);
    let untrusted = reg
        .servers
        .iter()
        .find(|s| s.id == server)
        .is_some_and(|s| matches!(s.source.as_deref(), Some("shared") | Some("registry")));
    let Some(reason) = approval::gate_reason(
        reg.requires_human_approval(is_dest, untrusted),
        is_dest,
        untrusted,
    ) else {
        return Ok(());
    };
    let arguments = json!({ "taskAction": "tasks/result", "taskId": task });
    let reason_token = approval_reason_token(reason);
    audit::record_approval_raised(reg, server, tool, cx.client, reason_token, &arguments);
    let request = approval::ApprovalRequest {
        token: String::new(),
        id: new_correlation_id(),
        client: cx.client.map(str::to_string),
        client_name: observation::current().client_name,
        client_label: active_client_label(),
        host: observation::current().host.map(|host| host.host),
        host_thread: observation::current().host.and_then(|host| host.thread),
        server: server.to_string(),
        tool: tool.to_string(),
        reason,
        arguments: arguments.clone(),
        tool_fingerprint: tool_fingerprint_for(name, cx.cached, router),
        url_elicitation: None,
        pii_release: None,
    };
    let asked = Instant::now();
    let decision = request_human_decision_with_cancel(request, cx.cancel);
    audit::record_decision(
        server,
        tool,
        cx.client,
        active_client_label().as_deref(),
        reason_token,
        decision_token(decision),
        &arguments,
        Some(asked.elapsed().as_millis() as u64),
    );
    if decision.is_approved() {
        return Ok(());
    }
    Err(
        refused_call_result(name, decision, reason_token)["content"][0]["text"]
            .as_str()
            .unwrap_or("Toolport: the request was not approved")
            .to_string(),
    )
}
