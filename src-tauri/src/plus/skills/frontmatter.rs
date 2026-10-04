//! Would the client accept the YAML frontmatter of an emitted skill file?
//!
//! Claude Code lists a skill for the model only when it can read its frontmatter. A parse failure
//! hides the skill without a message, and a strict YAML parse is not enough: a double-quoted or
//! single-quoted value that continues on a line starting in column 0 is valid for libyaml but is
//! rejected by Claude Code's own parser. [`frontmatter_accepted`] checks both.

use super::parser::{find_fence_line, is_fence_line};
use serde_yaml::Value;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The opening `---` has no closing `---` line.
    Unterminated,
    /// A quoted multi-line value whose continuation line `line` (1-based, in the file) is not
    /// indented further than the key that owns it.
    UnindentedContinuation {
        line: usize,
        text: String,
    },
    InvalidYaml(String),
    NotAMapping,
}

impl Reason {
    pub fn code(&self) -> &'static str {
        match self {
            Reason::Unterminated => "unterminated",
            Reason::UnindentedContinuation { .. } => "unindented-continuation",
            Reason::InvalidYaml(_) => "invalid-yaml",
            Reason::NotAMapping => "not-a-mapping",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Unterminated => f.write_str("the frontmatter has no closing `---` line"),
            Reason::UnindentedContinuation { line, text } => {
                let shown: String = text.chars().take(60).collect();
                write!(
                    f,
                    "line {line} continues a quoted multi-line value without indentation: {shown}"
                )
            }
            Reason::InvalidYaml(message) => write!(f, "invalid YAML: {message}"),
            Reason::NotAMapping => f.write_str("the frontmatter is not a YAML mapping"),
        }
    }
}

/// How much of the check a client's output gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strictness {
    /// The frontmatter must parse as YAML and contain no unindented continuation line.
    Strict,
    /// Only the unindented continuation check: the client reads `globs: **/*.py` and similar in a
    /// dialect of its own, which a YAML parser refuses.
    Shape,
}

/// Clients whose skill files are plain YAML frontmatter.
pub fn strictness_for(client_key: &str) -> Strictness {
    match client_key {
        "claude-code" | "codex-cli" | "gemini-cli" | "goose-cli" => Strictness::Strict,
        _ => Strictness::Shape,
    }
}

/// `Ok` for a file without frontmatter: there is nothing to reject.
pub fn frontmatter_accepted(text: &str) -> Result<(), Reason> {
    check(text, Strictness::Strict)
}

/// The check at the strictness `client_key` is held to.
pub fn output_accepted(client_key: &str, text: &str) -> Result<(), Reason> {
    match strictness_for(client_key) {
        Strictness::Strict => frontmatter_accepted(text),
        Strictness::Shape => check(text, Strictness::Shape),
    }
}

fn check(text: &str, strictness: Strictness) -> Result<(), Reason> {
    let Some(newline) = text.find('\n') else {
        return Ok(());
    };
    let yaml_start = newline + 1;
    if !is_fence_line(&text[..yaml_start]) {
        return Ok(());
    }
    let Some(close) = find_fence_line(text, yaml_start) else {
        return Err(Reason::Unterminated);
    };
    let yaml = &text[yaml_start..close];
    if let Some((index, line)) = unindented_continuation(yaml) {
        return Err(Reason::UnindentedContinuation {
            line: index + 2,
            text: line,
        });
    }
    if strictness == Strictness::Shape {
        return Ok(());
    }
    match serde_yaml::from_str::<Value>(yaml) {
        Ok(Value::Mapping(_) | Value::Null) => Ok(()),
        Ok(_) => Err(Reason::NotAMapping),
        Err(e) => Err(Reason::InvalidYaml(e.to_string())),
    }
}

enum State {
    Top,
    /// Inside a quoted scalar opened on a line indented by `owner`.
    Quoted {
        quote: char,
        owner: usize,
    },
    /// Inside a block scalar (`|` or `>`) whose key is indented by `owner`.
    Block {
        owner: usize,
    },
}

/// Does `text` hold the closing quote of a scalar that is already open?
fn closes(text: &str, quote: char) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if quote == '"' => {
                chars.next();
            }
            '\'' if quote == '\'' => {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    return true;
                }
            }
            '"' if quote == '"' => return true,
            _ => {}
        }
    }
    false
}

/// The value part of a `key: value` or `- value` line, with the indent that owns it.
fn value_of(line: &str, indent: usize) -> Option<(usize, &str)> {
    let mut rest = &line[indent..];
    let mut owner = indent;
    let mut sequence = false;
    while let Some(after) = rest.strip_prefix("- ") {
        owner = indent;
        sequence = true;
        rest = after.trim_start_matches(' ');
    }
    let key_end = match rest.chars().next()? {
        quote @ ('"' | '\'') => rest[1..]
            .find(quote)
            .map(|at| at + 2)
            .filter(|close| rest[*close..].starts_with(':')),
        '#' => None,
        _ => rest
            .match_indices(':')
            .find(|(i, _)| rest[i + 1..].is_empty() || rest[i + 1..].starts_with(' '))
            .map(|(i, _)| i),
    };
    match key_end {
        Some(end) => Some((owner, rest[end + 1..].trim_start_matches(' '))),
        None if sequence => Some((owner, rest)),
        None => None,
    }
}

/// The first continuation line of a quoted multi-line value that is not indented further than the
/// line that opened it: its index in `yaml` and its text.
fn unindented_continuation(yaml: &str) -> Option<(usize, String)> {
    let mut state = State::Top;
    for (index, line) in yaml.lines().enumerate() {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let blank = line.trim().is_empty();
        match state {
            State::Quoted { quote, owner } => {
                if blank {
                    continue;
                }
                if indent <= owner {
                    return Some((index, line.to_string()));
                }
                if closes(&line[indent..], quote) {
                    state = State::Top;
                }
                continue;
            }
            State::Block { owner } if blank || indent > owner => continue,
            _ => {}
        }
        state = State::Top;
        let Some((owner, value)) = value_of(line, indent) else {
            continue;
        };
        match value.chars().next() {
            Some(quote @ ('"' | '\'')) if !closes(&value[1..], quote) => {
                state = State::Quoted { quote, owner };
            }
            Some('|' | '>') => state = State::Block { owner },
            _ => {}
        }
    }
    None
}
