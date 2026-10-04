use super::store::{day_from_ms, Event, Locked};
use serde_json::Value;
use std::collections::BTreeMap;

const CONTENT_ATTRS: [&str; 8] = [
    "prompt",
    "tool_parameters",
    "tool_input",
    "tool_result",
    "tool_output",
    "user.email",
    "user.account_uuid",
    "user_email",
];
const MAX_ATTR_CHARS: usize = 512;

fn attr_map(list: Option<&Value>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Some(items) = list.and_then(Value::as_array) else {
        return out;
    };
    for item in items {
        let Some(key) = item.get("key").and_then(Value::as_str) else {
            continue;
        };
        if CONTENT_ATTRS.contains(&key) {
            continue;
        }
        let Some(value) = item.get("value") else {
            continue;
        };
        if let Some(v) = any_value(value) {
            out.insert(key.to_string(), v);
        }
    }
    out
}

fn any_value(v: &Value) -> Option<Value> {
    if let Some(s) = v.get("stringValue").and_then(Value::as_str) {
        return Some(Value::String(s.chars().take(MAX_ATTR_CHARS).collect()));
    }
    if let Some(i) = v.get("intValue") {
        let n = match i {
            Value::String(s) => s.parse::<i64>().ok()?,
            other => other.as_i64()?,
        };
        return Some(Value::from(n));
    }
    if let Some(d) = v.get("doubleValue").and_then(Value::as_f64) {
        return Some(Value::from(d));
    }
    v.get("boolValue").and_then(Value::as_bool).map(Value::Bool)
}

fn nanos_to_ms(v: Option<&Value>) -> i64 {
    let n = match v {
        Some(Value::String(s)) => s.parse::<i128>().unwrap_or(0),
        Some(other) => other.as_i64().map(i128::from).unwrap_or(0),
        None => 0,
    };
    (n / 1_000_000) as i64
}

fn point_value(p: &Value) -> Option<f64> {
    match p.get("asDouble").or_else(|| p.get("asInt"))? {
        Value::String(s) => s.parse().ok(),
        other => other.as_f64(),
    }
}

fn normalize_name(name: &str) -> &str {
    name.strip_prefix("claude_code.").unwrap_or(name)
}

fn resource_attrs(res: &Value) -> BTreeMap<String, Value> {
    attr_map(res.get("resource").and_then(|r| r.get("attributes")))
}

pub fn parse_metrics(body: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    for rm in body
        .get("resourceMetrics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let res = resource_attrs(rm);
        for sm in rm
            .get("scopeMetrics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for metric in sm
                .get("metrics")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = metric.get("name").and_then(Value::as_str).unwrap_or("");
                let kind = match normalize_name(name) {
                    "token.usage" => "token_usage",
                    "cost.usage" => "cost",
                    _ => continue,
                };
                let points = ["sum", "gauge"]
                    .iter()
                    .find_map(|k| metric.get(*k))
                    .and_then(|d| d.get("dataPoints"))
                    .and_then(Value::as_array);
                for p in points.into_iter().flatten() {
                    let Some(value) = point_value(p) else {
                        continue;
                    };
                    let ts_ms = nanos_to_ms(p.get("timeUnixNano"));
                    let mut attrs = res.clone();
                    attrs.extend(attr_map(p.get("attributes")));
                    out.push(Event {
                        kind: kind.into(),
                        ts_ms,
                        day: day_from_ms(ts_ms),
                        value: Some(value),
                        attrs,
                    });
                }
            }
        }
    }
    out
}

pub fn parse_logs(body: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    for rl in body
        .get("resourceLogs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let res = resource_attrs(rl);
        for sl in rl
            .get("scopeLogs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for rec in sl
                .get("logRecords")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let mut attrs = res.clone();
                attrs.extend(attr_map(rec.get("attributes")));
                let name = attrs
                    .get("event.name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        rec.get("body")
                            .and_then(any_value)
                            .and_then(|v| v.as_str().map(str::to_string))
                    })
                    .unwrap_or_default();
                let kind = match normalize_name(&name) {
                    "api_request" => "api_request",
                    "tool_decision" => "tool_decision",
                    "mcp_server_connection" => "mcp_connection",
                    _ => continue,
                };
                let ts_ms = nanos_to_ms(rec.get("timeUnixNano"));
                out.push(Event {
                    kind: kind.into(),
                    ts_ms,
                    day: day_from_ms(ts_ms),
                    value: None,
                    attrs,
                });
            }
        }
    }
    out
}

pub fn parse_payload(body: &Value) -> Vec<Event> {
    let mut events = parse_metrics(body);
    events.extend(parse_logs(body));
    events
}

pub fn ingest(lock: &Locked, body: &Value) -> Result<usize, String> {
    let events = parse_payload(body);
    lock.append_events(&events)?;
    Ok(events.len())
}

#[cfg(test)]
pub(crate) mod fixtures {
    use serde_json::{json, Value};

    fn kv(k: &str, v: Value) -> Value {
        json!({"key": k, "value": v})
    }

