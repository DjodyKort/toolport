//! A fake slow MCP stdio server (`plus/fixtures/stdio/slow-mcp.mjs`) behind the real gateway
//! binary, and a line-based client for it. Durations are scaled to hundreds of milliseconds
//! so the cases stay fast; the per-server timeouts in the registry are scaled the same way.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use conduit_lib::approval;
use conduit_lib::registry::{self, EnvVar, Profile, Registry, ServerEntry};
use serde_json::{json, Value};

pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
pub const SERVER_ID: &str = "slow";
pub const PEER_ID: &str = "other";

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The cases start real daemons and then time them, so they run one at a time.
pub static CASE_LOCK: Mutex<()> = Mutex::new(());

pub fn case_lock() -> std::sync::MutexGuard<'static, ()> {
    CASE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How the fake server treats concurrent requests and a cancellation.
#[derive(Clone, Copy, Default)]
pub struct Behaviour {
    /// Handle one `tools/call` at a time, in arrival order.
    pub serial: bool,
    /// Keep working on a cancelled call and answer it late.
    pub ignore_cancel: bool,
    /// Declare MCP tasks and take task-augmented `tools/call`s.
    pub tasks: bool,
    /// With `tasks`: declare them only to a gateway that offers 2025-11-25 in its `initialize`.
    pub tasks_only_at_2025_11_25: bool,
    /// Tasks per `tasks/list` page; 0 keeps the fake's default.
    pub task_page: u32,
    /// Seconds of `_meta["odh.dev/budget"]` on the listing of `odoo_job_wait`; 0 lists none.
    pub job_budget_s: u32,
}

/// The per-server timeouts the registry carries, in milliseconds.
#[derive(Clone, Copy, Default)]
pub struct Timeouts {
    pub request_ms: Option<u64>,
    pub max_request_ms: Option<u64>,
    /// `toolBudgets` of the server: seconds per tool name.
    pub tool_budgets: &'static [(&'static str, f64)],
    /// The gateway sends this server one call at a time (`serial`).
    pub serial: bool,
}

/// A scratch data directory; killing the daemons the case started is part of dropping it.
pub struct Case {
    pub dir: PathBuf,
    pub log: PathBuf,
    /// The log of the second server of a pair.
    pub peer_log: PathBuf,
}

impl Case {
    pub fn new(tag: &str, behaviour: Behaviour, timeouts: Timeouts) -> Self {
        Self::build(tag, behaviour, timeouts, None)
    }

    /// Two fake servers, `slow` and `other`, each with its own timeouts and log.
    pub fn pair(tag: &str, behaviour: Behaviour, timeouts: Timeouts, peer: Timeouts) -> Self {
        Self::build(tag, behaviour, timeouts, Some((behaviour, peer)))
    }

    /// [`Case::pair`] whose second server behaves differently from the first.
    pub fn pair_with(
        tag: &str,
        behaviour: Behaviour,
        timeouts: Timeouts,
        peer_behaviour: Behaviour,
        peer: Timeouts,
    ) -> Self {
        Self::build(tag, behaviour, timeouts, Some((peer_behaviour, peer)))
    }

