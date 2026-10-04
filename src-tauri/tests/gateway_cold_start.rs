//! Cold start of the gateway with servers that connect at different speeds.
//!
//! Three downstream servers: a fast stdio mock, a slow stdio mock held at its start gate until
//! the test opens it, and an http endpoint that answers every request with `401`. A client that
//! lists once right after `initialize` must get the whole catalog when the servers connect
//! within the configured cold-start bound, and the servers that are up plus a later
//! `notifications/tools/list_changed` when they do not. Nothing here asserts a duration: the
//! test decides when the slow server may start and polls with deadlines.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use conduit_lib::plus::gateway_build;
use conduit_lib::registry;
use serde_json::{json, Value};

const SECRET_KEY: &str = "abababababababababababababababababababababababababababababababab";
const LIST_CHANGED: &str = "notifications/tools/list_changed";
const REPLY_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug)]
enum Mode {
    /// The default topology: a stdio adapter in front of the host daemon.
    Adapter,
    /// The client-spawned gateway that serves the client itself.
    Legacy,
}

/// An http MCP endpoint that refuses every request.
struct Unauthorized {
    url: String,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Unauthorized {
    fn start() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let (hits, stop) = (hits.clone(), stop.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                        continue;
                    };
                    hits.fetch_add(1, Ordering::SeqCst);
                    let challenge = tiny_http::Header::from_bytes(
                        b"WWW-Authenticate".as_slice(),
                        b"Bearer".as_slice(),
                    )
                    .unwrap();
                    let _ = request.respond(
                        tiny_http::Response::from_string("")
                            .with_status_code(401)
                            .with_header(challenge),
                    );
                }
            })
        };
        Self {
            url: format!("http://127.0.0.1:{port}/mcp"),
            hits,
            stop,
            handle: Some(handle),
        }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

