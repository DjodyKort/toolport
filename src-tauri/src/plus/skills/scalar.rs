//! YAML scalar forms for the frontmatter the transpilers write.
//!
//! mcpm writes `description: "<text>"` and `key: <text>` with the text spliced in. That is valid YAML
//! only for one-line text without quotes, backslashes or other special characters, and a multi-line
//! description became a double-quoted scalar whose continuation lines start in column 0, which Claude
//! Code rejects. These helpers keep mcpm's bytes whenever a strict parser reads them back as the same
//! string and otherwise fall back to a form that it does.

use serde_yaml::Value;

fn reads_back(scalar: &str, value: &str) -> bool {
    match serde_yaml::from_str::<Value>(&format!("k: {scalar}\n")) {
        Ok(Value::Mapping(map)) => {
            map.len() == 1 && map.get("k").and_then(Value::as_str) == Some(value)
        }
        _ => false,
    }
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn one_line(value: &str) -> bool {
    !value.chars().any(is_line_break)
}

fn single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn needs_escape(c: char) -> bool {
    let code = c as u32;
    code < 0x20
        || (0x7f..=0x9f).contains(&code)
        || matches!(c, '\u{2028}' | '\u{2029}' | '\u{feff}')
        || code & 0xfffe == 0xfffe
}

fn double_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '\u{85}' => out.push_str("\\N"),
            '\u{2028}' => out.push_str("\\L"),
            '\u{2029}' => out.push_str("\\P"),
            c if needs_escape(c) && (c as u32) <= 0xffff => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c if needs_escape(c) => out.push_str(&format!("\\U{:08X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A literal block scalar, indented by two spaces so every continuation line is indented. `None`
/// when the text has no faithful block form (leading blanks, trailing blanks, extra blank lines).
fn block_literal(value: &str) -> Option<String> {
    let (body, header) = match value.strip_suffix('\n') {
        Some(rest) if rest.ends_with('\n') => return None,
        Some(rest) => (rest, "|"),
        None => (value, "|-"),
    };
    if body.is_empty() || body.starts_with([' ', '\t', '\n']) {
        return None;
    }
    let mut out = String::from(header);
    for line in body.split('\n') {
        if line != line.trim_end() {
            return None;
        }
        if !line.is_empty() {
            out.push_str("\n  ");
            out.push_str(line);
        } else {
            out.push('\n');
        }
    }
    Some(out)
}

/// A scalar that fits anywhere a value can stand, including inside a flow list: no block form.
fn inline_scalar(value: &str) -> String {
    if one_line(value) {
        for candidate in [format!("\"{value}\""), single_quoted(value)] {
            if reads_back(&candidate, value) {
                return candidate;
            }
        }
    }
    double_quoted(value)
}

/// A one-line value stays on its line; only several lines get a block.
fn safe_scalar(value: &str) -> String {
    if !one_line(value) {
        if let Some(block) = block_literal(value) {
            if reads_back(&block, value) {
                return block;
            }
        }
    }
    inline_scalar(value)
}

/// What mcpm writes as `"<text>"` (descriptions): the same bytes while they read back unchanged.
pub(crate) fn quoted(value: &str) -> String {
    let verbatim = format!("\"{value}\"");
    if one_line(value) && reads_back(&verbatim, value) {
        verbatim
    } else {
        safe_scalar(value)
    }
}

/// Like [`quoted`] but for an item of a flow list.
pub(crate) fn quoted_item(value: &str) -> String {
    let verbatim = format!("\"{value}\"");
    if one_line(value) && reads_back(&verbatim, value) {
        verbatim
    } else {
        inline_scalar(value)
    }
}

/// What mcpm writes unquoted (`allowed-tools`, a skill's `paths`): the same bytes while a strict
/// parser reads them back as the same string.
pub(crate) fn plain(value: &str) -> String {
    if one_line(value) && reads_back(value, value) {
        value.to_string()
    } else {
        safe_scalar(value)
    }
}

/// Unquoted text for clients that read their own loose dialect (`globs: **/*.py`): kept as it is
/// unless it spans lines.
pub(crate) fn loose(value: &str) -> String {
    if one_line(value) {
        value.to_string()
    } else {
        safe_scalar(value)
    }
}
