use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;

use serde_json::{json, Value};
use tempfile::TempDir;
use toolport_plus_api::MemoryHost;

use super::config::{self, SpendConfig};
use super::event::*;
use super::otlp::{self, Signal};
use super::receiver::{Limits, Receiver};
use super::report::{self, Inputs, Range, RunSpan};
use super::sample;
use super::setup;
use super::store::Store;
use super::transcript;
use super::{day_of, MS_PER_DAY};

const NOW: i64 = 1_800_000_000_000;

fn attr(key: &str, value: Value) -> Value {
    json!({"key": key, "value": value})
}

fn text(key: &str, value: &str) -> Value {
    attr(key, json!({"stringValue": value}))
}

fn metric_payload(session: &str, model: &str, ts_ms: i64) -> Value {
    let point = |kind: &str, value: i64| {
        json!({
            "attributes": [text("session.id", session), text("model", model), text("type", kind),
                           text("user.email", "someone@example.test")],
            "timeUnixNano": (ts_ms * 1_000_000).to_string(),
            "asInt": value.to_string(),
        })
    };
    json!({"resourceMetrics": [{
        "resource": {"attributes": [text("service.name", "claude-code")]},
        "scopeMetrics": [{"metrics": [
            {"name": "claude_code.token.usage", "sum": {"dataPoints": [
                point("input", 100), point("output", 50), point("cacheRead", 1000), point("cacheCreation", 200)]}},
            {"name": "claude_code.cost.usage", "sum": {"dataPoints": [{
                "attributes": [text("session.id", session), text("model", model)],
                "timeUnixNano": (ts_ms * 1_000_000).to_string(), "asDouble": 0.42}]}},
            {"name": "claude_code.lines_of_code.count", "sum": {"dataPoints": [{"asInt": "9"}]}},
        ]}]
    }]})
}

fn parse(signal: Signal, payload: &Value) -> Vec<Event> {
    otlp::parse(&signal, payload.to_string().as_bytes()).unwrap()
}

fn token_event(session: &str, kind: &str, value: f64, ts: i64) -> Event {
    Event {
        session: Some(session.into()),
        model: Some("claude-sonnet-4".into()),
        token_type: Some(kind.into()),
        value,
        ..Event::new(ts, KIND_TOKENS)
    }
}

fn cost_event(session: &str, value: f64, ts: i64) -> Event {
    Event {
        session: Some(session.into()),
        model: Some("claude-sonnet-4".into()),
        value,
        ..Event::new(ts, KIND_COST)
    }
}

#[test]
fn test01_metrics_become_token_and_cost_events_and_unknown_metrics_are_ignored() {
    let events = parse(
        Signal::Metrics,
        &metric_payload("s-1", "claude-sonnet-4", NOW),
    );
    assert_eq!(events.len(), 5);
    let input = events
        .iter()
        .find(|e| e.token_type.as_deref() == Some(TOKEN_INPUT))
        .unwrap();
    assert_eq!(
        (input.value, input.ts, input.session.as_deref()),
        (100.0, NOW, Some("s-1"))
    );
    let cost = events.iter().find(|e| e.kind == KIND_COST).unwrap();
    assert_eq!(cost.value, 0.42);
}

#[test]
fn test02_content_attributes_never_reach_an_event() {
    let payload = json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [{
    "timeUnixNano": (NOW * 1_000_000).to_string(),
    "attributes": [
        text("event.name", "claude_code.api_request"), text("session.id", "s-2"),
        text("model", "claude-haiku-4"), text("user_prompt", "SECRET-PROMPT-TEXT"),
        text("tool_parameters", "SECRET-PARAMS"), attr("input_tokens", json!({"intValue": "10"})),
        attr("cost_usd", json!({"doubleValue": 0.01})),
    ]}]}]}]});
    let events = parse(Signal::Logs, &payload);
    assert_eq!(events.len(), 1);
    let line = serde_json::to_string(&events[0]).unwrap();
    assert!(!line.contains("SECRET"));
    assert_eq!((events[0].input, events[0].value), (Some(10), 0.01));
}