impl Drop for Unauthorized {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct World {
    base: PathBuf,
    data: PathBuf,
    home: PathBuf,
    gate: PathBuf,
    unauthorized: Unauthorized,
    _override: registry::DataDirOverride,
    _lock: std::sync::MutexGuard<'static, ()>,
}

fn open_gate(gate: &Path) {
    let _ = std::fs::write(gate, "");
}

impl World {
    fn new(tag: &str) -> Self {
        let lock = registry::data_dir_test_lock();
        let base =
            std::env::temp_dir().join(format!("gateway-cold-start-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("data");
        let home = base.join("home");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let guard = registry::DataDirOverride::set(&data);
        Self {
            gate: base.join("slow.gate"),
            base,
            data,
            home,
            unauthorized: Unauthorized::start(),
            _override: guard,
            _lock: lock,
        }
    }

    /// Full discovery, the named servers enabled in registry order, and the cold-start bound.
    fn write_registry(&self, servers: &[&str], cold_start_wait_ms: u64) {
        let mock = env!("CARGO_BIN_EXE_mock-mcp-server");
        let entry = |id: &str| match id {
            "fast" => json!({
                "id": "fast", "name": "fast", "transport": "stdio",
                "command": mock, "args": []
            }),
            "slow" => json!({
                "id": "slow", "name": "slow", "transport": "stdio",
                "command": mock, "args": [],
                "env": [{
                    "key": "MOCK_MCP_START_GATE",
                    "value": self.gate.to_string_lossy(),
                    "secret": false
                }]
            }),
            "broken" => json!({
                "id": "broken", "name": "broken", "transport": "http",
                "url": self.unauthorized.url
            }),
            other => panic!("unknown fixture server {other}"),
        };
        registry::update(|reg| {
            reg.set_lazy_discovery(false);
            reg.cold_start_wait_ms = Some(cold_start_wait_ms);
            for id in servers {
                reg.servers.push(serde_json::from_value(entry(id)).unwrap());
            }
            let active = reg.active_profile_id.clone();
            for profile in reg
                .profiles
                .iter_mut()
                .filter(|p| Some(&p.id) == active.as_ref())
            {
                profile.enabled_server_ids = servers.iter().map(|id| id.to_string()).collect();
            }
            Ok(())
        })
        .expect("write the registry");
        let stored: Value = serde_json::from_str(
            &std::fs::read_to_string(self.data.join("registry.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(stored["coldStartWaitMs"], cold_start_wait_ms, "{stored}");
        assert_eq!(
            stored["servers"].as_array().map(Vec::len),
            Some(servers.len()),
            "{stored}"
        );
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.data.join("gateway.log")).unwrap_or_default()
    }

    fn wait_for_log(&self, needle: &str, within: Duration) {
        let deadline = Instant::now() + within;
        while !self.log().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "the gateway log never said {needle:?}:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The build state of the gateway process that serves the client, read from the data
    /// directory the way `toolportctl status` does.
    fn build(&self) -> Option<gateway_build::BuildState> {
        gateway_build::read_live(&self.data).into_iter().next()
    }

    fn wait_for_build(
        &self,
        what: &str,
        within: Duration,
        ready: impl Fn(&gateway_build::BuildState) -> bool,
    ) -> gateway_build::BuildState {
        let deadline = Instant::now() + within;
        loop {
            if let Some(build) = self.build().filter(|build| ready(build)) {
                return build;
            }
            assert!(
                Instant::now() < deadline,
                "the build never reached {what}: {:?}\n{}",
                self.build(),
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `toolportctl --json status`, the data it reports for the gateway's build.
    fn ctl_build(&self) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
            .args(["--json", "status"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("TOOLPORT_DATA_DIR", &self.data)
            .env("TOOLPORT_SECRET_KEY", SECRET_KEY)
            .stdin(Stdio::null())
            .output()
            .expect("run toolportctl status");
        assert!(
            output.status.success(),
            "toolportctl status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value =
            serde_json::from_slice(&output.stdout).expect("toolportctl prints one JSON document");
        let data = envelope.get("data").unwrap_or(&envelope);
        data["gateway"].clone()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        open_gate(&self.gate);
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    seen: Vec<Value>,
    data: PathBuf,
    gate: PathBuf,
    next_id: i64,
}

impl Client {
    fn start(world: &World, mode: Mode) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_toolport-gateway"));
        match mode {
            Mode::Adapter => {
                command
                    .arg("--stdio-adapter")
                    .env_remove("TOOLPORT_GATEWAY_TOPOLOGY");
            }
            Mode::Legacy => {
                command.env("TOOLPORT_GATEWAY_TOPOLOGY", "legacy");
            }
        }
        let mut child = command
            .env("HOME", &world.home)
            .env("TOOLPORT_DATA_DIR", &world.data)
            .env("TOOLPORT_REGISTRY", world.data.join("registry.json"))
            .env("TOOLPORT_SECRET_KEY", SECRET_KEY)
            .env("TOOLPORT_CLIENT_ID", "cold-start-test")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the gateway");
        let stdin = child.stdin.take().expect("gateway stdin");
        let stdout = child.stdout.take().expect("gateway stdout");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            seen: Vec::new(),
            data: world.data.clone(),
            gate: world.gate.clone(),
            next_id: 0,
        }
    }

    fn write(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").expect("write to the gateway");
        self.stdin.flush().expect("flush to the gateway");
    }

    fn send(&mut self, method: &str, params: Value) -> i64 {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    /// Reads one more line into `seen`; false when nothing came before the deadline.
    fn pull(&mut self, deadline: Instant) -> bool {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.lines.recv_timeout(left) {
            Ok(line) => {
                self.seen
                    .push(serde_json::from_str(&line).expect("the gateway speaks json lines"));
                true
            }
            Err(_) => false,
        }
    }

    fn reply(&mut self, id: i64, within: Duration) -> Option<Value> {
        let deadline = Instant::now() + within;
        loop {
            if let Some(found) = self
                .seen
                .iter()
                .find(|message| message["id"] == id && message.get("method").is_none())
            {
                return Some(found.clone());
            }
            if !self.pull(deadline) {
                return None;
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send(method, params);
        self.reply(id, REPLY_DEADLINE)
            .unwrap_or_else(|| panic!("no answer to {method}"))
    }

    fn handshake(&mut self) -> Value {
        let init = self.request(
            "initialize",
            json!({"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "cold-start", "version": "1"}}),
        );
        self.write(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        init
    }

    /// Whether `method` arrived at or after message number `from`.
    fn notified_since(&mut self, method: &str, from: usize, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        loop {
            if self.seen[from..]
                .iter()
                .any(|message| message["method"] == method)
            {
                return true;
            }
            if !self.pull(deadline) {
                return false;
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // A mock held at its gate does not read its stdin: open it so it can see the end of it.
        open_gate(&self.gate);
        if let Ok(entries) = std::fs::read_dir(&self.data) {
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
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The names a client sees, without the gateway's own tools.
fn catalog(reply: &Value) -> BTreeSet<String> {
    reply["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("a tools array in {reply}"))
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .filter(|name| !name.starts_with("toolport_"))
        .map(String::from)
        .collect()
}

fn of_server(names: &BTreeSet<String>, server: &str) -> BTreeSet<String> {
    let prefix = format!("{server}__");
    names
        .iter()
        .filter_map(|name| name.strip_prefix(&prefix))
        .map(String::from)
        .collect()
}

fn assert_whole_catalog(names: &BTreeSet<String>, log: &str) {
    let fast = of_server(names, "fast");
    assert!(
        fast.contains("echo"),
        "the fast server is listed: {names:?}\n{log}"
    );
    assert_eq!(
        of_server(names, "slow"),
        fast,
        "the slow server lists the same tools as the fast one: {names:?}\n{log}"
    );
    assert!(
        of_server(names, "broken").is_empty(),
        "a server that answers 401 lists nothing: {names:?}"
    );
}

fn complete_catalog_within_the_bound(mode: Mode) {
    let world = World::new(&format!("within-{mode:?}"));
    world.write_registry(&["fast", "slow", "broken"], 120_000);
    let mut client = Client::start(&world, mode);
    let init = client.handshake();
    assert_eq!(
        init["result"]["serverInfo"]["name"], "toolport-gateway",
        "{init}"
    );

    let list = client.send("tools/list", json!({}));
    world.wait_for_log(
        "tools/list waiting up to 120000 ms for the initial catalog build",
        REPLY_DEADLINE,
    );
    let building =
        world.wait_for_build("one connected and one failed server", REPLY_DEADLINE, |b| {
            b.servers_connected == 1 && b.servers_failed == 1
        });
    assert!(building.building, "{building:?}");
    assert_eq!(building.servers_total, 3, "{building:?}");
    assert!(
        client.reply(list, Duration::from_millis(300)).is_none(),
        "the list is held while a server is still connecting"
    );
    assert!(
        world.unauthorized.hits() >= 1,
        "the failing server was asked"
    );

    open_gate(&world.gate);
    let reply = client
        .reply(list, REPLY_DEADLINE)
        .expect("the list is answered");
    let names = catalog(&reply);
    assert_whole_catalog(&names, &world.log());
    let log = world.log();
    assert!(
        log.contains("stopped because the build completed"),
        "the wait ended with the build, not with the bound:\n{log}"
    );

    let done = world.build().expect("the gateway publishes its build");
    assert!(!done.building, "{done:?}");
    assert_eq!(
        (
            done.servers_total,
            done.servers_connected,
            done.servers_failed
        ),
        (3, 2, 1),
        "{done:?}"
    );
    assert_eq!(done.tools_so_far, names.len(), "{done:?}");
}

#[test]
fn the_first_list_returns_the_whole_catalog_when_the_servers_connect_within_the_bound() {
    complete_catalog_within_the_bound(Mode::Adapter);
}

#[test]
fn the_first_list_of_a_legacy_gateway_returns_the_whole_catalog_within_the_bound() {
    complete_catalog_within_the_bound(Mode::Legacy);
}

fn partial_catalog_then_list_changed(mode: Mode) {
    let world = World::new(&format!("partial-{mode:?}"));
    world.write_registry(&["fast", "slow", "broken"], 1_500);
    let mut client = Client::start(&world, mode);
    client.handshake();

    // The fast server is up and the failing one has given up before the client asks, so what
    // the bound cuts off is the slow server alone.
    world.wait_for_build("one connected and one failed server", REPLY_DEADLINE, |b| {
        b.servers_connected == 1 && b.servers_failed == 1
    });
    let reply = client.request("tools/list", json!({}));
    let partial = catalog(&reply);
    assert!(
        of_server(&partial, "fast").contains("echo"),
        "the servers that are up are listed: {partial:?}\n{}",
        world.log()
    );
    assert!(
        of_server(&partial, "slow").is_empty() && of_server(&partial, "broken").is_empty(),
        "{partial:?}"
    );
    let log = world.log();
    assert!(
        log.contains("tools/list waiting up to 1500 ms for the initial catalog build"),
        "{log}"
    );
    assert!(
        log.contains("stopped because the 1500 ms bound was reached"),
        "{log}"
    );

    let building = world.build().expect("the gateway publishes its build");
    assert!(building.building, "{building:?}");
    assert_eq!(
        (
            building.servers_total,
            building.servers_connected,
            building.servers_failed
        ),
        (3, 1, 1),
        "{building:?}"
    );
    assert_eq!(building.tools_so_far, partial.len(), "{building:?}");
    let reported = world.ctl_build();
    assert_eq!(reported["build"]["building"], true, "{reported}");
    assert_eq!(reported["build"]["serversTotal"], 3, "{reported}");
    assert_eq!(reported["build"]["serversConnected"], 1, "{reported}");
    assert_eq!(reported["build"]["serversFailed"], 1, "{reported}");
    assert_eq!(reported["build"]["toolsSoFar"], partial.len(), "{reported}");

    let mark = client.seen.len();
    open_gate(&world.gate);
    assert!(
        client.notified_since(LIST_CHANGED, mark, REPLY_DEADLINE),
        "the client is told when the rest of the catalog arrives\n{}",
        world.log()
    );
    let names = catalog(&client.request("tools/list", json!({})));
    assert_whole_catalog(&names, &world.log());

    world.wait_for_build("the end of the build", REPLY_DEADLINE, |b| !b.building);
    let done = world.build().expect("the gateway publishes its build");
    assert_eq!(
        (done.servers_connected, done.servers_failed),
        (2, 1),
        "{done:?}"
    );
    assert_eq!(done.tools_so_far, names.len(), "{done:?}");
}

#[test]
fn a_list_past_the_bound_gets_the_servers_that_are_up_and_a_later_list_changed() {
    partial_catalog_then_list_changed(Mode::Adapter);
}

#[test]
fn a_legacy_gateway_past_the_bound_answers_partially_and_announces_the_rest() {
    partial_catalog_then_list_changed(Mode::Legacy);
}

#[test]
fn a_failing_server_does_not_hold_the_wait_to_the_bound() {
    let world = World::new("failing");
    // The largest bound the setting allows: the list can only come back early because the
    // build ended, and the log says which of the two it was.
    world.write_registry(&["fast", "broken"], registry::MAX_COLD_START_WAIT_MS);
    let mut client = Client::start(&world, Mode::Adapter);
    client.handshake();

    let names = catalog(&client.request("tools/list", json!({})));
    assert!(
        of_server(&names, "fast").contains("echo"),
        "{names:?}\n{}",
        world.log()
    );
    assert!(of_server(&names, "broken").is_empty(), "{names:?}");
    assert!(
        world.unauthorized.hits() >= 1,
        "the failing server was asked"
    );
    let log = world.log();
    assert!(
        !log.contains("bound was reached"),
        "the bound was not what ended the wait:\n{log}"
    );
    if log.contains("tools/list waiting up to") {
        assert!(log.contains("stopped because the build completed"), "{log}");
    }

    let done = world.build().expect("the gateway publishes its build");
    assert!(!done.building, "{done:?}");
    assert_eq!(
        (
            done.servers_total,
            done.servers_connected,
            done.servers_failed
        ),
        (2, 1, 1),
        "{done:?}"
    );
}
