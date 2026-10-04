//! End-to-end MCP client against the real `toolport-selfmcp` stdio binary
//! (MIG-HRD-1): handshake, the tool and resource catalog, one call per confirm
//! tier (including the refusal without `confirm`) and every resource read.
//!
//! The binary runs with a synthetic data directory, home and skills
//! repository, so nothing outside the scratch tree is read or written.

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use conduit_lib::plus::selfmcp::{Gate, RESOURCES, TOOLS};
use serde_json::{json, Value};

#[path = "common/claude_stub.rs"]
mod claude_stub;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/sources_world.rs"]
mod sources_world;

const FAKE_SECRET: &str = "FAKE-SECRET-VALUE-do-not-print-7f3a";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const TOOL_COUNT: usize = 82;
const RESOURCE_COUNT: usize = 11;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct World {
    base: PathBuf,
    data: PathBuf,
    home: PathBuf,
    repo: PathBuf,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "selfmcp-stdio-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&base);
        let world = Self {
            data: base.join("data"),
            home: base.join("home"),
            repo: base.join("skills-repo"),
            base,
        };
        std::fs::create_dir_all(&world.data).unwrap();
        std::fs::create_dir_all(&world.home).unwrap();
        std::fs::write(
            world.data.join("registry.json"),
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "servers": [
                    {
                        "id": "srv-alpha", "name": "alpha", "transport": "stdio",
                        "command": "alpha-mcp", "args": [],
                        "env": [{"key": "API_KEY", "value": FAKE_SECRET, "secret": true},
                                {"key": "PLAIN", "value": FAKE_SECRET}]
                    },
                    {"id": "srv-beta", "name": "beta", "transport": "http",
                     "url": "https://example.invalid/mcp"}
                ],
                "profiles": [{"id": "default", "name": "Default",
                              "enabledServerIds": ["srv-alpha"]}],
                "activeProfileId": "default"
            }))
            .unwrap(),
        )
        .unwrap();
        for (rel, text) in [
            (
                "skills/demo/SKILL.md",
                "---\nname: demo\ndescription: A synthetic demo skill\n---\nBody text\n",
            ),
            (
                "agents/helper/AGENT.md",
                "---\nname: helper\ndescription: A synthetic helper agent\nmodel: inherit\n---\nAgent prompt\n",
            ),
            (
                "styles/plain/STYLE.md",
                "---\nname: plain\ndescription: A synthetic plain style\nkeep-coding-instructions: true\n---\nStyle text\n",
            ),
        ] {
            let path = world.repo.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        world
    }

    fn registry(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.data.join("registry.json")).unwrap())
            .unwrap()
    }

    fn server_names(&self) -> BTreeSet<String> {
        self.registry()["servers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap().to_string())
            .collect()
    }

    fn skill_file(&self) -> PathBuf {
        self.repo.join("skills/demo/SKILL.md")
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        collect(&self.base, &mut files);
        files
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn same_path(actual: &Value, expected: &Path) {
    let actual = Path::new(actual.as_str().expect("a path string"));
    assert_eq!(
        std::fs::canonicalize(actual).unwrap(),
        std::fs::canonicalize(expected).unwrap()
    );
}

fn collect(dir: &Path, into: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !path.ends_with("plus/cache") {
                collect(&path, into);
            }
        } else if path.file_name().and_then(|n| n.to_str()) != Some("registry.json.lock") {
            // the registry lock file is a flock sentinel that a read path may create
            into.insert(path.clone(), std::fs::read(&path).unwrap_or_default());
        }
    }
}

struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    next_id: i64,
}

impl Client {
    fn spawn(world: &World) -> Self {
        Self::spawn_with(world, &[])
    }

