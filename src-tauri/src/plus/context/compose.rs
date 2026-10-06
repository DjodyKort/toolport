//! `context compose`: the instruction text Claude Code gets when it starts in a folder, part by
//! part in the order `context loads` lists it: memory files, the files they import, rules. Read
//! only. The tokens are the estimate of `loads` (bytes / 4); the text is included so a screen can
//! show the composed result. CLAUDE.md files hold no secrets by rule, so nothing is redacted.

use super::config::ContextConfig;
use super::loads::{what_loads_with, LoadItem, LoadsOptions};
use super::roots::Roots;
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

const KINDS: [&str; 3] = ["memory", "import", "rule"];
const LEVEL_FILES: [&str; 3] = ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"];

fn part(item: &LoadItem) -> Option<Value> {
    let path = item.path.as_deref()?;
    let text = fsx::read_text(Path::new(path), fsx::TEXT_CAP).unwrap_or_default();
    let layers: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("<!-- toolport:layer:begin "))
        .filter_map(|l| l.strip_suffix(" -->"))
        .collect();
    Some(json!({
        "origin": item.origin,
        "path": path,
        "via": item.via,
        "lazy": item.lazy,
        "tokens": { "value": item.tokens, "basis": item.basis },
        "text": text,
        "kind": item.kind,
        "name": item.name,
        "source": item.source,
        "writable": item.writable,
        "layers": layers,
    }))
}

/// Every folder Claude Code walks down to `cwd`, with the memory files that load from it, so a
/// level without any shows as one. Below the home folder the walk starts at home; a level above
/// it is listed only when something loads from it.
fn levels(roots: &Roots, cwd: &Path, parts: &[Value]) -> Vec<Value> {
    let loaded: BTreeSet<&str> = parts.iter().filter_map(|p| p["path"].as_str()).collect();
    let in_home = cwd.starts_with(&roots.home);
    let mut chain: Vec<&Path> = cwd.ancestors().collect();
    chain.reverse();
    chain
        .into_iter()
        .filter_map(|dir| {
            let files: Vec<&str> = LEVEL_FILES
                .into_iter()
                .filter(|rel| {
                    let path = dir.join(rel);
                    !path.starts_with(&roots.claude_home)
                        && loaded.contains(path.display().to_string().as_str())
                })
                .collect();
            let shown = !in_home || dir.starts_with(&roots.home) || !files.is_empty();
            shown.then(|| json!({ "dir": dir.display().to_string(), "files": files }))
        })
        .collect()
}

pub fn compose(roots: &Roots, config: &ContextConfig, cwd: &Path) -> Result<Value, String> {
    let loads = what_loads_with(roots, config, None, cwd, &LoadsOptions::default())?;
    let mut parts = Vec::new();
    let mut skipped = Vec::new();
    let mut total = 0;
    for item in loads.items.iter().filter(|i| KINDS.contains(&i.kind)) {
        if !item.loaded && !item.lazy {
            skipped.push(json!({ "path": item.path, "reason": item.reason }));
            continue;
        }
        if let Some(part) = part(item) {
            if item.loaded {
                total += item.tokens;
            }
            parts.push(part);
        }
    }
    let levels = levels(roots, Path::new(&loads.cwd), &parts);
    Ok(json!({
        "cwd": loads.cwd,
        "levels": levels,
        "parts": parts,
        "total": { "value": total, "basis": "estimate" },
        "skipped": skipped,
        "notes": loads.notes,
    }))
}

pub fn compose_here(cwd: &Path) -> Result<Value, crate::plus::op::OpError> {
    let (roots, config) = crate::plus::sources::host_world()
        .ok_or_else(|| crate::plus::op::OpError::failed("no_home", "home directory could not be resolved"))?;
    if !cwd.is_dir() {
        return Err(crate::plus::op::OpError::usage(format!("{} is not a folder", cwd.display())));
    }
    compose(&roots, &config, cwd).map_err(|e| crate::plus::op::OpError::failed("compose_failed", e))
}
