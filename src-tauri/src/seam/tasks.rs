//! The gateway side of the MCP Tasks relay: which request a thread is serving, what a
//! task-augmented `tools/call` sends the server, and how a client's `tasks/*` request reaches the
//! server that owns the task. The relay itself is `toolport_plus::mcp_tasks`.

use std::cell::RefCell;
use std::collections::HashSet;

use serde_json::{json, Value};
use std::time::Duration;

use toolport_plus::mcp_tasks::{self, Forwarded, Governance, Wait};
use toolport_plus::progress;

use crate::downstream::CancelContext;
use crate::router::Router;

#[derive(Clone)]
struct Request {
    tool: Option<String>,
    task: Option<Value>,
    session: String,
}

#[derive(Clone)]
struct Outgoing {
    task: Value,
    tool: String,
    session: String,
}

thread_local! {
    static REQUEST: RefCell<Option<Request>> = const { RefCell::new(None) };
    static OUTGOING: RefCell<Option<Outgoing>> = const { RefCell::new(None) };
}

/// Holds what the request being served says about tasks until it is dropped.
pub struct RequestGuard(Option<Request>);

impl Drop for RequestGuard {
    fn drop(&mut self) {
        REQUEST.with(|current| *current.borrow_mut() = self.0.take());
    }
}

/// The `task` of a `tools/call` and the session of a `tasks/*` request, while tasks are on.
pub fn request_task(req: &Value, session: fn() -> Option<String>) -> RequestGuard {
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params");
    let task = params
        .and_then(|params| params.get("task"))
        .filter(|task| task.is_object())
        .cloned();
    let relevant = (method == "tools/call" && task.is_some()) || method.starts_with("tasks/");
    let request = (relevant && mcp_tasks::enabled()).then(|| Request {
        tool: params
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        task,
        session: session().unwrap_or_default(),
    });
    RequestGuard(REQUEST.with(|current| current.replace(request)))
}

/// Keeps the `task` of a call on the thread that sends it.
pub struct OutgoingGuard;

impl Drop for OutgoingGuard {
    fn drop(&mut self) {
        OUTGOING.with(|current| *current.borrow_mut() = None);
    }
}

/// The call of tool `name` (`tool` on the server) on `server_id` is the one the client sent with
/// a `task`, and the server takes tasks: its `task` goes along.
pub fn arm(server_id: &str, tool: &str, name: &str) -> Option<OutgoingGuard> {
    let outgoing = REQUEST.with(|current| {
        let current = current.borrow();
        let request = current.as_ref()?;
        // No session, no task: the call goes to the server as a plain one, which a receiver that
        // does not take tasks is required to do with the `task` it is sent.
        if request.session.is_empty() {
            return None;
        }
        request
            .tool
            .as_deref()
            .is_some_and(|tool| tool.eq_ignore_ascii_case(name))
            .then(|| {
                request.task.clone().map(|task| Outgoing {
                    task,
                    tool: tool.to_string(),
                    session: request.session.clone(),
                })
            })
            .flatten()
    })?;
    if !mcp_tasks::server_creates(server_id) {
        return None;
    }
    OUTGOING.with(|current| *current.borrow_mut() = Some(outgoing));
    Some(OutgoingGuard)
}

pub fn with_task(mut params: Value) -> Value {
    if let Some(outgoing) = OUTGOING.with(|current| current.borrow().clone()) {
        params["task"] = outgoing.task;
    }
    params
}

pub fn task_created(server_id: &str, result: &mut Value) {
    if let Some(outgoing) = OUTGOING.with(|current| current.borrow().clone()) {
        mcp_tasks::created(server_id, &outgoing.tool, &outgoing.session, result);
    }
}

struct Governed<'a>(super::TaskAdmit<'a>);

impl Governance for Governed<'_> {
    fn admit(
        &mut self,
        method: &str,
        server: &str,
        tool: Option<&str>,
        task: Option<&str>,
    ) -> Result<(), String> {
        (self.0)(method, server, tool, task)
    }

    fn record(
        &mut self,
        method: &str,
        server: &str,
        tool: Option<&str>,
        task: Option<&str>,
        outcome: &str,
        elapsed: Duration,
    ) {
        let Some(path) = crate::audit::audit_path() else {
            return;
        };
        let mut entry = json!({
            "kind": "task",
            "server": crate::router::sanitize_segment(server),
            "serverId": server,
            "action": method,
            "outcome": outcome,
            "durationMs": u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
        });
        if let Some(tool) = tool {
            entry["tool"] = json!(tool);
        }
        if let Some(task) = task {
            entry["taskId"] = json!(task);
        }
        crate::session_observability::enrich(&mut entry);
        crate::audit::record_session_at(&path, entry);
    }
}

/// Answers a client's `tasks/*` request with the JSON-RPC response for `id`. `admit` is asked
/// before a request reaches a server.
pub fn request(
    id: Value,
    req: &Value,
    router: &Router,
    allowed: Option<&HashSet<String>>,
    cancel: Option<CancelContext>,
    shape: &mut dyn FnMut(&str, Value) -> Value,
    admit: super::TaskAdmit<'_>,
) -> Value {
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    let session = REQUEST.with(|current| {
        current
            .borrow()
            .as_ref()
            .map(|request| request.session.clone())
            .unwrap_or_default()
    });
    let in_scope = |server: &str| allowed.is_none_or(|scope| scope.contains(server));
    let mut forward = |server: &str, method: &str, mut params: Value, wait: Wait| -> Forwarded {
        let _lease = (wait == Wait::UntilTerminal).then(|| {
            let cap = progress::cap_from(super::max_request_timeout_ms(Some(server.to_string())));
            let (token, lease) = progress::lease_wait(cap);
            params["_meta"] = json!({ "progressToken": token });
            lease
        });
        router.plus_task_request(server, method, params, cancel.clone())
    };
    match mcp_tasks::handle(
        method,
        &params,
        &session,
        &in_scope,
        &mut forward,
        shape,
        &mut Governed(admit),
    ) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER: &str = "sessionless-fixture";

    fn tasks_offered() {
        mcp_tasks::server_connected(
            SERVER,
            &json!({ "capabilities": { "tasks": {
                "list": {}, "cancel": {}, "requests": { "tools": { "call": {} } } } } }),
        );
        let mut result = json!({ "protocolVersion": mcp_tasks::TASKS_VERSION, "capabilities": {} });
        mcp_tasks::advertise(
            &mut result,
            &json!({ "params": {
                "protocolVersion": mcp_tasks::TASKS_VERSION, "capabilities": { "tasks": {} } } }),
            &|_| true,
        );
    }

    fn task_call() -> Value {
        json!({ "method": "tools/call",
            "params": { "name": "fixture__job", "task": { "ttl": 1000 } } })
    }

    #[test]
    fn a_call_from_a_client_without_a_session_is_sent_without_its_task() {
        tasks_offered();
        {
            let _request = request_task(&task_call(), || None);
            assert!(arm(SERVER, "job", "fixture__job").is_none());
            assert_eq!(
                with_task(json!({ "name": "job" })),
                json!({ "name": "job" })
            );
        }
        let _request = request_task(&task_call(), || Some("session-1".to_string()));
        assert!(arm(SERVER, "job", "fixture__job").is_some());
    }
}
