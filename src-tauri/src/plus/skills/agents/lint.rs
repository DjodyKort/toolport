use super::Agent;
use crate::plus::skills::lint::LintResult;

const VALID_MODELS: &[&str] = &["sonnet", "opus", "haiku", "inherit"];

pub fn lint_agent(agent: &Agent) -> LintResult {
    let mut result = LintResult::default();
    let fm = &agent.frontmatter;
    let name = fm.name.as_str();
    let dir = parent_dir_name_of(agent);
    let is_agent_md = agent
        .source_path
        .file_name()
        .is_some_and(|n| n == "AGENT.md");
    if dir != name && is_agent_md {
        result.add(
            "error",
            name,
            format!("Agent name '{name}' does not match directory name '{dir}'"),
        );
    }
    if fm.description.chars().count() < 20 {
        result.add("warning", name, "Description is very short (<20 chars).");
    }
    if agent.body.trim().is_empty() {
        result.add("warning", name, "Body (system prompt) is empty.");
    }
    if let Some(model) = fm.model.as_deref().filter(|m| !m.is_empty()) {
        if !VALID_MODELS.contains(&model) && !model.starts_with("claude-") {
            result.add(
                "info",
                name,
                format!("Model '{model}' is not a standard shorthand (sonnet/opus/haiku/inherit)."),
            );
        }
    }
    if !fm.tools.is_empty() && !fm.disallowed_tools.is_empty() {
        let mut overlap: Vec<&str> = Vec::new();
        for t in &fm.tools {
            if fm.disallowed_tools.contains(t) && !overlap.contains(&t.as_str()) {
                overlap.push(t);
            }
        }
        if !overlap.is_empty() {
            result.add(
                "error",
                name,
                format!(
                    "Tools in both allowed and disallowed: {}",
                    overlap.join(", ")
                ),
            );
        }
    }
    if fm.permission_mode == Some(super::PermissionMode::FullAuto) && fm.readonly {
        result.add(
            "warning",
            name,
            "readonly=true conflicts with permission-mode=full-auto.",
        );
    }
    if let Some(n) = fm.max_turns {
        if !(1..=200).contains(&n) {
            result.add(
                "warning",
                name,
                format!("max-turns={n} is outside typical range (1-200)."),
            );
        }
    }
    result
}

fn parent_dir_name_of(agent: &Agent) -> String {
    agent
        .source_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn lint_agents(agents: &[Agent]) -> LintResult {
    let mut result = LintResult::default();
    for agent in agents {
        result.messages.extend(lint_agent(agent).messages);
    }
    let mut seen: Vec<&str> = Vec::new();
    for agent in agents {
        if seen.contains(&agent.name()) {
            result.add("error", agent.name(), "Duplicate agent name found.");
        }
        seen.push(agent.name());
    }
    result
}
