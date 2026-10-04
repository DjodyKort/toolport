//! What the gateway does today for an ODH-shaped downstream server (MIG-ODH-1).
//!
//! Every test starts the real `toolport-gateway --stdio-adapter` (the default topology: adapter
//! plus shared host daemon) on a scratch data directory whose registry holds one server `odh`
//! backed by `mock-mcp-server` with `MOCK_MCP_PROFILE=odh`. The rows pin behavior that exists
//! today; none of them asks for a change. `docs/odh-integration.md` names each row.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use conduit_lib::plus::import_mcpm::{exposed_prefix, MAX_TOOL_NAME_LEN, TOOL_NAME_PREFIX};
use conduit_lib::registry::{self, EnvVar, Registry, ServerEntry};
use serde_json::{json, Value};

#[path = "common/exec.rs"]
mod exec;

static NEXT: AtomicUsize = AtomicUsize::new(0);

const MOCK_INSTRUCTIONS: &str = "FAKE-odh-downstream-instructions";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "odh-integration-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }

    fn transcript(&self) -> PathBuf {
        self.0.join("odh-transcript.jsonl")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        kill_daemons(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn kill_daemons(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with("daemon-") && name.ends_with(".json")) {
            continue;
        }
        let pid = std::fs::read_to_string(entry.path())
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|v| v["pid"].as_u64());
        if let Some(pid) = pid {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
        }
    }
}

fn odh_entry(command: &str, args: Vec<String>, transcript: &Path) -> ServerEntry {
    ServerEntry {
        id: "odh".into(),
        name: "odh".into(),
        transport: "stdio".into(),
        command: Some(command.into()),
        args,
        env: vec![
            EnvVar {
                key: "MOCK_MCP_PROFILE".into(),
                value: Some("odh".into()),
                secret: false,
            },
            EnvVar {
                key: "MOCK_MCP_TRANSCRIPT".into(),
                value: Some(transcript.display().to_string()),
                secret: false,
            },
        ],
        url: None,
        cwd: None,
        source: Some("manual".into()),
        disabled_tools: Vec::new(),
        client_credentials: None,
        request_timeout_ms: None,
        initialize_timeout_ms: None,
        launch: None,
        unknown_fields: serde_json::Map::new(),
    }
}

fn mock_entry(scratch: &Scratch) -> ServerEntry {
    odh_entry(
        env!("CARGO_BIN_EXE_mock-mcp-server"),
        Vec::new(),
        &scratch.transcript(),
    )
}

fn write_registry(scratch: &Scratch, entry: ServerEntry, tweak: impl FnOnce(&mut Registry)) {
    let mut reg = Registry::default();
    reg.set_lazy_discovery(false);
    reg.servers = vec![entry];
    let active = reg.active_profile_id.clone();
    for profile in reg
        .profiles
        .iter_mut()
        .filter(|p| Some(&p.id) == active.as_ref())
    {
        profile.enabled_server_ids = vec!["odh".into()];
    }
    tweak(&mut reg);
    registry::save_to(&scratch.0.join("registry.json"), &reg).expect("write registry");
}

struct Client {
    child: Child,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    responses: mpsc::Receiver<Value>,
    notifications: Arc<Mutex<Vec<Value>>>,
    server_requests: Arc<Mutex<Vec<Value>>>,
    next_id: i64,
}

