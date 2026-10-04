use super::{exposed_prefix, NameMap};
use crate::plus::args::flag_or;
use crate::registry::atomic_write;
use crate::router::sanitize_segment;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const OLD_PREFIX: &str = "mcp__mcpm_";
const EXTENSIONS: &[&str] = &[
    "md", "mdc", "markdown", "txt", "yaml", "yml", "toml", "json",
];
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target"];

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"mcp__mcpm_[A-Za-z0-9_-]+\*?").expect("static regex"))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileRewrite {
    pub path: String,
    pub replaced: usize,
}

/// A reference that was left as it is. `orphans` hold the ones that name a server the import
/// knows (an unknown tool, a pattern that cannot be mapped); `dead` holds the ones that name a
/// server that is gone. `rule` is `ask` or `deny` when the reference sits in such a permission
/// list, which is what makes an orphan blocking.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct Orphan {
    pub path: String,
    pub reference: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RenameReport {
    pub dry_run: bool,
    pub scanned: usize,
    pub files: Vec<FileRewrite>,
    pub orphans: Vec<Orphan>,
    pub dead: Vec<Orphan>,
    pub replaced: usize,
}

impl RenameReport {
    #[cfg(test)]
    pub fn changed(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// The `ask` and `deny` rules that would be left pointing at a name that no longer exists.
    /// A reference to a vanished server has nothing left to gate, so it is not in here.
    pub fn blocking(&self) -> Vec<&Orphan> {
        self.orphans.iter().filter(|o| o.rule.is_some()).collect()
    }

    fn refusal(&self) -> String {
        let blocking = self.blocking();
        let mut lines = vec![format!(
            "{} ask/deny rule(s) would be left without a replacement:",
            blocking.len()
        )];
        for o in blocking {
            lines.push(format!("  {}", describe(o)));
        }
        lines.join("\n")
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "{}: {} files scanned, {} files changed, {} references rewritten, {} orphans, {} dead",
            if self.dry_run { "dry run" } else { "applied" },
            self.scanned,
            self.files.len(),
            self.replaced,
            self.orphans.len(),
            self.dead.len()
        )];
        for o in &self.orphans {
            lines.push(format!("orphan {}", describe(o)));
        }
        for o in &self.dead {
            lines.push(format!("dead {}", describe(o)));
        }
        if self.dry_run && !self.blocking().is_empty() {
            lines.push(format!("error: apply would refuse: {}", self.refusal()));
        }
        lines.join("\n")
    }
}

fn describe(o: &Orphan) -> String {
    let rule = o
        .rule
        .as_deref()
        .map(|r| format!(" [{r} rule]"))
        .unwrap_or_default();
    format!("{} in {}{rule}: {}", o.reference, o.path, o.reason)
}

fn resolve<'a>(token: &str, map: &'a NameMap) -> Option<(usize, &'a str)> {
    let mut end = token.len();
    loop {
        if let Some(new) = map.map.get(&token[..end]) {
            return Some((end, new));
        }
        end = token[..end]
            .rfind(['-', '_'])
            .filter(|i| *i > OLD_PREFIX.len())?;
    }
}

fn known_servers(map: &NameMap) -> impl Iterator<Item = &String> {
    map.servers.keys().chain(map.imported.iter())
}

/// The longest known server a name starts with, as `mcp__mcpm_<server>__`.
fn owner_of<'a>(name: &str, map: &'a NameMap) -> Option<&'a String> {
    known_servers(map)
        .filter(|server| {
            name.strip_prefix(OLD_PREFIX)
                .and_then(|rest| rest.strip_prefix(server.as_str()))
                .is_some_and(|rest| rest.starts_with("__"))
        })
        .max_by_key(|server| server.len())
}