    fn spawn_with(world: &World, env: &[(&str, &Path)]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_toolport-selfmcp"))
            .current_dir(&world.repo)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &world.home)
            .env("XDG_CONFIG_HOME", world.home.join(".config"))
            .env("XDG_DATA_HOME", world.home.join(".local/share"))
            .env("XDG_CACHE_HOME", world.home.join(".cache"))
            .env("TOOLPORT_DATA_DIR", &world.data)
            .env("TOOLPORT_SECRET_KEY", "ab".repeat(32))
            .env("TOOLPORT_SOURCES_TIME_SCALE", "20")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn toolport-selfmcp");
        let stdin = child.stdin.take().unwrap();
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
            stdin: Some(stdin),
            lines,
            next_id: 0,
        }
    }

    fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").expect("write to the server");
        stdin.flush().expect("flush");
    }

    fn read_line(&mut self, what: &str) -> Value {
        let line = self
            .lines
            .recv_timeout(RESPONSE_TIMEOUT)
            .unwrap_or_else(|e| panic!("no answer to {what}: {e}"));
        let value: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
        assert!(
            !line.contains(FAKE_SECRET),
            "secret value leaked in {what}: {line}"
        );
        assert_eq!(value["jsonrpc"], "2.0", "{line}");
        value
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send_raw(
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string(),
        );
        let reply = self.read_line(method);
        assert_eq!(
            reply["id"], id,
            "reply to {method} carries another id: {reply}"
        );
        reply
    }

    fn notify(&mut self, method: &str) {
        self.send_raw(&json!({"jsonrpc": "2.0", "method": method}).to_string());
    }

    fn handshake(&mut self) -> Value {
        let init = self.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "selfmcp-stdio-test", "version": "1"}}),
        );
        self.notify("notifications/initialized");
        init
    }

    fn call(&mut self, name: &str, args: Value) -> Reply {
        let reply = self.request("tools/call", json!({"name": name, "arguments": args}));
        assert!(
            reply.get("error").is_none(),
            "{name} must answer with a tool result, not an RPC error: {reply}"
        );
        Reply(reply["result"].clone())
    }

    fn read(&mut self, uri: &str) -> Value {
        self.request("resources/read", json!({"uri": uri}))
    }

    fn close(mut self) -> std::process::ExitStatus {
        drop(self.stdin.take());
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "server did not exit on stdin EOF"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Reply(Value);

impl Reply {
    fn is_error(&self) -> bool {
        self.0["isError"].as_bool().expect("isError is a boolean")
    }

    fn data(&self) -> &Value {
        &self.0["structuredContent"]
    }

    fn error_kind(&self) -> &str {
        assert!(self.is_error(), "expected an error result: {}", self.0);
        self.0["structuredContent"]["error"]["kind"]
            .as_str()
            .expect("error kind")
    }

    fn error_message(&self) -> &str {
        self.0["structuredContent"]["error"]["message"]
            .as_str()
            .expect("error message")
    }

    fn ok(&self) -> &Value {
        assert!(!self.is_error(), "expected success: {}", self.0);
        let text = self.0["content"][0]["text"].as_str().expect("text content");
        let parsed: Value = serde_json::from_str(text).expect("text content is the JSON payload");
        assert_eq!(
            &parsed,
            self.data(),
            "text and structuredContent must agree"
        );
        self.data()
    }
}

fn sample_args(descriptor: &Value) -> Value {
    let schema = &descriptor["inputSchema"];
    let required: Vec<&str> = schema["required"]
        .as_array()
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut args = serde_json::Map::new();
    for name in required {
        let value = match schema["properties"][name]["type"].as_str() {
            Some("string") => json!("x"),
            Some("boolean") => json!(true),
            Some("object") => json!({}),
            Some("array") => json!(["x"]),
            Some("integer") => json!(1),
            other => panic!("unexpected parameter type {other:?} for {name}"),
        };
        args.insert(name.to_string(), value);
    }
    Value::Object(args)
}

fn listed_tools(client: &mut Client) -> Vec<Value> {
    client.request("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .expect("tools array")
        .clone()
}

#[test]
fn handshake_then_tools_and_resources_match_the_catalog() {
    let world = World::new("catalog");
    let mut client = Client::spawn(&world);
    let init = client.handshake();
    let result = &init["result"];
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert_eq!(result["serverInfo"]["name"], "toolport-plus-self");
    assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        result["capabilities"],
        json!({"tools": {}, "resources": {}})
    );
    assert!(result["instructions"]
        .as_str()
        .unwrap()
        .contains("mcpm://paths"));
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));

    let tools = listed_tools(&mut client);
    assert_eq!(tools.len(), TOOL_COUNT);
    assert_eq!(TOOLS.len(), TOOL_COUNT);
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    let catalog: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
    assert_eq!(names, catalog);
    assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), TOOL_COUNT);
    assert_eq!(
        Value::Array(tools.clone()),
        conduit_lib::plus::selfmcp::tools_list()
    );
    for (tool, def) in tools.iter().zip(TOOLS) {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{}", def.name);
        assert_eq!(schema["additionalProperties"], false, "{}", def.name);
        assert!(
            tool["description"]
                .as_str()
                .unwrap()
                .starts_with(&format!("[tier {} ", def.tier)),
            "{}",
            def.name
        );
        assert_eq!(tool["annotations"]["readOnlyHint"], def.tier == 1);
        assert_eq!(tool["annotations"]["destructiveHint"], def.tier >= 4);
        assert_eq!(
            schema["properties"].get("confirm").is_some(),
            def.gate != Gate::None,
            "{} exposes confirm exactly when it is gated",
            def.name
        );
    }

    let listed = client.request("resources/list", json!({}))["result"]["resources"]
        .as_array()
        .expect("resources array")
        .clone();
    assert_eq!(listed.len(), RESOURCE_COUNT);
    assert_eq!(RESOURCES.len(), RESOURCE_COUNT);
    assert_eq!(
        Value::Array(listed.clone()),
        conduit_lib::plus::selfmcp::resources_list()
    );
    for (resource, def) in listed.iter().zip(RESOURCES) {
        assert_eq!(resource["uri"], def.uri);
        assert_eq!(resource["mimeType"], def.mime);
        assert!(resource["uri"].as_str().unwrap().starts_with("mcpm://"));
    }
    assert!(client.close().success());
}

#[test]
fn every_gated_tool_refuses_without_confirm_and_changes_nothing() {
    let world = World::new("refusal");
    let mut client = Client::spawn(&world);
    client.handshake();
    let tools = listed_tools(&mut client);
    let before = world.snapshot();

    let mut seen = BTreeMap::new();
    for (descriptor, def) in tools.iter().zip(TOOLS) {
        if def.gate == Gate::None {
            continue;
        }
        let mut args = sample_args(descriptor);
        if descriptor["inputSchema"]["properties"]["dry_run"]["default"] == json!(true) {
            args["dry_run"] = json!(false);
        }
        for variant in [args.clone(), {
            let mut with_false = args.clone();
            with_false["confirm"] = json!(false);
            with_false
        }] {
            let reply = client.call(def.name, variant);
            assert_eq!(reply.error_kind(), "refused", "{}", def.name);
            let message = reply.error_message();
            assert!(message.contains(def.name), "{message}");
            assert!(message.contains("confirm=true"), "{message}");
            assert!(message.contains(&format!("tier {}", def.tier)), "{message}");
            assert_eq!(
                message.contains("WARNING"),
                def.tier >= 4,
                "{}: {message}",
                def.name
            );
            let text = reply.0["content"][0]["text"].as_str().unwrap();
            assert!(text.starts_with("refused: "), "{text}");
        }
        *seen.entry(def.tier).or_insert(0) += 1;
    }
    assert!(seen.keys().copied().eq([2, 3, 4]), "{seen:?}");
    assert_eq!(world.snapshot(), before, "a refused call must not write");
    assert!(client.close().success());
}

