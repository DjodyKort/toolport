//! The real gateway process hosts the OTLP receiver for as long as `obs otel` is enabled (MIG-OBS-3).

#![cfg(unix)]

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn ctl(data: &Path, home: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_toolportctl"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("TOOLPORT_SECRET_KEY", "ab".repeat(32))
        .arg("--json")
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .expect("run toolportctl");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{e}: {text:?}"))
}

fn http(port: u16, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect_timeout(
        &(Ipv4Addr::LOCALHOST, port).into(),
        Duration::from_millis(500),
    )
    .ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).ok()?;
    stream.write_all(body.as_bytes()).ok()?;
    let mut out = String::new();
    let _ = stream.read_to_string(&mut out);
    let status = out.split(' ').nth(1)?.parse().ok()?;
    Some((status, out.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()))
}

fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_gateway_serves_the_receiver_while_enabled_and_stops_it_when_disabled() {
    let base = std::env::temp_dir().join(format!("otel-gateway-host-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (data, home) = (base.join("data"), base.join("home"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let original = json!({"theme": "dark", "env": {"FOO": "bar"}});
    std::fs::write(home.join(".claude/settings.json"), original.to_string()).unwrap();

    let gateway = Command::new(env!("CARGO_BIN_EXE_toolport-gateway"))
        .env("TOOLPORT_REGISTRY", data.join("registry.json"))
        .env("TOOLPORT_DATA_DIR", &data)
        .env("TOOLPORT_GATEWAY_TOPOLOGY", "legacy")
        .env("TOOLPORT_CLIENT_ID", "otel-host-test")
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn gateway");
    let _gateway = ChildGuard(gateway);

    let before = ctl(&data, &home, &["obs", "otel", "status", "--home", home.to_str().unwrap()]);
    assert_eq!(before["data"]["receiver"]["state"], "disabled");
    assert!(http(port, "GET", "/healthz", "").is_none(), "nothing listens while disabled");

    let on = ctl(&data, &home, &["obs", "otel", "enable", "--port", &port.to_string(), "--home", home.to_str().unwrap()]);
    assert_eq!(on["ok"], true, "{on}");
    wait_for("the gateway to start the receiver", || {
        http(port, "GET", "/healthz", "").is_some_and(|(status, _)| status == 200)
    });
    let status = ctl(&data, &home, &["obs", "otel", "status", "--home", home.to_str().unwrap()]);
    assert_eq!(status["data"]["receiver"]["state"], "listening");

    let metrics = json!({"resourceMetrics": [{"scopeMetrics": [{"metrics": [
        {"name": "claude_code.token.usage", "sum": {"dataPoints": [
            {"timeUnixNano": "1759492800000000000", "asInt": "42",
             "attributes": [{"key": "type", "value": {"stringValue": "input"}},
                            {"key": "model", "value": {"stringValue": "claude-a"}}]}
        ]}}
    ]}]}]})
    .to_string();
    let (code, body) = http(port, "POST", "/v1/metrics", &metrics).expect("the receiver answers");
    assert_eq!(code, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap(), json!({"partialSuccess": {}}));
    let empty_root = base.join("projects");
    let usage = ctl(&data, &home, &["usage", "--root", empty_root.to_str().unwrap()]);
    assert_eq!(usage["data"]["otel"]["tokens"]["input"], 42.0, "{usage}");

    let off = ctl(&data, &home, &["obs", "otel", "disable", "--home", home.to_str().unwrap()]);
    assert_eq!(off["ok"], true, "{off}");
    wait_for("the gateway to stop the receiver", || {
        http(port, "GET", "/healthz", "").is_none()
    });
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(home.join(".claude/settings.json")).unwrap()).unwrap();
    assert_eq!(settings, original);
    let _ = std::fs::remove_dir_all(&base);
}
