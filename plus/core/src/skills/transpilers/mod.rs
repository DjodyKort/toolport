//! Per-client skill transpilers: claude-code writes the files, agents-md lists the skills in
//! AGENTS.md.

pub mod agents_md;
pub mod claude_code;

use super::scalar;
use super::transpiler::TranspilerRegistry;

pub(crate) enum Field<'a> {
    Raw(&'a str),
    Bool(bool),
    /// Free text mcpm wraps in double quotes (descriptions).
    Quoted(&'a str),
    /// Free text mcpm writes unquoted, which a strict YAML parser must read back unchanged.
    Text(&'a str),
}

/// Mirrors `BaseSkillTranspiler._render_frontmatter`: values are emitted as mcpm emits them, unquoted
/// or double-quoted, except where those bytes are not valid YAML for the value (several lines, a
/// quote or a backslash): `scalar` writes a form a strict parser reads back unchanged (D-070).
pub(crate) fn render_frontmatter(fields: &[(&str, Field<'_>)]) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let mut lines = vec!["---".to_string()];
    for (key, value) in fields {
        match value {
            Field::Bool(b) => lines.push(format!("{key}: {b}")),
            Field::Raw(v) => lines.push(format!("{key}: {v}")),
            Field::Quoted(v) => lines.push(format!("{key}: {}", scalar::quoted(v))),
            Field::Text(v) => lines.push(format!("{key}: {}", scalar::plain(v))),
        }
    }
    lines.push("---".into());
    lines.join("\n")
}

/// Registers the transpilers in the alphabetical order mcpm imported them, which fixes iteration
/// order and therefore lock output order.
pub fn registry() -> TranspilerRegistry {
    let mut registry = TranspilerRegistry::new();
    registry.register(Box::new(agents_md::AgentsMd));
    registry.register(Box::new(claude_code::ClaudeCode));
    registry
}

#[cfg(test)]
mod tests;
