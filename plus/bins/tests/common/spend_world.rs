//! Synthetic Spend data for the contract cases: OTLP-derived events, a pruned day, one
//! transcript and upstream savings rows. Nothing here is a real transcript or settings file.

use serde_json::json;

use super::world::World;

pub const NOW_SECONDS: &str = "1800000000";
const NOW_MS: i64 = 1_800_000_000_000;
const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

fn put(path: std::path::PathBuf, text: String) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn lines(values: &[serde_json::Value]) -> String {
    values.iter().map(|v| format!("{v}\n")).collect()
}

fn session(id: &str, model: &str, ts: i64, usd: f64) -> Vec<serde_json::Value> {
    let token = |kind: &str, value: u64| {
        json!({"ts": ts, "kind": "tokens", "session": id, "model": model, "project": "/work/alpha",
               "type": kind, "value": value})
    };
    vec![
        token("input", 1000),
        token("output", 200),
        token("cacheRead", 8000),
        token("cacheCreation", 500),
        json!({"ts": ts, "kind": "cost", "session": id, "model": model, "project": "/work/alpha", "value": usd}),
    ]
}

pub fn build(world: &World) {
    let dir = world.data.join("plus/spend");
    let mut events = session("s-a", "claude-sonnet-4", NOW_MS - 2 * HOUR, 0.12);
    events.extend(session("s-b", "claude-haiku-4", NOW_MS - DAY - HOUR, 0.03));
    events.extend(session("s-old", "claude-sonnet-4", NOW_MS - 45 * DAY, 0.5));
    put(dir.join("events.jsonl"), lines(&events));
    put(
        dir.join("daily.jsonl"),
        lines(&[json!({"day": "2026-11-01", "project": "/work/alpha", "model": "claude-sonnet-4",
                       "input": 5000, "output": 900, "cacheRead": 40000, "cacheCreation": 2000,
                       "cost": 0.9, "costEstimated": 0.0, "requests": 12, "sessions": ["s-gone"]})]),
    );
    let transcript = json!({"type": "assistant", "sessionId": "t-1", "cwd": "/work/beta",
        "timestamp": "2027-01-15T05:00:00Z",
        "message": {"id": "m1", "model": "claude-sonnet-4",
                    "usage": {"input_tokens": 300, "output_tokens": 60,
                              "cache_read_input_tokens": 2000, "cache_creation_input_tokens": 100}}});
    put(
        world.home.join(".claude/projects/-work-beta/t-1.jsonl"),
        format!("{transcript}\n"),
    );
    put(
        world.data.join("savings-v4.jsonl"),
        lines(&[json!({"v": 4, "kind": "catalog_exposure", "ts": NOW_MS - HOUR, "sessionId": "s-a",
                       "fullTokens": 5000, "exposedTokens": 800, "tokenizer": "cl100k_base"})]),
    );
}

pub fn settings_with_a_conflict(world: &World) {
    put(
        world.home.join(".claude/settings.json"),
        "{\n  \"theme\": \"dark\",\n  \"env\": {\"OTEL_METRICS_EXPORTER\": \"prometheus\"}\n}\n"
            .to_string(),
    );
}

pub fn settings_without_env(world: &World) {
    put(
        world.home.join(".claude/settings.json"),
        "{\n  \"theme\": \"dark\"\n}\n".to_string(),
    );
}