#[test]
fn confirm_must_be_a_boolean_and_unknown_arguments_are_rejected() {
    let world = World::new("validation");
    let mut client = Client::spawn(&world);
    client.handshake();
    let before = world.snapshot();

    let reply = client.call("skills_delete", json!({"name": "demo", "confirm": "yes"}));
    assert_eq!(reply.error_kind(), "invalid_arguments");
    let reply = client.call("skills_delete", json!({"confirm": true}));
    assert_eq!(reply.error_kind(), "invalid_arguments");
    assert!(reply.error_message().contains("name"));
    let reply = client.call("skills_list", json!({"confirm": true}));
    assert_eq!(
        reply.error_kind(),
        "invalid_arguments",
        "a read-only tool has no confirm parameter"
    );
    let reply = client.call("skills_list", json!({"bogus": 1}));
    assert_eq!(reply.error_kind(), "invalid_arguments");
    let reply = client.call("skills_get", json!({"name": 7}));
    assert_eq!(reply.error_kind(), "invalid_arguments");
    let reply = client.call("no_such_tool", json!({}));
    assert_eq!(reply.error_kind(), "unknown_tool");
    assert_eq!(world.snapshot(), before);
    assert!(client.close().success());
}

fn tier_one_calls() -> BTreeMap<&'static str, Value> {
    BTreeMap::from([
        ("skills_list", json!({})),
        ("sources_ls", json!({"items": true})),
        ("skills_get", json!({"name": "demo"})),
        ("skills_lint", json!({})),
        ("skills_status", json!({})),
        ("skills_list_transpilers", json!({})),
        ("skills_diff", json!({})),
        ("skills_audit", json!({})),
        ("skills_tap_list", json!({})),
        ("skills_search", json!({"query": "review"})),
        ("agents_list", json!({})),
        ("agents_diff", json!({})),
        ("agents_audit", json!({})),
        ("agents_status", json!({})),
        ("agents_get", json!({"name": "helper"})),
        ("agents_lint", json!({})),
        ("agents_list_transpilers", json!({})),
        ("styles_list", json!({})),
        ("styles_diff", json!({})),
        ("styles_status", json!({})),
        ("styles_get", json!({"name": "plain"})),
        ("styles_lint", json!({})),
        ("styles_active", json!({})),
        ("styles_list_transpilers", json!({})),
        ("servers_list", json!({})),
        ("servers_get", json!({"name": "alpha"})),
        ("servers_list_profiles", json!({})),
        ("servers_detect_source", json!({"name": "alpha"})),
        ("servers_git_status", json!({"name": "alpha"})),
        ("servers_check_updates", json!({})),
        ("clients_list", json!({})),
        ("client_direct_ls", json!({})),
        ("compression_status", json!({})),
        ("where_am_i", json!({})),
        ("doctor", json!({})),
        ("flow_diagram", json!({})),
    ])
}

#[test]
fn tier_one_tools_read_without_confirm_and_never_write() {
    let world = World::new("tier1");
    let mut client = Client::spawn(&world);
    client.handshake();
    let calls = tier_one_calls();
    let catalog: BTreeSet<&str> = TOOLS
        .iter()
        .filter(|t| t.tier == 1)
        .map(|t| t.name)
        .collect();
    assert_eq!(
        calls.keys().copied().collect::<BTreeSet<_>>(),
        catalog,
        "the table must cover exactly the tier 1 tools"
    );
    let before = world.snapshot();

    let mut results = BTreeMap::new();
    for (name, args) in &calls {
        let reply = client.call(name, args.clone());
        reply.ok();
        results.insert(*name, reply.data().clone());
    }
    assert_eq!(
        world.snapshot(),
        before,
        "tier 1 tools must leave the data dir, home and repository untouched"
    );

    assert_eq!(results["skills_list"]["skills"][0]["name"], "demo");
    assert_eq!(results["sources_ls"]["partial"], false);
    assert!(results["sources_ls"]["items"].is_array());
    assert_eq!(results["skills_get"]["body"], "Body text");
    assert_eq!(results["skills_diff"]["noLockfile"], true);
    assert_eq!(results["skills_diff"]["new"], json!(["demo"]));
    assert_eq!(results["skills_audit"]["clean"], true);
    assert_eq!(results["skills_audit"]["skillCount"], 1);
    assert_eq!(results["skills_tap_list"]["taps"], json!([]));
    assert_eq!(results["skills_search"]["tapCount"], 0);
    assert_eq!(results["skills_search"]["results"], json!([]));
    assert_eq!(results["agents_list"]["agents"][0]["name"], "helper");
    assert_eq!(results["agents_diff"]["new"], json!(["helper"]));
    assert_eq!(results["agents_audit"]["clean"], true);
    assert_eq!(results["agents_status"]["lockfilePresent"], false);
    assert_eq!(results["styles_diff"]["new"], json!(["plain"]));
    assert_eq!(results["styles_status"]["lockfilePresent"], false);
    assert_eq!(results["styles_list"]["styles"][0]["name"], "plain");
    assert!(results["skills_list_transpilers"]["transpilers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t == "claude-code"));
    let servers = results["servers_list"]["servers"].as_array().unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0]["name"], "alpha");
    assert_eq!(servers[0]["enabled"], true);
    assert_eq!(servers[1]["enabled"], false);
    assert_eq!(
        results["servers_get"]["env"],
        json!([{"key": "API_KEY", "secret": true}, {"key": "PLAIN", "secret": false}])
    );
    assert_eq!(results["servers_list_profiles"]["activeProfile"], "default");
    assert_eq!(results["client_direct_ls"]["entries"], json!([]));
    assert_eq!(results["compression_status"]["configExists"], false);
    assert_eq!(results["compression_status"]["provider"], "none");
    assert_eq!(results["servers_git_status"]["isGit"], false);
    same_path(&results["where_am_i"]["dataDir"], &world.data);
    assert_eq!(results["where_am_i"]["serverCount"], 2);
    assert!(results["doctor"]["checks"].as_array().unwrap().len() >= 4);
    assert!(results["flow_diagram"]["markdown"]
        .as_str()
        .unwrap()
        .contains("->"));
    assert!(client.close().success());
}

