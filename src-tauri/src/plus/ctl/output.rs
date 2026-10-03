use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct CtlError {
    pub code: String,
    pub message: String,
}

impl CtlError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new("usage", message)
    }
}

/// A successful command result: structured data for `--json` and the text
/// rendering for humans. `failed` keeps the data but exits 1 (doctor).
pub struct Output {
    pub data: Value,
    pub human: String,
    pub failed: bool,
}

impl Output {
    pub fn new(data: Value, human: String) -> Self {
        Self {
            data,
            human,
            failed: false,
        }
    }
}

pub struct Envelope {
    pub ok: bool,
    pub command: String,
    pub data: Option<Value>,
    pub error: Option<CtlError>,
}

impl Envelope {
    pub fn success(command: &str, data: Value) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            data: Some(data),
            error: None,
        }
    }

    pub fn failure(command: &str, error: CtlError) -> Self {
        Self {
            ok: false,
            command: command.to_string(),
            data: None,
            error: Some(error),
        }
    }

    pub fn failure_with_data(command: &str, error: CtlError, data: Value) -> Self {
        Self {
            ok: false,
            command: command.to_string(),
            data: Some(data),
            error: Some(error),
        }
    }

    pub fn to_value(&self) -> Value {
        let mut value = json!({
            "ok": self.ok,
            "command": self.command,
            "schemaVersion": super::SCHEMA_VERSION,
        });
        if let Some(data) = &self.data {
            value["data"] = data.clone();
        }
        if let Some(error) = &self.error {
            value["error"] = json!({"code": error.code, "message": error.message});
        }
        value
    }
}
