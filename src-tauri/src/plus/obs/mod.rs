//! Observability: Claude Code transcript indexer and local OTel sink (OBS).

pub mod monitor_db;
pub mod otel;
pub mod store;
pub mod transcript;

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use store::{Event, Locked, State};

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    messages: u64,
    input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
}

impl Tokens {
    fn add(&mut self, r: &store::MsgRecord) {
        self.messages += 1;
        self.input += r.input;
        self.output += r.output;
        self.cache_creation += r.cache_creation;
        self.cache_read += r.cache_read;
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionAgg {
    #[serde(flatten)]
    tokens: Tokens,
    cwd: String,
    first_ts: String,
    last_ts: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpAgg {
    calls: u64,
    tools: BTreeMap<String, u64>,
}

fn attr_str<'a>(e: &'a Event, key: &str) -> &'a str {
    e.attrs.get(key).and_then(Value::as_str).unwrap_or("")
}

fn attr_f64(e: &Event, key: &str) -> f64 {
    e.attrs.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

fn otel_summary(events: &[Event]) -> Value {
    let mut tokens: BTreeMap<String, f64> = BTreeMap::new();
    let mut cost_by_model: BTreeMap<String, f64> = BTreeMap::new();
    let mut cost = 0.0;
    let (mut api_count, mut api_cost, mut api_ms) = (0u64, 0.0, 0.0);
    let mut decisions: BTreeMap<String, u64> = BTreeMap::new();
    let mut failures: BTreeMap<String, (u64, String)> = BTreeMap::new();
    let mut connections = 0u64;

    for e in events {
        match e.kind.as_str() {
            "token_usage" => {
                *tokens.entry(attr_str(e, "type").to_string()).or_default() +=
                    e.value.unwrap_or(0.0);
            }
            "cost" => {
                let v = e.value.unwrap_or(0.0);
                cost += v;
                *cost_by_model
                    .entry(attr_str(e, "model").to_string())
                    .or_default() += v;
            }
            "api_request" => {
                api_count += 1;
                api_cost += attr_f64(e, "cost_usd");
                api_ms += attr_f64(e, "duration_ms");
            }
            "tool_decision" => {
                *decisions
                    .entry(attr_str(e, "decision").to_string())
                    .or_default() += 1;
            }
            "mcp_connection" => {
                connections += 1;
                if attr_str(e, "status") == "failed" {
                    let slot = failures
                        .entry(attr_str(e, "server_name").to_string())
                        .or_default();
                    slot.0 += 1;
                    slot.1 = attr_str(e, "error_code").to_string();
                }
            }
            _ => {}
        }
    }

    let failure_list: Vec<Value> = failures
        .into_iter()
        .map(|(server, (count, code))| json!({"server": server, "count": count, "lastErrorCode": code}))
        .collect();
    json!({
        "events": events.len(),
        "tokens": tokens,
        "costUsd": round6(cost),
        "costByModel": cost_by_model.into_iter().map(|(k, v)| (k, round6(v))).collect::<BTreeMap<_, _>>(),
        "apiRequests": {"count": api_count, "costUsd": round6(api_cost), "durationMs": api_ms},
        "toolDecisions": decisions,
        "mcpConnections": {"total": connections, "failures": failure_list},
    })
}

pub fn summarize(state: &State, events: &[Event]) -> Value {
    let mut total = Tokens::default();
    let mut by_day: BTreeMap<&str, Tokens> = BTreeMap::new();
    let mut by_model: BTreeMap<&str, Tokens> = BTreeMap::new();
    let mut by_session: BTreeMap<&str, SessionAgg> = BTreeMap::new();
    let mut by_mcp: BTreeMap<String, McpAgg> = BTreeMap::new();

    for r in state.messages.values() {
        total.add(r);
        by_day.entry(&r.day).or_default().add(r);
        by_model.entry(&r.model).or_default().add(r);
        let s = by_session.entry(&r.session).or_default();
        s.tokens.add(r);
        if s.cwd.is_empty() {
            s.cwd = r.cwd.clone();
        }
        if s.first_ts.is_empty() || r.ts < s.first_ts {
            s.first_ts = r.ts.clone();
        }
        if r.ts > s.last_ts {
            s.last_ts = r.ts.clone();
        }
        for name in r.tools.values() {
            if let Some(server) = transcript::mcp_server_of(name) {
                let agg = by_mcp.entry(server.to_string()).or_default();
                agg.calls += 1;
                *agg.tools.entry(name.clone()).or_default() += 1;
            }
        }
    }

    let failures: Vec<Value> = state
        .transcript_mcp_failures
        .values()
        .map(|f| json!({"session": f.session, "server": f.server, "ts": f.ts}))
        .collect();

    json!({
        "totals": total,
        "byDay": by_day,
        "byModel": by_model,
        "bySession": by_session,
        "byMcpServer": by_mcp,
        "mcpFailures": failures,
        "otel": otel_summary(events),
        "index": {"files": state.files.len(), "messages": state.messages.len()},
    })
}

pub fn default_projects_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

pub fn run_summary(dir: &Path, root: Option<&Path>, refresh: bool) -> Result<Value, String> {
    let lock = Locked::acquire(dir)?;
    if refresh {
        if let Some(root) = root {
            transcript::index(&lock, root)?;
        }
    }
    let mut out = summarize(&lock.load_state(), &lock.read_events());
    if let Some(history) = lock.load_history() {
        out["mcpmHistory"] = history;
    }
    Ok(out)
}

pub fn summary_handler(args: Value) -> Result<Value, String> {
    let dir = crate::registry::conduit_dir()
        .ok_or_else(|| "data directory unavailable".to_string())?
        .join("obs");
    let root = match args.get("root").and_then(Value::as_str) {
        Some(p) => Some(PathBuf::from(p)),
        None => default_projects_root(),
    };
    let refresh = args.get("refresh").and_then(Value::as_bool).unwrap_or(true);
    run_summary(&dir, root.as_deref(), refresh)
}

pub fn import_monitor_handler(args: Value) -> Result<Value, String> {
    let dir = crate::registry::conduit_dir()
        .ok_or_else(|| "data directory unavailable".to_string())?
        .join("obs");
    let path = match args.get("path").and_then(Value::as_str) {
        Some(p) => PathBuf::from(p),
        None => monitor_db::default_path().ok_or_else(|| "home directory unavailable".to_string())?,
    };
    let lock = Locked::acquire(&dir)?;
    monitor_db::import(&lock, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::obs::transcript::fixtures::assistant;

    fn scratch(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("obs-sum-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_fixture(proj: &Path) {
        std::fs::create_dir_all(proj).unwrap();
        let s1 = [
            assistant(
                "m1",
                "s1",
                "2026-10-01T10:00:00Z",
                "claude-a",
                (10, 1, 500, 0),
                &[],
            ),
            assistant(
                "m1",
                "s1",
                "2026-10-01T10:00:02Z",
                "claude-a",
                (10, 40, 500, 7000),
                &[("t1", "mcp__github__list")],
            ),
            assistant(
                "m2",
                "s1",
                "2026-10-02T09:00:00Z",
                "claude-b",
                (3, 4, 0, 100),
                &[("t2", "mcp__github__get"), ("t3", "mcp__github__list")],
            ),
        ];
        std::fs::write(proj.join("s1.jsonl"), s1.join("\n") + "\n").unwrap();
        let s2 = [
            assistant("m3", "s2", "2026-10-02T11:00:00Z", "claude-a", (1, 2, 3, 4), &[("t4", "Bash"), ("t5", "mcp__figma__x")]),
            r#"{"type":"attachment","sessionId":"s2","timestamp":"2026-10-02T11:00:01Z","attachment":{"type":"deferred_tools_delta","failedMcpServers":["slack"]}}"#.to_string(),
        ];
        std::fs::write(proj.join("s2.jsonl"), s2.join("\n") + "\n").unwrap();
    }

    #[test]
    fn golden_aggregate() {
        let base = scratch("golden");
        write_fixture(&base.join("projects"));
        let lock = Locked::acquire(&base.join("obs")).unwrap();
        lock.append_events(&otel::parse_payload(&otel::fixtures::metrics_payload()))
            .unwrap();
        lock.append_events(&otel::parse_payload(&otel::fixtures::logs_payload()))
            .unwrap();
        drop(lock);
        let got = run_summary(&base.join("obs"), Some(&base.join("projects")), true).unwrap();
        let again = run_summary(&base.join("obs"), Some(&base.join("projects")), true).unwrap();
        assert_eq!(got, again);
        let tok = |m, i, o, cc, cr| json!({"messages": m, "input": i, "output": o, "cacheCreation": cc, "cacheRead": cr});
        let expected = json!({
            "totals": tok(3, 14, 46, 503, 7104),
            "byDay": {
                "2026-10-01": tok(1, 10, 40, 500, 7000),
                "2026-10-02": tok(2, 4, 6, 3, 104)
            },
            "byModel": {
                "claude-a": tok(2, 11, 42, 503, 7004),
                "claude-b": tok(1, 3, 4, 0, 100)
            },
            "bySession": {
                "s1": {"messages": 2, "input": 13, "output": 44, "cacheCreation": 500, "cacheRead": 7100,
                       "cwd": "/work/demo", "firstTs": "2026-10-01T10:00:02Z", "lastTs": "2026-10-02T09:00:00Z"},
                "s2": {"messages": 1, "input": 1, "output": 2, "cacheCreation": 3, "cacheRead": 4,
                       "cwd": "/work/demo", "firstTs": "2026-10-02T11:00:00Z", "lastTs": "2026-10-02T11:00:00Z"}
            },
            "byMcpServer": {
                "figma": {"calls": 1, "tools": {"mcp__figma__x": 1}},
                "github": {"calls": 3, "tools": {"mcp__github__get": 1, "mcp__github__list": 2}}
            },
            "mcpFailures": [{"session": "s2", "server": "slack", "ts": "2026-10-02T11:00:01Z"}],
            "otel": {
                "events": 10,
                "tokens": {"cacheCreation": 300.0, "cacheRead": 9000.0, "input": 100.0, "output": 40.0},
                "costUsd": 0.25,
                "costByModel": {"claude-a": 0.25},
                "apiRequests": {"count": 1, "costUsd": 0.5, "durationMs": 1200.0},
                "toolDecisions": {"accept": 1, "reject": 1},
                "mcpConnections": {"total": 2, "failures": [{"server": "figma", "count": 1, "lastErrorCode": "ECONNREFUSED"}]}
            },
            "index": {"files": 2, "messages": 3}
        });
        assert_eq!(got, expected);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn summary_without_refresh_does_not_touch_transcripts() {
        let base = scratch("norefresh");
        write_fixture(&base.join("projects"));
        let got = run_summary(&base.join("obs"), Some(&base.join("projects")), false).unwrap();
        assert_eq!(got["index"]["messages"], 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn handler_uses_data_dir_override() {
        let _guard = crate::registry::data_dir_test_lock();
        let base = scratch("handler");
        write_fixture(&base.join("projects"));
        let _dir = crate::registry::DataDirOverride::set(base.join("data"));
        let out = crate::plus::dispatch(
            "plus.obs.summary",
            json!({"root": base.join("projects").to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(out["totals"]["messages"], 3);
        assert!(base.join("data/obs/transcripts.json").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