#[test]
fn tier_two_tools_write_additive_state_without_confirm() {
    let world = World::new("tier2");
    let mut client = Client::spawn(&world);
    client.handshake();

    let made = client.call("skills_scaffold", json!({"name": "fresh"}));
    let created = PathBuf::from(made.ok()["created_path"].as_str().unwrap());
    assert!(created.ends_with("skills/fresh/SKILL.md"), "{created:?}");
    assert!(created.is_file());
    assert_eq!(
        client
            .call("skills_scaffold", json!({"name": "fresh"}))
            .error_kind(),
        "conflict"
    );
    assert_eq!(
        client
            .call("skills_scaffold", json!({"name": "../escape"}))
            .error_kind(),
        "invalid_arguments"
    );
    let agent = client.call(
        "agents_scaffold",
        json!({"name": "scout", "model": "sonnet"}),
    );
    assert!(
        std::fs::read_to_string(agent.ok()["created_path"].as_str().unwrap())
            .unwrap()
            .contains("model: sonnet")
    );
    let style = client.call("styles_scaffold", json!({"name": "terse"}));
    assert!(Path::new(style.ok()["created_path"].as_str().unwrap()).is_file());

    let dry = client.call(
        "skills_sync",
        json!({"dry_run": true, "client_keys": ["claude-code"]}),
    );
    assert_eq!(dry.ok()["dryRun"], true);
    assert!(!world.home.join(".claude/skills").exists());
    let synced = client.call("skills_sync", json!({"client_keys": ["claude-code"]}));
    assert_eq!(synced.ok()["skillCount"], 2);
    assert!(world.home.join(".claude/skills/demo/SKILL.md").is_file());
    assert!(
        world.data.join("mcpm-skills.lock").is_file(),
        "sync records a lockfile"
    );
    let agents = client.call("agents_sync", json!({"client_keys": ["claude-code"]}));
    assert_eq!(agents.ok()["agentCount"], 2);
    assert!(world.home.join(".claude/agents/helper.md").is_file());
    client.call("styles_sync_tier1", json!({})).ok();

    let tagged = client.call(
        "servers_add_profile_tag",
        json!({"name": "beta", "profile_tag": "Default"}),
    );
    assert_eq!(tagged.ok()["profileTags"], json!(["Default"]));
    let profiles = world.registry()["profiles"].clone();
    assert!(profiles
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["enabledServerIds"] == json!(["srv-alpha", "srv-beta"])));
    let untagged = client.call(
        "servers_remove_profile_tag",
        json!({"name": "beta", "profile_tag": "Default"}),
    );
    assert_eq!(untagged.ok()["profileTags"], json!([]));

    assert_eq!(
        client.call("clients_sync", json!({})).error_kind(),
        "refused",
        "clients_sync is gated unless it is a dry run"
    );
    let dry = client.call(
        "clients_sync",
        json!({"dry_run": true, "safe": true, "force_legacy": true}),
    );
    assert_eq!(dry.ok()["dryRun"], true);
    assert_eq!(dry.ok()["ignoredOptions"], json!(["safe", "force_legacy"]));
    assert_eq!(
        client
            .call(
                "clients_sync",
                json!({"dry_run": true, "client": "no-such-client"})
            )
            .error_kind(),
        "not_found"
    );
    assert!(client.close().success());
}

#[test]
fn the_tap_tools_that_write_default_to_a_dry_run() {
    let world = World::new("taps");
    let mut client = Client::spawn(&world);
    client.handshake();
    let listed = listed_tools(&mut client);
    for name in [
        "skills_tap_add",
        "skills_tap_remove",
        "skills_tap_update",
        "skills_install",
    ] {
        let tool = listed.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["dry_run"]["default"], true,
            "{name}"
        );
    }
    let before = world.snapshot();

    let add = client.call("skills_tap_add", json!({"repo": "acme/skills"}));
    assert_eq!(add.ok()["dryRun"], true);
    assert_eq!(add.ok()["cloned"], false);
    assert_eq!(add.ok()["url"], "https://github.com/acme/skills.git");
    let install = client.call("skills_install", json!({"spec": "@acme/skills"}));
    assert_eq!(install.ok()["dryRun"], true);
    assert_eq!(install.ok()["tapMissing"], true);
    assert_eq!(
        client
            .call("skills_tap_remove", json!({"name": "acme-skills"}))
            .error_kind(),
        "not_found"
    );
    assert_eq!(
        client.call("skills_tap_update", json!({})).ok()["results"],
        json!([])
    );
    assert_eq!(
        client
            .call(
                "skills_tap_add",
                json!({"repo": "acme/skills", "name": "../x"})
            )
            .error_kind(),
        "invalid_arguments"
    );
    assert_eq!(world.snapshot(), before, "a default call must not write");
    assert!(client.close().success());
}

