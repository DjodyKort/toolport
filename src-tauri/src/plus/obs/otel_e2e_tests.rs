use super::otel::fixtures::{logs_payload, metrics_payload};
use super::otel_host::Host;
use super::receiver::{probe, Probe};
use super::receiver_tests::wait_closed;
use super::store::iso_to_ms;
use super::transcript::fixtures::{assistant, with_request_id};
use crate::plus::ctl::run_with;
use crate::plus::testutil::DataDirFx;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn ctl(args: &[&str]) -> (i32, String, String) {
    let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&argv, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn ctl_json(args: &[&str]) -> (i32, Value) {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    let (code, out, err) = ctl(&full);
    let text = if out.trim().is_empty() { &err } else { &out };
    (code, serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{e}: {out:?} {err:?}")))
}

fn free_port() -> u16 {
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    wait_closed(port);
    port
}

fn post(port: u16, path: &str, body: &Value) -> u16 {
    let payload = serde_json::to_vec(body).unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    let mut out = String::new();
    let _ = stream.read_to_string(&mut out);
    out.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
}

struct ApiRequest<'a> {
    request_id: &'a str,
    session: &'a str,
    ts: &'a str,
    tokens: (u64, u64, u64, u64),
}

fn api_requests(list: &[ApiRequest]) -> Value {
    let kv = |k: &str, v: Value| json!({"key": k, "value": v});
    let records: Vec<Value> = list
        .iter()
        .map(|r| {
            let nanos = (iso_to_ms(r.ts).unwrap() as i128 * 1_000_000).to_string();
            let mut attrs = vec![
                kv("event.name", json!({"stringValue": "claude_code.api_request"})),
                kv("session.id", json!({"stringValue": r.session})),
                kv("model", json!({"stringValue": "claude-a"})),
                kv("cost_usd", json!({"doubleValue": 0.1})),
                kv("duration_ms", json!({"intValue": "900"})),
                kv("input_tokens", json!({"intValue": r.tokens.0.to_string()})),
                kv("output_tokens", json!({"intValue": r.tokens.1.to_string()})),
                kv("cache_creation_tokens", json!({"intValue": r.tokens.2.to_string()})),
                kv("cache_read_tokens", json!({"intValue": r.tokens.3.to_string()})),
            ];
            if !r.request_id.is_empty() {
                attrs.push(kv("request_id", json!({"stringValue": r.request_id})));
            }
            json!({"timeUnixNano": nanos, "attributes": attrs})
        })
        .collect();
    json!({"resourceLogs": [{"scopeLogs": [{"logRecords": records}]}]})
}

struct Fx {
    fx: DataDirFx,
}

impl Fx {
    fn new(tag: &str) -> Fx {
        Fx {
            fx: DataDirFx::new("otel-e2e", tag),
        }
    }

    fn obs(&self) -> PathBuf {
        self.fx.dir.join("obs")
    }

    fn home(&self) -> String {
        self.fx.dir.join("home").to_string_lossy().into_owned()
    }

    fn settings(&self) -> PathBuf {
        self.fx.dir.join("home/.claude/settings.json")
    }

    fn write_transcripts(&self) -> String {
        let root = self.fx.dir.join("projects/demo");
        std::fs::create_dir_all(&root).unwrap();
        let lines = [
            with_request_id(
                &assistant("msg_1", "s1", "2026-10-01T10:00:00Z", "claude-a", (10, 40, 500, 7000), &[]),
                "req_1",
            ),
            with_request_id(
                &assistant("msg_2", "s1", "2026-10-01T10:05:00Z", "claude-a", (3, 4, 0, 100), &[]),
                "req_2",
            ),
        ];
        std::fs::write(root.join("s1.jsonl"), lines.join("\n") + "\n").unwrap();
        self.fx.dir.join("projects").to_string_lossy().into_owned()
    }

    fn usage(&self, root: &str) -> Value {
        let (code, value) = ctl_json(&["usage", "--root", root]);
        assert_eq!(code, 0, "{value}");
        value["data"].clone()
    }
}

#[test]
fn otel_events_received_over_tcp_change_the_summary_and_usage_without_double_counting() {
    let fx = Fx::new("dedupe");
    let root = fx.write_transcripts();
    let base = fx.usage(&root);
    assert_eq!(base["totals"]["messages"], 2);
    assert_eq!(base["sources"], json!({"transcriptMessages": 2, "otelRequests": 0, "otelOnly": 0}));
    let (_, human, _) = ctl(&["usage", "--root", &root]);
    assert!(!human.contains("otel:"), "{human}");

    let receiver = super::receiver::Receiver::start(&fx.obs(), 0).unwrap();
    let port = receiver.port();
    assert_eq!(post(port, "/v1/metrics", &metrics_payload()), 200);
    assert_eq!(post(port, "/v1/logs", &logs_payload()), 200);

    let after_fixture = fx.usage(&root);
    assert_eq!(after_fixture["otel"]["events"], 10);
    assert_eq!(after_fixture["otel"]["tokens"]["input"], 100.0);
    assert_eq!(after_fixture["otel"]["apiRequests"]["count"], 1);
    assert_eq!(after_fixture["sources"], json!({"transcriptMessages": 2, "otelRequests": 1, "otelOnly": 1}));
    assert_eq!(after_fixture["totals"]["messages"], 3, "a request only OTel saw is counted");
    assert_eq!(after_fixture["totals"]["input"], 13 + 10);
    let (_, human, _) = ctl(&["usage", "--root", &root]);
    assert!(
        human.contains("otel: 1 api requests, 1 not in the transcripts"),
        "{human}"
    );
    assert!(human.contains("indexed 2 messages from 1 transcript files"));

    let same_request_through_both = api_requests(&[ApiRequest {
        request_id: "req_1",
        session: "s1",
        ts: "2026-10-01T10:00:03Z",
        tokens: (10, 40, 500, 7000),
    }]);
    assert_eq!(post(port, "/v1/logs", &same_request_through_both), 200);
    assert_eq!(post(port, "/v1/logs", &same_request_through_both), 200, "an exporter retry");
    let deduped = fx.usage(&root);
    assert_eq!(deduped["totals"], after_fixture["totals"], "the request counts once");
    assert_eq!(deduped["byDay"], after_fixture["byDay"]);
    assert_eq!(deduped["bySession"], after_fixture["bySession"]);
    assert_eq!(deduped["sources"], json!({"transcriptMessages": 2, "otelRequests": 2, "otelOnly": 1}));
    assert_eq!(deduped["otel"]["apiRequests"]["count"], 2, "the retried delivery is one request");

    let without_id = api_requests(&[ApiRequest {
        request_id: "",
        session: "s1",
        ts: "2026-10-01T10:05:02Z",
        tokens: (3, 4, 0, 100),
    }]);
    assert_eq!(post(port, "/v1/logs", &without_id), 200);
    let matched = fx.usage(&root);
    assert_eq!(matched["totals"], after_fixture["totals"], "no id: matched on session, model and tokens");
    assert_eq!(matched["sources"]["otelOnly"], 1);

    let fresh = api_requests(&[ApiRequest {
        request_id: "req_9",
        session: "s1",
        ts: "2026-10-02T08:00:00Z",
        tokens: (1, 2, 3, 4),
    }]);
    assert_eq!(post(port, "/v1/logs", &fresh), 200);
    let more = fx.usage(&root);
    assert_eq!(more["totals"]["messages"], 4);
    assert_eq!(more["totals"]["cacheRead"], 7100 + 4);
    assert_eq!(more["byDay"]["2026-10-02"]["messages"], 1);
    assert_eq!(more["sources"], json!({"transcriptMessages": 2, "otelRequests": 4, "otelOnly": 2}));

    let summary = crate::plus::dispatch("plus.obs.summary", json!({"root": root})).unwrap();
    assert_eq!(summary["totals"], more["totals"], "the IPC handler agrees with the ctl output");
    drop(receiver);
}

#[test]
fn enable_serve_ingest_and_disable_round_trip_through_toolportctl() {
    let fx = Fx::new("roundtrip");
    let home = fx.home();
    let original = json!({"theme": "dark", "env": {"FOO": "bar"}});
    std::fs::create_dir_all(fx.settings().parent().unwrap()).unwrap();
    std::fs::write(fx.settings(), serde_json::to_string_pretty(&original).unwrap()).unwrap();
    let port = free_port();
    let port_text = port.to_string();

    let (code, plan) = ctl_json(&["obs", "otel", "enable", "--port", &port_text, "--home", &home, "--dry-run"]);
    assert_eq!(code, 0, "{plan}");
    assert_eq!(plan["command"], "obs otel enable");
    assert_eq!(plan["data"]["dryRun"], true);
    assert!(!fx.obs().join("otel.json").exists());
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(fx.settings()).unwrap()).unwrap(), original);

    let (code, on) = ctl_json(&["obs", "otel", "enable", "--port", &port_text, "--home", &home]);
    assert_eq!(code, 0, "{on}");
    assert_eq!(on["data"]["endpoint"], format!("http://127.0.0.1:{port}"));
    let written: Value = serde_json::from_str(&std::fs::read_to_string(fx.settings()).unwrap()).unwrap();
    assert_eq!(written["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"], format!("http://127.0.0.1:{port}"));
    assert_eq!(written["env"]["FOO"], "bar");

    let (_, status) = ctl_json(&["obs", "otel", "status", "--home", &home]);
    assert_eq!(status["data"]["enabled"], true);
    assert_eq!(status["data"]["settings"]["state"], "configured");
    assert_eq!(status["data"]["receiver"]["state"], "stopped", "no gateway is running yet");

    let log = RefCell::new(Vec::<String>::new());
    let record = |line: &str| log.borrow_mut().push(line.to_string());
    let mut host = Host::new(&fx.obs());
    host.tick(Instant::now(), &record);
    assert_eq!(host.port(), Some(port));
    assert_eq!(log.borrow().as_slice(), [format!("otel receiver listening on 127.0.0.1:{port}")]);
    assert_eq!(probe(port), Probe::Ours);
    host.tick(Instant::now(), &record);
    assert_eq!(log.borrow().len(), 1, "a steady host stays quiet");

    let (_, status) = ctl_json(&["obs", "otel", "status", "--home", &home]);
    assert_eq!(status["data"]["receiver"]["state"], "listening");
    let (_, human, _) = ctl(&["obs", "otel", "status", "--home", &home]);
    assert!(human.contains("receiver: listening at http://127.0.0.1:"), "{human}");
    assert!(human.contains("(configured)"), "{human}");

    let root = fx.write_transcripts();
    assert_eq!(post(port, "/v1/metrics", &metrics_payload()), 200);
    assert_eq!(post(port, "/v1/logs", &logs_payload()), 200);
    let usage = fx.usage(&root);
    assert_eq!(usage["otel"]["events"], 10);
    let (_, status) = ctl_json(&["obs", "otel", "status", "--home", &home]);
    assert_eq!(status["data"]["events"]["count"], 10);
    assert_eq!(status["data"]["events"]["latest"], "2025-10-03T12:00:01Z");

    let (code, off) = ctl_json(&["obs", "otel", "disable", "--home", &home]);
    assert_eq!(code, 0, "{off}");
    assert_eq!(off["data"]["changed"], true);
    host.tick(Instant::now(), &record);
    assert_eq!(host.port(), None);
    assert!(log.borrow().last().unwrap().contains("otel receiver stopped"));
    wait_closed(port);
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(fx.settings()).unwrap()).unwrap(),
        original,
        "the settings are back as the user had them"
    );
    let (_, status) = ctl_json(&["obs", "otel", "status", "--home", &home]);
    assert_eq!(status["data"]["enabled"], false);
    assert_eq!(status["data"]["receiver"]["state"], "disabled");
    assert_eq!(status["data"]["events"]["count"], 10, "the collected events stay");
    assert!(!fx.obs().join("otel-receiver.json").exists());
}

