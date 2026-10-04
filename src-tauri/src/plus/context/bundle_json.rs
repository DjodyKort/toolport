//! Surgical edits of a JSON text. A bundle changes a few keys in a settings file that Claude Code
//! writes too, so every byte outside the keys it owns must stay as it was: values are replaced,
//! members and array items are inserted after the last one and removed by the exact inverse, and
//! nothing is re-serialised. The text is parsed again after every edit.

use serde_json::Value;

#[derive(Debug)]
enum Node {
    Scalar { start: usize, end: usize },
    Obj { start: usize, end: usize, members: Vec<Member> },
    Arr { start: usize, end: usize, items: Vec<Node> },
}

#[derive(Debug)]
struct Member {
    key: String,
    key_start: usize,
    val: Node,
}

impl Node {
    fn span(&self) -> (usize, usize) {
        match self {
            Node::Scalar { start, end } | Node::Obj { start, end, .. } | Node::Arr { start, end, .. } => {
                (*start, *end)
            }
        }
    }
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

const MAX_DEPTH: usize = 64;

impl Parser<'_> {
    fn ws(&mut self) {
        while self.at < self.bytes.len() && matches!(self.bytes[self.at], b' ' | b'\t' | b'\n' | b'\r') {
            self.at += 1;
        }
    }

    fn string(&mut self) -> Result<(usize, usize), String> {
        let start = self.at;
        self.at += 1;
        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'\\' => self.at += 2,
                b'"' => {
                    self.at += 1;
                    return Ok((start, self.at));
                }
                _ => self.at += 1,
            }
        }
        Err("unterminated string".into())
    }

    fn value(&mut self, depth: usize) -> Result<Node, String> {
        if depth > MAX_DEPTH {
            return Err("nested too deeply".into());
        }
        self.ws();
        let start = self.at;
        match self.bytes.get(self.at) {
            None => Err("unexpected end".into()),
            Some(b'{') => {
                self.at += 1;
                let mut members = Vec::new();
                loop {
                    self.ws();
                    match self.bytes.get(self.at) {
                        Some(b'}') if members.is_empty() => {
                            self.at += 1;
                            break;
                        }
                        Some(b'"') => {}
                        _ => return Err(format!("expected a key at byte {}", self.at)),
                    }
                    let (key_start, key_end) = self.string()?;
                    let key: String = serde_json::from_str(&self.text[key_start..key_end])
                        .map_err(|e| format!("bad key: {e}"))?;
                    self.ws();
                    if self.bytes.get(self.at) != Some(&b':') {
                        return Err(format!("expected ':' at byte {}", self.at));
                    }
                    self.at += 1;
                    let val = self.value(depth + 1)?;
                    members.push(Member { key, key_start, val });
                    self.ws();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            break;
                        }
                        _ => return Err(format!("expected ',' or '}}' at byte {}", self.at)),
                    }
                }
                Ok(Node::Obj { start, end: self.at, members })
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.bytes.get(self.at) == Some(&b']') && items.is_empty() {
                        self.at += 1;
                        break;
                    }
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.bytes.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            break;
                        }
                        _ => return Err(format!("expected ',' or ']' at byte {}", self.at)),
                    }
                }
                Ok(Node::Arr { start, end: self.at, items })
            }
            Some(b'"') => {
                let (start, end) = self.string()?;
                Ok(Node::Scalar { start, end })
            }
            Some(_) => {
                while self.at < self.bytes.len()
                    && !matches!(self.bytes[self.at], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r')
                {
                    self.at += 1;
                }
                if self.at == start {
                    return Err(format!("unexpected character at byte {start}"));
                }
                Ok(Node::Scalar { start, end: self.at })
            }
        }
    }
}

fn parse(text: &str) -> Result<Node, String> {
    let mut parser = Parser { text, bytes: text.as_bytes(), at: 0 };
    let node = parser.value(0)?;
    parser.ws();
    if parser.at != text.len() {
        return Err(format!("trailing data at byte {}", parser.at));
    }
    serde_json::from_str::<Value>(text).map_err(|e| e.to_string())?;
    Ok(node)
}