#[test]
fn test03_long_values_are_cut() {
    let long = "x".repeat(1000);
    let events = parse(Signal::Metrics, &metric_payload(&long, "m", NOW));
    assert_eq!(
        events[0].session.as_ref().unwrap().len(),
        otlp::MAX_VALUE_CHARS
    );
}

#[test]
fn test04_a_payload_that_is_not_json_is_refused() {
    assert!(otlp::parse(&Signal::Metrics, b"not json").is_err());
}

#[test]
fn test05_source_rule_prefers_metrics_over_request_logs_for_a_session() {
    let mut events = vec![
        token_event("a", TOKEN_INPUT, 100.0, NOW),
        cost_event("a", 0.5, NOW),
    ];
    events.push(Event {
        session: Some("a".into()),
        input: Some(100),
        value: 0.5,
        ..Event::new(NOW, KIND_REQUEST)
    });
    events.push(Event {
        session: Some("b".into()),
        model: Some("claude-sonnet-4".into()),
        input: Some(70),
        output: Some(30),
        value: 0.2,
        ..Event::new(NOW, KIND_REQUEST)
    });
    let samples = sample::from_events(&events);
    let total_input: u64 = samples.iter().map(|s| s.input).sum();
    let total_cost: f64 = samples.iter().map(|s| s.cost).sum();
    assert_eq!(total_input, 170);
    assert!((total_cost - 0.7).abs() < 1e-9);
}

#[test]
fn test06_tokens_without_a_cost_metric_get_an_estimated_cost() {
    let samples = sample::from_events(&[token_event("a", TOKEN_OUTPUT, 1_000_000.0, NOW)]);
    assert_eq!(samples[0].cost, 0.0);
    assert!((samples[0].cost_estimated - 15.0).abs() < 1e-9);
}

#[test]
fn test07_prune_drops_old_events_and_the_daily_totals_keep_them() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    let old = NOW - 40 * MS_PER_DAY;
    store
        .append(&[
            token_event("old", TOKEN_INPUT, 500.0, old),
            cost_event("old", 1.5, old),
            token_event("new", TOKEN_INPUT, 7.0, NOW),
        ])
        .unwrap();
    let preview = store.prune(NOW, 30, u64::MAX, &|_| None, true).unwrap();
    assert_eq!((preview.removed_events, preview.dry_run), (2, true));
    assert_eq!(store.read_events().0.len(), 3);
    let done = store
        .prune(NOW, 30, u64::MAX, &|_| Some("/repo".into()), false)
        .unwrap();
    assert_eq!(
        (done.removed_events, done.kept_events, done.folded_days),
        (2, 1, 1)
    );
    let daily = store.read_daily();
    assert_eq!(daily.len(), 1);
    assert_eq!(
        (daily[0].input, daily[0].cost, daily[0].day.clone()),
        (500, 1.5, day_of(old))
    );
    assert_eq!(daily[0].project, "/repo");
    assert_eq!(daily[0].sessions, vec!["old".to_string()]);
}

#[test]
fn test08_prune_removes_the_oldest_events_first_when_over_the_size_cap() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    let events: Vec<Event> = (0..20)
        .map(|i| token_event(&format!("s{i}"), TOKEN_INPUT, 1.0, NOW - (20 - i) * 1000))
        .collect();
    store.append(&events).unwrap();
    let size = std::fs::metadata(store.events_path()).unwrap().len();
    let report = store.prune(NOW, 30, size / 2, &|_| None, false).unwrap();
    assert!(report.removed_events >= 10 && report.kept_bytes <= size / 2);
    let kept = store.read_events().0;
    assert_eq!(kept.last().unwrap().session.as_deref(), Some("s19"));
    assert!(kept.iter().all(|e| e.session.as_deref() != Some("s0")));
}

#[test]
fn test09_a_line_that_does_not_parse_is_skipped_and_counted() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    store
        .append(&[token_event("a", TOKEN_INPUT, 1.0, NOW)])
        .unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(store.events_path())
        .unwrap();
    writeln!(file, "{{broken").unwrap();
    assert_eq!(store.stats().skipped_lines, 1);
    assert_eq!(store.read_events().0.len(), 1);
}

