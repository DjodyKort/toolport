#![allow(dead_code)]

//! A JSON-RPC client for the real `toolport-selfmcp` stdio binary, running in a `CtlWorld`.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ctl_world::CtlWorld;

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

pub struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    next_id: i64,
    forbidden: Vec<String>,
}

impl Client {
    /// `forbidden` strings must appear in no line the server writes.
    pub fn spawn(world: &CtlWorld, forbidden: &[&str]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_toolport-selfmcp"));
        command
            .current_dir(&world.repo)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in world.env() {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("spawn toolport-selfmcp");
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
            forbidden: forbidden.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").expect("write to the server");
        stdin.flush().expect("flush");
    }

    pub fn read_line(&mut self, what: &str) -> Value {
        let line = self
            .lines
            .recv_timeout(RESPONSE_TIMEOUT)
            .unwrap_or_else(|e| panic!("no answer to {what}: {e}"));
        for secret in &self.forbidden {
            assert!(
                !line.contains(secret.as_str()),
                "a secret value leaked in the answer to {what}"
            );
        }
        let value: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("stdout line is not JSON ({e}): {line}"));
        assert_eq!(value["jsonrpc"], "2.0", "{line}");
        value
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
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

    pub fn handshake(&mut self) {
        self.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "selfmcp-envelopes-test", "version": "1"}}),
        );
        self.send_raw(
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string(),
        );
    }

    pub fn call(&mut self, name: &str, args: Value) -> Reply {
        let reply = self.request("tools/call", json!({"name": name, "arguments": args}));
        assert!(
            reply.get("error").is_none(),
            "{name} must answer with a tool result, not an RPC error: {reply}"
        );
        Reply(reply["result"].clone())
    }

    pub fn list_tools(&mut self) -> Vec<Value> {
        self.request("tools/list", json!({}))["result"]["tools"]
            .as_array()
            .expect("tools array")
            .clone()
    }

    pub fn read(&mut self, uri: &str) -> Value {
        let reply = self.request("resources/read", json!({"uri": uri}));
        assert!(reply.get("error").is_none(), "{uri}: {reply}");
        reply["result"]["contents"][0].clone()
    }

    pub fn close(mut self) -> std::process::ExitStatus {
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

pub struct Reply(pub Value);

impl Reply {
    pub fn is_error(&self) -> bool {
        self.0["isError"].as_bool().expect("isError is a boolean")
    }

    pub fn data(&self) -> &Value {
        &self.0["structuredContent"]
    }

    pub fn text(&self) -> &str {
        self.0["content"][0]["text"].as_str().expect("text content")
    }

    pub fn error_kind(&self) -> Option<&str> {
        self.data()["error"]["kind"].as_str()
    }
}
