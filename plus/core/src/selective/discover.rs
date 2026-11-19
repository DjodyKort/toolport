//! The items a source offers: skills, agents, commands and rules from the `.claude` layout, and
//! the command hooks of `hooks/hooks.json`, each with a content hash. A hook's hash covers its
//! command text, so a changed command is a changed item.

use super::store::Kind;
use crate::hashing::{finish_lock_hash, lock_hash};
use crate::sources::budget::Budget;
use crate::sources::layout::{self, Trust};
use crate::untrusted::{self, Limits, Links};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

const HOOKS_FILE: &str = "hooks/hooks.json";
const SCAN_TIME: Duration = Duration::from_secs(30);
const MAX_ENTRIES: usize = 50_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub kind: Kind,
    pub id: String,
    pub hash: String,
    /// The file or folder inside the source; `None` for hooks.
    pub rel: Option<String>,
    pub spec: Option<Value>,
}

#[derive(Clone, Debug, Default)]
pub struct Discovery {
    pub items: Vec<Found>,
    pub warnings: Vec<String>,
}

/// The hash of a file, or of a folder's files and their relative names. A link anywhere in it is
/// an error: nothing a source offers may lead outside the source.
pub fn hash_path(path: &Path) -> Result<String, String> {
    if untrusted::is_file(path) {
        let bytes = untrusted::read_bytes(path, untrusted::MAX_FILE_BYTES)?;
        return Ok(lock_hash(bytes));
    }
    let tree = untrusted::walk(path, Links::Reject, &Limits::ITEM, &[])?;
    let mut hasher = Sha256::new();
    for (rel, file, _) in tree.files() {
        let bytes = untrusted::read_bytes(file, untrusted::MAX_FILE_BYTES)?;
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        hasher.update(&bytes);
        hasher.update([0]);
    }
    Ok(finish_lock_hash(hasher))
}

pub fn hook_hash(handler: &Value) -> String {
    lock_hash(handler.get("command").and_then(Value::as_str).unwrap_or(""))
}

fn hooks(root: &Path, out: &mut Discovery) {
    let file = match untrusted::below(root, HOOKS_FILE) {
        Ok(file) => file,
        Err(why) => {
            out.warnings.push(format!(
                "{HOOKS_FILE} is not read, so its hooks are not offered: {why}"
            ));
            return;
        }
    };
    if !untrusted::is_file(&file) {
        return;
    }
    let Some(doc) = untrusted::read_json(&file, untrusted::MAX_FILE_BYTES) else {
        out.warnings.push(format!(
            "{HOOKS_FILE} is not valid JSON, so its hooks are not offered"
        ));
        return;
    };
    let events = doc.get("hooks").unwrap_or(&doc);
    let Some(events) = events.as_object() else {
        return;
    };
    let mut skipped = 0;
    let mut seen: Vec<String> = Vec::new();
    for (event, groups) in events {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group.get("matcher").and_then(Value::as_str);
            for handler in group
                .get("hooks")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let is_command = handler.get("type").and_then(Value::as_str) == Some("command")
                    && handler.get("command").and_then(Value::as_str).is_some();
                if !is_command {
                    skipped += 1;
                    continue;
                }
                let base = format!("{event}:{}", matcher.unwrap_or("*"));
                let n = seen.iter().filter(|s| **s == base).count();
                seen.push(base.clone());
                let id = if n == 0 {
                    base
                } else {
                    format!("{base}#{}", n + 1)
                };
                let mut spec = json!({"event": event, "handler": handler});
                if let Some(matcher) = matcher {
                    spec["matcher"] = json!(matcher);
                }
                out.items.push(Found {
                    kind: Kind::Hook,
                    id,
                    hash: hook_hash(handler),
                    rel: None,
                    spec: Some(spec),
                });
            }
        }
    }
    if skipped > 0 {
        out.warnings.push(format!(
            "{skipped} hook(s) that are not shell commands are not offered"
        ));
    }
}

pub fn discover(root: &Path) -> Discovery {
    let mut out = Discovery::default();
    let budget = Budget::new(SCAN_TIME, 6, Some(MAX_ENTRIES));
    let mut links = Vec::new();
    let scanned = layout::scan_layout_as(root, &budget, Trust::Untrusted, &mut links);
    out.warnings.extend(
        links
            .into_iter()
            .map(|rel| format!("{rel} is a symbolic link, so it is not offered")),
    );
    for found in scanned {
        let Some(kind) = Kind::parse(found.kind) else {
            continue;
        };
        let rel = if kind == Kind::Skill {
            found.rel.trim_end_matches("/SKILL.md").to_string()
        } else {
            found.rel.clone()
        };
        match hash_path(&root.join(&rel)) {
            Ok(hash) => out.items.push(Found {
                kind,
                id: found.fallback,
                hash,
                rel: Some(rel),
                spec: None,
            }),
            Err(why) => out
                .warnings
                .push(format!("{} is skipped: {why}", found.fallback)),
        }
    }
    hooks(root, &mut out);
    out.items
        .sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    out
}

fn root_refs(command: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"CLAUDE_PLUGIN_ROOT\}?"?/([A-Za-z0-9_@.+/-]+)"#).expect("plugin root pattern")
    });
    re.captures_iter(command)
        .map(|c| c[1].trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty() && !p.split('/').any(|part| part.is_empty() || part == ".."))
        .collect()
}

/// The files and folders of the source that the given hooks run through `${CLAUDE_PLUGIN_ROOT}`;
/// they travel with the slice so a ticked hook has its script.
pub fn support_paths(handlers: &[&Value]) -> Vec<String> {
    let mut paths: Vec<String> = handlers
        .iter()
        .filter_map(|h| h.get("command").and_then(Value::as_str))
        .flat_map(root_refs)
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

pub fn support_hash(root: &Path, paths: &[String]) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let mut hasher = Sha256::new();
    for rel in paths {
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        let hash = untrusted::below(root, rel)
            .and_then(|path| hash_path(&path))
            .unwrap_or_default();
        hasher.update(hash.as_bytes());
        hasher.update([0]);
    }
    Some(finish_lock_hash(hasher))
}

#[cfg(test)]
mod tests {
    use super::root_refs;

    #[test]
    fn test01_plugin_root_references_stay_inside_the_source() {
        let command =
            r#""${CLAUDE_PLUGIN_ROOT}"/scripts/format.sh --all && $CLAUDE_PLUGIN_ROOT/../x"#;
        assert_eq!(root_refs(command), ["scripts/format.sh"]);
    }
}
