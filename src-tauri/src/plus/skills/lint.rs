//! Lint checks for skills, agents and styles; message texts and ordering match mcpm.

use super::parser::{Activation, Skill};
use super::transpilers::windsurf::WINDSURF_WORKSPACE_CHAR_LIMIT;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LintLevel {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintMessage {
    pub level: LintLevel,
    pub name: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LintResult {
    pub messages: Vec<LintMessage>,
}

impl LintResult {
    fn add(&mut self, level: LintLevel, name: &str, message: impl Into<String>) {
        self.messages.push(LintMessage {
            level,
            name: name.to_string(),
            message: message.into(),
        });
    }

    pub fn error(&mut self, name: &str, message: impl Into<String>) {
        self.add(LintLevel::Error, name, message);
    }

    pub fn warning(&mut self, name: &str, message: impl Into<String>) {
        self.add(LintLevel::Warning, name, message);
    }

    pub fn info(&mut self, name: &str, message: impl Into<String>) {
        self.add(LintLevel::Info, name, message);
    }

    pub fn has_errors(&self) -> bool {
        self.errors().next().is_some()
    }

    pub fn errors(&self) -> impl Iterator<Item = &LintMessage> {
        self.at(LintLevel::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &LintMessage> {
        self.at(LintLevel::Warning)
    }

    fn at(&self, level: LintLevel) -> impl Iterator<Item = &LintMessage> {
        self.messages.iter().filter(move |m| m.level == level)
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
        result.error(
            name,
            format!("Skill name '{name}' does not match directory name '{dir}'"),
        );
    }

    let dlen = fm.description.chars().count();
    if dlen < 20 {
        result.warning(
            name,
            "Description is very short (<20 chars). Add detail about when to use this skill.",
        );
    }
    let lowered = fm.description.to_lowercase();
    if ["todo", "todo:", "fixme", "placeholder"].contains(&lowered.as_str()) {
        result.warning(
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
        result.info(
            name,
            "Description lacks 'when to use' guidance. Consider adding context for agent discovery.",
        );
    }

    let lines = stripped_lines(&skill.body);
    if lines > 500 {
        result.warning(
            name,
            format!(
                "Body is {lines} lines (Agent Skills spec recommends <500). Consider moving detail to references/."
            ),
        );
    }
    if skill.body.trim().is_empty() {
        result.warning(name, "Body is empty. Add instructions for the agent.");
    }

    let body_len = skill.body.chars().count();
    if body_len > WINDSURF_WORKSPACE_CHAR_LIMIT {
        result.warning(
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
                result.warning(name, "Empty glob pattern found in globs field.");
            } else if pattern == "**/*" {
                result.info(
                    name,
                    "Glob '**/*' matches all files. Consider being more specific.",
                );
            }
        }
        if fm.activation == Activation::Always {
            result.info(
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
            result.error(skill.name(), "Duplicate skill name found.");
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
                result.warning(
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

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn levels_keep_their_wire_strings() {
        for (level, wire) in [
            (LintLevel::Error, "error"),
            (LintLevel::Warning, "warning"),
            (LintLevel::Info, "info"),
        ] {
            assert_eq!(serde_json::to_value(level).unwrap(), wire);
        }
    }

    #[test]
    fn each_helper_records_its_level_and_the_filters_split_them() {
        let mut result = LintResult::default();
        result.warning("a", "w");
        result.info("b", "i");
        assert!(!result.has_errors());
        result.error("c", "e");
        let levels: Vec<LintLevel> = result.messages.iter().map(|m| m.level).collect();
        assert_eq!(
            levels,
            [LintLevel::Warning, LintLevel::Info, LintLevel::Error]
        );
        assert!(result.has_errors());
        let names = |it: &mut dyn Iterator<Item = &LintMessage>| -> Vec<String> {
            it.map(|m| m.name.clone()).collect()
        };
        assert_eq!(names(&mut result.errors()), ["c"]);
        assert_eq!(names(&mut result.warnings()), ["a"]);
    }
}