#[test]
fn a_conflicting_settings_key_is_an_exit_1_conflict_and_writes_nothing() {
    let fx = Fx::new("conflict");
    let home = fx.home();
    std::fs::create_dir_all(fx.settings().parent().unwrap()).unwrap();
    let theirs = json!({"env": {"OTEL_EXPORTER_OTLP_ENDPOINT": "http://collector.internal:4318"}});
    std::fs::write(fx.settings(), serde_json::to_string(&theirs).unwrap()).unwrap();
    let before = std::fs::read(fx.settings()).unwrap();
    let (code, value) = ctl_json(&["obs", "otel", "enable", "--home", &home]);
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "conflict");
    let message = value["error"]["message"].as_str().unwrap();
    assert!(message.contains("OTEL_EXPORTER_OTLP_ENDPOINT"), "{message}");
    assert!(!message.contains("collector.internal"), "the value is not printed");
    assert_eq!(std::fs::read(fx.settings()).unwrap(), before);
    assert!(!fx.obs().join("otel.json").exists());
}

#[test]
fn a_port_held_by_another_program_is_reported_and_retried() {
    let fx = Fx::new("taken");
    let home = fx.home();
    let squatter = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = squatter.local_addr().unwrap().port();
    let port_text = port.to_string();
    let (code, _) = ctl_json(&["obs", "otel", "enable", "--port", &port_text, "--home", &home]);
    assert_eq!(code, 0);

    let log = RefCell::new(Vec::<String>::new());
    let record = |line: &str| log.borrow_mut().push(line.to_string());
    let mut host = Host::new(&fx.obs());
    let start = Instant::now();
    host.tick(start, &record);
    assert_eq!(host.port(), None);
    assert_eq!(log.borrow().len(), 1);
    assert!(log.borrow()[0].contains("used by another program"), "{:?}", log.borrow());
    assert!(log.borrow()[0].contains("obs otel enable --port"));

    let (_, status) = ctl_json(&["obs", "otel", "status", "--home", &home]);
    assert_eq!(status["data"]["receiver"]["state"], "port-in-use");
    assert!(status["data"]["receiver"]["error"].as_str().unwrap().contains("used by another program"));
    let (_, human, _) = ctl(&["obs", "otel", "status", "--home", &home]);
    assert!(human.contains("receiver: port-in-use at http://127.0.0.1:"), "{human}");

    host.tick(start + Duration::from_secs(3), &record);
    host.tick(start + Duration::from_secs(9), &record);
    assert_eq!(log.borrow().len(), 1, "no retry and no log line before the retry delay");
    host.tick(start + Duration::from_secs(11), &record);
    assert_eq!(log.borrow().len(), 1, "the same failure is logged once");

    drop(squatter);
    wait_closed(port);
    host.tick(start + Duration::from_secs(22), &record);
    assert_eq!(host.port(), Some(port));
    assert!(log.borrow().last().unwrap().contains("listening on"));
    let (_, status) = ctl_json(&["obs", "otel", "status", "--home", &home]);
    assert_eq!(status["data"]["receiver"]["state"], "listening");
    assert!(status["data"]["receiver"]["error"].is_null());
    drop(host);
    wait_closed(port);
}