    pub fn metrics_payload() -> Value {
        let point = |ty: &str, model: &str, v: i64| {
            json!({
                "timeUnixNano": "1759492800000000000",
                "asInt": v.to_string(),
                "attributes": [kv("type", json!({"stringValue": ty})), kv("model", json!({"stringValue": model}))]
            })
        };
        json!({"resourceMetrics": [{
            "resource": {"attributes": [kv("service.name", json!({"stringValue": "claude-code"}))]},
            "scopeMetrics": [{"metrics": [
                {"name": "claude_code.token.usage", "sum": {"dataPoints": [
                    point("input", "claude-a", 100), point("output", "claude-a", 40),
                    point("cacheRead", "claude-a", 9000), point("cacheCreation", "claude-a", 300)
                ]}},
                {"name": "claude_code.cost.usage", "sum": {"dataPoints": [
                    {"timeUnixNano": "1759492800000000000", "asDouble": 0.25,
                     "attributes": [kv("model", json!({"stringValue": "claude-a"}))]}
                ]}},
                {"name": "claude_code.unrelated", "sum": {"dataPoints": [{"asInt": "1"}]}}
            ]}]
        }]})
    }

    pub fn logs_payload() -> Value {
        let rec = |name: &str, extra: Vec<Value>| {
            let mut attrs = vec![
                kv("event.name", json!({"stringValue": name})),
                kv("session.id", json!({"stringValue": "s1"})),
            ];
            attrs.extend(extra);
            json!({"timeUnixNano": "1759492801000000000", "attributes": attrs})
        };
        json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [
            rec("claude_code.api_request", vec![
                kv("model", json!({"stringValue": "claude-a"})),
                kv("cost_usd", json!({"doubleValue": 0.5})),
                kv("duration_ms", json!({"intValue": "1200"})),
                kv("input_tokens", json!({"intValue": "10"}))
            ]),
            rec("claude_code.tool_decision", vec![
                kv("tool_name", json!({"stringValue": "mcp__github__list"})),
                kv("decision", json!({"stringValue": "accept"}))
            ]),
            rec("claude_code.tool_decision", vec![
                kv("tool_name", json!({"stringValue": "Bash"})),
                kv("decision", json!({"stringValue": "reject"}))
            ]),
            rec("claude_code.mcp_server_connection", vec![
                kv("server_name", json!({"stringValue": "figma"})),
                kv("status", json!({"stringValue": "failed"})),
                kv("error_code", json!({"stringValue": "ECONNREFUSED"}))
            ]),
            rec("claude_code.mcp_server_connection", vec![
                kv("server_name", json!({"stringValue": "github"})),
                kv("status", json!({"stringValue": "connected"}))
            ]),
            rec("claude_code.user_prompt", vec![])
        ]}]}]})
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use serde_json::json;

    fn kv(k: &str, v: Value) -> Value {
        json!({"key": k, "value": v})
    }

    #[test]
    fn metrics_become_token_and_cost_events() {
        let events = parse_metrics(&metrics_payload());
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].kind, "token_usage");
        assert_eq!(events[0].value, Some(100.0));
        assert_eq!(events[0].attrs["type"], "input");
        assert_eq!(events[0].attrs["service.name"], "claude-code");
        assert_eq!(events[0].day, "2025-10-03");
        assert_eq!(events[4].kind, "cost");
        assert_eq!(events[4].value, Some(0.25));
    }

    #[test]
    fn logs_map_known_events_and_drop_others() {
        let events = parse_logs(&logs_payload());
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "api_request",
                "tool_decision",
                "tool_decision",
                "mcp_connection",
                "mcp_connection"
            ]
        );
        assert_eq!(events[0].attrs["duration_ms"], 1200);
        assert_eq!(events[3].attrs["error_code"], "ECONNREFUSED");
    }

    #[test]
    fn event_name_falls_back_to_body_and_garbage_is_tolerated() {
        let body = json!({"resourceLogs": [{"scopeLogs": [{"logRecords": [
            {"timeUnixNano": 1759492800000000000i64, "body": {"stringValue": "claude_code.tool_decision"}},
            {"attributes": "wrong"}, 5
        ]}]}]});
        assert_eq!(parse_logs(&body).len(), 1);
        assert!(parse_payload(&json!({"x": 1})).is_empty());
        assert!(parse_payload(&json!(null)).is_empty());
    }

    #[test]
    fn content_attributes_are_dropped_and_long_values_cut() {
        let long = "x".repeat(2000);
        let body = json!({"resourceLogs": [{
            "resource": {"attributes": [
                kv("user.email", json!({"stringValue": "someone@example.invalid"})),
                kv("service.name", json!({"stringValue": "claude-code"}))
            ]},
            "scopeLogs": [{"logRecords": [{"timeUnixNano": "1759492800000000000", "attributes": [
                kv("event.name", json!({"stringValue": "claude_code.tool_decision"})),
                kv("prompt", json!({"stringValue": "synthetic secret prompt"})),
                kv("tool_parameters", json!({"stringValue": "{\"command\":\"ls\"}"})),
                kv("error", json!({"stringValue": long})),
                kv("decision", json!({"stringValue": "accept"}))
            ]}]}]
        }]});
        let events = parse_logs(&body);
        assert_eq!(events.len(), 1);
        let attrs = &events[0].attrs;
        for dropped in ["prompt", "tool_parameters", "user.email"] {
            assert!(!attrs.contains_key(dropped), "{dropped}");
        }
        assert_eq!(attrs["decision"], "accept");
        assert_eq!(attrs["service.name"], "claude-code");
        assert_eq!(attrs["error"].as_str().unwrap().chars().count(), 512);
    }

    #[test]
    fn ingest_appends_to_event_store() {
        let dir = std::env::temp_dir().join(format!("obs-otel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lock = Locked::acquire(&dir).unwrap();
        assert_eq!(ingest(&lock, &metrics_payload()).unwrap(), 5);
        assert_eq!(ingest(&lock, &logs_payload()).unwrap(), 5);
        assert_eq!(lock.read_events().len(), 10);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
