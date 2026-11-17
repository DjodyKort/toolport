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

/// Claude settings with a foreign rtk hook, an unrelated hook and a setting Toolport must not touch.
pub fn rtk_home(world: &World) {
    let dir = world.home.join(".claude");
    std::fs::create_dir_all(&dir).unwrap();
    let settings = json!({
        "theme": "dark",
        "hooks": {"PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "rtk hook claude"}]},
            {"matcher": "Write", "hooks": [{"type": "command", "command": "echo unrelated"}]}
        ]}
    });
    std::fs::write(
        dir.join("settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
}

/// One synthetic transcript: `turns` assistant messages from `at`; the first `times` of them call
/// `tool` and get a result of `result_len` bytes back.
fn write_session(
    world: &World,
    name: &str,
    at: &str,
    turns: usize,
    usage: (u64, u64, u64, u64),
    tool: Option<(&str, Value, usize, usize)>,
) {
    let dir = world.home.join(".claude/projects/demo");
    std::fs::create_dir_all(&dir).unwrap();
    let mut lines = Vec::new();
    for n in 0..turns {
        let calling = tool.as_ref().filter(|(_, _, _, times)| n < *times);
        let blocks = match calling {
            Some((tool, input, _, _)) => {
                json!([{"type": "tool_use", "id": format!("{name}-t{n}"), "name": tool, "input": input}])
            }
            None => json!([{"type": "text", "text": "synthetic"}]),
        };
        lines.push(
            json!({
                "type": "assistant",
                "uuid": format!("{name}-u{n}"),
                "cwd": "/work/demo",
                "timestamp": at,
                "message": {
                    "id": format!("{name}-m{n}"),
                    "role": "assistant",
                    "content": blocks,
                    "usage": {
                        "input_tokens": usage.0,
                        "cache_creation_input_tokens": usage.1,
                        "cache_read_input_tokens": usage.2,
                        "output_tokens": usage.3,
                    },
                },
            })
            .to_string(),
        );
        if let Some((_, _, len, _)) = calling {
            lines.push(
                json!({
                    "type": "user",
                    "timestamp": at,
                    "message": {"role": "user", "content": [
                        {"type": "tool_result", "tool_use_id": format!("{name}-t{n}"), "content": "x".repeat(*len)}
                    ]},
                })
                .to_string(),
            );
        }
    }
    std::fs::write(dir.join(format!("{name}.jsonl")), lines.join("\n") + "\n").unwrap();
}

/// Hooks wrapped on 11-05 and paused on 11-12, with sessions in each window, a short session and
/// counted calls: every number of `compression measure` has a source in this world.
pub fn measure_world(world: &World) {
    let usage = (100, 2000, 20_000, 400);
    for n in 0..9 {
        let tool = match n % 3 {
            0 => ("Bash", json!({"command": "git status"}), 4000, 1),
            1 => ("Read", json!({"file_path": "/work/demo/a.rs"}), 8000, 1),
            _ => ("mcp__demo__list", json!({}), 2000, 1),
        };
        write_session(
            world,
            &format!("on{n}"),
            "2026-11-08T10:00:00.000Z",
            5,
            usage,
            Some(tool),
        );
    }
    for n in 0..3 {
        write_session(
            world,
            &format!("paused{n}"),
            "2026-11-12T10:00:00.000Z",
            5,
            (100, 2000, 24_000, 400),
            Some(("Bash", json!({"command": "ls"}), 12_000, 2)),
        );
    }
    for n in 0..2 {
        write_session(
            world,
            &format!("off{n}"),
            "2026-10-20T10:00:00.000Z",
            6,
            usage,
            None,
        );
    }
    write_session(world, "short", "2026-11-09T10:00:00.000Z", 2, usage, None);

    let ledger = json!({
        "entries": [{
            "file": world.home.join(".claude/settings.json"),
            "provider": "rtk",
            "original": "rtk hook claude",
            "wrapped": "toolportctl compression hook rtk -- /bin/sh -c 'rtk hook claude'",
            "at": "2026-11-05T09:00:00.000Z"
        }],
        "pausedSince": null,
        "epochs": [
            {"event": "wrap", "at": "2026-11-05T09:00:00.000Z", "provider": "rtk"},
            {"event": "pause", "at": "2026-11-12T00:00:00.000Z"},
            {"event": "resume", "at": "2026-11-13T00:00:00.000Z"}
        ],
        "egressAccepted": []
    });
    std::fs::write(
        world.data.join("compression-hooks.json"),
        ledger.to_string(),
    )
    .unwrap();
    let counters: Vec<String> = (1..=8)
        .map(|n| {
            json!({
                "at": format!("2026-11-0{}T10:00:00.000Z", 5 + (n - 1) / 3),
                "provider": if n <= 6 { "rtk" } else { "read-once" },
                "bytesIn": 900,
                "bytesOut": 300,
                "latencyMs": 20 + n * 5,
                "answered": n % 2 == 0,
                "ok": true
            })
            .to_string()
        })
        .collect();
    std::fs::write(
        world.data.join("compression-hooks.jsonl"),
        counters.join("\n") + "\n",
    )
    .unwrap();
}

/// Claude settings that lost the hooks Toolport wrapped.
pub fn lose_hooks(world: &World) {
    std::fs::write(
        world.home.join(".claude/settings.json"),
        json!({"theme": "dark"}).to_string(),
    )
    .unwrap();
}
