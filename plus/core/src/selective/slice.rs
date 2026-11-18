//! The generated plugin: one `<name>-selected` per source in the Toolport-owned local marketplace
//! `toolport-selections`. The entry has `strict:false` and lists exactly the chosen items, which
//! are copies inside the marketplace; the folder carries no `plugin.json`, because a plugin
//! manifest next to a marketplace entry that lists components is a conflict for Claude Code.

use super::discover::support_paths;
use super::store::{Item, Kind, SourceSel};
use crate::context::fsx::{self, Kind as FileKind};
use crate::fsx::{atomic_write, atomic_write_bytes, Mode};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const MARKETPLACE: &str = "toolport-selections";
pub const SUFFIX: &str = "-selected";
const MARKET_FILE: &str = ".claude-plugin/marketplace.json";

pub fn slice_name(base_name: &str) -> String {
    format!("{base_name}{SUFFIX}")
}

pub fn plugin_id(slice: &str) -> String {
    format!("{slice}@{MARKETPLACE}")
}

pub fn market_root(base: &Path) -> PathBuf {
    base.join("marketplace")
}

pub fn market_file(base: &Path) -> PathBuf {
    market_root(base).join(MARKET_FILE)
}

pub fn slice_dir(base: &Path, slice: &str) -> PathBuf {
    market_root(base).join(slice)
}

#[cfg(unix)]
fn mode_of(path: &Path) -> Mode {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| Mode::Exact(m.permissions().mode() & 0o777))
        .unwrap_or(Mode::Private)
}

#[cfg(not(unix))]
fn mode_of(_: &Path) -> Mode {
    Mode::Private
}

/// Copies a file or a folder's files; symlinks are not followed.
pub fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    if fsx::is_file(from) {
        let bytes = std::fs::read(from).map_err(|e| format!("{}: {e}", from.display()))?;
        return atomic_write_bytes(to, &bytes, mode_of(from))
            .map_err(|e| format!("{}: {e}", to.display()));
    }
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in fsx::list_dir(from) {
        if entry.symlink {
            continue;
        }
        match entry.kind {
            FileKind::Dir | FileKind::File => copy_tree(&entry.path, &to.join(&entry.name))?,
            FileKind::Other => {}
        }
    }
    Ok(())
}

fn included(sel: &SourceSel) -> impl Iterator<Item = &Item> {
    sel.items
        .iter()
        .filter(|i| i.kind.in_slice() && i.state.wanted() && i.in_slice)
}

fn hook_handlers(sel: &SourceSel) -> Vec<&Value> {
    included(sel)
        .filter(|i| i.kind == Kind::Hook)
        .filter_map(|i| i.spec.as_ref()?.get("handler"))
        .collect()
}

/// Builds the slice folder for `sel` in `staging`: refreshed items come from the latest version
/// of the source, every other item from the folder the slice has now.
pub fn build(
    staging: &Path,
    sel: &SourceSel,
    latest: &Path,
    pinned: &Path,
    refresh: &BTreeSet<String>,
) -> Result<(), String> {
    std::fs::create_dir_all(staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    for item in included(sel).filter(|i| i.kind != Kind::Hook) {
        let rel = item
            .path
            .as_deref()
            .ok_or_else(|| format!("{} has no path", item.key()))?;
        let from = if refresh.contains(&item.key()) {
            latest
        } else {
            pinned
        };
        copy_tree(&from.join(rel), &staging.join(rel))?;
    }
    for rel in support_paths(&hook_handlers(sel)) {
        let from = [latest, pinned]
            .into_iter()
            .map(|root| root.join(&rel))
            .find(|p| p.exists())
            .ok_or_else(|| format!("{rel} is missing, and a ticked hook runs it"))?;
        copy_tree(&from, &staging.join(&rel))?;
    }
    Ok(())
}

fn listed(sel: &SourceSel, kind: Kind) -> Vec<Value> {
    included(sel)
        .filter(|i| i.kind == kind)
        .filter_map(|i| i.path.as_ref())
        .map(|p| json!(format!("./{p}")))
        .collect()
}

fn hooks_value(sel: &SourceSel) -> Option<Value> {
    let mut events: Map<String, Value> = Map::new();
    for item in included(sel).filter(|i| i.kind == Kind::Hook) {
        let Some(spec) = &item.spec else { continue };
        let event = spec["event"].as_str().unwrap_or_default().to_string();
        let groups = events
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()?;
        let matcher = spec.get("matcher");
        let slot = groups.iter_mut().find(|g| g.get("matcher") == matcher);
        match slot {
            Some(group) => group["hooks"].as_array_mut()?.push(spec["handler"].clone()),
            None => {
                let mut group = json!({"hooks": [spec["handler"].clone()]});
                if let Some(matcher) = matcher {
                    group["matcher"] = matcher.clone();
                }
                groups.push(group);
            }
        }
    }
    (!events.is_empty()).then_some(Value::Object(events))
}

pub fn entry(sel: &SourceSel, label: &str) -> Value {
    let mut entry = json!({
        "name": sel.slice,
        "source": format!("./{}", sel.slice),
        "strict": false,
        "version": format!("0.1.{}", sel.rev),
        "description": format!("The items of {label} chosen in Toolport (generated; do not edit)"),
    });
    for (key, kind) in [
        ("skills", Kind::Skill),
        ("agents", Kind::Agent),
        ("commands", Kind::Command),
    ] {
        let paths = listed(sel, kind);
        if !paths.is_empty() {
            entry[key] = Value::Array(paths);
        }
    }
    if let Some(hooks) = hooks_value(sel) {
        entry["hooks"] = hooks;
    }
    entry
}

pub fn marketplace_doc(entries: Vec<Value>) -> Value {
    json!({
        "name": MARKETPLACE,
        "owner": {"name": "Toolport"},
        "description": "Items chosen in Toolport, generated per source",
        "plugins": entries,
    })
}

pub fn write_marketplace(base: &Path, entries: Vec<Value>) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(&marketplace_doc(entries)).map_err(|e| e.to_string())?;
    atomic_write(&market_file(base), &format!("{text}\n"), Mode::Private)
        .map_err(|e| format!("{}: {e}", market_file(base).display()))
}

pub fn entries_of(base: &Path) -> Vec<Value> {
    fsx::read_text(&market_file(base), 4 * 1024 * 1024)
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|d| d.get("plugins").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

pub fn has_content(sel: &SourceSel) -> bool {
    included(sel).next().is_some()
}