impl Client {
    fn start(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_toolport-gateway"))
            .arg("--stdio-adapter")
            .env("TOOLPORT_DATA_DIR", dir)
            .env("TOOLPORT_REGISTRY", dir.join("registry.json"))
            .env_remove("TOOLPORT_PROFILE")
            .env_remove("TOOLPORT_CLIENT_ID")
            .env_remove("TOOLPORT_GATEWAY_TOPOLOGY")
            .env_remove("CONDUIT_GATEWAY_TOPOLOGY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the adapter");
        let stdin = Arc::new(Mutex::new(Some(child.stdin.take().expect("adapter stdin"))));
        let stdout = child.stdout.take().expect("adapter stdout");
        let (sender, responses) = mpsc::channel();
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let server_requests = Arc::new(Mutex::new(Vec::new()));
        let responder = Arc::clone(&stdin);
        let seen_notes = Arc::clone(&notifications);
        let seen_requests = Arc::clone(&server_requests);
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                match (value.get("method"), value.get("id")) {
                    (Some(method), Some(id)) => {
                        seen_requests.lock().unwrap().push(value.clone());
                        let reply = if method == "elicitation/create" {
                            json!({"jsonrpc": "2.0", "id": id,
                                   "result": {"action": "accept", "content": {"approved": true}}})
                        } else {
                            json!({"jsonrpc": "2.0", "id": id,
                                   "error": {"code": -32601, "message": "harness answers elicitation only"}})
                        };
                        if let Some(handle) = responder.lock().unwrap().as_mut() {
                            let _ = writeln!(handle, "{reply}");
                            let _ = handle.flush();
                        }
                    }
                    (Some(_), None) => seen_notes.lock().unwrap().push(value),
                    _ => {
                        if sender.send(value).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self {
            child,
            stdin,
            responses,
            notifications,
            server_requests,
            next_id: 0,
        }
    }

    fn send(&self, message: Value) {
        let mut guard = self.stdin.lock().unwrap();
        let handle = guard.as_mut().expect("client stdin open");
        writeln!(handle, "{message}").expect("write to the adapter");
        handle.flush().expect("flush to the adapter");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.request_within(method, params, Duration::from_secs(60))
    }

    fn request_within(&mut self, method: &str, params: Value, within: Duration) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let value = self
                .responses
                .recv_timeout(left)
                .unwrap_or_else(|e| panic!("no answer to {method}: {e}"));
            if value["id"] == id {
                return value;
            }
        }
    }

    fn initialize(&mut self, version: &str, capabilities: Value) -> Value {
        let init = self.request(
            "initialize",
            json!({"protocolVersion": version, "capabilities": capabilities,
                   "clientInfo": {"name": "odh-integration", "version": "1"}}),
        );
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        init
    }

