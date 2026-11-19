//! MCP Tasks (2025-11-25, experimental) relayed through the gateway.
//!
//! The gateway is one endpoint over many servers, so it relays: a task-augmented `tools/call` to a
//! server that declared `tasks.requests.tools.call` is forwarded with its `task` parameter, and
//! `tasks/get|result|cancel|list` go to the server that owns the task.
//!
//! A task id handed to a client is `tpt.<hex server id>.<connection>.<hex server task id>`: it names
//! its owner, decodes without a lookup, cannot clash between servers that mint the same id, and a
//! restarted server (a new connection) that mints the same id again gets a different one. The gateway
//! also remembers every id it handed out, with the session that created it; an id that is not
//! remembered (never issued, expired, or dropped because its server restarted or died) is answered
//! as unknown.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

pub const RELATED_TASK_KEY: &str = "io.modelcontextprotocol/related-task";
pub const UNKNOWN_TASK: &str = "Toolport: task no longer available";
pub const NO_SESSION: &str = "Toolport: tasks belong to a client session and this client has none, so it cannot create, list or read tasks. Send `initialize` first and keep the session";

/// The revision that introduced tasks; clients and servers speak them only at this one.
pub const TASKS_VERSION: &str = "2025-11-25";

const ID_PREFIX: &str = "tpt.";
const MAX_REMEMBERED: usize = 4096;
const UNTIMED_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const RETENTION_GRACE: Duration = Duration::from_secs(60);
const INVALID_REQUEST: i64 = -32600;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;

/// Delivers a notification to the client session that created a task.
pub type Sink = Arc<dyn Fn(&str, &Value) + Send + Sync>;

/// How long a forwarded request may wait for its answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wait {
    /// The server's own call window.
    Window,
    /// Until the task ends: bounded by the call cap and the client's cancel only.
    UntilTerminal,
}

/// What came back from the server that owns a task.
#[derive(Debug)]
pub enum Forwarded {
    Reply(Value),
    /// The server answered with this JSON-RPC error object.
    Rpc(Value),
    /// The connection to the server is gone, so its tasks are too.
    Gone,
    Failed(String),
}

/// What the host does around a `tasks/*` request: the policy re-check and human approval a
/// `tools/call` of the task's tool goes through, and the audit row of the action.
pub trait Governance {
    /// Lets the request through (`Ok`) or refuses it with the message the client gets. `tool` is
    /// the tool the task runs, `task` the id the client knows it by.
    fn admit(
        &mut self,
        method: &str,
        server: &str,
        tool: Option<&str>,
        task: Option<&str>,
    ) -> Result<(), String>;

    /// One finished action: `outcome` is `ok`, `error` or `refused`.
    fn record(
        &mut self,
        method: &str,
        server: &str,
        tool: Option<&str>,
        task: Option<&str>,
        outcome: &str,
        elapsed: Duration,
    );
}

struct Entry {
    server: String,
    tool: String,
    session: String,
    status: String,
    until: Instant,
}

