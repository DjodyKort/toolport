//! `toolportctl direct run` driven the way a client drives it: the command, args and env of
//! a direct entry that `client direct add` wrote, started with nothing else, speaking MCP to
//! the real `mock-mcp-server` behind a wrapper that records what the child was handed.

#![cfg(unix)]

#[path = "common/exec.rs"]
mod exec_fixture;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const CTL: &str = env!("CARGO_BIN_EXE_toolportctl");
const MOCK: &str = env!("CARGO_BIN_EXE_mock-mcp-server");
const SECRET: &str = "FAKE-direct-secret-4c9e1a";
const REPLY_WAIT: Duration = Duration::from_secs(30);

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct World {
    root: PathBuf,
    data: PathBuf,
    home: PathBuf,
    work: PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl World {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "direct-launcher-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let world = Self {
            data: root.join("data"),
            home: root.join("home"),
            work: root.join("work"),
            root,
        };
        for dir in [&world.data, &world.home, &world.work] {
            std::fs::create_dir_all(dir).unwrap();
        }
        world
    }

    fn secret_key() -> String {
        "ab".repeat(32)
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("TOOLPORT_DATA_DIR", &self.data)
            .env("TOOLPORT_SECRET_KEY", Self::secret_key())
            .env("RUST_BACKTRACE", "0")
            .current_dir(&self.work);
        cmd
    }

    fn ctl(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let mut cmd = self.command(CTL);
        cmd.args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn toolportctl");
        if let Some(text) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        }
        child.wait_with_output().unwrap()
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.work.join(name);
        exec_fixture::write_executable(&path, &format!("#!/bin/sh\n{body}\n"));
        path
    }

    fn write_registry(&self, servers: Value) {
        let ids: Vec<Value> = servers
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].clone())
            .collect();
        std::fs::write(
            self.data.join("registry.json"),
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "servers": servers,
                "profiles": [{"id": "default", "name": "Default", "enabledServerIds": ids}],
                "activeProfileId": "default"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn seen(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.work.join(name)).ok()
    }

    fn client_file(&self) -> PathBuf {
        self.home.join(".claude.json")
    }

    fn client_config(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.client_file()).unwrap()).unwrap()
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn recorder(world: &World) -> PathBuf {
    world.script(
        "wrap.sh",
        &format!(
            "here=\"$(dirname \"$0\")\"\n\
             printf '%s' \"$DIRECT_API_KEY\" > \"$here/seen-key\"\n\
             printf '%s' \"$PLAIN_FLAG\" > \"$here/seen-flag\"\n\
             printf '%s' \"$*\" > \"$here/seen-args\"\n\
             env | grep '^TOOLPORT_' > \"$here/seen-toolport\"\n\
             exec \"{MOCK}\""
        ),
    )
}

fn server(id: &str, command: &Path, args: Value) -> Value {
    json!({
        "id": id, "name": id, "transport": "stdio",
        "command": command.to_string_lossy(), "args": args,
        "env": [{"key": "DIRECT_API_KEY", "secret": true}, {"key": "PLAIN_FLAG", "value": "on"}]
    })
}

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    next: i64,
}

impl Session {
    fn spawn(mut cmd: Command) -> Self {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the server side");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
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
            next: 0,
        }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self
                .lines
                .recv_timeout(REPLY_WAIT)
                .unwrap_or_else(|e| panic!("no answer to {method}: {e}"));
            let value: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("stdout is not JSON-RPC ({e}): {line}"));
            if value["id"] == id {
                assert!(value["error"].is_null(), "{method}: {value}");
                return value["result"].clone();
            }
        }
    }

    fn handshake(&mut self) {
        let init = self.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "direct-launcher-test", "version": "1"}}),
        );
        assert_eq!(init["serverInfo"]["name"], "mock-mcp-server");
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    }

    fn finish(mut self) -> std::process::ExitStatus {
        drop(self.stdin.take());
        self.child.wait().unwrap()
    }
}