    fn tools(&mut self) -> Vec<Value> {
        let list = self.request("tools/list", json!({}));
        list["result"]["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn wait_for_tool(&mut self, name: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(tool) = self.tools().into_iter().find(|t| t["name"] == name) {
                return tool;
            }
            assert!(Instant::now() < deadline, "the gateway never listed {name}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn call(&mut self, name: &str, arguments: Value, meta: Option<Value>) -> Value {
        let mut params = json!({"name": name, "arguments": arguments});
        if let Some(meta) = meta {
            params["_meta"] = meta;
        }
        self.request("tools/call", params)
    }

    fn notifications_named(&self, method: &str) -> Vec<Value> {
        self.notifications
            .lock()
            .unwrap()
            .iter()
            .filter(|n| n["method"] == method)
            .cloned()
            .collect()
    }

    fn wait_for_notifications(&self, method: &str, count: usize) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let found = self.notifications_named(method);
            if found.len() >= count || Instant::now() >= deadline {
                return found;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stdin.lock().unwrap().take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn transcript(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn text_of(result: &Value) -> String {
    result["result"]["content"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[test]
fn the_gateway_advertises_what_it_serves_and_declares_nothing_downstream() {
    let scratch = Scratch::new("handshake");
    write_registry(&scratch, mock_entry(&scratch), |_| {});
    let mut client = Client::start(&scratch.0);
    let init = client.initialize(
        "2025-06-18",
        json!({"elicitation": {}, "roots": {}, "sampling": {}}),
    );

    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "toolport-gateway");
    let capabilities = init["result"]["capabilities"].as_object().unwrap();
    let advertised: BTreeSet<&str> = capabilities.keys().map(String::as_str).collect();
    assert_eq!(
        advertised,
        BTreeSet::from(["completions", "prompts", "resources", "tools"]),
        "no logging, elicitation, sampling or tasks capability is advertised: {capabilities:?}"
    );
    assert_eq!(capabilities["tools"]["listChanged"], true);

    let instructions = init["result"]["instructions"].as_str().unwrap_or_default();
    assert!(
        !instructions.is_empty() && !instructions.contains(MOCK_INSTRUCTIONS),
        "the gateway sends its own text and never forwards the downstream server's instructions"
    );

    client.wait_for_tool("odh__odoo_search_read");
    let lines = transcript(&scratch.transcript());
    let handshake = lines
        .iter()
        .find(|l| l["method"] == "initialize")
        .expect("the downstream saw an initialize");
    assert_eq!(handshake["params"]["protocolVersion"], "2025-06-18");
    assert_eq!(
        handshake["params"]["capabilities"],
        json!({}),
        "the gateway declares no client capability downstream, even when its own client declared several"
    );
    assert_eq!(
        handshake["params"]["clientInfo"]["name"],
        "toolport-gateway"
    );
}

#[test]
fn the_gateway_answers_every_known_revision_with_that_revision() {
    let scratch = Scratch::new("versions");
    write_registry(&scratch, mock_entry(&scratch), |_| {});
    let mut client = Client::start(&scratch.0);
    for version in ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"] {
        let init = client.initialize(version, json!({}));
        assert_eq!(init["result"]["protocolVersion"], version);
    }
    let unknown = client.initialize("2099-01-01", json!({}));
    assert_eq!(
        unknown["result"]["protocolVersion"], "2025-06-18",
        "an unknown revision is answered with the default legacy revision"
    );
}

#[test]
fn tool_names_carry_the_server_prefix_and_keep_the_output_schema() {
    let scratch = Scratch::new("names");
    write_registry(&scratch, mock_entry(&scratch), |_| {});
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({}));
    let tool = client.wait_for_tool("odh__odoo_search_read");

    assert_eq!(
        tool["outputSchema"]["required"],
        json!(["count", "records"]),
        "outputSchema reaches the client unchanged: {tool}"
    );
    assert_eq!(
        tool["outputSchema"]["properties"]["records"]["type"],
        "array"
    );

    let names: Vec<String> = client
        .tools()
        .iter()
        .filter_map(|t| t["name"].as_str().map(String::from))
        .filter(|n| n.starts_with("odh__"))
        .collect();
    assert!(names.contains(&"odh__odoo_export".to_string()), "{names:?}");
    for name in &names {
        let shown = format!("{TOOL_NAME_PREFIX}{name}");
        assert!(shown.starts_with(&exposed_prefix("odh")), "{shown}");
        assert!(shown.len() <= MAX_TOOL_NAME_LEN, "{shown} is too long");
    }

    let reply = client.call(
        "odh__odoo_search_read",
        json!({"model": "res.partner"}),
        None,
    );
    let sent = transcript(&scratch.transcript())
        .into_iter()
        .find(|l| l["method"] == "tools/call")
        .expect("the downstream saw a tools/call");
    assert_eq!(
        sent["params"]["name"], "odoo_search_read",
        "the downstream receives the original tool name, not the exposed one"
    );
    assert_eq!(sent["params"]["arguments"], json!({"model": "res.partner"}));
    assert_eq!(
        reply["result"]["structuredContent"]["records"][1]["name"], "FAKE-b",
        "{reply}"
    );
    assert_eq!(reply["result"]["structuredContent"]["count"], 2);
}

#[test]
fn resource_links_pass_through_and_only_listed_or_templated_uris_can_be_read() {
    let scratch = Scratch::new("resources");
    write_registry(&scratch, mock_entry(&scratch), |_| {});
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({}));
    client.wait_for_tool("odh__odoo_export");

    let listed = client.call("odh__odoo_export", json!({}), None);
    let link = &listed["result"]["content"][1];
    assert_eq!(link["type"], "resource_link", "{listed}");
    assert_eq!(link["uri"], "odh://export/1");
    assert_eq!(link["mimeType"], "text/csv");

    let read = client.request("resources/read", json!({"uri": "odh://export/1"}));
    assert_eq!(
        read["result"]["contents"][0]["text"], "id,name\n1,FAKE-row\n",
        "{read}"
    );

    let templated = client.call("odh__odoo_export_report", json!({}), None);
    assert_eq!(templated["result"]["content"][1]["uri"], "odh://report/7");
    let read = client.request("resources/read", json!({"uri": "odh://report/7"}));
    assert_eq!(
        read["result"]["contents"][0]["text"], "body of odh://report/7",
        "a URI covered by a listed resource template is routable: {read}"
    );

    let dynamic = client.call("odh__odoo_export_dynamic", json!({}), None);
    let uri = dynamic["result"]["content"][1]["uri"].as_str().unwrap();
    assert_eq!(
        uri, "odh://dyn/42",
        "the link itself is forwarded untouched"
    );
    let read = client.request("resources/read", json!({"uri": uri}));
    assert_eq!(read["error"]["code"], -32602, "{read}");
    assert!(
        read["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Toolport: no server owns resource 'odh___dyn_42'"),
        "a link to a URI that is neither listed nor templated is refused at the gateway, with the URI sanitized in the message: {read}"
    );
    let reads = transcript(&scratch.transcript())
        .into_iter()
        .filter(|l| l["method"] == "resources/read")
        .count();
    assert_eq!(
        reads, 2,
        "the refused read never reached the downstream server"
    );
}

#[test]
fn progress_messages_reach_the_client_only_when_it_sent_a_token() {
    let scratch = Scratch::new("progress");
    write_registry(&scratch, mock_entry(&scratch), |_| {});
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({}));
    client.wait_for_tool("odh__odoo_progress");

    let silent = client.call("odh__odoo_progress", json!({"steps": 2}), None);
    assert_eq!(text_of(&silent), "progress done");
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        client
            .notifications_named("notifications/progress")
            .is_empty(),
        "without a progressToken the gateway does not ask the server for progress"
    );