#[test]
fn the_direct_tools_plan_by_default_and_apply_on_request() {
    let world = World::new("direct");
    let config = world.home.join(".claude.json");
    std::fs::write(&config, "{\"mcpServers\": {}}\n").unwrap();
    let mut client = Client::spawn(&world);
    client.handshake();
    let listed = listed_tools(&mut client);
    for name in ["client_direct_add", "client_direct_rm"] {
        let tool = listed.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["dry_run"]["default"], true,
            "{name}"
        );
    }
    let before = world.snapshot();

    let args = json!({"server": "alpha", "client": "claude-code"});
    let planned = client.call("client_direct_add", args.clone());
    assert_eq!(planned.ok()["dryRun"], true);
    assert_eq!(planned.ok()["action"], "added");
    assert!(planned.ok()["launcher"]["command"]
        .as_str()
        .unwrap()
        .contains("toolportctl"));
    assert_eq!(planned.ok()["launcher"]["args"], json!(["direct", "run", "srv-alpha"]));
    assert_eq!(world.snapshot(), before, "a default call must not write");

    let mut apply = args.clone();
    apply["dry_run"] = json!(false);
    let added = client.call("client_direct_add", apply.clone());
    assert_eq!(added.ok()["action"], "added");
    assert_eq!(added.ok()["dryRun"], false);
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("\"alpha\""), "{text}");
    assert!(text.contains("srv-alpha"), "{text}");
    assert!(!text.contains(FAKE_SECRET), "a client file must hold no secret");
    assert_eq!(
        client.call("client_direct_add", apply.clone()).ok()["action"],
        "unchanged"
    );

    let listed = client.call("client_direct_ls", json!({"client": "claude-code"}));
    assert_eq!(listed.ok()["entries"][0]["server"], "srv-alpha");
    assert_eq!(listed.ok()["entries"][0]["state"], "ok");

    let kept = client.call("client_direct_rm", args.clone());
    assert_eq!(kept.ok()["dryRun"], true);
    assert!(std::fs::read_to_string(&config).unwrap().contains("srv-alpha"));
    let removed = client.call("client_direct_rm", apply);
    assert_eq!(removed.ok()["action"], "removed");
    assert!(!std::fs::read_to_string(&config).unwrap().contains("srv-alpha"));

    assert_eq!(
        client
            .call(
                "client_direct_add",
                json!({"server": "beta", "client": "claude-code", "dry_run": false})
            )
            .error_kind(),
        "invalid_arguments"
    );
    assert_eq!(
        client
            .call("client_direct_add", json!({"server": "alpha"}))
            .error_kind(),
        "invalid_arguments"
    );
    assert!(client.close().success());
}

#[test]
fn the_destructive_skills_tools_plan_by_default_and_apply_only_when_confirmed() {
    let world = World::new("destructive");
    let mut client = Client::spawn(&world);
    client.handshake();
    let listed = listed_tools(&mut client);
    for name in [
        "skills_bundle",
        "skills_unbundle",
        "skills_clean",
        "skills_uninstall",
        "skills_resolve",
    ] {
        let tool = listed.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["dry_run"]["default"], true,
            "{name}"
        );
        assert!(
            tool["description"]
                .as_str()
                .unwrap()
                .contains("dry_run is on by default"),
            "{name}"
        );
    }
    client
        .call("skills_sync", json!({"client_keys": ["claude-code"]}))
        .ok();
    let output = world.home.join(".claude/skills/demo/SKILL.md");
    let lockfile = world.data.join("mcpm-skills.lock");
    let original = std::fs::read(world.skill_file()).unwrap();
    assert!(output.is_file() && lockfile.is_file());
    let bundle = world.repo.join("skills-repo-bundle.zip");

    let before = world.snapshot();
    let planned = client.call("skills_bundle", json!({}));
    assert_eq!(planned.ok()["dryRun"], true);
    assert_eq!(world.snapshot(), before, "a default bundle must not write");
    let made = client.call("skills_bundle", json!({"dry_run": false}));
    assert_eq!(made.ok()["dryRun"], false);
    assert!(bundle.is_file());
    assert_eq!(
        client
            .call("skills_bundle", json!({"dry_run": false}))
            .error_kind(),
        "conflict",
        "a bundle never overwrites a file"
    );

    let before = world.snapshot();
    let planned = client.call("skills_uninstall", json!({"name": "demo"}));
    assert_eq!(planned.ok()["dryRun"], true);
    assert_eq!(planned.ok()["lockUpdated"], true);
    assert_eq!(world.snapshot(), before, "a default call must not write");
    let refused = client.call("skills_uninstall", json!({"name": "demo", "dry_run": false}));
    assert_eq!(refused.error_kind(), "refused");
    assert!(refused.error_message().contains("confirm=true"));
    assert_eq!(world.snapshot(), before, "a refused call must not write");
    for name in ["../agents/helper", "a/b", ".."] {
        let tampered = client.call(
            "skills_uninstall",
            json!({"name": name, "dry_run": false, "confirm": true}),
        );
        assert_eq!(tampered.error_kind(), "invalid_arguments", "{name}");
    }
    assert_eq!(world.snapshot(), before, "a tampered name must not write");
    let applied = client.call(
        "skills_uninstall",
        json!({"name": "demo", "dry_run": false, "confirm": true}),
    );
    assert_eq!(applied.ok()["dryRun"], false);
    assert!(!world.skill_file().exists() && !output.exists());
    assert!(world.repo.join("agents/helper/AGENT.md").is_file());

    let args = json!({"bundle_path": bundle.to_string_lossy()});
    let before = world.snapshot();
    let planned = client.call("skills_unbundle", args.clone());
    assert_eq!(planned.ok()["dryRun"], true);
    assert_eq!(planned.ok()["files"], json!(["skills/demo/SKILL.md"]));
    assert_eq!(world.snapshot(), before, "a default call must not write");
    let mut apply = args;
    apply["dry_run"] = json!(false);
    assert_eq!(
        client.call("skills_unbundle", apply.clone()).error_kind(),
        "refused"
    );
    assert_eq!(world.snapshot(), before, "a refused call must not write");
    apply["confirm"] = json!(true);
    client.call("skills_unbundle", apply).ok();
    assert_eq!(std::fs::read(world.skill_file()).unwrap(), original);

    client
        .call("skills_sync", json!({"client_keys": ["claude-code"]}))
        .ok();
    assert!(output.is_file());
    let before = world.snapshot();
    let planned = client.call("skills_clean", json!({}));
    assert_eq!(planned.ok()["dryRun"], true);
    assert_eq!(planned.ok()["lockfileRemoved"], true);
    assert_eq!(world.snapshot(), before, "a default call must not write");
    assert_eq!(
        client
            .call("skills_clean", json!({"dry_run": false}))
            .error_kind(),
        "refused"
    );
    assert_eq!(world.snapshot(), before, "a refused call must not write");
    let cleaned = client.call("skills_clean", json!({"dry_run": false, "confirm": true}));
    assert_eq!(cleaned.ok()["dryRun"], false);
    assert!(!output.exists() && !lockfile.exists());
    assert_eq!(std::fs::read(world.skill_file()).unwrap(), original);
    assert!(client.close().success());
}

