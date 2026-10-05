//! Keeps captured values out of everything a task writes. The values live in this struct and
//! nowhere else; `scrub` replaces them in any text before it reaches a run record or a log.

use crate::plus::redact;
use serde_json::Value;

const MASK: &str = "[redacted]";
const MIN_LEN: usize = 3;

#[derive(Default)]
pub struct Redactor {
    values: Vec<String>,
}

impl Redactor {
    pub fn add(&mut self, value: &str) {
        if value.len() >= MIN_LEN && !self.values.iter().any(|v| v == value) {
            self.values.push(value.to_string());
            self.values.sort_by_key(|v| std::cmp::Reverse(v.len()));
        }
    }

    pub fn scrub(&self, text: &str) -> String {
        let masked = self.values.iter().fold(text.to_string(), |acc, v| acc.replace(v.as_str(), MASK));
        redact::scrub_text(masked)
    }

    pub fn scrub_value(&self, value: Value) -> Value {
        match redact::scrub(value) {
            Value::String(s) => Value::String(self.scrub(&s)),
            Value::Array(items) => Value::Array(items.into_iter().map(|v| self.scrub_value(v)).collect()),
            Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, self.scrub_value(v))).collect()),
            other => other,
        }
    }
}
