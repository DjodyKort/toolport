//! Lint checks for skills, agents and styles; message texts and ordering match mcpm.

use super::assets::{inventory, Inventory, SKILL_FILE};
use super::frontmatter::frontmatter_accepted;
use super::kind::ParseFailure;
use super::parser::{Activation, Skill, SPEC_DESCRIPTION_LIMIT};
use super::transpiler::TranspilerRegistry;
use regex::Regex;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

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

/// A skill whose file exists but does not parse is dropped from discovery, so nothing else
/// mentions it.
pub fn lint_parse_failures(failures: &[ParseFailure]) -> LintResult {
    let mut result = LintResult::default();
    for failure in failures {
        result.error(
            &failure.name,
            format!(
                "{} cannot be read and is not synced: {}",
                failure.path, failure.reason
            ),
        );
    }
    result
}

/// A description between the spec limit and what Claude Code keeps is cut by every other client.
/// It is a warning while only Claude Code receives skills and an error once another client does.
pub fn lint_description_lengths(skills: &[Skill], other_clients_on: bool) -> LintResult {
    let mut result = LintResult::default();
    for skill in skills {
        let len = skill.frontmatter.description.chars().count();
        if len <= SPEC_DESCRIPTION_LIMIT {
            continue;
        }
        let message = format!(
            "Description is {len} characters. Claude Code keeps it, but the Agent Skills spec allows {SPEC_DESCRIPTION_LIMIT} and other clients cut it."
        );
        if other_clients_on {
            result.error(skill.name(), message);
        } else {
            result.warning(skill.name(), message);
        }
    }
    result
}

fn link_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"\[[^\]\n]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"\n]*")?\s*\)"#)
            .expect("link pattern")
    })
}

fn without_code(body: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            let mut kept = String::new();
            for (i, part) in line.split('`').enumerate() {
                if i % 2 == 0 {
                    kept.push_str(part);
                }
            }
            out.push_str(&kept);
            out.push('\n');
        }
    }
    out
}

/// The relative path a markdown link points at, or `None` for web links, anchors and absolute paths.
fn relative_target(raw: &str) -> Option<String> {
    let target = raw.split(['#', '?']).next().unwrap_or("");
    if target.is_empty()
        || target.starts_with('/')
        || target.starts_with('~')
        || target.contains("://")
    {
        return None;
    }
    if target.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic())
    }) {
        return None;
    }
    Some(target.to_string())
}

fn normalize(rel: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

fn installed(found: &Inventory, rel: &Path) -> bool {
    let text = super::assets::rel_string(rel);
    text == SKILL_FILE
        || found.files.iter().any(|f| f.rel == text)
        || found
            .files
            .iter()
            .any(|f| f.rel.starts_with(&format!("{text}/")))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// What a sync does with the files around each SKILL.md: info for every file it leaves out, a
/// warning for every relative link in SKILL.md whose target would not be installed, an error for
/// a hook script that is not executable (the parser already drops a skill whose script is missing).
pub fn lint_files(skills: &[Skill]) -> LintResult {
    let mut result = LintResult::default();
    for skill in skills {
        if skill.skill_type != super::parser::SkillType::Skill
            || skill
                .source_path
                .file_name()
                .is_none_or(|n| n != SKILL_FILE)
        {
            continue;
        }
        let name = skill.name();
        let dir = skill.source_dir();
        let found = inventory(dir);
        for skipped in &found.skipped {
            result.info(
                name,
                format!(
                    "{} is not installed by a sync: {}",
                    skipped.rel,
                    skipped.reason.describe()
                ),
            );
        }
        let mut seen = Vec::new();
        for caps in link_pattern().captures_iter(&without_code(&skill.body)) {
            let Some(target) = relative_target(&caps[1]) else {
                continue;
            };
            if seen.contains(&target) {
                continue;
            }
            seen.push(target.clone());
            let Some(rel) = normalize(&target) else {
                result.warning(name, format!("Link '{target}' in SKILL.md leaves the skill folder and would not be installed."));
                continue;
            };
            if !dir.join(&rel).exists() {
                result.warning(
                    name,
                    format!("Link '{target}' in SKILL.md points to a file that does not exist."),
                );
            } else if !installed(&found, &rel) {
                let why = found
                    .skipped
                    .iter()
                    .find(|s| {
                        super::assets::rel_string(&rel).starts_with(s.rel.trim_end_matches('/'))
                    })
                    .map_or_else(|| "not installed".to_string(), |s| s.reason.describe());
                result.warning(name, format!("Link '{target}' in SKILL.md points to a file that would not be installed ({why})."));
            }
        }
        for (event, hook) in skill.frontmatter.hooks.iter().flatten() {
            if Path::new(&hook.command).is_absolute() {
                continue;
            }
            let script = dir.join(&hook.command);
            if script.is_file() && !is_executable(&script) {
                result.error(
                    name,
                    format!(
                        "Hook script '{}' of the {event} hook is not executable.",
                        hook.command
                    ),
                );
            }
        }
    }
    result
}

/// Transpiles every skill for every client with a file of its own and fails the ones whose emitted
/// frontmatter the client would reject; a skill that Claude Code cannot read is hidden from the model
/// without a message (MIG-GFX-4).
pub fn lint_outputs(skills: &[Skill], registry: &TranspilerRegistry) -> LintResult {
    let mut result = LintResult::default();
    for skill in skills {
        for transpiler in registry.all() {
            if transpiler.capabilities().append_mode {
                continue;
            }
            let Ok(out) = transpiler.transpile(skill, Path::new(".")) else {
                continue;
            };
            if let Err(reason) = frontmatter_accepted(&out.content) {
                result.error(
                    skill.name(),
                    format!(
                        "Emitted {} frontmatter would be rejected: {reason}",
                        transpiler.client_key()
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