#[derive(Default)]
struct State {
    servers: HashMap<String, bool>,
    connections: HashMap<String, u64>,
    versions: HashMap<String, String>,
    connected: u64,
    told: bool,
    remembered: HashMap<String, Entry>,
    sink: Option<Sink>,
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(|| Mutex::new(State::default()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn hex(text: &str) -> String {
    text.bytes().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

fn encode(server: &str, connection: u64, native: &str) -> String {
    format!("{ID_PREFIX}{}.{connection}.{}", hex(server), hex(native))
}

/// The id a client knows the task `native` of `server` by, on the server's current connection.
pub fn exposed_id(server: &str, native: &str) -> String {
    encode(
        server,
        state().connections.get(server).copied().unwrap_or(0),
        native,
    )
}

fn decode(exposed: &str) -> Option<(String, String)> {
    let mut parts = exposed.strip_prefix(ID_PREFIX)?.split('.');
    let (server, connection, native) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || connection.parse::<u64>().is_err() {
        return None;
    }
    Some((unhex(server)?, unhex(native)?))
}

fn is_terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

fn declares_tasks(capabilities: Option<&Value>) -> bool {
    capabilities
        .and_then(|capabilities| capabilities.get("tasks"))
        .is_some_and(Value::is_object)
}

/// A server answered `initialize`: notes whether it takes task-augmented `tools/call`s. A new
/// connection is a new process, so the tasks of the previous one are gone.
pub fn server_connected(server_id: &str, result: &Value) {
    let tasks = result.get("capabilities").and_then(|c| c.get("tasks"));
    let creates = tasks
        .and_then(|tasks| tasks.get("requests")?.get("tools")?.get("call"))
        .is_some_and(Value::is_object);
    let lists = tasks.is_some_and(|tasks| tasks.get("list").is_some_and(Value::is_object));
    let mut state = state();
    state.connected += 1;
    let connection = state.connected;
    state.connections.insert(server_id.to_string(), connection);
    match result.get("protocolVersion").and_then(Value::as_str) {
        Some(version) => state
            .versions
            .insert(server_id.to_string(), version.to_string()),
        None => state.versions.remove(server_id),
    };
    if creates {
        state.servers.insert(server_id.to_string(), lists);
    } else {
        state.servers.remove(server_id);
    }
    state
        .remembered
        .retain(|_, entry| entry.server != server_id);
}

fn capable(state: &State, in_scope: &dyn Fn(&str) -> bool) -> bool {
    state.servers.keys().any(|server| in_scope(server))
}

/// The revision the server answered its `initialize` with, as its last connection negotiated it.
pub fn negotiated_version(server_id: &str) -> Option<String> {
    state().versions.get(server_id).cloned()
}

/// The revision the gateway offers a server in its `initialize`: the tasks revision to a stdio
/// server, which answers with the lower one it speaks, and `default` to an HTTP server, whose
/// `MCP-Protocol-Version` header keeps naming `default`.
pub fn offered_version(http: bool, default: &'static str) -> &'static str {
    if http {
        default
    } else {
        TASKS_VERSION
    }
}

fn is_newer_revision(requested: &str) -> bool {
    let dated = requested.len() == 10
        && requested.bytes().enumerate().all(|(at, byte)| {
            matches!(at, 4 | 7) == (byte == b'-') && (byte == b'-' || byte.is_ascii_digit())
        });
    dated && requested > TASKS_VERSION
}

/// Answers the `initialize` of a client that declared tasks at the tasks revision, or newer: that
/// revision is negotiated, and `capabilities.tasks` is added when a server in the client's scope
/// takes task-augmented calls. Every other client is answered as before.
pub fn advertise(result: &mut Value, request: &Value, in_scope: &dyn Fn(&str) -> bool) {
    let params = request.get("params");
    if !declares_tasks(params.and_then(|params| params.get("capabilities"))) {
        return;
    }
    let requested = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if is_newer_revision(requested) {
        result["protocolVersion"] = json!(TASKS_VERSION);
    }
    if result.get("protocolVersion").and_then(Value::as_str) != Some(TASKS_VERSION) {
        return;
    }
    let mut state = state();
    if !capable(&state, in_scope) {
        return;
    }
    state.told = true;
    drop(state);
    if let Some(capabilities) = result
        .get_mut("capabilities")
        .and_then(Value::as_object_mut)
    {
        capabilities.insert(
            "tasks".to_string(),
            json!({ "list": {}, "cancel": {}, "requests": { "tools": { "call": {} } } }),
        );
    }
}

/// Whether some client was told tasks exist and a server still takes them.
pub fn enabled() -> bool {
    let state = state();
    state.told && !state.servers.is_empty()
}

/// Whether a task-augmented call to this server is relayed with its `task` parameter.
pub fn server_creates(server_id: &str) -> bool {
    let state = state();
    state.told && state.servers.contains_key(server_id)
}

pub fn set_sink(sink: Sink) {
    state().sink = Some(sink);
}

fn rewrite_related(value: &mut Value, exposed: &str) {
    let related = value
        .get_mut("_meta")
        .and_then(|meta| meta.get_mut(RELATED_TASK_KEY))
        .and_then(Value::as_object_mut);
    if let Some(related) = related {
        related.insert("taskId".to_string(), json!(exposed));
    }
}

fn retention(task: &Value) -> Duration {
    task.get("ttl")
        .and_then(Value::as_u64)
        .map_or(UNTIMED_RETENTION, |ms| {
            Duration::from_millis(ms) + RETENTION_GRACE
        })
}

fn make_room(state: &mut State, now: Instant) {
    state.remembered.retain(|_, entry| entry.until > now);
    while state.remembered.len() >= MAX_REMEMBERED {
        let victim = state
            .remembered
            .iter()
            .min_by_key(|(_, entry)| (!is_terminal(&entry.status), entry.until))
            .map(|(id, _)| id.clone());
        match victim {
            Some(id) => state.remembered.remove(&id),
            None => break,
        };
    }
}

/// A server answered a task-augmented `tools/call` of `tool`. When the answer is a
/// `CreateTaskResult`, its task id is replaced by the one the client gets and the task is
/// remembered for `session`. Without a session nothing is remembered and the client gets an
/// error result instead of an id it could never use: tasks of sessionless clients would all
/// belong to one shared owner.
pub fn created(server_id: &str, tool: &str, session: &str, result: &mut Value) {
    if result.get("content").is_some() {
        return;
    }
    if session.is_empty() {
        if result.get("task").is_some_and(Value::is_object) {
            *result = json!({
                "content": [{ "type": "text", "text": NO_SESSION }],
                "isError": true,
            });
        }
        return;
    }
    let Some(task) = result.get_mut("task").filter(|task| task.is_object()) else {
        return;
    };
    let Some(native) = task
        .get("taskId")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    let exposed = exposed_id(server_id, &native);
    task["taskId"] = json!(exposed);
    let status = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("working")
        .to_string();
    let now = Instant::now();
    let until = now + retention(task);
    let mut state = state();
    make_room(&mut state, now);
    state.remembered.insert(
        exposed,
        Entry {
            server: server_id.to_string(),
            tool: tool.to_string(),
            session: session.to_string(),
            status,
            until,
        },
    );
}

/// Tasks of `server_id` that were last seen working.
pub fn running(server_id: &str) -> usize {
    let now = Instant::now();
    state()
        .remembered
        .values()
        .filter(|entry| {
            entry.server == server_id && !is_terminal(&entry.status) && entry.until > now
        })
        .count()
}

/// A server sent `notification`. A `notifications/tasks/status` for a task the gateway handed out
/// goes to the session that created it, under the id that session knows.
pub fn status_seen(server_id: &str, notification: &Value) {
    if notification.get("method").and_then(Value::as_str) != Some("notifications/tasks/status") {
        return;
    }
    let Some(native) = notification
        .get("params")
        .and_then(|params| params.get("taskId"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let exposed = exposed_id(server_id, native);
    let (session, sink) = {
        let mut state = state();
        let Some(entry) = state.remembered.get_mut(&exposed) else {
            return;
        };
        if let Some(status) = notification["params"].get("status").and_then(Value::as_str) {
            entry.status = status.to_string();
        }
        (entry.session.clone(), state.sink.clone())
    };
    let Some(sink) = sink.filter(|_| !session.is_empty()) else {
        return;
    };
    let mut relayed = notification.clone();
    relayed["params"]["taskId"] = json!(exposed);
    rewrite_related(&mut relayed["params"], &exposed);
    sink(&session, &relayed);
}

fn rpc_error(code: i64, message: &str) -> Value {
    json!({ "code": code, "message": message })
}

fn unknown() -> Value {
    rpc_error(INVALID_PARAMS, UNKNOWN_TASK)
}

struct Owned {
    server: String,
    native: String,
    tool: String,
}

fn owned(exposed: &str, session: &str, in_scope: &dyn Fn(&str) -> bool) -> Option<Owned> {
    if session.is_empty() {
        return None;
    }
    let (server, native) = decode(exposed)?;
    let state = state();
    let entry = state.remembered.get(exposed)?;
    (entry.session == session
        && entry.server == server
        && entry.until > Instant::now()
        && in_scope(&server))
    .then(|| Owned {
        server,
        native,
        tool: entry.tool.clone(),
    })
}

fn remember_status(exposed: &str, status: Option<&str>) {
    if let (Some(status), Some(entry)) = (status, state().remembered.get_mut(exposed)) {
        entry.status = status.to_string();
    }
}

fn forget_server(server: &str) {
    state().remembered.retain(|_, entry| entry.server != server);
}

fn rewrite_error(mut error: Value, exposed: &str) -> Value {
    if let Some(data) = error.get_mut("data") {
        rewrite_related(data, exposed);
    }
    error
}

fn outcome_of(answer: &Forwarded) -> &'static str {
    if matches!(answer, Forwarded::Reply(_)) {
        "ok"
    } else {
        "error"
    }
}

fn by_id(
    method: &str,
    params: &Value,
    session: &str,
    in_scope: &dyn Fn(&str) -> bool,
    forward: &mut dyn FnMut(&str, &str, Value, Wait) -> Forwarded,
    shape: &mut dyn FnMut(&str, Value) -> Value,
    governance: &mut dyn Governance,
) -> Result<Value, Value> {
    let exposed = params
        .get("taskId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            rpc_error(
                INVALID_PARAMS,
                &format!("Toolport: {method} requires params.taskId"),
            )
        })?;
    let Some(Owned {
        server,
        native,
        tool,
    }) = owned(exposed, session, in_scope)
    else {
        return Err(unknown());
    };
    let (tool, task) = (Some(tool.as_str()), Some(exposed));
    if let Err(message) = governance.admit(method, &server, tool, task) {
        governance.record(method, &server, tool, task, "refused", Duration::ZERO);
        return Err(rpc_error(INVALID_PARAMS, &message));
    }
    let wait = if method == "tasks/result" {
        Wait::UntilTerminal
    } else {
        Wait::Window
    };
    let started = Instant::now();
    let forwarded = forward(&server, method, json!({ "taskId": native }), wait);
    let outcome = outcome_of(&forwarded);
    governance.record(method, &server, tool, task, outcome, started.elapsed());
    match forwarded {
        Forwarded::Reply(mut reply) => {
            if method == "tasks/result" {
                remember_status(exposed, Some("completed"));
                rewrite_related(&mut reply, exposed);
                return Ok(shape(&server, reply));
            }
            remember_status(exposed, reply.get("status").and_then(Value::as_str));
            if reply.get("taskId").is_some() {
                reply["taskId"] = json!(exposed);
            }
            Ok(reply)
        }
        Forwarded::Rpc(error) => {
            if method == "tasks/get"
                && error.get("code").and_then(Value::as_i64) == Some(INVALID_PARAMS)
            {
                state().remembered.remove(exposed);
            }
            if method == "tasks/result" {
                remember_status(exposed, Some("failed"));
            }
            Err(rewrite_error(error, exposed))
        }
        Forwarded::Gone => {
            forget_server(&server);
            Err(unknown())
        }
        Forwarded::Failed(message) => Err(rpc_error(INTERNAL_ERROR, &message)),
    }
}

fn encode_cursor(server: &str, cursor: Option<&str>) -> String {
    hex(&json!({ "server": server, "cursor": cursor }).to_string())
}

fn decode_cursor(cursor: &str) -> Option<(String, Option<String>)> {
    let value: Value = serde_json::from_str(&unhex(cursor)?).ok()?;
    let server = value.get("server")?.as_str()?.to_string();
    let inner = value
        .get("cursor")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((server, inner))
}

fn list(
    params: &Value,
    session: &str,
    in_scope: &dyn Fn(&str) -> bool,
    forward: &mut dyn FnMut(&str, &str, Value, Wait) -> Forwarded,
    governance: &mut dyn Governance,
) -> Result<Value, Value> {
    let mut listable: Vec<String> = state()
        .servers
        .iter()
        .filter(|(server, lists)| **lists && in_scope(server))
        .map(|(server, _)| server.clone())
        .collect();
    listable.sort();
    let (server, inner) = match params.get("cursor") {
        None | Some(Value::Null) => match listable.first() {
            Some(first) => (first.clone(), None),
            None => return Ok(json!({ "tasks": [] })),
        },
        Some(cursor) => cursor
            .as_str()
            .and_then(decode_cursor)
            .filter(|(server, _)| listable.contains(server))
            .ok_or_else(|| rpc_error(INVALID_PARAMS, "Toolport: invalid cursor"))?,
    };
    let mut forwarded = Map::new();
    if let Some(inner) = &inner {
        forwarded.insert("cursor".to_string(), json!(inner));
    }
    let mut tasks = Vec::new();
    let mut next_inner = None;
    let answer = match governance.admit("tasks/list", &server, None, None) {
        Ok(()) => {
            let started = Instant::now();
            let answer = forward(
                &server,
                "tasks/list",
                Value::Object(forwarded),
                Wait::Window,
            );
            governance.record(
                "tasks/list",
                &server,
                None,
                None,
                outcome_of(&answer),
                started.elapsed(),
            );
            Some(answer)
        }
        Err(_) => {
            governance.record("tasks/list", &server, None, None, "refused", Duration::ZERO);
            None
        }
    };
    match answer {
        Some(Forwarded::Reply(reply)) => {
            next_inner = reply
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_string);
            for mut task in reply
                .get("tasks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let Some(native) = task.get("taskId").and_then(Value::as_str) else {
                    continue;
                };
                let exposed = exposed_id(&server, native);
                if owned(&exposed, session, in_scope).is_none() {
                    continue;
                }
                remember_status(&exposed, task.get("status").and_then(Value::as_str));
                task["taskId"] = json!(exposed);
                tasks.push(task);
            }
        }
        Some(Forwarded::Gone) => forget_server(&server),
        Some(Forwarded::Rpc(_) | Forwarded::Failed(_)) | None => {}
    }
    let mut page = json!({ "tasks": tasks });
    let next = match next_inner {
        Some(inner) => Some(encode_cursor(&server, Some(&inner))),
        None => listable
            .iter()
            .find(|candidate| **candidate > server)
            .map(|following| encode_cursor(following, None)),
    };
    if let Some(next) = next {
        page["nextCursor"] = json!(next);
    }
    Ok(page)
}

/// Answers one `tasks/*` request of a client. `forward` sends a request to a server, `shape`
/// passes a `tasks/result` result through content defense and `governance` admits and records
/// every action. The error is a JSON-RPC error object.
pub fn handle(
    method: &str,
    params: &Value,
    session: &str,
    in_scope: &dyn Fn(&str) -> bool,
    forward: &mut dyn FnMut(&str, &str, Value, Wait) -> Forwarded,
    shape: &mut dyn FnMut(&str, Value) -> Value,
    governance: &mut dyn Governance,
) -> Result<Value, Value> {
    if session.is_empty() {
        return Err(rpc_error(INVALID_REQUEST, NO_SESSION));
    }
    match method {
        "tasks/list" => list(params, session, in_scope, forward, governance),
        "tasks/get" | "tasks/result" | "tasks/cancel" => by_id(
            method, params, session, in_scope, forward, shape, governance,
        ),
        other => Err(rpc_error(-32601, &format!("Method not found: {other}"))),
    }
}

#[cfg(test)]
pub(crate) fn reset() {
    let mut state = state();
    state.servers.clear();
    state.connections.clear();
    state.versions.clear();
    state.connected = 0;
    state.told = false;
    state.remembered.clear();
    state.sink = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn guard() -> MutexGuard<'static, ()> {
        let held = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        reset();
        held
    }

    fn capable_server(id: &str, lists: bool) {
        let list = if lists { json!({}) } else { Value::Null };
        server_connected(
            id,
            &json!({ "capabilities": { "tasks": {
                "list": list, "cancel": {}, "requests": { "tools": { "call": {} } } } } }),
        );
    }

    fn told_client() -> Value {
        client_asking(TASKS_VERSION)
    }

    fn client_asking(version: &str) -> Value {
        json!({ "params": { "protocolVersion": version, "capabilities": { "tasks": {} } } })
    }

    fn answer(version: &str) -> Value {
        json!({ "protocolVersion": version, "capabilities": { "tools": {} } })
    }

    fn all(_: &str) -> bool {
        true
    }

    fn create(server: &str, native: &str, session: &str) -> String {
        let mut result = json!({ "task": { "taskId": native, "status": "working", "ttl": 60000 } });
        created(server, "tool", session, &mut result);
        result["task"]["taskId"].as_str().unwrap().to_string()
    }

    fn forwarding(
        script: impl Fn(&str, &str, Value, Wait) -> Forwarded,
    ) -> impl FnMut(&str, &str, Value, Wait) -> Forwarded {
        move |server, method, params, wait| script(server, method, params, wait)
    }

    fn keep(_: &str, value: Value) -> Value {
        value
    }

    struct Open;

    impl Governance for Open {
        fn admit(
            &mut self,
            _: &str,
            _: &str,
            _: Option<&str>,
            _: Option<&str>,
        ) -> Result<(), String> {
            Ok(())
        }

        fn record(
            &mut self,
            _: &str,
            _: &str,
            _: Option<&str>,
            _: Option<&str>,
            _: &str,
            _: Duration,
        ) {
        }
    }

    #[test]
    fn test01_ids_round_trip_and_never_clash_between_servers() {
        let _held = guard();
        let a = encode("alpha", 1, "task-1");
        let b = encode("beta", 1, "task-1");
        assert_ne!(a, b);
        assert_ne!(
            a,
            encode("alpha", 2, "task-1"),
            "a new connection mints new ids"
        );
        assert_eq!(decode(&a), Some(("alpha".into(), "task-1".into())));
        let odd = encode("a.b:c__d", 7, "ünï.id/ 1");
        assert_eq!(decode(&odd), Some(("a.b:c__d".into(), "ünï.id/ 1".into())));
        assert_eq!(decode("task-1"), None);
        assert_eq!(decode("tpt.zz.1.00"), None);
        assert_eq!(decode("tpt.616c.x.00"), None);
        assert_eq!(decode("tpt.616c.00"), None);
    }

    #[test]
    fn test02_capability_is_advertised_only_to_a_client_that_declared_tasks_and_with_a_capable_server(
    ) {
        let _held = guard();
        let mut result = answer(TASKS_VERSION);
        advertise(&mut result, &told_client(), &all);
        assert!(
            result["capabilities"].get("tasks").is_none(),
            "no capable server"
        );

        capable_server("alpha", true);
        let mut old = answer(TASKS_VERSION);
        advertise(
            &mut old,
            &json!({ "params": { "protocolVersion": TASKS_VERSION, "capabilities": {} } }),
            &all,
        );
        assert_eq!(old, answer(TASKS_VERSION));
        assert!(!enabled(), "nobody was told");

        advertise(&mut result, &told_client(), &all);
        assert_eq!(
            result["capabilities"]["tasks"],
            json!({ "list": {}, "cancel": {}, "requests": { "tools": { "call": {} } } })
        );
        assert!(enabled());
        assert!(server_creates("alpha"));
        assert!(!server_creates("beta"));
    }

    #[test]
    fn test03_a_server_outside_the_clients_scope_does_not_make_the_gateway_advertise_tasks() {
        let _held = guard();
        capable_server("alpha", true);
        let mut result = answer(TASKS_VERSION);
        advertise(&mut result, &told_client(), &|server| server == "beta");
        assert!(result["capabilities"].get("tasks").is_none());
    }

    #[test]
    fn test19_the_tasks_revision_is_negotiated_only_for_a_client_that_declared_tasks() {
        let _held = guard();
        capable_server("alpha", true);
        let mut at_tasks = answer("2025-06-18");
        advertise(&mut at_tasks, &client_asking("2025-06-18"), &all);
        assert_eq!(
            at_tasks,
            answer("2025-06-18"),
            "an older revision gets no tasks"
        );
        assert!(!enabled());

        for newer in ["2025-11-26", "2026-07-28", "2099-01-01"] {
            let mut result = answer(newer);
            advertise(&mut result, &client_asking(newer), &all);
            assert_eq!(result["protocolVersion"], TASKS_VERSION, "{newer}");
            assert!(result["capabilities"].get("tasks").is_some(), "{newer}");
        }

        for odd in [
            "",
            "newest",
            "2026-7-28",
            "20261-1-28",
            "2026/07/28",
            "zzzzzzzzzz",
        ] {
            let mut result = answer("2025-06-18");
            advertise(&mut result, &client_asking(odd), &all);
            assert_eq!(result, answer("2025-06-18"), "{odd:?} is not a revision");
        }

        let mut plain = answer("2026-07-28");
        advertise(
            &mut plain,
            &json!({ "params": { "protocolVersion": "2026-07-28", "capabilities": {} } }),
            &all,
        );
        assert_eq!(plain, answer("2026-07-28"), "without tasks nothing changes");
    }

    #[test]
    fn test20_the_revision_a_server_negotiated_is_recorded_per_connection() {
        let _held = guard();
        server_connected("alpha", &answer("2025-11-25"));
        server_connected("beta", &answer("2025-06-18"));
        server_connected("gamma", &json!({ "capabilities": {} }));
        assert_eq!(negotiated_version("alpha").as_deref(), Some("2025-11-25"));
        assert_eq!(negotiated_version("beta").as_deref(), Some("2025-06-18"));
        assert_eq!(negotiated_version("gamma"), None);
        server_connected("alpha", &answer("2025-06-18"));
        assert_eq!(negotiated_version("alpha").as_deref(), Some("2025-06-18"));
    }

    #[test]
    fn test21_stdio_servers_are_offered_the_tasks_revision_and_http_servers_keep_theirs() {
        assert_eq!(offered_version(false, "2025-06-18"), TASKS_VERSION);
        assert_eq!(offered_version(true, "2025-06-18"), "2025-06-18");
    }

    #[test]
    fn test04_a_server_without_the_tools_call_declaration_is_not_task_capable() {
        let _held = guard();
        server_connected("plain", &json!({ "capabilities": { "tools": {} } }));
        server_connected(
            "listonly",
            &json!({ "capabilities": { "tasks": { "list": {} } } }),
        );
        assert!(state().servers.is_empty());
    }

    #[test]
    fn test05_a_create_result_gets_a_namespaced_id_and_a_plain_result_is_untouched() {
        let _held = guard();
        let mut created_result = json!({ "task": { "taskId": "task-1", "status": "working" } });
        created("alpha", "tool", "s1", &mut created_result);
        assert_eq!(
            created_result["task"]["taskId"],
            json!(exposed_id("alpha", "task-1"))
        );
        let mut plain =
            json!({ "content": [{ "type": "text", "text": "hi" }], "task": { "taskId": "x" } });
        let before = plain.clone();
        created("alpha", "tool", "s1", &mut plain);
        assert_eq!(plain, before);
        assert_eq!(running("alpha"), 1);
    }

    #[test]
    fn test06_get_forwards_the_native_id_and_answers_under_the_clients_id() {
        let _held = guard();
        capable_server("alpha", true);
        let exposed = create("alpha", "task-1", "s1");
        let seen = Mutex::new(Vec::new());
        let mut forward = forwarding(|server, method, params, wait| {
            seen.lock()
                .unwrap()
                .push((server.to_string(), method.to_string(), params, wait));
            Forwarded::Reply(json!({ "taskId": "task-1", "status": "completed" }))
        });
        let reply = handle(
            "tasks/get",
            &json!({ "taskId": exposed }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        )
        .unwrap();
        assert_eq!(reply["taskId"], json!(exposed));
        assert_eq!(
            seen.lock().unwrap()[0],
            (
                "alpha".to_string(),
                "tasks/get".to_string(),
                json!({ "taskId": "task-1" }),
                Wait::Window
            )
        );
        assert_eq!(
            running("alpha"),
            0,
            "the status that came back is remembered"
        );
    }

    #[test]
    fn test07_result_waits_until_terminal_and_carries_the_related_task_under_the_clients_id() {
        let _held = guard();
        let exposed = create("alpha", "task-1", "s1");
        let waits = Mutex::new(Vec::new());
        let mut forward = forwarding(|_, _, _, wait| {
            waits.lock().unwrap().push(wait);
            Forwarded::Reply(json!({
                "content": [{ "type": "text", "text": "done" }],
                "_meta": { RELATED_TASK_KEY: { "taskId": "task-1" } }
            }))
        });
        let reply = handle(
            "tasks/result",
            &json!({ "taskId": exposed }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        )
        .unwrap();
        assert_eq!(reply["_meta"][RELATED_TASK_KEY]["taskId"], json!(exposed));
        assert_eq!(reply["content"][0]["text"], "done");
        assert_eq!(*waits.lock().unwrap(), vec![Wait::UntilTerminal]);
    }

    #[test]
    fn test08_a_json_rpc_error_of_the_task_comes_back_as_it_is_with_the_clients_id() {
        let _held = guard();
        let exposed = create("alpha", "task-1", "s1");
        let mut forward = forwarding(|_, _, _, _| {
            Forwarded::Rpc(json!({
                "code": -32603, "message": "job failed",
                "data": { "_meta": { RELATED_TASK_KEY: { "taskId": "task-1" } } }
            }))
        });
        let error = handle(
            "tasks/result",
            &json!({ "taskId": exposed }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        )
        .unwrap_err();
        assert_eq!(error["code"], -32603);
        assert_eq!(error["message"], "job failed");
        assert_eq!(
            error["data"]["_meta"][RELATED_TASK_KEY]["taskId"],
            json!(exposed)
        );
    }

    #[test]
    fn test09_an_id_that_was_never_issued_belongs_to_another_session_or_is_forged_is_unknown() {
        let _held = guard();
        let exposed = create("alpha", "task-1", "s1");
        let called = Mutex::new(0);
        let mut forward = forwarding(|_, _, _, _| {
            *called.lock().unwrap() += 1;
            Forwarded::Reply(json!({}))
        });
        for (id, session) in [
            (exposed.clone(), "s2"),
            (exposed_id("alpha", "task-2"), "s1"),
            ("task-1".to_string(), "s1"),
            (exposed_id("ghost", "task-1"), "s1"),
        ] {
            let error = handle(
                "tasks/get",
                &json!({ "taskId": id }),
                session,
                &all,
                &mut forward,
                &mut keep,
                &mut Open,
            )
            .unwrap_err();
            assert_eq!(error["code"], -32602, "{id}");
            assert_eq!(error["message"], UNKNOWN_TASK);
        }
        let outside = handle(
            "tasks/get",
            &json!({ "taskId": exposed }),
            "s1",
            &|_: &str| false,
            &mut forward,
            &mut keep,
            &mut Open,
        );
        assert_eq!(outside.unwrap_err()["code"], -32602);
        let missing = handle(
            "tasks/get",
            &json!({}),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        )
        .unwrap_err();
        assert_eq!(missing["code"], -32602);
        assert_eq!(*called.lock().unwrap(), 0, "nothing reached a server");
    }

    #[test]
    fn test10_a_reconnected_server_drops_its_ids_and_keeps_the_others() {
        let _held = guard();
        capable_server("alpha", true);
        capable_server("beta", true);
        let old = create("alpha", "task-1", "s1");
        let other = create("beta", "task-1", "s1");
        capable_server("alpha", true);
        let mut forward = forwarding(|_, _, _, _| Forwarded::Reply(json!({ "taskId": "task-1" })));
        let gone = handle(
            "tasks/get",
            &json!({ "taskId": old }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        );
        assert_eq!(gone.unwrap_err()["message"], UNKNOWN_TASK);
        assert!(handle(
            "tasks/get",
            &json!({ "taskId": other }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open
        )
        .is_ok());
    }

    #[test]
    fn test11_a_server_that_is_gone_drops_all_its_ids_and_answers_unknown() {
        let _held = guard();
        let first = create("alpha", "task-1", "s1");
        let second = create("alpha", "task-2", "s1");
        let mut gone = forwarding(|_, _, _, _| Forwarded::Gone);
        let error = handle(
            "tasks/get",
            &json!({ "taskId": first }),
            "s1",
            &all,
            &mut gone,
            &mut keep,
            &mut Open,
        )
        .unwrap_err();
        assert_eq!(error["message"], UNKNOWN_TASK);
        let mut forward = forwarding(|_, _, _, _| Forwarded::Reply(json!({})));
        let error = handle(
            "tasks/get",
            &json!({ "taskId": second }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        )
        .unwrap_err();
        assert_eq!(
            error["message"], UNKNOWN_TASK,
            "no request reaches a server for a dropped id"
        );
        assert_eq!(running("alpha"), 0);
    }

    #[test]
    fn test12_list_walks_the_servers_page_by_page_and_shows_only_the_sessions_own_tasks() {
        let _held = guard();
        capable_server("alpha", true);
        capable_server("beta", true);
        capable_server("quiet", false);
        let a1 = create("alpha", "task-1", "s1");
        let b1 = create("beta", "task-1", "s1");
        create("beta", "task-9", "s2");
        let mut forward = forwarding(
            |server, _, params, _| match (server, params.get("cursor")) {
                ("alpha", None) => Forwarded::Reply(json!({
                    "tasks": [{ "taskId": "task-1", "status": "working" }, { "taskId": "task-5" }],
                    "nextCursor": "more"
                })),
                ("alpha", Some(_)) => Forwarded::Reply(json!({ "tasks": [] })),
                ("beta", None) => Forwarded::Reply(json!({
                    "tasks": [{ "taskId": "task-1", "status": "completed" }, { "taskId": "task-9" }]
                })),
                other => panic!("unexpected list {other:?}"),
            },
        );
        let mut ids = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0;
        loop {
            let params = cursor
                .as_ref()
                .map_or(json!({}), |c| json!({ "cursor": c }));
            let page = handle(
                "tasks/list",
                &params,
                "s1",
                &all,
                &mut forward,
                &mut keep,
                &mut Open,
            )
            .unwrap();
            ids.extend(
                page["tasks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["taskId"].as_str().unwrap().to_string()),
            );
            pages += 1;
            match page.get("nextCursor").and_then(Value::as_str) {
                Some(next) => cursor = Some(next.to_string()),
                None => break,
            }
        }
        assert_eq!(ids, vec![a1, b1]);
        assert_eq!(pages, 3, "alpha page 1, alpha page 2, beta");
        let bad = handle(
            "tasks/list",
            &json!({ "cursor": "nonsense" }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        );
        assert_eq!(bad.unwrap_err()["code"], -32602);
    }

    #[test]
    fn test13_a_status_notification_reaches_the_creating_session_under_its_id() {
        let _held = guard();
        let exposed = create("alpha", "task-1", "s1");
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let sink_log = Arc::clone(&delivered);
        set_sink(Arc::new(move |session, note| {
            sink_log
                .lock()
                .unwrap()
                .push((session.to_string(), note.clone()));
        }));
        let note = |native: &str, status: &str| {
            json!({
                "jsonrpc": "2.0", "method": "notifications/tasks/status",
                "params": { "taskId": native, "status": status }
            })
        };
        status_seen("alpha", &note("task-1", "completed"));
        status_seen("alpha", &note("task-7", "completed"));
        status_seen("beta", &note("task-1", "completed"));
        status_seen(
            "alpha",
            &json!({ "method": "notifications/progress", "params": {} }),
        );
        let delivered = delivered.lock().unwrap();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].0, "s1");
        assert_eq!(delivered[0].1["params"]["taskId"], json!(exposed));
        assert_eq!(
            running("alpha"),
            0,
            "the completed task no longer counts as running"
        );
    }

    #[test]
    fn test14_a_task_past_its_ttl_is_forgotten_and_no_longer_counts_as_running() {
        let _held = guard();
        let mut result = json!({ "task": { "taskId": "task-1", "status": "working", "ttl": 0 } });
        created("alpha", "tool", "s1", &mut result);
        {
            let mut state = state();
            for entry in state.remembered.values_mut() {
                entry.until = Instant::now() - Duration::from_secs(1);
            }
        }
        assert_eq!(running("alpha"), 0);
        let exposed = exposed_id("alpha", "task-1");
        let mut forward = forwarding(|_, _, _, _| Forwarded::Reply(json!({})));
        let error = handle(
            "tasks/get",
            &json!({ "taskId": exposed }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut Open,
        );
        assert_eq!(error.unwrap_err()["code"], -32602);
    }

    #[derive(Default)]
    struct Watching {
        refuse: Vec<(String, String, String)>,
        admitted: Vec<(String, String, Option<String>, Option<String>)>,
        recorded: Vec<(String, String, Option<String>, String)>,
    }

    impl Governance for Watching {
        fn admit(
            &mut self,
            method: &str,
            server: &str,
            tool: Option<&str>,
            task: Option<&str>,
        ) -> Result<(), String> {
            self.admitted.push((
                method.to_string(),
                server.to_string(),
                tool.map(str::to_string),
                task.map(str::to_string),
            ));
            match self
                .refuse
                .iter()
                .find(|(m, s, _)| m == method && s == server)
            {
                Some((_, _, message)) => Err(message.clone()),
                None => Ok(()),
            }
        }

        fn record(
            &mut self,
            method: &str,
            server: &str,
            tool: Option<&str>,
            _: Option<&str>,
            outcome: &str,
            _: Duration,
        ) {
            self.recorded.push((
                method.to_string(),
                server.to_string(),
                tool.map(str::to_string),
                outcome.to_string(),
            ));
        }
    }

    #[test]
    fn test15_every_by_id_action_is_admitted_with_its_tool_and_recorded_with_its_outcome() {
        let _held = guard();
        let exposed = create("alpha", "task-1", "s1");
        let mut watching = Watching::default();
        let mut forward = forwarding(|_, method, _, _| match method {
            "tasks/cancel" => Forwarded::Rpc(json!({ "code": -32602, "message": "terminal" })),
            _ => Forwarded::Reply(json!({ "status": "working", "content": [] })),
        });
        for method in ["tasks/get", "tasks/result", "tasks/cancel"] {
            let reply = handle(
                method,
                &json!({ "taskId": exposed }),
                "s1",
                &all,
                &mut forward,
                &mut keep,
                &mut watching,
            );
            assert_eq!(reply.is_ok(), method != "tasks/cancel");
        }
        let admitted: Vec<_> = watching.admitted.iter().map(|a| a.0.as_str()).collect();
        assert_eq!(admitted, ["tasks/get", "tasks/result", "tasks/cancel"]);
        assert!(watching
            .admitted
            .iter()
            .all(|(_, server, tool, task)| server == "alpha"
                && tool.as_deref() == Some("tool")
                && task.as_deref() == Some(exposed.as_str())));
        let outcomes: Vec<_> = watching.recorded.iter().map(|r| r.3.as_str()).collect();
        assert_eq!(outcomes, ["ok", "ok", "error"]);
    }

    #[test]
    fn test16_a_refused_action_is_answered_with_the_refusal_and_never_reaches_the_server() {
        let _held = guard();
        let exposed = create("alpha", "task-1", "s1");
        let mut watching = Watching {
            refuse: vec![(
                "tasks/result".to_string(),
                "alpha".to_string(),
                "Blocked by Toolport".to_string(),
            )],
            ..Watching::default()
        };
        let sent = Mutex::new(0);
        let mut forward = forwarding(|_, _, _, _| {
            *sent.lock().unwrap() += 1;
            Forwarded::Reply(json!({ "status": "working" }))
        });
        let error = handle(
            "tasks/result",
            &json!({ "taskId": exposed }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut watching,
        )
        .unwrap_err();
        assert_eq!(error["code"], -32602);
        assert_eq!(error["message"], "Blocked by Toolport");
        assert_eq!(*sent.lock().unwrap(), 0);
        assert_eq!(watching.recorded.len(), 1);
        assert_eq!(watching.recorded[0].3, "refused");
        assert!(handle(
            "tasks/get",
            &json!({ "taskId": exposed }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut watching,
        )
        .is_ok());
    }

    #[test]
    fn test17_an_unknown_id_is_never_admitted_so_it_cannot_probe_policy() {
        let _held = guard();
        let mut watching = Watching::default();
        let mut forward = forwarding(|_, _, _, _| Forwarded::Reply(json!({})));
        let error = handle(
            "tasks/get",
            &json!({ "taskId": exposed_id("alpha", "task-9") }),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut watching,
        )
        .unwrap_err();
        assert_eq!(error["message"], UNKNOWN_TASK);
        assert!(watching.admitted.is_empty() && watching.recorded.is_empty());
    }

    #[test]
    fn test18_list_admits_each_server_and_skips_one_that_is_refused() {
        let _held = guard();
        capable_server("alpha", true);
        capable_server("beta", true);
        let mut watching = Watching {
            refuse: vec![(
                "tasks/list".to_string(),
                "alpha".to_string(),
                "off".to_string(),
            )],
            ..Watching::default()
        };
        let sent = Mutex::new(Vec::new());
        let mut forward = forwarding(|server, _, _, _| {
            sent.lock().unwrap().push(server.to_string());
            Forwarded::Reply(json!({ "tasks": [] }))
        });
        let first = handle(
            "tasks/list",
            &json!({}),
            "s1",
            &all,
            &mut forward,
            &mut keep,
            &mut watching,
        )
        .unwrap();
        assert!(sent.lock().unwrap().is_empty());
        assert_eq!(watching.recorded[0].3, "refused");
        assert_eq!(watching.recorded[0].2, None);
        assert!(first["nextCursor"].is_string(), "beta is still to come");
    }

    #[test]
    fn test30_a_client_without_a_session_cannot_create_list_or_read_tasks() {
        let _held = guard();
        let mut result = json!({ "task": { "taskId": "t-1", "status": "working", "ttl": 60000 } });
        created("alpha", "tool", "", &mut result);
        assert_eq!(result["isError"], true, "{result}");
        assert_eq!(result["content"][0]["text"], NO_SESSION);
        assert!(
            result.get("task").is_none(),
            "no id for a client that has no owner"
        );
        assert_eq!(
            running("alpha"),
            0,
            "nothing is remembered for the empty session"
        );

        let mut plain = json!({ "content": [{ "type": "text", "text": "done" }] });
        created("alpha", "tool", "", &mut plain);
        assert_eq!(plain["content"][0]["text"], "done", "a plain result passes");

        let mine = create("alpha", "task-2", "s1");
        let mut forward = forwarding(|_, _, _, _| panic!("a refused request reaches no server"));
        for (method, params) in [
            ("tasks/list", json!({})),
            ("tasks/get", json!({ "taskId": mine })),
            ("tasks/result", json!({ "taskId": mine })),
            ("tasks/cancel", json!({ "taskId": mine })),
        ] {
            let error = handle(
                method,
                &params,
                "",
                &all,
                &mut forward,
                &mut keep,
                &mut Open,
            )
            .unwrap_err();
            assert_eq!(error["code"], INVALID_REQUEST, "{method}");
            assert_eq!(error["message"], NO_SESSION, "{method}");
        }
    }

    #[test]
    fn test31_a_task_of_a_session_is_not_reachable_with_the_empty_session() {
        let _held = guard();
        let mine = create("alpha", "task-3", "s1");
        assert!(owned(&mine, "", &all).is_none());
        assert!(owned(&mine, "s1", &all).is_some());
    }
}