    let reply = client.call(
        "odh__odoo_progress",
        json!({"steps": 2}),
        Some(json!({"progressToken": "FAKE-client-token"})),
    );
    assert_eq!(text_of(&reply), "progress done");
    let notes = client.wait_for_notifications("notifications/progress", 2);
    assert_eq!(notes.len(), 2, "{notes:?}");
    for (index, note) in notes.iter().enumerate() {
        assert_eq!(note["params"]["progressToken"], "FAKE-client-token");
        assert_eq!(note["params"]["progress"], index as u64 + 1);
        assert_eq!(
            note["params"]["message"],
            format!("FAKE-step {} of 2", index + 1),
            "the message field survives the hop: {note}"
        );
    }
    let sent = transcript(&scratch.transcript())
        .into_iter()
        .filter(|l| l["method"] == "tools/call")
        .collect::<Vec<_>>();
    assert!(
        sent[0]["params"].get("_meta").is_none(),
        "no token, no _meta"
    );
    assert_ne!(
        sent[1]["params"]["_meta"]["progressToken"], "FAKE-client-token",
        "the downstream sees a gateway-minted token, never the client's"
    );
}

#[test]
fn a_server_elicitation_is_forwarded_to_a_client_that_declared_the_capability() {
    let scratch = Scratch::new("elicit-yes");
    write_registry(&scratch, mock_entry(&scratch), |_| {});
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({"elicitation": {}}));
    client.wait_for_tool("odh__legacy_elicitation");

    let reply = client.call("odh__legacy_elicitation", json!({}), None);
    assert_eq!(text_of(&reply), "legacy confirmed", "{reply}");
    let asked = client.server_requests.lock().unwrap().clone();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0]["method"], "elicitation/create");
    let message = asked[0]["params"]["message"].as_str().unwrap();
    assert!(
        message.contains("Continue the legacy mock operation?") && message.contains("odh"),
        "the client sees the question and which server asked it: {message}"
    );
}

#[test]
fn a_server_elicitation_is_refused_when_the_client_never_declared_the_capability() {
    let scratch = Scratch::new("elicit-no");
    let mut entry = mock_entry(&scratch);
    entry.request_timeout_ms = Some(1_500);
    write_registry(&scratch, entry, |_| {});
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({}));
    client.wait_for_tool("odh__legacy_elicitation");

    let reply = client.call("odh__legacy_elicitation", json!({}), None);
    assert!(
        client.server_requests.lock().unwrap().is_empty(),
        "nothing is forwarded to a client that cannot answer"
    );
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    let answered = transcript(&scratch.transcript())
        .into_iter()
        .find(|l| l["id"] == "mock-legacy-elicitation")
        .expect("the gateway answered the server's request itself");
    assert_eq!(answered["error"]["code"], -32601);
    assert_eq!(
        answered["error"]["message"],
        "upstream client does not support elicitation/create"
    );
}