fn transcript_file(root: &std::path::Path, folder: &str, session: &str, cwd: &str, ts: &str) {
    let dir = root.join(".claude").join("projects").join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let line = |id: &str, out: u64| {
        json!({"type": "assistant", "sessionId": session, "cwd": cwd, "timestamp": ts,
               "message": {"id": id, "model": "claude-sonnet-4",
                           "usage": {"input_tokens": 10, "output_tokens": out,
                                     "cache_read_input_tokens": 100, "cache_creation_input_tokens": 5}}})
        .to_string()
    };
    let body = [line("m1", 1), line("m1", 20), line("m2", 30)].join("\n");
    std::fs::write(dir.join(format!("{session}.jsonl")), body).unwrap();
}

fn inputs<'a>(store: &'a Store, roots: &[std::path::PathBuf], runs: Vec<RunSpan>) -> Inputs<'a> {
    Inputs {
        store,
        transcripts: transcript::scan(roots),
        runs,
        now: NOW,
    }
}

#[test]
fn test10_transcript_reader_keeps_the_last_usage_per_message() {
    let home = TempDir::new().unwrap();
    transcript_file(
        home.path(),
        "-repo",
        "t-1",
        "/work/repo",
        "2027-01-15T08:00:00Z",
    );
    let roots = transcript::roots_under(home.path(), None);
    let sessions = transcript::scan(&roots);
    let session = &sessions["t-1"];
    assert_eq!(session.samples.len(), 2);
    assert_eq!(session.samples.iter().map(|s| s.output).sum::<u64>(), 50);
    assert_eq!(session.cwd.as_deref(), Some("/work/repo"));
    assert!(session
        .samples
        .iter()
        .all(|s| s.cost_estimated > 0.0 && s.transcript));
}

#[test]
fn test11_a_session_with_otel_events_is_not_counted_again_from_its_transcript() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    let home = TempDir::new().unwrap();
    let ts = day_of(NOW);
    transcript_file(
        home.path(),
        "-repo",
        "dup",
        "/work/repo",
        &format!("{ts}T01:00:00Z"),
    );
    transcript_file(
        home.path(),
        "-repo",
        "only-transcript",
        "/work/repo",
        &format!("{ts}T01:00:00Z"),
    );
    store
        .append(&[
            token_event("dup", TOKEN_INPUT, 1.0, NOW),
            cost_event("dup", 0.1, NOW),
        ])
        .unwrap();
    let roots = transcript::roots_under(home.path(), None);
    let summary = report::summary(&inputs(&store, &roots, vec![]), Range::Today, None);
    assert_eq!(summary["sources"]["otelSessions"], 1);
    assert_eq!(summary["sources"]["transcriptSessions"], 1);
    assert_eq!(summary["totals"]["tokens"]["input"], 1 + 20);
    assert_eq!(summary["totals"]["cost"]["badge"], "Estimate");
    assert!(summary["sources"]["estimatedShare"].as_f64().unwrap() > 0.0);
}

#[test]
fn test12_pruned_otel_sessions_are_not_double_counted_from_transcripts() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    let home = TempDir::new().unwrap();
    let old = NOW - 3 * MS_PER_DAY;
    transcript_file(
        home.path(),
        "-repo",
        "gone",
        "/work/repo",
        &format!("{}T01:00:00Z", day_of(old)),
    );
    store
        .append(&[
            token_event("gone", TOKEN_INPUT, 40.0, old),
            cost_event("gone", 0.3, old),
        ])
        .unwrap();
    store
        .prune(NOW, 1, u64::MAX, &|_| Some("/work/repo".into()), false)
        .unwrap();
    let roots = transcript::roots_under(home.path(), None);
    let summary = report::summary(&inputs(&store, &roots, vec![]), Range::Days7, None);
    assert_eq!(summary["totals"]["tokens"]["input"], 40);
    assert_eq!(summary["sources"]["transcriptSessions"], 0);
    assert_eq!(summary["totals"]["cost"]["badge"], "Measured");
}

