//! OTLP/HTTP JSON to events. Only an allowlist of attributes is read, so prompts, tool
//! parameters and other content attributes are dropped on receipt; the strings that are kept are
//! cut and passed through the secret redaction, because `events.jsonl` sits outside upstream's.

use serde_json::Value;

use super::event::*;

pub const MAX_VALUE_CHARS: usize = 200;

const SESSION_KEYS: &[&str] = &["session.id", "session_id"];
const PROJECT_KEYS: &[&str] = &[
    "project.path",
    "project",
    "working_directory",
    "workspace.host_path",
    "cwd",
];
const TOKEN_METRIC: &str = "token.usage";
const COST_METRIC: &str = "cost.usage";

#[derive(Debug, PartialEq)]
pub enum Signal {
    Metrics,
    Logs,
    Traces,
}

pub fn signal_of(path: &str) -> Option<Signal> {
    match path.split('?').next().unwrap_or("") {
        "/v1/metrics" => Some(Signal::Metrics),
        "/v1/logs" => Some(Signal::Logs),
        "/v1/traces" => Some(Signal::Traces),
        _ => None,
    }
}

pub fn parse(signal: &Signal, body: &[u8]) -> Result<Vec<Event>, String> {
    let root: Value = serde_json::from_slice(body).map_err(|e| format!("not JSON: {e}"))?;
    Ok(match signal {
        Signal::Metrics => metrics(&root),
        Signal::Logs => logs(&root),
        Signal::Traces => Vec::new(),
    })
}

fn clean(text: &str) -> String {
    let cut: String = text.chars().take(MAX_VALUE_CHARS).collect();
    crate::redact::scrub_text(cut)
}

struct Attrs(Vec<(String, Value)>);

impl Attrs {
    fn of(list: Option<&Value>) -> Self {
        let items = list.and_then(Value::as_array).cloned().unwrap_or_default();
        Self(
            items
                .into_iter()
                .filter_map(|item| {
                    let key = item.get("key")?.as_str()?.to_string();
                    Some((key, item.get("value")?.clone()))
                })
                .collect(),
        )
    }

    fn raw(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn text(&self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|key| {
            let value = self.raw(key)?;
            let text = value
                .get("stringValue")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| number_of(value).map(|n| n.to_string()))?;
            let text = clean(text.trim());
            (!text.is_empty()).then_some(text)
        })
    }

    fn number(&self, keys: &[&str]) -> Option<f64> {
        keys.iter().find_map(|key| number_of(self.raw(key)?))
    }
}

fn number_of(value: &Value) -> Option<f64> {
    if let Some(n) = value.get("doubleValue").and_then(Value::as_f64) {
        return Some(n);
    }
    let int = value.get("intValue").or_else(|| value.get("asInt"))?;
    int.as_f64().or_else(|| int.as_str()?.parse().ok())
}

fn point_value(point: &Value) -> Option<f64> {
    if let Some(n) = point.get("asDouble").and_then(Value::as_f64) {
        return Some(n);
    }
    let int = point.get("asInt")?;
    int.as_f64().or_else(|| int.as_str()?.parse().ok())
}

fn nanos_to_ms(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    let nanos = value
        .as_str()
        .and_then(|s| s.parse::<u128>().ok())
        .or_else(|| value.as_u64().map(u128::from))?;
    (nanos > 0).then(|| (nanos / 1_000_000) as i64)
}

fn metric_suffix(name: &str) -> &str {
    name.strip_prefix("claude_code.").unwrap_or(name)
}

fn token_type(raw: &str) -> Option<&'static str> {
    match raw {
        "input" => Some(TOKEN_INPUT),
        "output" => Some(TOKEN_OUTPUT),
        "cacheRead" | "cache_read" => Some(TOKEN_CACHE_READ),
        "cacheCreation" | "cache_creation" => Some(TOKEN_CACHE_CREATION),
        _ => None,
    }
}