fn walk<'a>(node: &'a Node, path: &[&str]) -> Option<&'a Node> {
    let mut at = node;
    for part in path {
        let Node::Obj { members, .. } = at else {
            return None;
        };
        at = &members.iter().find(|m| m.key == *part)?.val;
    }
    Some(at)
}

pub fn check(text: &str) -> Result<(), String> {
    match parse(text)? {
        Node::Obj { .. } => Ok(()),
        _ => Err("the top level is not an object".into()),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    Scalar(String),
    Container,
}

pub fn get(text: &str, path: &[&str]) -> Option<Found> {
    let root = parse(text).ok()?;
    match walk(&root, path)? {
        Node::Scalar { start, end } => Some(Found::Scalar(text[*start..*end].to_string())),
        _ => Some(Found::Container),
    }
}

pub fn value(text: &str, path: &[&str]) -> Option<Value> {
    let root = parse(text).ok()?;
    let (start, end) = walk(&root, path)?.span();
    serde_json::from_str(&text[start..end]).ok()
}

pub fn has_members(text: &str, path: &[&str]) -> bool {
    parse(text)
        .ok()
        .and_then(|root| match walk(&root, path)? {
            Node::Obj { members, .. } => Some(!members.is_empty()),
            Node::Arr { items, .. } => Some(!items.is_empty()),
            Node::Scalar { .. } => None,
        })
        .unwrap_or(false)
}

fn line_indent(text: &str, at: usize) -> &str {
    let line = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let rest = &text[line..];
    let width = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    &rest[..width]
}

fn unit(text: &str) -> String {
    for line in text.lines() {
        let trimmed = line.trim_start_matches([' ', '\t']);
        let width = line.len() - trimmed.len();
        if width > 0 && !trimmed.is_empty() {
            return line[..width].to_string();
        }
    }
    "  ".into()
}

fn gap_before(text: &str, at: usize) -> &str {
    let head = &text[..at];
    let kept = head.trim_end_matches([' ', '\t', '\n', '\r']).len();
    &text[kept..at]
}

fn render_key(key: &str) -> String {
    serde_json::to_string(key).unwrap_or_default()
}

fn replace(text: &mut String, from: usize, to: usize, with: &str) {
    text.replace_range(from..to, with);
}

fn insert_into(text: &mut String, path: &[&str], key: Option<&str>, value: &str) -> Result<(), String> {
    let root = parse(text)?;
    let parent = walk(&root, path).ok_or_else(|| format!("{} is not there", path.join(".")))?;
    let unit = unit(text);
    let entry = |sep: &str| match key {
        Some(k) => format!("{sep}{}: {value}", render_key(k)),
        None => format!("{sep}{value}"),
    };
    match (parent, key) {
        (Node::Obj { start, end, members }, Some(_)) => {
            if let Some(last) = members.last() {
                let sep = gap_before(text, last.key_start).to_string();
                let at = last.val.span().1;
                text.insert_str(at, &format!(",{}", entry(&sep)));
            } else {
                let indent = line_indent(text, *start).to_string();
                let inner = format!("{}\n{indent}", entry(&format!("\n{indent}{unit}")));
                replace(text, start + 1, end - 1, &inner);
            }
            Ok(())
        }
        (Node::Arr { start, end, items }, None) => {
            if let Some(last) = items.last() {
                let (first, at) = last.span();
                let sep = gap_before(text, first).to_string();
                text.insert_str(at, &format!(",{}", entry(&sep)));
            } else {
                let indent = line_indent(text, *start).to_string();
                let inner = format!("{}\n{indent}", entry(&format!("\n{indent}{unit}")));
                replace(text, start + 1, end - 1, &inner);
            }
            Ok(())
        }
        _ => Err(format!("{} is not the kind of container expected", path.join("."))),
    }
}

fn ensure_objects(text: &mut String, path: &[&str]) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for depth in 0..path.len() {
        let root = parse(text)?;
        match walk(&root, &path[..=depth]) {
            Some(Node::Obj { .. }) => {}
            Some(_) => return Err(format!("{} is not an object", path[..=depth].join("."))),
            None => {
                insert_into(text, &path[..depth], Some(path[depth]), "{}")?;
                created.push(path[..=depth].join("."));
            }
        }
    }
    Ok(created)
}

