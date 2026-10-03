//! Lint checks for skills, agents and styles; message texts and ordering match mcpm.

use super::parser::{Activation, Skill};
use super::transpilers::windsurf::WINDSURF_WORKSPACE_CHAR_LIMIT;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintMessage {
    pub level: &'static str,
    pub name: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LintResult {
    pub messages: Vec<LintMessage>,
}

impl LintResult {
    pub fn add(&mut self, level: &'static str, name: &str, message: impl Into<String>) {
        self.messages.push(LintMessage {
            level,
            name: name.to_string(),
            message: message.into(),
        });
    }

    pub fn has_errors(&self) -> bool {
        self.messages.iter().any(|m| m.level == "error")
    }

    pub fn errors(&self) -> impl Iterator<Item = &LintMessage> {
        self.messages.iter().filter(|m| m.level == "error")
    }

    pub fn warnings(&self) -> impl Iterator<Item = &LintMessage> {
        self.messages.iter().filter(|m| m.level == "warning")
    }
}

/// `body.strip().split("\n")`, or no lines for a blank body.
pub(crate) fn stripped_lines(body: &str) -> usize {
    let stripped = body.trim();
    if stripped.is_empty() {
        0
    } else {
        stripped.split('\n').count()
    }
}

pub(crate) fn parent_dir_name(skill: &Skill) -> String {
    skill
        .source_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn lint_skill(skill: &Skill) -> LintResult {
    let mut result = LintResult::default();
    let fm = &skill.frontmatter;
    let name = fm.name.as_str();
    let dir = parent_dir_name(skill);
    let is_skill_md = skill
        .source_path
        .file_name()
        .is_some_and(|n| n == "SKILL.md");
    if dir != name && is_skill_md {
        result.add(
            "error",
            name,
            format!("Skill name '{name}' does not match directory name '{dir}'"),
        );
    }

    let dlen = fm.description.chars().count();
    if dlen < 20 {
        result.add(
            "warning",
            name,
            "Description is very short (<20 chars). Add detail about when to use this skill.",
        );
    }
    let lowered = fm.description.to_lowercase();
    if ["todo", "todo:", "fixme", "placeholder"].contains(&lowered.as_str()) {
        result.add(
            "warning",
            name,
            "Description is a placeholder. Fill it in before syncing.",
        );
    }
    let when_keywords = [
        "when",
        "use for",
        "use when",
        "use this",
        "for handling",
        "for working",
    ];
    if !when_keywords.iter().any(|kw| lowered.contains(kw)) && dlen < 100 {
        result.add(
            "info",
            name,
            "Description lacks 'when to use' guidance. Consider adding context for agent discovery.",
        );
    }

    let lines = stripped_lines(&skill.body);
    if lines > 500 {
        result.add(
            "warning",
            name,
            format!(
                "Body is {lines} lines (Agent Skills spec recommends <500). Consider moving detail to references/."
            ),
        );
    }
    if skill.body.trim().is_empty() {
        result.add(
            "warning",
            name,
            "Body is empty. Add instructions for the agent.",
        );
    }

    let body_len = skill.body.chars().count();
    if body_len > WINDSURF_WORKSPACE_CHAR_LIMIT {
        result.add(
            "warning",
            name,
            format!(
                "Body ({body_len} chars) exceeds Windsurf workspace limit ({WINDSURF_WORKSPACE_CHAR_LIMIT}). Will be truncated during sync."
            ),
        );
    }

    if let Some(globs) = fm.globs.as_deref().filter(|g| !g.is_empty()) {
        for pattern in globs.split(',') {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                result.add("warning", name, "Empty glob pattern found in globs field.");
            } else if pattern == "**/*" {
                result.add(
                    "info",
                    name,
                    "Glob '**/*' matches all files. Consider being more specific.",
                );
            }
        }
        if fm.activation == Activation::Always {
            result.add(
                "info",
                name,
                "Skill has activation 'always' with globs set. Globs are ignored when activation is 'always'.",
            );
        }
    }
    result
}

pub fn lint_skills(skills: &[Skill]) -> LintResult {
    let mut result = LintResult::default();
    for skill in skills {
        result.messages.extend(lint_skill(skill).messages);
    }

    let mut seen: Vec<&str> = Vec::new();
    for skill in skills {
        if seen.contains(&skill.name()) {
            result.add("error", skill.name(), "Duplicate skill name found.");
        }
        seen.push(skill.name());
    }

    let with_globs: Vec<(&str, &str, Activation)> = skills
        .iter()
        .filter_map(|s| {
            let g = s.frontmatter.globs.as_deref().filter(|g| !g.is_empty())?;
            Some((s.name(), g, s.frontmatter.activation))
        })
        .collect();
    for (i, (name_a, globs_a, act_a)) in with_globs.iter().enumerate() {
        for (name_b, globs_b, act_b) in &with_globs[i + 1..] {
            if act_a == act_b && globs_a == globs_b {
                result.add(
                    "warning",
                    name_a,
                    format!(
                        "Has identical globs and activation as '{name_b}'. May cause conflicts."
                    ),
                );
            }
        }
    }
    result
}