#[test]
fn a_second_gateway_waits_quietly_and_takes_over_when_the_first_goes_away() {
    let fx = Fx::new("peers");
    let port = free_port();
    let (code, _) = ctl_json(&["obs", "otel", "enable", "--port", &port.to_string(), "--home", &fx.home()]);
    assert_eq!(code, 0);
    let log = RefCell::new(Vec::<String>::new());
    let record = |line: &str| log.borrow_mut().push(line.to_string());
    let (mut first, mut second) = (Host::new(&fx.obs()), Host::new(&fx.obs()));
    let start = Instant::now();
    first.tick(start, &record);
    second.tick(start, &record);
    assert_eq!(first.port(), Some(port));
    assert_eq!(second.port(), None);
    assert!(log.borrow()[1].contains("another Toolport process"), "{:?}", log.borrow());
    let (_, status) = ctl_json(&["obs", "otel", "status", "--home", &fx.home()]);
    assert_eq!(status["data"]["receiver"]["state"], "listening");
    assert!(status["data"]["receiver"]["error"].is_null(), "a peer holding the port is not an error");

    assert_eq!(post(port, "/v1/metrics", &metrics_payload()), 200);
    drop(first);
    wait_closed(port);
    second.tick(start + Duration::from_secs(11), &record);
    assert_eq!(second.port(), Some(port));
    assert_eq!(post(port, "/v1/metrics", &metrics_payload()), 200);
    let events = super::store::stream_events(&fx.obs()).count();
    assert_eq!(events, 10);
}

