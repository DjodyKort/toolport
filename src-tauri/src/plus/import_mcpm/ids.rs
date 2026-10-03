use crate::registry::slugify;
use crate::router::sanitize_segment;
use std::collections::{BTreeMap, HashSet};

pub const TOOL_NAME_PREFIX: &str = "mcp__toolport__";
pub const MAX_TOOL_NAME_LEN: usize = 64;
pub const MAX_ID_LEN: usize = 20;

pub fn exposed_prefix(id: &str) -> String {
    format!("{TOOL_NAME_PREFIX}{}__", sanitize_segment(id))
}

pub fn tool_name_fits(id: &str, tool: &str) -> bool {
    exposed_prefix(id).len() + sanitize_segment(tool).len() <= MAX_TOOL_NAME_LEN
}

pub fn short_id(name: &str, table: &BTreeMap<String, String>, taken: &HashSet<String>) -> String {
    let base = table.get(name).cloned().unwrap_or_else(|| generic(name));
    let mut candidate = base.clone();
    let mut n = 2;
    while taken
        .iter()
        .any(|t| sanitize_segment(t) == sanitize_segment(&candidate))
    {
        let suffix = format!("-{n}");
        let keep = MAX_ID_LEN.saturating_sub(suffix.len());
        candidate = format!("{}{suffix}", base.chars().take(keep).collect::<String>());
        n += 1;
    }
    candidate
}

fn generic(name: &str) -> String {
    let slug = slugify(name);
    let slug = slug.strip_suffix("-mcp").unwrap_or(&slug);
    let slug = if slug.is_empty() { "server" } else { slug };
    slug.chars()
        .take(MAX_ID_LEN)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}