/// Sets `path` to `value` (JSON text), creating the objects on the way; returns the paths of the
/// objects it created, outermost first. A value in the way that is not an object is an error.
pub fn set(text: &mut String, path: &[&str], value: &str) -> Result<Vec<String>, String> {
    check(text)?;
    let (last, parents) = path.split_last().ok_or("an empty path")?;
    let created = ensure_objects(text, parents)?;
    let root = parse(text)?;
    match walk(&root, path) {
        Some(node) => {
            let (start, end) = node.span();
            replace(text, start, end, value);
        }
        None => insert_into(text, parents, Some(last), value)?,
    }
    Ok(created)
}

/// Appends `value` to the array at `path`, creating the array and the objects on the way.
pub fn push(text: &mut String, path: &[&str], value: &str) -> Result<Vec<String>, String> {
    check(text)?;
    let (last, parents) = path.split_last().ok_or("an empty path")?;
    let mut created = ensure_objects(text, parents)?;
    let root = parse(text)?;
    match walk(&root, path) {
        Some(Node::Arr { .. }) => {}
        Some(_) => return Err(format!("{} is not a list", path.join("."))),
        None => {
            insert_into(text, parents, Some(last), "[]")?;
            created.push(path.join("."));
        }
    }
    insert_into(text, path, None, value)?;
    Ok(created)
}

fn remove_span(text: &mut String, spans: &[(usize, usize)], first_key: &[usize], index: usize, open: usize, close: usize) {
    let n = spans.len();
    if n == 1 {
        replace(text, open + 1, close - 1, "");
    } else if index > 0 {
        replace(text, spans[index - 1].1, spans[index].1, "");
    } else {
        replace(text, first_key[0], first_key[1], "");
    }
}

/// Removes the member at `path`; false when it is not there.
pub fn remove(text: &mut String, path: &[&str]) -> Result<bool, String> {
    let (last, parents) = path.split_last().ok_or("an empty path")?;
    let root = parse(text)?;
    let Some(Node::Obj { start, end, members }) = walk(&root, parents) else {
        return Ok(false);
    };
    let Some(index) = members.iter().position(|m| m.key == *last) else {
        return Ok(false);
    };
    let spans: Vec<(usize, usize)> = members.iter().map(|m| m.val.span()).collect();
    let keys = [members[0].key_start, members.get(1).map_or(0, |m| m.key_start)];
    let (start, end) = (*start, *end);
    remove_span(text, &spans, &keys, index, start, end);
    Ok(true)
}

/// Removes the first item of the array at `path` whose value equals `item`.
pub fn pull(text: &mut String, path: &[&str], item: &Value) -> Result<bool, String> {
    let root = parse(text)?;
    let Some(Node::Arr { start, end, items }) = walk(&root, path) else {
        return Ok(false);
    };
    let Some(index) = items.iter().position(|n| {
        let (s, e) = n.span();
        serde_json::from_str::<Value>(&text[s..e]).is_ok_and(|v| v == *item)
    }) else {
        return Ok(false);
    };
    let spans: Vec<(usize, usize)> = items.iter().map(Node::span).collect();
    let keys = [spans[0].0, spans.get(1).map_or(0, |s| s.0)];
    let (start, end) = (*start, *end);
    remove_span(text, &spans, &keys, index, start, end);
    Ok(true)
}

/// Removes the member at `path` when it is an empty object or list.
pub fn prune_empty(text: &mut String, path: &[&str]) -> Result<bool, String> {
    let root = parse(text)?;
    let empty = match walk(&root, path) {
        Some(Node::Obj { members, .. }) => members.is_empty(),
        Some(Node::Arr { items, .. }) => items.is_empty(),
        _ => false,
    };
    if empty {
        remove(text, path)
    } else {
        Ok(false)
    }
}