#[test]
fn test13_costs_group_by_model_project_and_task_with_unattributed_rows() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    let mut a = vec![
        token_event("in-run", TOKEN_INPUT, 10.0, NOW),
        cost_event("in-run", 1.0, NOW),
    ];
    a.extend([
        token_event("manual", TOKEN_INPUT, 5.0, NOW + 4 * 3_600_000),
        cost_event("manual", 2.0, NOW + 4 * 3_600_000),
    ]);
    store.append(&a).unwrap();
    let runs = vec![RunSpan {
        task: "nightly".into(),
        start_ms: NOW - 1000,
        end_ms: Some(NOW + 60_000),
        session: None,
    }];
    let mut query_inputs = inputs(&store, &[], runs);
    query_inputs.now = NOW + 5 * 3_600_000;
    let summary = report::summary(&query_inputs, Range::Days7, None);
    let tasks: BTreeMap<String, f64> = summary["byTask"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["key"].as_str().unwrap().to_string(),
                r["cost"]["usd"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(tasks["nightly"], 1.0);
    assert_eq!(tasks[report::UNATTRIBUTED], 2.0);
    assert_eq!(summary["byModel"][0]["key"], "claude-sonnet-4");
    assert_eq!(summary["byProject"][0]["key"], "Unknown");
}

#[test]
fn test14_a_run_naming_the_session_wins_over_a_start_time_match() {
    let runs = vec![
        RunSpan {
            task: "by-time".into(),
            start_ms: 1000,
            end_ms: Some(9000),
            session: None,
        },
        RunSpan {
            task: "by-id".into(),
            start_ms: 100_000,
            end_ms: None,
            session: Some("s".into()),
        },
    ];
    assert_eq!(report::task_of(&runs, "s", 2000).as_deref(), Some("by-id"));
    assert_eq!(
        report::task_of(&runs, "other", 2000).as_deref(),
        Some("by-time")
    );
    assert_eq!(report::task_of(&runs, "other", 500_000), None);
}

#[test]
fn test15_a_git_worktree_groups_under_its_main_checkout() {
    let root = TempDir::new().unwrap();
    let main = root.path().join("repo");
    let worktree = root.path().join("slot");
    std::fs::create_dir_all(main.join(".git/worktrees/slot")).unwrap();
    std::fs::create_dir_all(worktree.join("src")).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}/.git/worktrees/slot\n", main.display()),
    )
    .unwrap();
    let main_text = main.to_string_lossy().into_owned();
    assert_eq!(
        transcript::project_root(&worktree.join("src").to_string_lossy()),
        main_text
    );
    assert_eq!(transcript::project_root(&main_text), main_text);
}

fn setup_host(dir: &TempDir, existing: Option<&str>) -> (MemoryHost, std::path::PathBuf) {
    let settings = dir.path().join("profile").join("settings.json");
    if let Some(text) = existing {
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, text).unwrap();
    }
    let host = MemoryHost {
        settings_paths: vec![settings.clone()],
        ..MemoryHost::default()
    };
    (host, settings)
}

fn env_of(path: &std::path::Path) -> Value {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    v["env"].clone()
}