#[test]
fn the_agent_and_style_clean_and_uninstall_tools_plan_by_default_and_apply_when_confirmed() {
    let world = World::new("agent-style");
    let mut client = Client::spawn(&world);
    client.handshake();
    let listed = listed_tools(&mut client);
    for name in ["agents_clean", "agents_uninstall", "styles_clean"] {
        let tool = listed.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["dry_run"]["default"], true,
            "{name}"
        );
        assert!(
            tool["description"]
                .as_str()
                .unwrap()
                .contains("dry_run is on by default"),
            "{name}"
        );
    }
    client
        .call("agents_sync", json!({"client_keys": ["claude-code"]}))
        .ok();
    client.call("styles_sync_tier1", json!({})).ok();
    let agent_output = world.home.join(".claude/agents/helper.md");
    let style_output = world.home.join(".claude/output-styles/plain.md");
    assert!(agent_output.is_file() && style_output.is_file());

    let before = world.snapshot();
    for (name, args) in [
        ("agents_clean", json!({})),
        ("agents_uninstall", json!({"name": "helper"})),
        ("styles_clean", json!({})),
    ] {
        assert_eq!(client.call(name, args.clone()).ok()["dryRun"], true, "{name}");
        let mut apply = args;
        apply["dry_run"] = json!(false);
        assert_eq!(client.call(name, apply).error_kind(), "refused", "{name}");
        assert_eq!(world.snapshot(), before, "{name} must not write by default");
    }
    let tampered = client.call(
        "agents_uninstall",
        json!({"name": "../skills/demo", "dry_run": false, "confirm": true}),
    );
    assert_eq!(tampered.error_kind(), "invalid_arguments");
    assert_eq!(world.snapshot(), before);

    let cleaned = client.call("styles_clean", json!({"dry_run": false, "confirm": true}));
    assert_eq!(cleaned.ok()["dryRun"], false);
    assert!(!style_output.exists() && agent_output.is_file());
    let cleaned = client.call("agents_clean", json!({"dry_run": false, "confirm": true}));
    assert_eq!(cleaned.ok()["dryRun"], false);
    assert!(!agent_output.exists());
    assert!(world.repo.join("agents/helper/AGENT.md").is_file());
    let gone = client.call(
        "agents_uninstall",
        json!({"name": "helper", "dry_run": false, "confirm": true}),
    );
    assert_eq!(gone.ok()["dryRun"], false);
    assert!(!world.repo.join("agents/helper").exists());
    assert!(world.skill_file().is_file());
    assert!(client.close().success());
}

#[test]
fn the_compression_tools_plan_by_default_and_apply_when_confirmed() {
    let world = World::new("compression");
    let mut client = Client::spawn(&world);
    client.handshake();
    let listed = listed_tools(&mut client);
    for name in [
        "compression_enable",
        "compression_disable",
        "compression_set_provider",
        "compression_use",
        "compression_sync",
        "compression_seal",
    ] {
        let tool = listed.iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(
            tool["inputSchema"]["properties"]["dry_run"]["default"], true,
            "{name}"
        );
        assert!(
            tool["description"]
                .as_str()
                .unwrap()
                .contains("dry_run is on by default"),
            "{name}"
        );
    }
    let enable = listed.iter().find(|t| t["name"] == "compression_enable").unwrap();
    assert_eq!(enable["inputSchema"]["properties"]["port"]["type"], "integer");

    let before = world.snapshot();
    let args = json!({"provider": "rtk-only", "port": 9411, "mode": "cache"});
    let planned = client.call("compression_enable", args.clone());
    assert_eq!(planned.ok()["dryRun"], true);
    assert_eq!(planned.ok()["provider"], "rtk-only");
    assert_eq!(world.snapshot(), before, "a default call must not write");
    let mut apply = args;
    apply["dry_run"] = json!(false);
    assert_eq!(
        client.call("compression_enable", apply.clone()).error_kind(),
        "refused"
    );
    assert_eq!(world.snapshot(), before, "a refused call must not write");
    for bad in [json!({"port": "9411"}), json!({"port": 0}), json!({"provider": "bogus"})] {
        let mut bad_apply = bad.clone();
        bad_apply["dry_run"] = json!(false);
        bad_apply["confirm"] = json!(true);
        assert_eq!(
            client.call("compression_enable", bad_apply).error_kind(),
            "invalid_arguments",
            "{bad}"
        );
    }
    assert_eq!(world.snapshot(), before);

    apply["confirm"] = json!(true);
    assert_eq!(client.call("compression_enable", apply).ok()["dryRun"], false);
    let status = client.call("compression_status", json!({}));
    assert_eq!(status.ok()["configExists"], true);
    assert_eq!(status.ok()["provider"], "rtk-only");
    assert_eq!(status.ok()["preset"]["port"], 9411);
    let config = PathBuf::from(status.ok()["configPath"].as_str().unwrap());
    same_path(
        &json!(config.parent().unwrap().to_string_lossy()),
        &world.data,
    );
    assert!(config.is_file());

    let before = world.snapshot();
    assert_eq!(
        client.call("compression_disable", json!({})).ok()["dryRun"],
        true
    );
    assert_eq!(
        client
            .call("compression_disable", json!({"dry_run": false}))
            .error_kind(),
        "refused"
    );
    assert_eq!(world.snapshot(), before);
    let done = client.call(
        "compression_disable",
        json!({"dry_run": false, "confirm": true}),
    );
    assert_eq!(done.ok()["provider"], "none");
    assert_eq!(
        client.call("compression_status", json!({})).ok()["provider"],
        "none"
    );
    assert!(client.close().success());
}