#[test]
fn changing_the_port_restarts_the_receiver_on_the_new_one() {
    let fx = Fx::new("repoint");
    let (first_port, second_port) = (free_port(), free_port());
    let home = fx.home();
    ctl_json(&["obs", "otel", "enable", "--port", &first_port.to_string(), "--home", &home]);
    let log = RefCell::new(Vec::<String>::new());
    let record = |line: &str| log.borrow_mut().push(line.to_string());
    let mut host = Host::new(&fx.obs());
    host.tick(Instant::now(), &record);
    assert_eq!(probe(first_port), Probe::Ours);
    let (code, _) = ctl_json(&["obs", "otel", "enable", "--port", &second_port.to_string(), "--home", &home]);
    assert_eq!(code, 0);
    host.tick(Instant::now(), &record);
    assert_eq!(host.port(), Some(second_port));
    wait_closed(first_port);
    assert_eq!(probe(second_port), Probe::Ours);
}

#[test]
fn the_obs_otel_handlers_are_registered_for_the_ipc_surface() {
    let fx = Fx::new("ipc");
    let status = crate::plus::dispatch("plus.obs.otel.status", json!({"home": fx.home()})).unwrap();
    assert_eq!(status["enabled"], false);
    let plan = crate::plus::dispatch("plus.obs.otel.enable", json!({"home": fx.home(), "dryRun": true})).unwrap();
    assert_eq!(plan["dryRun"], true);
    let off = crate::plus::dispatch("plus.obs.otel.disable", json!({"home": fx.home()})).unwrap();
    assert_eq!(off["changed"], false);
    assert!(!Path::new(&fx.home()).join(".claude/settings.json").exists());
}
