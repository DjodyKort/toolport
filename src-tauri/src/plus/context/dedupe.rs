//! Removes cf-dev-tools' legacy bare-name MCP entries from a Claude Code config file.
//!
//! cf registers `context7`/`playwright`/... under their bare names; the same servers live as
//! `mcpm_<name>` entries, so the bare ones are dead duplicates. Only keys under `mcpServers`
//! change; everything else round-trips in order.

use super::backup::snapshot;
use super::config::DedupePolicy;
use super::roots::Roots;
use crate::plus::skills::json::{parse, J};
use std::fs;
use std::path::Path;

fn load(path: &Path) -> Option<J> {
    parse(&fs::read_to_string(path).ok()?).ok()
}

fn server_names(data: &J) -> Vec<&str> {
    match data.get("mcpServers") {
        Some(J::Obj(items)) => items.iter().map(|(k, _)| k.as_str()).collect(),
        _ => Vec::new(),
    }
}

/// Legacy entry names that would be removed. Pure: no writes.
pub fn plan_dedupe(claude_json: &Path, policy: &DedupePolicy) -> Vec<String> {
    if !policy.enabled || !claude_json.exists() {
        return Vec::new();
    }
    let Some(data) = load(claude_json) else {
        return Vec::new();
    };
    let servers = server_names(&data);
    policy
        .legacy_names
        .iter()
        .filter(|name| {
            servers.contains(&name.as_str())
                && (!policy.require_mcpm_twin || servers.contains(&format!("mcpm_{name}").as_str()))
        })
        .cloned()
        .collect()
}

pub fn apply_dedupe(
    roots: &Roots,
    claude_json: &Path,
    policy: &DedupePolicy,
    backup: bool,
    dry_run: bool,
) -> Result<Vec<String>, String> {
    let removed = plan_dedupe(claude_json, policy);
    if removed.is_empty() || dry_run {
        return Ok(removed);
    }
    if backup {
        snapshot(roots, claude_json)?;
    }
    let mut data = load(claude_json).ok_or("claude config changed while deduping")?;
    if let J::Obj(root) = &mut data {
        if let Some((_, J::Obj(servers))) = root.iter_mut().find(|(k, _)| k == "mcpServers") {
            servers.retain(|(k, _)| !removed.contains(k));
        }
    }
    fs::write(claude_json, format!("{}\n", data.dumps()))
        .map_err(|e| format!("{}: {e}", claude_json.display()))?;
    Ok(removed)
}
