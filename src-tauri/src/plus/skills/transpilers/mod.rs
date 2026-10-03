//! Per-client skill transpilers. Wave 1: agents-md, claude-code, cline, cursor, windsurf.

pub mod agents_md;
pub mod claude_code;
pub mod cline;
pub mod cursor;
pub mod windsurf;

use super::transpiler::TranspilerRegistry;


pub(crate) enum Field<'a> {
    Raw(&'a str),
    Bool(bool),
}

/// Mirrors `BaseSkillTranspiler._render_frontmatter`: values are emitted verbatim, unquoted.
pub(crate) fn render_frontmatter(fields: &[(&str, Field<'_>)]) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let mut lines = vec!["---".to_string()];
    for (key, value) in fields {
        match value {
            Field::Bool(b) => lines.push(format!("{key}: {b}")),
            Field::Raw(v) => lines.push(format!("{key}: {v}")),
        }
    }
    lines.push("---".into());
    lines.join("\n")
}


/// Registers wave-1 transpilers in mcpm's import order (alphabetical module order of
/// `transpilers/__init__.py`), which fixes iteration order and therefore lock output order.
pub fn register_wave1(registry: &mut TranspilerRegistry) {
    registry.register(Box::new(agents_md::AgentsMd));
    registry.register(Box::new(claude_code::ClaudeCode));
    registry.register(Box::new(cline::Cline));
    registry.register(Box::new(cursor::Cursor));
    registry.register(Box::new(windsurf::Windsurf));
}

#[cfg(test)]
mod tests;