#[test]
fn a_call_past_request_timeout_ms_fails_even_while_progress_keeps_arriving() {
    let scratch = Scratch::new("deadline");
    let mut entry = mock_entry(&scratch);
    entry.request_timeout_ms = Some(1_500);
    write_registry(&scratch, entry, |_| {});
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({}));
    client.wait_for_tool("odh__odoo_slow");

    let started = Instant::now();
    let quick = client.call("odh__odoo_slow", json!({"delayMs": 400}), None);
    assert_eq!(text_of(&quick), "slow done", "{quick}");
    assert!(started.elapsed() < Duration::from_millis(1_400));

    let started = Instant::now();
    let slow = client.call(
        "odh__odoo_slow",
        json!({"delayMs": 4_000, "progressEveryMs": 300}),
        Some(json!({"progressToken": "FAKE-slow-token"})),
    );
    let took = started.elapsed();
    assert_eq!(slow["result"]["isError"], true, "{slow}");
    assert!(
        text_of(&slow).contains("timed out waiting for 'tools/call' response"),
        "{slow}"
    );
    assert!(
        took >= Duration::from_millis(1_400) && took < Duration::from_millis(3_500),
        "the deadline fires near requestTimeoutMs, not when the 4 s call would have ended: {took:?}"
    );
    let progress = client.wait_for_notifications("notifications/progress", 3);
    assert!(
        progress.len() >= 3,
        "progress kept flowing for the whole wait and did not extend it: {progress:?}"
    );
}

#[test]
fn oversized_results_are_shaped_and_lose_structured_content_unless_the_budget_is_raised() {
    let shaped = {
        let scratch = Scratch::new("shaped");
        write_registry(&scratch, mock_entry(&scratch), |_| {});
        let mut client = Client::start(&scratch.0);
        client.initialize("2025-06-18", json!({}));
        client.wait_for_tool("odh__odoo_big");
        client.call("odh__odoo_big", json!({"bytes": 40_000}), None)
    };
    assert!(
        shaped["result"].get("structuredContent").is_none(),
        "shaping stashes structuredContent behind a cursor: {shaped}"
    );
    assert!(
        text_of(&shaped).contains("toolport_fetch_result"),
        "the client gets a head and a fetch cursor instead: {shaped}"
    );

    let scratch = Scratch::new("budget-off");
    write_registry(&scratch, mock_entry(&scratch), |reg| {
        reg.result_budgets.insert("odh".into(), 0);
    });
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({}));
    client.wait_for_tool("odh__odoo_big");
    let whole = client.call("odh__odoo_big", json!({"bytes": 40_000}), None);
    assert_eq!(
        whole["result"]["structuredContent"]["blob"]
            .as_str()
            .map(str::len),
        Some(40_000),
        "resultBudgets.odh = 0 leaves the result whole"
    );
    assert!(!text_of(&whole).contains("toolport_fetch_result"));
}

#[test]
fn a_profile_instruction_of_two_thousand_characters_is_sent_unchanged() {
    let scratch = Scratch::new("instructions");
    let text = format!("FAKE-{}", "i".repeat(1_995));
    assert_eq!(text.chars().count(), 2_000);
    let sent = text.clone();
    write_registry(&scratch, mock_entry(&scratch), move |reg| {
        reg.profiles[0].instructions = Some(sent);
    });
    let mut client = Client::start(&scratch.0);
    let init = client.initialize("2025-06-18", json!({}));
    assert_eq!(init["result"]["instructions"], text.as_str());
}

#[test]
fn the_uv_run_directory_form_launches_through_the_gateway() {
    let scratch = Scratch::new("uv");
    let bin = scratch.0.join("bin");
    let repo = scratch.0.join("repo");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(repo.join("mcp-server")).unwrap();
    let argv = scratch.0.join("uv-argv.txt");
    let uv = bin.join("uv");
    exec::write_executable(
        &uv,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nexec '{}'\n",
            argv.display(),
            env!("CARGO_BIN_EXE_mock-mcp-server")
        ),
    );
    let directory = repo.join("mcp-server").display().to_string();
    let args: Vec<String> = ["run", "--directory", directory.as_str(), "odh-mcp"]
        .into_iter()
        .map(String::from)
        .collect();
    write_registry(
        &scratch,
        odh_entry(uv.to_str().unwrap(), args.clone(), &scratch.transcript()),
        |_| {},
    );
    let mut client = Client::start(&scratch.0);
    client.initialize("2025-06-18", json!({}));
    client.wait_for_tool("odh__odoo_search_read");

    let received = std::fs::read_to_string(&argv).expect("the launcher ran");
    assert_eq!(received.lines().collect::<Vec<_>>(), args);
    let log = std::fs::read_to_string(scratch.0.join("gateway.log")).unwrap_or_default();
    assert!(
        log.contains("connected 'odh'") && !log.contains("refusing to launch"),
        "{log}"
    );
}