    fn build(
        tag: &str,
        behaviour: Behaviour,
        timeouts: Timeouts,
        peer: Option<(Behaviour, Timeouts)>,
    ) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "toolport-longcall-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch data dir");
        let log = dir.join("slow-mcp.jsonl");
        let peer_log = dir.join("slow-mcp-other.jsonl");
        let mut servers = vec![server_entry(SERVER_ID, &log, behaviour, timeouts)];
        if let Some((peer_behaviour, peer)) = peer {
            servers.push(server_entry(PEER_ID, &peer_log, peer_behaviour, peer));
        }
        write_registry(&dir, servers);
        Self { dir, log, peer_log }
    }

    /// A client that sees the whole catalog and calls tools by name.
    pub fn client(&self) -> Client {
        Client::spawn(&self.dir, "full")
    }

    /// A client in lazy discovery: only the gateway's meta-tools are listed, a tool of the fake
    /// server is run through `toolport_call_tool`.
    pub fn lazy_client(&self) -> Client {
        Client::spawn(&self.dir, "lazy")
    }

    /// Everything the fake server recorded, in order. `t` is wall-clock milliseconds.
    pub fn entries(&self) -> Vec<Value> {
        Self::entries_of(&self.log)
    }

    pub fn entries_of(log: &Path) -> Vec<Value> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub fn received(&self, method: &str) -> Vec<Value> {
        self.entries()
            .into_iter()
            .filter(|entry| entry["dir"] == "in" && entry["method"] == method)
            .collect()
    }

    pub fn received_calls(&self, tool: &str) -> Vec<Value> {
        self.received("tools/call")
            .into_iter()
            .filter(|entry| entry["tool"] == tool)
            .collect()
    }

    /// Kills the fake server process of the log `log` (its latest start).
    pub fn kill_server(&self, log: &Path) {
        let pid = Self::entries_of(log)
            .iter()
            .rev()
            .find(|entry| entry["dir"] == "start")
            .and_then(|entry| entry["pid"].as_u64())
            .expect("the fake server logged its start");
        let _ = Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    /// Rows of the data dir's audit log, in order.
    pub fn audit(&self) -> Vec<Value> {
        std::fs::read_to_string(self.dir.join("audit.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Rewrites the registry the daemon reads; the daemon takes the change up on its own.
    pub fn edit_registry(&self, edit: impl FnOnce(&mut Value)) {
        let path = self.dir.join("registry.json");
        let mut registry: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read registry")).expect("parse");
        edit(&mut registry);
        registry::atomic_write(
            &path,
            &serde_json::to_string_pretty(&registry).expect("serialize registry"),
        )
        .expect("save registry");
    }

    /// An approval broker in the data dir that answers its requests with `decisions` in turn
    /// (`approved`, `denied`, ...), the last one for every request after that, and keeps the
    /// requests it saw.
    pub fn broker(&self, decisions: &'static [&'static str]) -> Broker {
        Broker::start(&self.dir, decisions)
    }

    pub fn answered(&self, id: &Value) -> bool {
        self.entries()
            .iter()
            .any(|entry| entry["dir"] == "out" && entry["id"] == *id)
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        kill_daemons(&self.dir);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn node() -> PathBuf {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join("node"))
        .find(|candidate| candidate.is_file())
        .expect("node on PATH runs the fake slow MCP server")
}

fn server_entry(id: &str, log: &Path, behaviour: Behaviour, timeouts: Timeouts) -> ServerEntry {
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plus/fixtures/stdio/slow-mcp.mjs"
    );
    let mut env = vec![("SLOWMCP_LOG", log.display().to_string())];
    if behaviour.serial {
        env.push(("SLOWMCP_MODE", "serial".to_string()));
    }
    if behaviour.ignore_cancel {
        env.push(("SLOWMCP_CANCEL", "ignore".to_string()));
    }
    if behaviour.tasks {
        let mode = if behaviour.tasks_only_at_2025_11_25 {
            "2025-11-25"
        } else {
            "1"
        };
        env.push(("SLOWMCP_TASKS", mode.to_string()));
    }
    if behaviour.task_page > 0 {
        env.push(("SLOWMCP_TASK_PAGE", behaviour.task_page.to_string()));
    }
    if behaviour.job_budget_s > 0 {
        env.push(("SLOWMCP_JOB_BUDGET", behaviour.job_budget_s.to_string()));
    }
    let mut unknown_fields = serde_json::Map::new();
    if let Some(ms) = timeouts.max_request_ms {
        unknown_fields.insert("maxRequestTimeoutMs".to_string(), json!(ms));
    }
    if !timeouts.tool_budgets.is_empty() {
        let budgets: serde_json::Map<String, Value> = timeouts
            .tool_budgets
            .iter()
            .map(|(tool, secs)| ((*tool).to_string(), json!(secs)))
            .collect();
        unknown_fields.insert("toolBudgets".to_string(), Value::Object(budgets));
    }
    if timeouts.serial {
        unknown_fields.insert("serial".to_string(), json!(true));
    }
    ServerEntry {
        enabled: true,
        inherit_env: false,
        id: id.to_string(),
        name: format!("{}{}", id[..1].to_uppercase(), &id[1..]),
        transport: "stdio".to_string(),
        command: Some(node().display().to_string()),
        args: vec![script.to_string()],
        env: env
            .into_iter()
            .map(|(key, value)| EnvVar {
                key: key.to_string(),
                value: Some(value),
                secret: false,
                unknown_fields: Default::default(),
            })
            .collect(),
        url: None,
        source: Some("manual".to_string()),
        disabled_tools: vec![],
        hide_prompts: false,
        cwd: None,
        client_credentials: None,
        request_timeout_ms: timeouts.request_ms,
        initialize_timeout_ms: None,
        launch: None,
        unknown_fields,
    }
}

fn write_registry(dir: &Path, servers: Vec<ServerEntry>) {
    let mut registry_value = Registry::default();
    registry_value.set_lazy_discovery(false);
    let ids: Vec<String> = servers.iter().map(|server| server.id.clone()).collect();
    registry_value.servers = servers;
    if let Some(active) = registry_value.active_profile_id.clone() {
        if let Some(profile) = registry_value
            .profiles
            .iter_mut()
            .find(|profile: &&mut Profile| profile.id == active)
        {
            profile.enabled_server_ids = ids;
        }
    }
    registry::save_to(&dir.join("registry.json"), &registry_value).expect("write registry");
}

fn kill_daemons(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        let is_descriptor = path.extension().is_some_and(|ext| ext == "json")
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("daemon-"));
        if !is_descriptor {
            continue;
        }
        let pid = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|descriptor| descriptor["pid"].as_u64());
        if let Some(pid) = pid {
            let _ = Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// A stand-in for the app's approval service.
pub struct Broker {
    stop: Arc<AtomicBool>,
    seen: Arc<Mutex<Vec<approval::ApprovalRequest>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Broker {
    fn start(dir: &Path, decisions: &'static [&'static str]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("approval listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let token = "longcall-approval-token".to_string();
        let descriptor = approval::EndpointDescriptor {
            endpoint: listener.local_addr().unwrap().to_string(),
            unix_endpoint: None,
            token: token.clone(),
        };
        std::fs::write(
            dir.join(approval::ENDPOINT_FILE),
            serde_json::to_vec(&descriptor).unwrap(),
        )
        .expect("approval descriptor");
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (stopping, seeing) = (stop.clone(), seen.clone());
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).expect("blocking socket");
                        stream
                            .set_read_timeout(Some(Duration::from_secs(10)))
                            .unwrap();
                        let mut reader = BufReader::new(stream.try_clone().unwrap());
                        let mut line = String::new();
                        reader.read_line(&mut line).expect("challenge");
                        let challenge: approval::BrokerChallenge =
                            serde_json::from_str(&line).expect("valid challenge");
                        let proof = approval::BrokerProof {
                            toolport_approval_proof: approval::challenge_proof(
                                &token,
                                &challenge.toolport_approval_challenge,
                            ),
                        };
                        writeln!(stream, "{}", serde_json::to_string(&proof).unwrap())
                            .expect("proof");
                        line.clear();
                        reader.read_line(&mut line).expect("approval request");
                        let request: approval::ApprovalRequest =
                            serde_json::from_str(&line).expect("valid approval request");
                        let decision = {
                            let mut seen = seeing.lock().unwrap();
                            seen.push(request);
                            decisions[(seen.len() - 1).min(decisions.len() - 1)]
                        };
                        writeln!(stream, "\"{decision}\"").expect("decision");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("approval accept failed: {error}"),
                }
            }
        });
        Self {
            stop,
            seen,
            thread: Some(thread),
        }
    }

    /// The approval requests received so far.
    pub fn requests(&self) -> Vec<approval::ApprovalRequest> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One AI client session: the real `--stdio-adapter` with a line-based MCP client on it.
