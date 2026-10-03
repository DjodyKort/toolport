//! Gateway smoke on a registry produced by the mcpm import (MIG-MCP-6).
//!
//! Imports a synthetic mcpm config, with the real `mock-mcp-server` as the
//! downstream behind a relocated helper script and behind a direct command,
//! into a scratch data directory. Then starts the real gateway
//! (`--stdio-adapter`) on that directory and checks `tools/list` against the
//! name map, the `initialize` instructions and a `tools/call` round trip.

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use conduit_lib::plus::import_mcpm::{
    name_map, run, RunOptions, MAX_TOOL_NAME_LEN, TOOL_NAME_PREFIX,
};
use conduit_lib::registry;
use serde_json::{json, Value};

const MOCK_TOOLS: &[&str] = &[
    "echo",
    "add",
    "echo_meta",
    "grow",
    "die",
    "legacy_elicitation",
    "mrtr_confirm",
    "progress_ping",
    "pwd",
];
const INSTRUCTIONS: &str = "FAKE-smoke-instructions";

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    dir: PathBuf,
}

impl Client {
    fn start(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_toolport-gateway"))
            .arg("--stdio-adapter")
            .env("TOOLPORT_DATA_DIR", dir)
            .env("TOOLPORT_REGISTRY", dir.join("registry.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the adapter");
        let stdin = child.stdin.take().expect("adapter stdin");
        let stdout = child.stdout.take().expect("adapter stdout");
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
            dir: dir.to_path_buf(),
        }
    }

    fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{message}").expect("write request");
        self.stdin.flush().expect("flush request");
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .unwrap_or_else(|e| panic!("no answer to {method}: {e}"));
            let value: Value = serde_json::from_str(&line).expect("json line");
            if value["id"] == id {
                return value;
            }
        }
    }

    fn notify(&mut self, method: &str) {
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "method": method})
        )
        .expect("write notification");
        self.stdin.flush().expect("flush notification");
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
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

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_fixture(base: &Path, home: &Path) -> PathBuf {
    let mock = env!("CARGO_BIN_EXE_mock-mcp-server");
    let bin = home.join(".config/mcpm/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let script = bin.join("start-mock.sh");
    std::fs::write(&script, format!("#!/bin/sh\nexec \"{mock}\"\n")).unwrap();
    let root = base.join("mcpm");
    std::fs::create_dir_all(&root).unwrap();
    let servers = json!({
        "alpha-mock": {"name": "alpha-mock", "profile_tags": ["smoke"],
                       "command": mock, "args": []},
        "beta-mock": {"name": "beta-mock", "profile_tags": ["smoke"],
                      "command": "sh", "args": [script.to_string_lossy()]}
    });
    std::fs::write(root.join("servers.json"), servers.to_string()).unwrap();
    let manifest: BTreeMap<&str, &[&str]> =
        BTreeMap::from([("alpha-mock", MOCK_TOOLS), ("beta-mock", MOCK_TOOLS)]);
    std::fs::write(
        base.join("tools.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
    root
}

#[test]
fn imported_registry_serves_the_name_map_through_the_gateway() {
    let _lock = registry::data_dir_test_lock();
    std::env::set_var("TOOLPORT_SECRET_KEY", "ab".repeat(32));
    let base = std::env::temp_dir().join(format!("import-gateway-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _scratch = Scratch(base.clone());
    let data = base.join("data");
    let home = base.join("home");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let root = write_fixture(&base, &home);

    let opts = RunOptions {
        root,
        home: Some(home.to_string_lossy().into_owned()),
        ..RunOptions::default()
    };
    let _override = registry::DataDirOverride::set(&data);
    let plan = run(&opts).expect("import");
    assert!(plan.rejects.is_empty(), "{:?}", plan.rejects);
    assert_eq!(plan.servers.len(), 2);
    assert!(data
        .join("imported-scripts/config_mcpm_bin/start-mock.sh")
        .is_file());
    let map = name_map(&opts, &base.join("tools.json")).expect("name map");
    assert_eq!(map.map.len(), 2 * MOCK_TOOLS.len());
    assert!(map
        .map
        .values()
        .all(|n| n.starts_with(TOOL_NAME_PREFIX) && n.len() <= MAX_TOOL_NAME_LEN));

    registry::update(|reg| {
        reg.set_lazy_discovery(false);
        reg.gateway_instructions = Some(INSTRUCTIONS.to_string());
        let ids: Vec<String> = reg.servers.iter().map(|s| s.id.clone()).collect();
        let active = reg.active_profile_id.clone();
        for p in reg
            .profiles
            .iter_mut()
            .filter(|p| Some(&p.id) == active.as_ref())
        {
            p.enabled_server_ids = ids.clone();
        }
        Ok(())
    })
    .expect("full discovery");

    let mut client = Client::start(&data);
    let init = client.request(
        1,
        "initialize",
        json!({"protocolVersion": "2024-11-05", "capabilities": {},
               "clientInfo": {"name": "import-smoke", "version": "1"}}),
    );
    assert_eq!(init["result"]["instructions"], INSTRUCTIONS);
    client.notify("notifications/initialized");
    let list = client.request(2, "tools/list", json!({}));
    let listed: BTreeSet<String> = list["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .filter(|n| !n.starts_with("toolport_"))
        .map(|n| format!("{TOOL_NAME_PREFIX}{n}"))
        .collect();
    let expected: BTreeSet<String> = map.map.values().cloned().collect();
    assert_eq!(listed, expected);

    for (id, (server, args, want)) in [
        ("alpha_mock", json!({"text": "FAKE-ping"}), "FAKE-ping"),
        ("beta_mock", json!({"text": "FAKE-pong"}), "FAKE-pong"),
    ]
    .into_iter()
    .enumerate()
    {
        let reply = client.request(
            10 + id as i64,
            "tools/call",
            json!({"name": format!("{server}__echo"), "arguments": args}),
        );
        assert_eq!(
            reply["result"]["content"][0]["text"], want,
            "unexpected reply: {reply}"
        );
    }
    let sum = client.request(
        20,
        "tools/call",
        json!({"name": "beta_mock__add", "arguments": {"a": 2, "b": 3}}),
    );
    assert_eq!(sum["result"]["content"][0]["text"], "5", "{sum}");
}
