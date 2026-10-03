use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    NotFound,
    Conflict,
    NotImplemented,
    Unhealthy,
    Failed(&'static str),
}

impl ErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::NotImplemented => "not_implemented",
            Self::Unhealthy => "unhealthy",
            Self::Failed(code) => code,
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Self::Usage => super::EXIT_USAGE,
            _ => super::EXIT_ERROR,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CtlError {
    pub kind: ErrorKind,
    pub message: String,
}

impl CtlError {
    fn of(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn failed(code: &'static str, message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Failed(code), message)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Usage, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Conflict, message)
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::NotImplemented, message)
    }

    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Unhealthy, message)
    }

    pub fn code(&self) -> &'static str {
        self.kind.code()
    }
}

pub(super) fn no_args(rest: &[String]) -> Result<(), CtlError> {
    match rest.first() {
        Some(extra) => Err(CtlError::usage(format!("unexpected argument: {extra}"))),
        None => Ok(()),
    }
}

/// A box-drawn table in the layout rich prints for mcpm's skills tables.
pub(super) fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            rows.iter()
                .map(|r| r[i].chars().count())
                .fold(h.chars().count(), usize::max)
        })
        .collect();
    let rule = |left: &str, mid: &str, right: &str, fill: &str| {
        let cells: Vec<String> = widths.iter().map(|w| fill.repeat(w + 2)).collect();
        format!("{left}{}{right}", cells.join(mid))
    };
    let line = |bar: &str, cells: &[&str]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!(" {c}{} ", " ".repeat(w - c.chars().count())))
            .collect();
        format!("{bar}{}{bar}", padded.join(bar))
    };
    let mut lines = vec![
        rule("┏", "┳", "┓", "━"),
        line("┃", headers),
        rule("┡", "╇", "┩", "━"),
    ];
    for row in rows {
        let cells: Vec<&str> = row.iter().map(String::as_str).collect();
        lines.push(line("│", &cells));
    }
    lines.push(rule("└", "┴", "┘", "─"));
    lines.join("\n")
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
            value["error"] = json!({"code": error.code(), "message": error.message});
        }
        value
    }
}