fn names_a_server(token: &str, map: &NameMap) -> bool {
    known_servers(map).any(|server| {
        let head = format!("{OLD_PREFIX}{server}");
        token == head
            || token
                .strip_prefix(&head)
                .is_some_and(|rest| rest.starts_with("__"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gone {
    No,
    Yes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Unmapped {
    reference: String,
    gone: Gone,
    reason: String,
}

fn unmapped(reference: &str, gone: Gone, reason: impl Into<String>) -> Unmapped {
    Unmapped {
        reference: reference.to_string(),
        gone,
        reason: reason.into(),
    }
}

fn unresolved(token: &str, map: &NameMap) -> Unmapped {
    let reference = token.trim_end_matches(['-', '_']);
    if names_a_server(reference, map) {
        let reason = if known_servers(map).any(|s| reference == format!("{OLD_PREFIX}{s}")) {
            "names a whole server; only `server__tool` and `server__prefix*` map"
        } else {
            "the server's tool manifest has no such tool"
        };
        unmapped(reference, Gone::No, reason)
    } else {
        unmapped(reference, Gone::Yes, "the server is not in the import")
    }
}

/// Maps `mcp__mcpm_<server>__<prefix>*`. Mappable when every tool the pattern matches belongs
/// to one server and the new pattern matches exactly the renamed tools and no others: the
/// sanitised prefix can match more than the old one did (`a-b` and `a_b` both start with `a_`).
fn resolve_wildcard(body: &str, map: &NameMap) -> Result<String, Unmapped> {
    let reference = format!("{body}*");
    let Some(server) = owner_of(body, map) else {
        let cuts_a_name = known_servers(map).any(|s| {
            let head = format!("{OLD_PREFIX}{s}");
            head.starts_with(body) || body.starts_with(&head)
        });
        return Err(if cuts_a_name {
            unmapped(
                &reference,
                Gone::No,
                "the wildcard cuts a server name; only `server__prefix*` patterns map",
            )
        } else {
            unmapped(&reference, Gone::Yes, "the server is not in the import")
        });
    };
    let Some(id) = map.servers.get(server) else {
        return Err(unmapped(
            &reference,
            Gone::No,
            format!("server {server} has no entry in the tool manifest"),
        ));
    };
    let prefix = &body[OLD_PREFIX.len() + server.len() + 2..];
    let mut wanted = BTreeSet::new();
    let from = (Bound::Included(body), Bound::Unbounded);
    for (old, new) in map.map.range::<str, _>(from) {
        if !old.starts_with(body) {
            break;
        }
        let owner = owner_of(old, map).and_then(|name| map.servers.get(name));
        if owner != Some(id) {
            return Err(unmapped(
                &reference,
                Gone::No,
                format!("matches the tools of more than one server (also {old})"),
            ));
        }
        wanted.insert(new.as_str());
    }
    let new_body = format!("{}{}", exposed_prefix(id), sanitize_segment(prefix));
    if let Some(extra) = map
        .map
        .values()
        .find(|new| new.starts_with(&new_body) && !wanted.contains(new.as_str()))
    {
        return Err(unmapped(
            &reference,
            Gone::No,
            format!("the renamed pattern would also match {extra}, which the old one did not"),
        ));
    }
    Ok(format!("{new_body}*"))
}

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

#[derive(Debug, Default)]
struct TextScan {
    text: String,
    replaced: usize,
    unmapped: Vec<Unmapped>,
}

fn scan_text(text: &str, map: &NameMap) -> TextScan {
    let mut scan = TextScan {
        text: String::with_capacity(text.len()),
        ..TextScan::default()
    };
    let mut seen = BTreeSet::new();
    let mut last = 0;
    for m in token_re().find_iter(text) {
        scan.text.push_str(&text[last..m.start()]);
        last = m.end();
        let raw = m.as_str();
        let next = text[m.end()..].chars().next();
        let (token, wildcard, emphasis) = match raw.strip_suffix('*') {
            Some(body) if next == Some('*') => (body, false, true),
            Some(body) => (body, true, false),
            None => (raw, false, false),
        };
        let outcome = if wildcard && next.is_some_and(token_char) {
            Err(unmapped(
                raw,
                Gone::No,
                "the wildcard is not at the end of the name",
            ))
        } else if wildcard {
            resolve_wildcard(token, map)
        } else {
            match resolve(token, map) {
                Some((len, new)) => Ok(format!("{new}{}", &token[len..])),
                None => Err(unresolved(token, map)),
            }
        };
        match outcome {
            Ok(new) => {
                scan.replaced += 1;
                scan.text.push_str(&new);
            }
            Err(miss) => {
                scan.text.push_str(token);
                if wildcard {
                    scan.text.push('*');
                }
                if seen.insert(miss.reference.clone()) {
                    scan.unmapped.push(miss);
                }
            }
        }
        if emphasis {
            scan.text.push('*');
        }
    }
    scan.text.push_str(&text[last..]);
    scan
}

#[cfg(test)]
pub fn rewrite_text(text: &str, map: &NameMap) -> (String, usize, Vec<String>) {
    let scan = scan_text(text, map);
    let refs: BTreeSet<String> = scan.unmapped.into_iter().map(|u| u.reference).collect();
    (scan.text, scan.replaced, refs.into_iter().collect())
}

/// Every string in an `ask` or `deny` array of a JSON document, with the list it came from.
fn gate_rules(path: &Path, text: &str) -> Vec<(String, &'static str)> {
    let is_json = path
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| x.eq_ignore_ascii_case("json"));
    let mut rules = Vec::new();
    if !is_json {
        return rules;
    }
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return rules;
    };
    let mut stack = vec![&doc];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(fields) => {
                for (key, v) in fields {
                    let list = match key.as_str() {
                        "ask" => Some("ask"),
                        "deny" => Some("deny"),
                        _ => None,
                    };
                    match (list, v) {
                        (Some(list), Value::Array(items)) => {
                            rules.extend(
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(|rule| (rule.to_string(), list)),
                            );
                        }
                        _ => stack.push(v),
                    }
                }
            }
            Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
    rules
}

fn rule_of(reference: &str, rules: &[(String, &'static str)]) -> Option<String> {
    let hit = |list: &str| {
        rules
            .iter()
            .any(|(rule, l)| *l == list && rule.contains(reference))
    };
    ["deny", "ask"]
        .into_iter()
        .find(|list| hit(list))
        .map(String::from)
}

pub(super) fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let name = e.file_name();
            if !SKIP_DIRS.iter().any(|s| name == *s) {
                collect(&path, out)?;
            }
        } else if path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| EXTENSIONS.contains(&x.to_ascii_lowercase().as_str()))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Scans every file first and writes only when no `ask` or `deny` rule would be left without a
/// replacement: dropping a gate silently is worse than not renaming. A dry run reports the same
/// findings and writes nothing.
pub fn rename_refs(
    roots: &[PathBuf],
    map: &NameMap,
    dry_run: bool,
) -> Result<RenameReport, String> {
    let mut report = RenameReport {
        dry_run,
        ..RenameReport::default()
    };
    let mut orphans = BTreeSet::new();
    let mut dead = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut pending: Vec<(PathBuf, String)> = Vec::new();
    for root in roots {
        let mut files = Vec::new();
        if root.is_dir() {
            collect(root, &mut files)?;
        } else if root.is_file() {
            files.push(root.clone());
        } else {
            return Err(format!("{} does not exist", root.display()));
        }
        for path in files {
            if !seen.insert(path.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            report.scanned += 1;
            if !text.contains(OLD_PREFIX) {
                continue;
            }
            let scan = scan_text(&text, map);
            let shown = path.display().to_string();
            let rules = gate_rules(&path, &text);
            for miss in scan.unmapped {
                let entry = Orphan {
                    path: shown.clone(),
                    rule: rule_of(&miss.reference, &rules),
                    reference: miss.reference,
                    reason: miss.reason,
                };
                match miss.gone {
                    Gone::No => orphans.insert(entry),
                    Gone::Yes => dead.insert(entry),
                };
            }
            if scan.replaced > 0 && scan.text != text {
                report.replaced += scan.replaced;
                report.files.push(FileRewrite {
                    path: shown,
                    replaced: scan.replaced,
                });
                pending.push((path, scan.text));
            }
        }
    }
    report.orphans = orphans.into_iter().collect();
    report.dead = dead.into_iter().collect();
    if !dry_run {
        if !report.blocking().is_empty() {
            return Err(report.refusal());
        }
        for (path, text) in pending {
            atomic_write(&path, &text)?;
        }
    }
    Ok(report)
}

pub fn rename_refs_handler(args: Value) -> Result<Value, String> {
    let root = args
        .get("root")
        .and_then(Value::as_str)
        .ok_or("root is required")?;
    let tools = args
        .get("tools")
        .and_then(Value::as_str)
        .ok_or("tools is required")?;
    let paths: Vec<PathBuf> = args
        .get("paths")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect()
        })
        .filter(|p: &Vec<PathBuf>| !p.is_empty())
        .ok_or("paths is required")?;
    let dry_run = flag_or(&args, "dryRun", true);
    let opts = super::RunOptions {
        root: PathBuf::from(root),
        short_ids_path: args
            .get("shortIds")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        home: args.get("home").and_then(Value::as_str).map(String::from),
        ..super::RunOptions::default()
    };
    let map = super::name_map(&opts, Path::new(tools))?;
    let report = rename_refs(&paths, &map, dry_run)?;
    Ok(json!({ "report": report.to_value(), "summary": report.summary() }))
}
