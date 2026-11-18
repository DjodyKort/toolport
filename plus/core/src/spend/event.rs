use serde::{Deserialize, Serialize};

pub const KIND_TOKENS: &str = "tokens";
pub const KIND_COST: &str = "cost";
pub const KIND_REQUEST: &str = "request";
pub const KIND_TOOL: &str = "tool";
pub const KIND_MCP: &str = "mcp";

pub const TOKEN_INPUT: &str = "input";
pub const TOKEN_OUTPUT: &str = "output";
pub const TOKEN_CACHE_READ: &str = "cacheRead";
pub const TOKEN_CACHE_CREATION: &str = "cacheCreation";

/// One line of `events.jsonl`. Our own format: OTLP is parsed as raw JSON and reduced to these
/// fields, so content attributes never reach the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ts: i64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default)]
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Event {
    pub fn new(ts: i64, kind: &str) -> Self {
        Self {
            ts,
            kind: kind.to_string(),
            ..Self::default()
        }
    }
}
