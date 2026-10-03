//! Per-client skill transpilers. Wave 1: agents-md, claude-code, cline, cursor, windsurf.
//! Wave 2: aider, amazon-q, codex-cli, continue, gemini-cli, goose-cli, jetbrains, roo-code, trae,
//! zed, plus the deliberately unregistered vscode copilot transpiler.

pub mod agents_md;
pub mod aider;
pub mod amazon_q;
pub mod claude_code;
pub mod cline;
pub mod codex_cli;
pub mod continue_dev;
pub mod cursor;
pub mod gemini_cli;
pub mod goose;
pub mod jetbrains;
pub mod roo_code;
pub mod trae;
pub mod vscode_copilot;
pub mod windsurf;
pub mod zed;

use super::transpiler::TranspilerRegistry;
use std::path::PathBuf;


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

/// Every transpiler mcpm registers, in the alphabetical module order of its `__init__.py`.
/// `vscode` is absent on purpose (waiver candidate, see `register_vscode_copilot`).
pub fn register_all_with_home(registry: &mut TranspilerRegistry, home: Option<PathBuf>) {
    registry.register(Box::new(agents_md::AgentsMd));
    registry.register(Box::new(aider::Aider));
    registry.register(Box::new(amazon_q::AmazonQ));
    registry.register(Box::new(claude_code::ClaudeCode));
    registry.register(Box::new(cline::Cline));
    registry.register(Box::new(codex_cli::CodexCli::new(home)));
    registry.register(Box::new(continue_dev::ContinueDev));
    registry.register(Box::new(cursor::Cursor));
    registry.register(Box::new(gemini_cli::GeminiCli));
    registry.register(Box::new(goose::Goose));
    registry.register(Box::new(jetbrains::JetBrains));
    registry.register(Box::new(roo_code::RooCode));
    registry.register(Box::new(trae::Trae));
    registry.register(Box::new(windsurf::Windsurf));
    registry.register(Box::new(zed::Zed));
}

pub fn registry_with_home(home: Option<PathBuf>) -> TranspilerRegistry {
    let mut registry = TranspilerRegistry::new();
    register_all_with_home(&mut registry, home);
    registry
}

pub fn register_all(registry: &mut TranspilerRegistry) {
    register_all_with_home(registry, dirs::home_dir());
}

/// Mirrors a Python `import mcpm.skills.transpilers.vscode_copilot`: the transpiler lands last.
pub fn register_vscode_copilot(registry: &mut TranspilerRegistry) {
    registry.register(Box::new(vscode_copilot::VsCodeCopilot));
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_wave2;