#[test]
fn tier_three_and_four_tools_run_only_when_confirmed() {
    let world = World::new("tier34");
    let mut client = Client::spawn(&world);
    client.handshake();

    let original = std::fs::read_to_string(world.skill_file()).unwrap();
    let args = json!({"name": "demo", "new_body": "Changed body\n"});
    assert_eq!(
        client.call("skills_edit_body", args.clone()).error_kind(),
        "refused"
    );
    assert_eq!(
        std::fs::read_to_string(world.skill_file()).unwrap(),
        original
    );
    let mut confirmed = args;
    confirmed["confirm"] = json!(true);
    let done = client.call("skills_edit_body", confirmed);
    assert!(done.ok()["newHash"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    assert_eq!(
        std::fs::read_to_string(world.skill_file()).unwrap(),
        "---\nname: demo\ndescription: A synthetic demo skill\n---\nChanged body\n"
    );
    let got = client.call("skills_get", json!({"name": "demo"}));
    assert_eq!(got.ok()["body"], "Changed body");

    let install = json!({"name": "gamma", "config": {"command": "gamma-mcp", "args": ["--flag"]}});
    assert_eq!(
        client.call("servers_install", install.clone()).error_kind(),
        "refused"
    );
    assert!(!world.server_names().contains("gamma"));
    let mut confirmed = install;
    confirmed["confirm"] = json!(true);
    assert_eq!(
        client.call("servers_install", confirmed).ok()["installed"],
        true
    );
    assert!(world.server_names().contains("gamma"));
    let got = client.call("servers_get", json!({"name": "gamma"}));
    assert_eq!(got.ok()["command"], "gamma-mcp");

    assert_eq!(
        client
            .call("servers_uninstall", json!({"name": "gamma"}))
            .error_kind(),
        "refused",
        "tier 4"
    );
    assert!(world.server_names().contains("gamma"));
    let gone = client.call(
        "servers_uninstall",
        json!({"name": "gamma", "confirm": true}),
    );
    assert_eq!(gone.ok()["name"], "gamma");
    assert!(!world.server_names().contains("gamma"));
    assert_eq!(
        client
            .call("servers_get", json!({"name": "gamma"}))
            .error_kind(),
        "not_found"
    );

    assert_eq!(client.call("sync_push", json!({})).error_kind(), "refused");
    let waived = client.call("sync_push", json!({"dry_run": true}));
    assert_ne!(
        waived.0["structuredContent"]["error"]["kind"], "refused",
        "a dry run waives the tier 4 gate: {}",
        waived.0
    );
    assert_eq!(
        client
            .call("skills_git_push", json!({"commit_message": "msg"}))
            .error_kind(),
        "refused"
    );
    assert!(client.close().success());
}

#[test]
fn all_eleven_resources_list_and_read() {
    let world = World::new("resources");
    let mut client = Client::spawn(&world);
    client.handshake();
    let before = world.snapshot();

    let mut bodies = BTreeMap::new();
    for def in RESOURCES {
        let reply = client.read(def.uri);
        let content = &reply["result"]["contents"][0];
        assert_eq!(content["uri"], def.uri, "{reply}");
        assert_eq!(content["mimeType"], def.mime, "{reply}");
        let text = content["text"].as_str().expect("text").to_string();
        if def.mime == "application/json" {
            serde_json::from_str::<Value>(&text)
                .unwrap_or_else(|e| panic!("{} is not JSON ({e}): {text}", def.uri));
        }
        bodies.insert(def.uri, text);
    }
    assert_eq!(bodies.len(), RESOURCE_COUNT);
    assert_eq!(world.snapshot(), before, "reading resources must not write");

    let status: Value = serde_json::from_str(&bodies["mcpm://status"]).unwrap();
    same_path(&status["dataDir"], &world.data);
    assert_eq!(bodies["mcpm://paths"], bodies["mcpm://status"]);
    assert_eq!(
        bodies["mcpm://inventory/skills"],
        "demo - A synthetic demo skill"
    );
    assert_eq!(
        bodies["mcpm://inventory/agents"],
        "helper - A synthetic helper agent"
    );
    assert_eq!(
        bodies["mcpm://inventory/styles"],
        "plain - A synthetic plain style"
    );
    assert_eq!(
        bodies["mcpm://inventory/servers"],
        "alpha [stdio/unknown]\nbeta [http/unknown]"
    );
    assert!(bodies["mcpm://flow"].contains("->"));
    assert!(bodies["mcpm://architecture"].contains("gateway"));
    assert!(bodies["mcpm://workflows"].contains("servers_install"));
    let clients: Value = serde_json::from_str(&bodies["mcpm://clients"]).unwrap();
    assert!(clients["clients"].is_array());
    let router: Value = serde_json::from_str(&bodies["mcpm://router/status"]).unwrap();
    assert_eq!(router["router"]["present"], false);
    assert_eq!(router["router"]["decision"], "D-008");
    assert!(client.close().success());
}

#[test]
fn malformed_and_unsupported_requests_get_errors_and_the_server_keeps_serving() {
    let world = World::new("protocol");
    let mut client = Client::spawn(&world);
    client.handshake();

    client.send_raw("{ this is not json");
    let parse = client.read_line("a malformed line");
    assert_eq!(parse["error"]["code"], -32700);
    assert_eq!(parse["id"], Value::Null);

    let unknown = client.request("no/such/method", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
    let nameless = client.request("tools/call", json!({"arguments": {}}));
    assert_eq!(nameless["error"]["code"], -32602);
    let uriless = client.request("resources/read", json!({}));
    assert_eq!(uriless["error"]["code"], -32602);
    let missing = client.read("mcpm://does-not-exist");
    assert_eq!(missing["error"]["code"], -32002);

    client.notify("notifications/unknown");
    client.send_raw("");
    assert_eq!(client.request("ping", json!({}))["result"], json!({}));
    let tools = listed_tools(&mut client);
    assert_eq!(tools.len(), TOOL_COUNT);
    let status = client.close();
    assert!(status.success(), "{status:?}");
}

#[test]
fn sources_ls_maps_the_fixture_home_and_filters_by_source_and_kind() {
    let world = World::new("sources");
    let fixture = sources_world::build_in(&world.base);
    let mut client = Client::spawn(&world);
    client.handshake();
    let before = world.snapshot();

    let all = client.call("sources_ls", json!({"items": true}));
    let all = all.ok();
    let ids: Vec<&str> = all["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    for id in ["repo:odh", "client:acme", "org", "library", "loose", "inert"] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let odh = all["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "repo:odh")
        .unwrap();
    assert_eq!(odh["freshness"]["behind"], sources_world::ODH_BEHIND);
    assert_eq!(odh["freshness"]["inCheckout"], false);
    assert_eq!(
        odh["tokens"]["basis"], "estimate",
        "the token object is not scrubbed as a secret"
    );
    assert!(odh["tokens"]["value"].as_u64().unwrap() > 0);

    let library = client.call("sources_ls", json!({"source": "library", "items": true}));
    let library = library.ok();
    assert_eq!(library["sources"].as_array().unwrap().len(), 1);
    assert!(library["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["sourceId"] == "library"));

    let rules = client.call("sources_ls", json!({"kind": "rule", "items": true}));
    let rules = rules.ok();
    assert!(!rules["items"].as_array().unwrap().is_empty());
    assert!(rules["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["kind"] == "rule"));

    let bad = client.call("sources_ls", json!({"kind": "bogus"}));
    assert_eq!(bad.error_kind(), "invalid_arguments");
    assert_eq!(world.snapshot(), before, "sources_ls writes nothing but its cache");
    assert!(fixture.library.starts_with(&world.home));
    assert!(client.close().success());
}

#[test]
fn context_measure_measures_through_the_stub_and_answers_the_second_call_from_the_cache() {
    let world = World::new("measure");
    let cwd = world.home.join("work/client-repo");
    std::fs::create_dir_all(cwd.join(".git")).unwrap();
    let stub = claude_stub::ClaudeStub::install(&world.base.join("claude"), &world.base);
    let mut client = Client::spawn_with(&world, &[("TOOLPORT_CLAUDE_BIN", stub.bin.as_path())]);
    client.handshake();
    let before = world.snapshot();
    let args = json!({"cwd": cwd, "without": ["skill:skill-1*"]});

    let first = client.call("context_measure", args.clone());
    let first = first.ok();
    assert_eq!(first["cached"], false);
    assert_eq!(first["runs"][0]["total"], 68_445);
    assert_eq!(first["runs"][0]["parts"]["cacheCreation"], 54_639);
    assert_eq!(first["runs"][1]["label"], "without skill:skill-1*");
    let saved = first["deltas"][0]["tokens"]
        .as_i64()
        .expect("deltas[].tokens is a number, not a redacted secret");
    assert!(saved < 0, "{first}");
    assert_eq!(stub.requests().len(), 2);

    let second = client.call("context_measure", args);
    assert_eq!(second.ok()["cached"], true);
    assert_eq!(second.ok()["deltas"][0]["tokens"], saved);
    assert_eq!(stub.requests().len(), 2, "the cache answers without a request");

    let bad = client.call("context_measure", json!({"cwd": world.home.join("absent")}));
    assert_eq!(bad.error_kind(), "invalid_arguments");
    let unknown = client.call("context_measure", json!({"cwd": cwd, "yes": true}));
    assert_eq!(unknown.error_kind(), "invalid_arguments");
    assert_eq!(stub.requests().len(), 2);

    let log = world.base.join("claude-stub.log");
    let without_cache = |mut files: BTreeMap<PathBuf, Vec<u8>>| {
        files.retain(|path, _| !path.starts_with(world.data.join("plus")) && path != &log);
        files
    };
    let after = without_cache(world.snapshot());
    let expected = without_cache(before);
    assert_eq!(after, expected, "a measurement writes nothing but its cache");
    assert!(world.data.join("plus/cache/measure").is_dir());
    assert!(client.close().success());
}
