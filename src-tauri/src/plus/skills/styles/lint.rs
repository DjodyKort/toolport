use super::Style;
use crate::plus::skills::lint::{stripped_lines, LintResult};
use crate::plus::skills::transpilers::windsurf::WINDSURF_WORKSPACE_CHAR_LIMIT;

pub fn lint_style(style: &Style) -> LintResult {
    let mut result = LintResult::default();
    let fm = &style.frontmatter;
    let name = fm.name.as_str();
    let dir = style
        .source_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if dir != name
        && style
            .source_path
            .file_name()
            .is_some_and(|n| n == "STYLE.md")
    {
        result.add(
            "error",
            name,
            format!("Style name '{name}' does not match directory name '{dir}'"),
        );
    }
    if fm.description.chars().count() < 20 {
        result.add(
            "warning",
            name,
            "Description is very short (<20 chars). Add detail about the tone/style.",
        );
    }
    let lowered = fm.description.to_lowercase();
    if ["todo", "todo:", "fixme", "placeholder"].contains(&lowered.trim()) {
        result.add(
            "warning",
            name,
            "Description is a placeholder. Fill it in before syncing.",
        );
    }
    if style.body.trim().is_empty() {
        result.add("warning", name, "Body is empty. Add style instructions.");
    }
    if style.body.contains("TODO") && style.body.trim().starts_with("TODO") {
        result.add(
            "warning",
            name,
            "Body starts with TODO placeholder. Replace with actual style instructions.",
        );
    }
    let lines = stripped_lines(&style.body);
    if lines > 200 {
        result.add(
            "warning",
            name,
            format!("Body is {lines} lines. Output styles should be concise -- consider trimming."),
        );
    }
    let body_len = style.body.chars().count();
    if body_len > WINDSURF_WORKSPACE_CHAR_LIMIT {
        result.add(
            "warning",
            name,
            format!(
                "Body ({body_len} chars) exceeds Windsurf workspace limit ({WINDSURF_WORKSPACE_CHAR_LIMIT}). Will be truncated on Windsurf."
            ),
        );
    }
    result
}

pub fn lint_styles(styles: &[Style]) -> LintResult {
    let mut result = LintResult::default();
    for style in styles {
        result.messages.extend(lint_style(style).messages);
    }
    let mut seen: Vec<&str> = Vec::new();
    for style in styles {
        if seen.contains(&style.name()) {
            result.add("error", style.name(), "Duplicate style name found.");
        }
        seen.push(style.name());
    }
    result
}