fn metrics(root: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    for resource in array(root, "resourceMetrics") {
        let resource_attrs = Attrs::of(resource.get("resource").and_then(|r| r.get("attributes")));
        for scope in array(resource, "scopeMetrics") {
            for metric in array(scope, "metrics") {
                let name = metric.get("name").and_then(Value::as_str).unwrap_or("");
                let kind = match metric_suffix(name) {
                    TOKEN_METRIC => KIND_TOKENS,
                    COST_METRIC => KIND_COST,
                    _ => continue,
                };
                let body = metric.get("sum").or_else(|| metric.get("gauge"));
                for point in body.map(|b| array(b, "dataPoints")).unwrap_or_default() {
                    let attrs = Attrs::of(point.get("attributes"));
                    let Some(value) = point_value(point).filter(|v| v.is_finite() && *v >= 0.0)
                    else {
                        continue;
                    };
                    let mut event = Event::new(
                        nanos_to_ms(point.get("timeUnixNano")).unwrap_or_else(super::now_ms),
                        kind,
                    );
                    event.value = value;
                    event.session = attrs
                        .text(SESSION_KEYS)
                        .or_else(|| resource_attrs.text(SESSION_KEYS));
                    event.model = attrs.text(&["model"]);
                    event.project = attrs
                        .text(PROJECT_KEYS)
                        .or_else(|| resource_attrs.text(PROJECT_KEYS));
                    if kind == KIND_TOKENS {
                        let Some(kind_of) = attrs.text(&["type"]).and_then(|raw| token_type(&raw))
                        else {
                            continue;
                        };
                        event.token_type = Some(kind_of.to_string());
                    }
                    out.push(event);
                }
            }
        }
    }
    out
}

fn logs(root: &Value) -> Vec<Event> {
    let mut out = Vec::new();
    for resource in array(root, "resourceLogs") {
        let resource_attrs = Attrs::of(resource.get("resource").and_then(|r| r.get("attributes")));
        for scope in array(resource, "scopeLogs") {
            for record in array(scope, "logRecords") {
                let attrs = Attrs::of(record.get("attributes"));
                let name = attrs
                    .text(&["event.name"])
                    .or_else(|| {
                        record
                            .get("body")
                            .and_then(|b| b.get("stringValue"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                let kind = match metric_suffix(&name) {
                    "api_request" => KIND_REQUEST,
                    "tool_decision" | "tool_result" => KIND_TOOL,
                    "mcp_server_connection" => KIND_MCP,
                    _ => continue,
                };
                let ts = nanos_to_ms(record.get("timeUnixNano")).unwrap_or_else(super::now_ms);
                let mut event = Event::new(ts, kind);
                event.session = attrs
                    .text(SESSION_KEYS)
                    .or_else(|| resource_attrs.text(SESSION_KEYS));
                event.project = attrs
                    .text(PROJECT_KEYS)
                    .or_else(|| resource_attrs.text(PROJECT_KEYS));
                event.model = attrs.text(&["model"]);
                match kind {
                    KIND_REQUEST => {
                        event.value = attrs.number(&["cost_usd"]).unwrap_or(0.0);
                        event.input = whole(attrs.number(&["input_tokens"]));
                        event.output = whole(attrs.number(&["output_tokens"]));
                        event.cache_read = whole(attrs.number(&["cache_read_tokens"]));
                        event.cache_creation = whole(attrs.number(&["cache_creation_tokens"]));
                    }
                    KIND_TOOL => event.name = attrs.text(&["tool_name", "tool.name"]),
                    _ => event.name = attrs.text(&["server_name", "mcp_server_name"]),
                }
                out.push(event);
            }
        }
    }
    out
}

fn whole(value: Option<f64>) -> Option<u64> {
    value
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v as u64)
}

fn array<'a>(value: &'a Value, key: &str) -> Vec<&'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}
