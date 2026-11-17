//! Worlds for the compression goldens: Claude Code transcripts with billed usage and a fake
//! proxy that answers `GET /health` on a local port below the ephemeral range.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;

use serde_json::{json, Value};

use super::world::World;

const HEALTH: &str = r#"{"ready":true,"config":{"max_items_after_crush":50,"protect_recent":null,"accuracy_guard":true}}"#;
const PROXY_PORT: u16 = 29214;

pub fn transcripts(world: &World) {
    let dir = world.home.join(".claude/projects/demo");
    std::fs::create_dir_all(&dir).unwrap();
    let lines: Vec<String> = (1..=6)
        .map(|n| {
            let content = if n % 2 == 0 {
                json!([{"type": "tool_use", "id": format!("t{n}"), "name": "mcp__github__list", "input": {}}])
            } else {
                json!([{"type": "text", "text": "synthetic"}])
            };
            let message: Value = json!({
                "type": "assistant",
                "uuid": format!("u-m{n}"),
                "sessionId": "s1",
                "cwd": "/work/demo",
                "timestamp": format!("2026-10-01T10:00:0{n}Z"),
                "message": {
                    "id": format!("m{n}"),
                    "role": "assistant",
                    "model": "claude-a",
                    "content": content,
                    "usage": {
                        "input_tokens": 10,
                        "output_tokens": 40,
                        "cache_creation_input_tokens": 500,
                        "cache_read_input_tokens": 7000,
                        "service_tier": "standard"
                    }
                }
            });
            message.to_string()
        })
        .collect();
    std::fs::write(dir.join("s1.jsonl"), lines.join("\n") + "\n").unwrap();
}

/// Answers `GET /health` for the rest of the test process, so the commands that read the live
/// posture have a proxy to read without touching one of the machine that runs the test.
pub fn fake_proxy(_world: &World) {
    static STARTED: Mutex<bool> = Mutex::new(false);
    let mut started = STARTED.lock().unwrap_or_else(|e| e.into_inner());
    if *started {
        return;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let listener = loop {
        match TcpListener::bind(("127.0.0.1", PROXY_PORT)) {
            Ok(listener) => break listener,
            Err(e) if std::time::Instant::now() < deadline => {
                eprintln!("port {PROXY_PORT} is busy for the fake proxy ({e}), retrying");
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(e) => panic!("port {PROXY_PORT} is needed for the fake proxy: {e}"),
        }
    };
    *started = true;
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{HEALTH}",
                HEALTH.len()
            );
        }
    });
}
