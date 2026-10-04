//! `mcpm agents add`: the AGENT.md scaffold.

use crate::plus::skills::parser::valid_name;
use crate::plus::skills::pyfs::write_text;
use crate::plus::skills::repo::resolve_path;
use std::path::{Path, PathBuf};

pub fn agent_template(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: \"TODO: Describe what this agent does and when to use it.\"\nmodel: sonnet\ntools: [Read, Grep, Glob]\n---\n\nTODO: Write the system prompt for this agent.\n\nDescribe its role, expertise, and how it should approach tasks.\n"
    )
}

#[derive(Debug, PartialEq, Eq)]
pub struct AddReport {
    pub repo: PathBuf,
    pub agent_file: PathBuf,
}

/// Writes `agents/<name>/AGENT.md` from the template under `repo`, which need not be a
/// repository yet. Nothing is written on `dry_run`.
pub fn add_agent(repo: &Path, name: &str, dry_run: bool) -> Result<AddReport, String> {
    valid_name(name).map_err(|e| format!("Invalid agent name '{name}': {e}"))?;
    let repo = resolve_path(repo);
    let dir = repo.join("agents").join(name);
    if dir.exists() {
        return Err(format!("Agent '{name}' already exists."));
    }
    let agent_file = dir.join("AGENT.md");
    if !dry_run {
        write_text(&agent_file, &agent_template(name))?;
    }
    Ok(AddReport { repo, agent_file })
}