fn mock_tools() -> Value {
    let mut session = Session::spawn(Command::new(MOCK));
    session.handshake();
    let tools = session.request("tools/list", json!({}));
    assert!(session.finish().success());
    tools
}

#[test]
fn a_client_entry_starts_the_server_with_its_secret_and_none_of_toolports_own() {
    let world = World::new("e2e");
    let wrapper = recorder(&world);
    world.write_registry(json!([server("dalpha", &wrapper, json!(["--serve", "fast"]))]));
    std::fs::write(world.client_file(), "{\"mcpServers\": {}}\n").unwrap();
    let set = world.ctl(&["secret", "set", "dalpha", "DIRECT_API_KEY"], Some(SECRET));
    assert!(set.status.success(), "{}", text(&set));

    let planned = world.ctl(
        &["--json", "client", "direct", "add", "dalpha", "--client", "claude-code", "--dry-run"],
        None,
    );
    assert!(planned.status.success(), "{}", text(&planned));
    assert_eq!(world.client_config(), json!({"mcpServers": {}}));

    let added = world.ctl(
        &["--json", "client", "direct", "add", "dalpha", "--client", "claude-code"],
        None,
    );
    assert!(added.status.success(), "{}", text(&added));
    let file = std::fs::read_to_string(world.client_file()).unwrap();
    assert!(!file.contains(SECRET), "a client file must hold no secret: {file}");
    assert!(!file.contains(&World::secret_key()), "{file}");
    assert!(!file.contains("wrap.sh"), "the server's own command stays out: {file}");
    let entry = world.client_config()["mcpServers"]["dalpha"].clone();
    assert_eq!(entry["args"], json!(["direct", "run", "dalpha"]));
    assert_eq!(
        std::fs::canonicalize(entry["command"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(CTL).unwrap()
    );

    let again = world.ctl(
        &["--json", "client", "direct", "add", "dalpha", "--client", "claude-code"],
        None,
    );
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(std::fs::read_to_string(world.client_file()).unwrap(), file);

    let mut cmd = world.command(entry["command"].as_str().unwrap());
    cmd.args(entry["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()));
    for (key, value) in entry["env"].as_object().into_iter().flatten() {
        cmd.env(key, value.as_str().unwrap());
    }
    let mut session = Session::spawn(cmd);
    session.handshake();
    let listed = session.request("tools/list", json!({}));
    assert_eq!(listed, mock_tools(), "the launcher must pass tools/list through untouched");
    assert!(listed["tools"].as_array().unwrap().len() >= 2);
    let echoed = session.request(
        "tools/call",
        json!({"name": "echo", "arguments": {"text": "through the launcher"}}),
    );
    assert_eq!(echoed["content"][0]["text"], "through the launcher");
    let sum = session.request("tools/call", json!({"name": "add", "arguments": {"a": 2, "b": 3}}));
    assert!(sum.to_string().contains('5'), "{sum}");
    assert!(session.finish().success(), "the server's exit status must come back");

    assert_eq!(world.seen("seen-key").as_deref(), Some(SECRET));
    assert_eq!(world.seen("seen-flag").as_deref(), Some("on"));
    assert_eq!(world.seen("seen-args").as_deref(), Some("--serve fast"));
    assert_eq!(
        world.seen("seen-toolport").as_deref(),
        Some(""),
        "no TOOLPORT_ variable may reach the server"
    );

    let listing = world.ctl(&["--json", "client", "direct", "ls"], None);
    assert!(listing.status.success());
    let rows: Value = serde_json::from_slice(&listing.stdout).unwrap();
    assert_eq!(rows["data"]["entries"][0]["server"], "dalpha");
    assert_eq!(rows["data"]["entries"][0]["state"], "ok");

    let sync = world.ctl(&["--json", "client", "sync", "--client", "claude-code"], None);
    assert!(sync.status.success(), "{}", text(&sync));
    assert_eq!(
        world.client_config()["mcpServers"]["dalpha"],
        entry,
        "client sync leaves a direct entry alone"
    );

    let removed = world.ctl(&["--json", "server", "uninstall", "dalpha"], None);
    assert!(removed.status.success(), "{}", text(&removed));
    assert!(
        world.client_config()["mcpServers"].get("dalpha").is_none(),
        "uninstalling the server removes its direct entries: {}",
        world.client_config()
    );
    let leaks = text(&set) + &text(&planned) + &text(&added) + &text(&again) + &text(&sync);
    assert!(!leaks.contains(SECRET), "{leaks}");
}

#[test]
fn the_launchers_exit_status_is_the_servers() {
    let world = World::new("exit");
    for (name, code) in [("seven", 7), ("zero", 0), ("one", 1)] {
        let script = world.script(&format!("{name}.sh"), &format!("exit {code}"));
        world.write_registry(json!([{
            "id": name, "name": name, "transport": "stdio",
            "command": script.to_string_lossy(), "args": []
        }]));
        let run = world.ctl(&["direct", "run", name], None);
        assert_eq!(run.status.code(), Some(code), "{name}: {}", text(&run));
        assert!(run.stdout.is_empty(), "{name}: {}", text(&run));
    }
}

#[test]
fn a_launcher_that_cannot_start_the_server_says_why_and_leaks_nothing() {
    let world = World::new("fail");
    let wrapper = recorder(&world);
    world.write_registry(json!([server("dalpha", &wrapper, json!([]))]));

    let missing = world.ctl(&["direct", "run", "dalpha"], None);
    assert_eq!(missing.status.code(), Some(1), "{}", text(&missing));
    assert!(text(&missing).contains("secret set dalpha"), "{}", text(&missing));
    assert!(world.seen("seen-key").is_none(), "the server must not start without its secret");

    let unknown = world.ctl(&["direct", "run", "nope"], None);
    assert_eq!(unknown.status.code(), Some(1), "{}", text(&unknown));
    assert!(text(&unknown).contains("client direct rm nope"), "{}", text(&unknown));

    world.write_registry(json!([{
        "id": "gone", "name": "gone", "transport": "stdio",
        "command": world.work.join("no-such-binary").to_string_lossy(), "args": []
    }]));
    let absent = world.ctl(&["direct", "run", "gone"], None);
    assert_eq!(absent.status.code(), Some(127), "{}", text(&absent));
    assert!(text(&absent).contains("no-such-binary"), "{}", text(&absent));
}

#[test]
fn servers_only_the_gateway_can_serve_are_refused_without_touching_the_client() {
    let world = World::new("refused");
    let script = world.script("plain.sh", "exit 0");
    world.write_registry(json!([
        {"id": "remote", "name": "remote", "transport": "http", "url": "https://example.invalid/mcp"},
        {"id": "oauth", "name": "oauth", "transport": "stdio",
         "command": script.to_string_lossy(), "args": [],
         "clientCredentials": {"clientId": "FAKE-client"}},
        {"id": "rooted", "name": "rooted", "transport": "stdio",
         "command": script.to_string_lossy(), "args": [], "cwd": "${ROOT}/sub"}
    ]));
    let original = "{\"mcpServers\": {\"keep\": {\"command\": \"keep-me\"}}}\n";
    std::fs::write(world.client_file(), original).unwrap();
    for name in ["remote", "oauth", "rooted"] {
        let add = world.ctl(
            &["--json", "client", "direct", "add", name, "--client", "claude-code"],
            None,
        );
        assert_ne!(add.status.code(), Some(0), "{name}: {}", text(&add));
        assert!(add.status.code().is_some(), "{name} must not crash: {}", text(&add));
        let envelope: Value = serde_json::from_slice(&add.stdout).unwrap();
        assert!(
            envelope["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("gateway"),
            "{name}: {envelope}"
        );
        let run = world.ctl(&["direct", "run", name], None);
        assert_ne!(run.status.code(), Some(0), "{name}: {}", text(&run));
        assert_eq!(std::fs::read_to_string(world.client_file()).unwrap(), original, "{name}");
    }
    let backups: Vec<_> = std::fs::read_dir(&world.home)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("bak"))
        .collect();
    assert!(backups.is_empty(), "a refusal writes nothing, backups included");
}