pub struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<(Instant, Value)>,
    /// Messages read while waiting for another one, with the time they arrived.
    pub inbox: VecDeque<(Instant, Value)>,
    next_id: i64,
    dir: PathBuf,
}

impl Client {
    fn spawn(dir: &Path, discovery: &str) -> Self {
        let client_id = format!("longcall-{}", NEXT.fetch_add(1, Ordering::Relaxed));
        select_discovery(dir, &client_id, discovery);
        let mut child = Command::new(env!("CARGO_BIN_EXE_toolport-gateway"))
            .arg("--stdio-adapter")
            .env("TOOLPORT_DATA_DIR", dir)
            .env("TOOLPORT_REGISTRY", dir.join("registry.json"))
            .env_remove("TOOLPORT_GATEWAY_TOPOLOGY")
            .env_remove("CONDUIT_GATEWAY_TOPOLOGY")
            .env("TOOLPORT_CLIENT_ID", &client_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the stdio adapter");
        let stdin = child.stdin.take().expect("adapter stdin");
        let stdout = child.stdout.take().expect("adapter stdout");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if sender.send((Instant::now(), value)).is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            lines,
            inbox: VecDeque::new(),
            next_id: 0,
            dir: dir.to_path_buf(),
        }
    }

    pub fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().expect("adapter stdin open");
        writeln!(stdin, "{message}")
            .and_then(|()| stdin.flush())
            .expect("write to the adapter");
    }

    fn pull(&mut self, within: Duration) -> Option<(Instant, Value)> {
        let received = self.lines.recv_timeout(within).ok()?;
        if received.1.get("method").is_some() && received.1.get("id").is_some() {
            let id = received.1["id"].clone();
            self.send(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "harness client answers nothing" }
            }));
            return self.pull(within);
        }
        Some(received)
    }

    /// The response to `id`, with the time it reached the client. Everything else read on the
    /// way goes to `inbox`.
    pub fn response(&mut self, id: &Value, within: Duration) -> (Instant, Value) {
        let deadline = Instant::now() + within;
        loop {
            if let Some(position) = self
                .inbox
                .iter()
                .position(|(_, message)| message.get("method").is_none() && message["id"] == *id)
            {
                return self.inbox.remove(position).expect("buffered response");
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "no response to {id} within {within:?}; inbox: {:?}",
                self.inbox
            );
            if let Some(message) = self.pull(left) {
                self.inbox.push_back(message);
            }
        }
    }

    /// Reads whatever arrives within `quiet`, into `inbox`.
    pub fn listen(&mut self, quiet: Duration) {
        let deadline = Instant::now() + quiet;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            if let Some(message) = self.pull(left) {
                self.inbox.push_back(message);
            }
        }
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = json!(self.next_id);
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        self.response(&id, RESPONSE_TIMEOUT).1
    }

    pub fn initialize(&mut self) {
        self.initialize_with(json!({}));
    }

    /// Initializes declaring `capabilities`, asking for 2025-11-25 when they include tasks;
    /// returns the `initialize` result.
    pub fn initialize_with(&mut self, capabilities: Value) -> Value {
        let version = if capabilities.get("tasks").is_some() {
            "2025-11-25"
        } else {
            "2024-11-05"
        };
        self.initialize_at(version, capabilities)
    }

    /// Initializes asking for revision `version`; returns the `initialize` result.
    pub fn initialize_at(&mut self, version: &str, capabilities: Value) -> Value {
        let reply = self.request(
            "initialize",
            json!({
                "protocolVersion": version,
                "capabilities": capabilities,
                "clientInfo": { "name": "longcall", "version": "1" }
            }),
        );
        assert!(reply.get("result").is_some(), "initialize failed: {reply}");
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        reply["result"].clone()
    }

    /// Sends any request without waiting.
    pub fn start_request(&mut self, id: i64, method: &str, params: Value) -> Instant {
        let sent = Instant::now();
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        sent
    }

    /// The gateway's name for a tool of the fake server, once its catalog is published.
    pub fn tool_named(&mut self, tool: &str) -> String {
        Self::tool(self, tool).0
    }

    /// The gateway's name for a tool of the fake server `server`.
    pub fn tool_of(&mut self, server: &str, tool: &str) -> String {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        let name = format!("{server}__{tool}");
        loop {
            let reply = self.request("tools/list", json!({}));
            let listed = reply["result"]["tools"].as_array().into_iter().flatten();
            if listed
                .into_iter()
                .any(|listed| listed["name"] == name.as_str())
            {
                return name;
            }
            assert!(
                Instant::now() < deadline,
                "no tool {name} published: {reply}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// The gateway's name and listing of a tool of the fake server.
    pub fn tool(&mut self, tool: &str) -> (String, Value) {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        let suffix = format!("__{tool}");
        loop {
            let reply = self.request("tools/list", json!({}));
            let found = reply["result"]["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|listed| {
                    listed["name"]
                        .as_str()
                        .is_some_and(|name| name.ends_with(&suffix))
                })
                .cloned();
            if let Some(listed) = found {
                let name = listed["name"].as_str().unwrap_or_default().to_string();
                return (name, listed);
            }
            assert!(
                Instant::now() < deadline,
                "no tool {tool} published: {reply}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Sends a `toolport_call_tool` for `name` without waiting, as a lazy-discovery client does.
    pub fn start_meta_call(
        &mut self,
        id: i64,
        name: &str,
        arguments: Value,
        progress_token: Option<&str>,
    ) -> Instant {
        self.start_call(
            id,
            "toolport_call_tool",
            json!({ "name": name, "arguments": arguments }),
            progress_token,
        )
    }

    /// Sends a `tools/call` without waiting. `progress_token` is the client's own.
    pub fn start_call(
        &mut self,
        id: i64,
        name: &str,
        arguments: Value,
        progress_token: Option<&str>,
    ) -> Instant {
        let mut params = json!({ "name": name, "arguments": arguments });
        if let Some(token) = progress_token {
            params["_meta"] = json!({ "progressToken": token });
        }
        let sent = Instant::now();
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": params }));
        sent
    }

    pub fn cancel(&mut self, id: i64, reason: &str) {
        self.send(json!({
            "jsonrpc": "2.0", "method": "notifications/cancelled",
            "params": { "requestId": id, "reason": reason }
        }));
    }

    /// Progress notifications the client received for its own token, so far.
    pub fn progress_seen(&self, token: &str) -> usize {
        self.inbox
            .iter()
            .filter(|(_, message)| {
                message["method"] == "notifications/progress"
                    && message["params"]["progressToken"] == token
            })
            .count()
    }

    pub fn close_stdin(&mut self) {
        self.stdin.take();
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn select_discovery(dir: &Path, client_id: &str, discovery: &str) {
    let path = dir.join("registry.json");
    let raw = std::fs::read(&path).expect("read fixture registry");
    let mut registry: Value = serde_json::from_slice(&raw).expect("parse fixture registry");
    if !registry["clientDiscovery"].is_object() {
        registry["clientDiscovery"] = json!({});
    }
    registry["clientDiscovery"][client_id] = json!(discovery);
    registry::atomic_write(
        &path,
        &serde_json::to_string_pretty(&registry).expect("serialize fixture registry"),
    )
    .expect("save fixture discovery choice");
}

pub fn text_of(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// The error of a reply: a JSON-RPC error, or the text blocks of an `isError` result.
pub fn error_text(reply: &Value) -> String {
    if let Some(error) = reply.get("error") {
        return error.to_string();
    }
    if reply["result"]["isError"] != json!(true) {
        return String::new();
    }
    reply["result"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
