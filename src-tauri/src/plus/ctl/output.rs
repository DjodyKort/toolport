use serde_json::{json, Value};

pub use crate::plus::op::{ErrorKind, OpError as CtlError};

pub(super) fn no_args(rest: &[String]) -> Result<(), CtlError> {
    match rest.first() {
        Some(extra) => Err(CtlError::usage(format!("unexpected argument: {extra}"))),
        None => Ok(()),
    }
}

/// Terminal cells a character takes: emoji and wide East Asian characters take two, as in rich.
fn char_width(c: char) -> usize {
    let wide = matches!(
        c as u32,
        0x1100..=0x115F
            | 0x231A..=0x231B
            | 0x23E9..=0x23EC
            | 0x23F0
            | 0x23F3
            | 0x25FD..=0x25FE
            | 0x2614..=0x2615
            | 0x2648..=0x2653
            | 0x267F
            | 0x2693
            | 0x26A1
            | 0x26AA..=0x26AB
            | 0x26BD..=0x26BE
            | 0x26C4..=0x26C5
            | 0x26CE
            | 0x26D4
            | 0x26EA
            | 0x26F2..=0x26F3
            | 0x26F5
            | 0x26FA
            | 0x26FD
            | 0x2705
            | 0x270A..=0x270B
            | 0x2728
            | 0x274C
            | 0x274E
            | 0x2753..=0x2755
            | 0x2757
            | 0x2795..=0x2797
            | 0x27B0
            | 0x27BF
            | 0x2B1B..=0x2B1C
            | 0x2B50
            | 0x2B55
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F680..=0x1F6FF
            | 0x1F900..=0x1F9FF
            | 0x1FA70..=0x1FAFF
    );
    if wide {
        2
    } else {
        1
    }
}

fn cell_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// A box-drawn table in the layout rich prints for mcpm's skills tables. A cell holding newlines
/// spans that many lines.
pub(super) fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    table_min(headers, rows, &[])
}

/// [`table`] with a floor per column: rich sizes a wrapped column by its unwrapped content, so a
/// column capped at 50 stays 50 wide when every line wrapped shorter.
pub(super) fn table_min(headers: &[&str], rows: &[Vec<String>], floors: &[usize]) -> String {
    let split: Vec<Vec<Vec<&str>>> = rows
        .iter()
        .map(|r| r.iter().map(|c| c.split('\n').collect()).collect())
        .collect();
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            split
                .iter()
                .flat_map(|r| r[i].iter())
                .map(|l| cell_width(l))
                .fold(cell_width(h).max(floors.get(i).copied().unwrap_or(0)), usize::max)
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
            .map(|(c, w)| format!(" {c}{} ", " ".repeat(w - cell_width(c))))
            .collect();
        format!("{bar}{}{bar}", padded.join(bar))
    };
    let mut lines = vec![
        rule("┏", "┳", "┓", "━"),
        line("┃", headers),
        rule("┡", "╇", "┩", "━"),
    ];
    for row in &split {
        let height = row.iter().map(Vec::len).max().unwrap_or(1);
        for n in 0..height {
            let cells: Vec<&str> = row.iter().map(|c| c.get(n).copied().unwrap_or("")).collect();
            lines.push(line("│", &cells));
        }
    }
    lines.push(rule("└", "┴", "┘", "─"));
    lines.join("\n")
}

/// `text` word-wrapped to `width` characters the way rich wraps a table cell: a word that cannot
/// fit even on its own line is folded, spacing inside a line is kept.
pub(super) fn wrap_cell(text: &str, width: usize) -> String {
    text.split('\n')
        .map(|line| wrap_line(line, width))
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrap_line(text: &str, width: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut used = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest
            .char_indices()
            .skip_while(|(_, c)| c.is_whitespace())
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        let tail = rest[end..]
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map_or(rest.len() - end, |(i, _)| i);
        let (word, spaces) = (&rest[..end], &rest[end..end + tail]);
        rest = &rest[end + tail..];
        let len = word.chars().count();
        if used > 0 && used + len > width {
            lines.push(current.trim_end().to_string());
            current.clear();
            used = 0;
        }
        if len > width {
            let chars: Vec<char> = word.chars().collect();
            let mut chunks = chars.chunks(width).peekable();
            while let Some(chunk) = chunks.next() {
                if chunks.peek().is_some() {
                    lines.push(chunk.iter().collect());
                } else {
                    current = chunk.iter().collect();
                    used = chunk.len();
                }
            }
        } else {
            current.push_str(word);
            used += len;
        }
        current.push_str(spaces);
        used += spaces.chars().count();
    }
    lines.push(current.trim_end().to_string());
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
