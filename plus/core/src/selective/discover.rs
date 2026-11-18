//! The items a source offers: skills, agents, commands and rules from the `.claude` layout, and
//! the command hooks of `hooks/hooks.json`, each with a content hash. A hook's hash covers its
//! command text, so a changed command is a changed item.

use super::store::Kind;
use crate::context::fsx::{self, Kind as FileKind};
use crate::hashing::{finish_lock_hash, lock_hash};
use crate::sources::budget::Budget;
use crate::sources::layout;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

const HOOKS_FILE: &str = "hooks/hooks.json";
const SCAN_TIME: Duration = Duration::from_secs(30);
const MAX_ENTRIES: usize = 50_000;
const MAX_FILES: usize = 2_000;
const MAX_BYTES: usize = 20 * 1024 * 1024;
const MAX_FILE: usize = 4 * 1024 * 1024;

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

struct Tally {
    files: usize,
    bytes: usize,
}

fn feed(hasher: &mut Sha256, base: &Path, rel: &str, tally: &mut Tally) -> Result<(), String> {
    let path = if rel.is_empty() {
        base.to_path_buf()
    } else {
        base.join(rel)
    };
    for entry in fsx::list_dir(&path) {
        if entry.symlink {
            continue;
        }
        let child = if rel.is_empty() {
            entry.name.clone()
        } else {
            format!("{rel}/{}", entry.name)
        };
        match entry.kind {
            FileKind::Dir => feed(hasher, base, &child, tally)?,
            FileKind::File => {
                tally.files += 1;
                let bytes = fsx::read_bytes(&entry.path, MAX_FILE + 1)
                    .ok_or_else(|| format!("{child} cannot be read"))?;
                tally.bytes += bytes.len();
                if tally.files > MAX_FILES || bytes.len() > MAX_FILE || tally.bytes > MAX_BYTES {
                    return Err(format!("{child}: the folder is too large to select from"));
                }
                hasher.update(child.as_bytes());
                hasher.update([0]);
                hasher.update(&bytes);
                hasher.update([0]);
            }
            FileKind::Other => {}
        }
    }
    Ok(())
}

/// The hash of a file, or of a folder's files and their relative names (symlinks are left out).
pub fn hash_path(path: &Path) -> Result<String, String> {
    if fsx::is_file(path) {
        let bytes = fsx::read_bytes(path, MAX_FILE + 1)
            .filter(|b| b.len() <= MAX_FILE)
            .ok_or_else(|| format!("{} cannot be read", path.display()))?;
        return Ok(lock_hash(bytes));
    }
    let mut hasher = Sha256::new();
    let mut tally = Tally { files: 0, bytes: 0 };
    feed(&mut hasher, path, "", &mut tally)?;
    Ok(finish_lock_hash(hasher))
}

pub fn hook_hash(handler: &Value) -> String {
    lock_hash(handler.get("command").and_then(Value::as_str).unwrap_or(""))
}

fn hooks(root: &Path, out: &mut Discovery) {
    let file = root.join(HOOKS_FILE);
    if !fsx::is_file(&file) {
        return;
    }
    let Some(doc) =
        fsx::read_text(&file, MAX_FILE).and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
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
    for found in layout::scan_layout(root, &budget) {
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
        let hash = hash_path(&root.join(rel)).unwrap_or_default();
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