#[test]
fn test16_setup_writes_the_five_keys_and_never_a_content_flag() {
    let dir = TempDir::new().unwrap();
    let (host, settings) = setup_host(&dir, Some(r#"{"theme": "dark"}"#));
    let spend = dir.path().join("spend");
    let plan = setup::plan(&host, &spend, 4318, &[]).unwrap();
    assert_eq!(plan["writes"], 5);
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        r#"{"theme": "dark"}"#
    );
    let applied = setup::apply(&host, &spend, 4318, &[], "now").unwrap();
    assert_eq!(applied["complete"], true);
    let env = env_of(&settings);
    assert_eq!(env.as_object().unwrap().len(), 5);
    assert_eq!(env["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://127.0.0.1:4318");
    assert_eq!(env["OTEL_EXPORTER_OTLP_PROTOCOL"], "http/json");
    let all = env.to_string();
    for flag in [
        "OTEL_LOG_USER_PROMPTS",
        "OTEL_LOG_TOOL_DETAILS",
        "OTEL_LOG_TOOL_CONTENT",
    ] {
        assert!(!all.contains(flag));
    }
    assert!(config::load(&spend).enabled);
}

#[test]
fn test17_a_different_existing_value_is_a_conflict_until_the_person_chooses_to_overwrite() {
    let dir = TempDir::new().unwrap();
    let (host, settings) = setup_host(
        &dir,
        Some(r#"{"env": {"OTEL_METRICS_EXPORTER": "prometheus", "KEEP": "1"}}"#),
    );
    let spend = dir.path().join("spend");
    let plan = setup::plan(&host, &spend, 4318, &[]).unwrap();
    assert_eq!(
        (plan["writes"].as_u64(), plan["conflicts"].as_u64()),
        (Some(4), Some(1))
    );
    let applied = setup::apply(&host, &spend, 4318, &[], "now").unwrap();
    assert_eq!(applied["complete"], false);
    assert_eq!(env_of(&settings)["OTEL_METRICS_EXPORTER"], "prometheus");
    let again = setup::apply(
        &host,
        &spend,
        4318,
        &["OTEL_METRICS_EXPORTER".into()],
        "later",
    )
    .unwrap();
    assert_eq!(again["complete"], true);
    assert_eq!(env_of(&settings)["OTEL_METRICS_EXPORTER"], "otlp");
    setup::undo(&host, &spend).unwrap();
    let env = env_of(&settings);
    assert_eq!(env["OTEL_METRICS_EXPORTER"], "prometheus");
    assert_eq!(env["KEEP"], "1");
    assert_eq!(env.as_object().unwrap().len(), 2);
}

#[test]
fn test18_undo_leaves_a_key_the_person_changed_since() {
    let dir = TempDir::new().unwrap();
    let (host, settings) = setup_host(&dir, None);
    let spend = dir.path().join("spend");
    setup::apply(&host, &spend, 4318, &[], "now").unwrap();
    let mut root: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    root["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"] = json!("http://elsewhere:1");
    std::fs::write(&settings, root.to_string()).unwrap();
    let undone = setup::undo(&host, &spend).unwrap();
    assert_eq!(
        undone["profiles"][0]["keptChanged"],
        json!(["OTEL_EXPORTER_OTLP_ENDPOINT"])
    );
    assert_eq!(
        env_of(&settings),
        json!({"OTEL_EXPORTER_OTLP_ENDPOINT": "http://elsewhere:1"})
    );
    assert!(!config::load(&spend).enabled);
}

#[test]
fn test19_undo_removes_the_env_object_it_created() {
    let dir = TempDir::new().unwrap();
    let (host, settings) = setup_host(&dir, Some(r#"{"theme": "dark"}"#));
    let spend = dir.path().join("spend");
    setup::apply(&host, &spend, 4318, &[], "now").unwrap();
    setup::undo(&host, &spend).unwrap();
    let root: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(root, json!({"theme": "dark"}));
}

#[test]
fn test20_changing_the_port_updates_only_keys_toolport_wrote() {
    let dir = TempDir::new().unwrap();
    let (host, settings) = setup_host(&dir, None);
    let spend = dir.path().join("spend");
    setup::apply(&host, &spend, 4318, &[], "now").unwrap();
    let plan = setup::plan(&host, &spend, 4999, &[]).unwrap();
    assert_eq!(
        (plan["writes"].as_u64(), plan["conflicts"].as_u64()),
        (Some(1), Some(0))
    );
    setup::apply(&host, &spend, 4999, &[], "later").unwrap();
    assert_eq!(
        env_of(&settings)["OTEL_EXPORTER_OTLP_ENDPOINT"],
        "http://127.0.0.1:4999"
    );
    assert_eq!(config::load(&spend).port, 4999);
}

fn post(port: u16, path: &str, body: &[u8], extra: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let head = format!("POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}\r\n", body.len());
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    reply
}

#[test]
fn test21_receiver_stores_posted_events_on_an_ephemeral_loopback_port() {
    let dir = TempDir::new().unwrap();
    let receiver = Receiver::bind(0, dir.path().to_path_buf(), Limits::default()).unwrap();
    assert_ne!(receiver.port(), 4318);
    let body = metric_payload("rx", "claude-sonnet-4", NOW).to_string();
    let reply = post(receiver.port(), "/v1/metrics", body.as_bytes(), "");
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert_eq!(Store::new(dir.path()).read_events().0.len(), 5);
    assert_eq!(
        receiver
            .counters
            .events
            .load(std::sync::atomic::Ordering::Relaxed),
        5
    );
}

#[test]
fn test22_receiver_refuses_oversized_bodies_other_paths_and_other_methods() {
    let dir = TempDir::new().unwrap();
    let limits = Limits {
        max_body: 64,
        ..Limits::default()
    };
    let receiver = Receiver::bind(0, dir.path().to_path_buf(), limits).unwrap();
    assert!(post(receiver.port(), "/v1/metrics", &[b' '; 65], "").starts_with("HTTP/1.1 413"));
    assert!(post(receiver.port(), "/nope", b"{}", "").starts_with("HTTP/1.1 404"));
    assert!(post(receiver.port(), "/v1/logs", b"{", "").starts_with("HTTP/1.1 400"));
    assert!(post(
        receiver.port(),
        "/v1/logs",
        b"{}",
        "Content-Encoding: gzip\r\n"
    )
    .starts_with("HTTP/1.1 415"));
    let mut stream = TcpStream::connect(("127.0.0.1", receiver.port())).unwrap();
    stream.write_all(b"GET /v1/logs HTTP/1.1\r\n\r\n").unwrap();
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    assert!(reply.starts_with("HTTP/1.1 405"));
    assert!(Store::new(dir.path()).read_events().0.is_empty());
}

#[test]
fn test23_receiver_answers_503_beyond_its_connection_cap() {
    let dir = TempDir::new().unwrap();
    let limits = Limits {
        max_connections: 0,
        ..Limits::default()
    };
    let receiver = Receiver::bind(0, dir.path().to_path_buf(), limits).unwrap();
    assert!(post(receiver.port(), "/v1/logs", b"{}", "").starts_with("HTTP/1.1 503"));
}

#[test]
fn test24_a_tick_starts_the_receiver_only_when_collection_is_on_and_status_follows() {
    let dir = TempDir::new().unwrap();
    let spend = dir.path().join("spend");
    std::fs::create_dir_all(&spend).unwrap();
    super::runtime::tick(&spend, &[], super::runtime::Owner::App, NOW);
    assert_eq!(super::runtime::status(&spend, NOW)["state"], "off");
    let cfg = SpendConfig {
        enabled: true,
        port: 0,
        ..SpendConfig::default()
    };
    std::fs::write(
        spend.join(config::CONFIG_FILE),
        serde_json::to_string(&cfg).unwrap(),
    )
    .unwrap();
    super::runtime::tick(&spend, &[], super::runtime::Owner::App, NOW);
    let status = super::runtime::status(&spend, NOW);
    assert_eq!(
        (
            status["state"].as_str(),
            status["receiver"]["owner"].as_str()
        ),
        (Some("waiting"), Some("app"))
    );
    super::runtime::stop(&spend);
    assert_eq!(
        super::runtime::status(&spend, NOW)["state"],
        "not-listening"
    );
}

#[test]
fn test25_mcp_part_counts_tool_definitions_from_savings_rows_in_range() {
    let host = MemoryHost {
        savings_rows: vec![
            json!({"kind": "catalog_exposure", "ts": NOW, "exposedTokens": 800, "fullTokens": 5000}),
            json!({"kind": "catalog_exposure", "ts": NOW - 100 * MS_PER_DAY, "exposedTokens": 99, "fullTokens": 99}),
            json!({"kind": "catalog_response", "ts": NOW, "exposedTokens": 1}),
        ],
        ..MemoryHost::default()
    };
    let part = super::mcp_part::part(
        &host,
        std::path::Path::new("/nonexistent-data-dir"),
        NOW - MS_PER_DAY,
        10_000,
    );
    assert_eq!(part["toolDefinitions"]["exposedTokens"], 800);
    assert_eq!(part["toolDefinitions"]["badge"], "Counted");
    assert_eq!(part["shareOfInputTokens"]["value"], 0.08);
    assert_eq!(part["shareOfInputTokens"]["badge"], "Estimate");
}

#[test]
fn test26_config_bounds_are_enforced() {
    let ok = SpendConfig::default();
    assert!(ok.validate().is_ok());
    assert!(SpendConfig {
        retention_days: 3,
        ..ok.clone()
    }
    .validate()
    .is_err());
    assert!(SpendConfig {
        max_mb: 1000,
        ..ok.clone()
    }
    .validate()
    .is_err());
    assert!(SpendConfig { port: 80, ..ok }.validate().is_err());
}

#[test]
fn test27_tiles_carry_all_three_ranges_with_delta_and_estimated_days() {
    let dir = TempDir::new().unwrap();
    let store = Store::new(dir.path());
    store
        .append(&[
            token_event("a", TOKEN_INPUT, 10.0, NOW),
            cost_event("a", 2.0, NOW),
            token_event("b", TOKEN_INPUT, 10.0, NOW - MS_PER_DAY),
            cost_event("b", 1.0, NOW - MS_PER_DAY),
        ])
        .unwrap();
    let summary = report::summary(&inputs(&store, &[], vec![]), Range::Days7, None);
    let tiles = &summary["tiles"];
    assert_eq!(tiles["today"]["cost"]["usd"], 2.0);
    assert_eq!(tiles["today"]["previousUsd"], 1.0);
    assert_eq!(tiles["7d"]["cost"]["usd"], 3.0);
    assert_eq!(tiles["30d"]["days"], 30);
    assert_eq!(tiles["7d"]["estimatedDays"], 0);
}

#[test]
fn test28_compression_part_counts_hook_calls_and_ledger_savings() {
    let dir = TempDir::new().unwrap();
    let paths = crate::compression::store::Paths::new(dir.path());
    std::fs::create_dir_all(dir.path()).unwrap();
    let rows = [
        r#"{"at":"2027-01-01T10:00:00Z","provider":"rtk","answered":true,"ok":true}"#,
        r#"{"at":"2027-01-02T10:00:00Z","provider":"rtk","answered":false,"ok":true}"#,
    ];
    std::fs::write(
        paths.dir.join(crate::compression::hooks::COUNTERS_FILE),
        rows.join("\n"),
    )
    .unwrap();
    let part = super::mcp_part::part(&MemoryHost::default(), dir.path(), 0, 0);
    assert_eq!(part["compression"]["calls"], 2);
    assert_eq!(part["compression"]["hooks"], 1);
    assert_eq!(part["compression"]["tokensCut"], Value::Null);
    assert_eq!(part["compression"]["badge"], "Counted");
    assert_eq!(part["firstRequest"], Value::Null);
}

#[test]
fn test29_config_set_saves_bounded_values_and_reports_a_port_change() {
    let dir = TempDir::new().unwrap();
    let mut host = MemoryHost::default();
    host.registry.path = Some(dir.path().join("registry.json"));
    let edit = |port, days, mb, cost| super::api::ConfigEdit {
        port,
        retention_days: days,
        max_mb: mb,
        show_cost: cost,
    };
    let saved = super::api::config_set(
        &host,
        &edit(Some("4999"), Some("14"), Some("32"), Some("off")),
    )
    .unwrap();
    assert_eq!(saved["portChanged"], true);
    assert_eq!(saved["showCost"], false);
    let stored = config::load(&super::spend_dir(dir.path()));
    assert_eq!(
        (stored.port, stored.retention_days, stored.max_mb),
        (4999, 14, 32)
    );
    assert!(super::api::config_set(&host, &edit(None, Some("3"), None, None)).is_err());
    assert!(super::api::config_set(&host, &edit(None, None, None, Some("maybe"))).is_err());
}

#[test]
fn test30_stale_check_finds_sessions_after_the_last_event_and_project_overrides() {
    let work = TempDir::new().unwrap();
    std::fs::create_dir_all(work.path().join(".claude")).unwrap();
    std::fs::write(
        work.path().join(".claude/settings.json"),
        r#"{"env":{"OTEL_EXPORTER_OTLP_ENDPOINT":"https://collector.example.test:4317"}}"#,
    )
    .unwrap();
    let mut sessions = BTreeMap::new();
    sessions.insert(
        "s1".to_string(),
        transcript::TranscriptSession {
            session: "s1".into(),
            cwd: Some(work.path().display().to_string()),
            first_ts: NOW,
            last_ts: NOW + 1000,
            samples: Vec::new(),
        },
    );
    let found = super::stale::check(&sessions, NOW - MS_PER_DAY, None, "http://127.0.0.1:4318");
    assert_eq!(found["sessionsSince"], 1);
    assert_eq!(found["projectOverrides"].as_array().unwrap().len(), 1);
}
