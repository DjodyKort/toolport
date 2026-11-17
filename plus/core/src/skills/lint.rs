//! Lint checks for skills, agents and styles; message texts and ordering match mcpm.

use super::assets::{inventory, Inventory, SKILL_FILE};
use super::frontmatter::{frontmatter_accepted, Reason};
use super::kind::ParseFailure;
use super::parser::{
    field, find_fence_line, is_fence_line, parse_frontmatter, Activation, Skill, DESCRIPTION_LIMIT,
    SPEC_DESCRIPTION_LIMIT,
};
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

/// What the finding does to the skill in Claude Code; the Checks list groups rows by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Consequence {
    /// Claude Code does not list the skill for the model.
    Hidden,
    /// The description is longer than Claude Code keeps.
    Cut,
    /// A name that answers twice or does not match its folder.
    Shadowed,
    HookWontRun,
    /// A description above the Agent Skills spec limit that Claude Code still reads.
    OverSpec,
    DeadLink,
    NotCopied,
    ContextCost,
    WeakTrigger,
    Other,
}

/// A description length against the two limits one meter draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meter {
    pub length: usize,
    pub spec_limit: usize,
    pub limit: usize,
}

impl Meter {
    pub fn of(length: usize) -> Self {
        Self {
            length,
            spec_limit: SPEC_DESCRIPTION_LIMIT,
            limit: DESCRIPTION_LIMIT,
        }
    }
}

/// What the screen needs of a skills finding beyond its text. Rows of agents and styles carry none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub code: &'static str,
    pub consequence: Consequence,
    /// Relative to the skill folder.
    pub file: Option<String>,
    /// 1-based, in `file`.
    pub line: Option<usize>,
    /// The link target as written in SKILL.md.
    pub target: Option<String>,
    pub reason: Option<String>,
    /// The one-click fix the row offers: `chmod` or `move-link`.
    pub fixable: Option<&'static str>,
    pub meter: Option<Meter>,
}

impl Detail {
    pub fn new(code: &'static str, consequence: Consequence) -> Self {
        Self {
            code,
            consequence,
            file: None,
            line: None,
            target: None,
            reason: None,
            fixable: None,
            meter: None,
        }
    }

    fn file(self, file: impl Into<String>) -> Self {
        Self {
            file: Some(file.into()),
            ..self
        }
    }

    fn line(self, line: Option<usize>) -> Self {
        Self { line, ..self }
    }

    fn meter(self, length: usize) -> Self {
        Self {
            meter: Some(Meter::of(length)),
            ..self
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintMessage {
    pub level: LintLevel,
    pub name: String,
    pub message: String,
    pub detail: Option<Detail>,
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
            detail: None,
        });
    }

    fn found(&mut self, level: LintLevel, name: &str, detail: Detail, message: impl Into<String>) {
        self.messages.push(LintMessage {
            level,
            name: name.to_string(),
            message: message.into(),
            detail: Some(detail),
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
        result.found(
            LintLevel::Error,
            name,
            Detail::new("name-differs-from-folder", Consequence::Shadowed).file(SKILL_FILE),
            format!("Skill name '{name}' does not match directory name '{dir}'"),
        );
    }

    let dlen = fm.description.chars().count();
    if dlen < 20 {
        result.found(
            LintLevel::Warning,
            name,
            Detail::new("description-short", Consequence::WeakTrigger).file(SKILL_FILE),
            "Description is very short (<20 chars). Add detail about when to use this skill.",
        );
    }
    let lowered = fm.description.to_lowercase();
    if ["todo", "todo:", "fixme", "placeholder"].contains(&lowered.as_str()) {
        result.found(
            LintLevel::Warning,
            name,
            Detail::new("description-placeholder", Consequence::WeakTrigger).file(SKILL_FILE),
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
        result.found(
            LintLevel::Info,
            name,
            Detail::new("description-no-when", Consequence::WeakTrigger).file(SKILL_FILE),
            "Description lacks 'when to use' guidance. Consider adding context for agent discovery.",
        );
    }

    let lines = stripped_lines(&skill.body);
    if lines > 500 {
        result.found(
            LintLevel::Warning,
            name,
            Detail::new("body-long", Consequence::ContextCost).file(SKILL_FILE),
            format!(
                "Body is {lines} lines (Agent Skills spec recommends <500). Consider moving detail to references/."
            ),
        );
    }
    if skill.body.trim().is_empty() {
        result.found(
            LintLevel::Warning,
            name,
            Detail::new("body-empty", Consequence::ContextCost).file(SKILL_FILE),
            "Body is empty. Add instructions for the agent.",
        );
    }

    if let Some(globs) = fm.globs.as_deref().filter(|g| !g.is_empty()) {
        for pattern in globs.split(',') {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                result.found(
                    LintLevel::Warning,
                    name,
                    Detail::new("globs-empty-pattern", Consequence::Other).file(SKILL_FILE),
                    "Empty glob pattern found in globs field.",
                );
            } else if pattern == "**/*" {
                result.found(
                    LintLevel::Info,
                    name,
                    Detail::new("globs-match-all", Consequence::Other).file(SKILL_FILE),
                    "Glob '**/*' matches all files. Consider being more specific.",
                );
            }
        }
        if fm.activation == Activation::Always {
            result.found(
                LintLevel::Info,
                name,
                Detail::new("globs-ignored", Consequence::Other).file(SKILL_FILE),
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
            result.found(
                LintLevel::Error,
                skill.name(),
                Detail::new("duplicate-name", Consequence::Shadowed).file(SKILL_FILE),
                "Duplicate skill name found.",
            );
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
                result.found(
                    LintLevel::Warning,
                    name_a,
                    Detail::new("globs-identical", Consequence::Other).file(SKILL_FILE),
                    format!(
                        "Has identical globs and activation as '{name_b}'. May cause conflicts."
                    ),
                );
            }
        }
    }
    result
}

/// The line of the YAML error in the file itself; the frontmatter starts on line 2.
fn yaml_error_line(text: &str) -> Option<usize> {
    let yaml_start = text.find('\n')? + 1;
    let close = find_fence_line(text, yaml_start)?;
    let error = serde_yaml::from_str::<serde_yaml::Value>(&text[yaml_start..close]).err()?;
    error.location().map(|at| at.line() + 1)
}

fn description_length(text: &str) -> Option<usize> {
    let (fm, _) = parse_frontmatter(text).ok()?;
    match field(&fm, "description")? {
        serde_yaml::Value::String(s) => Some(s.chars().count()),
        _ => None,
    }
}

/// What a skill that did not parse is, read from the file again for the line and the length.
fn parse_failure_detail(repo: &Path, failure: &ParseFailure) -> Detail {
    let text = std::fs::read_to_string(repo.join(&failure.path)).unwrap_or_default();
    let reason = failure.reason.as_str();
    let detail = |code, consequence| Detail::new(code, consequence).file(SKILL_FILE);
    if reason.starts_with("description must be") {
        let found = detail("description-over-limit", Consequence::Cut);
        return match description_length(&text) {
            Some(length) => found.meter(length),
            None => found,
        };
    }
    if reason.starts_with("hook ") {
        return detail("hook-script-missing", Consequence::HookWontRun);
    }
    if reason.starts_with("invalid YAML") {
        return detail("frontmatter-invalid-yaml", Consequence::Hidden)
            .line(yaml_error_line(&text));
    }
    if reason.starts_with("No YAML frontmatter") {
        return detail("frontmatter-missing", Consequence::Hidden).line(Some(1));
    }
    if reason.starts_with("name") {
        return detail("frontmatter-name", Consequence::Hidden);
    }
    detail("frontmatter-invalid", Consequence::Hidden)
}

/// A skill whose file exists but does not parse is dropped from discovery, so nothing else
/// mentions it.
pub fn lint_parse_failures(repo: &Path, failures: &[ParseFailure]) -> LintResult {
    let mut result = LintResult::default();
    for failure in failures {
        result.found(
            LintLevel::Error,
            &failure.name,
            parse_failure_detail(repo, failure),
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
        let level = if other_clients_on {
            LintLevel::Error
        } else {
            LintLevel::Warning
        };
        result.found(
            level,
            skill.name(),
            Detail::new("description-over-spec", Consequence::OverSpec)
                .file(SKILL_FILE)
                .meter(len),
            message,
        );
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

/// The relative link targets in the body of a SKILL.md with the file line of their first
/// occurrence, outside code fences and inline code.
fn body_links(text: &str) -> Vec<(String, usize)> {
    let mut lines = text.lines().enumerate().peekable();
    if lines.peek().is_some_and(|(_, line)| is_fence_line(line)) {
        lines.next();
        for (_, line) in lines.by_ref() {
            if is_fence_line(line) {
                break;
            }
        }
    }
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut fenced = false;
    for (index, line) in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let outside: String = line
            .split('`')
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .map(|(_, part)| part)
            .collect();
        for caps in link_pattern().captures_iter(&outside) {
            let Some(target) = relative_target(&caps[1]) else {
                continue;
            };
            if !out.iter().any(|(seen, _)| *seen == target) {
                out.push((target, index + 1));
            }
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
pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub(crate) fn is_executable(_path: &Path) -> bool {
    true
}

/// The skills whose folder a sync copies next to `SKILL.md`.
pub(crate) fn folder_skills(skills: &[Skill]) -> impl Iterator<Item = &Skill> {
    skills.iter().filter(|skill| {
        skill.skill_type == super::parser::SkillType::Skill
            && skill
                .source_path
                .file_name()
                .is_some_and(|n| n == SKILL_FILE)
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LinkState {
    Installed,
    OutsideFolder,
    Missing,
    /// The target exists in the library and a sync leaves it out, for this reason.
    NotInstalled(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Link {
    pub target: String,
    pub line: usize,
    pub state: LinkState,
}

/// Every relative link in the body of SKILL.md with what a sync does with its target.
pub(crate) fn links_of(skill: &Skill, found: &Inventory) -> Vec<Link> {
    let dir = skill.source_dir();
    let text = std::fs::read_to_string(&skill.source_path).unwrap_or_default();
    body_links(&text)
        .into_iter()
        .map(|(target, line)| {
            let state = match normalize(&target) {
                None => LinkState::OutsideFolder,
                Some(rel) if !dir.join(&rel).exists() => LinkState::Missing,
                Some(rel) if installed(found, &rel) => LinkState::Installed,
                Some(rel) => LinkState::NotInstalled(
                    found
                        .skipped
                        .iter()
                        .find(|s| {
                            super::assets::rel_string(&rel).starts_with(s.rel.trim_end_matches('/'))
                        })
                        .map_or_else(|| "not installed".to_string(), |s| s.reason.describe()),
                ),
            };
            Link {
                target,
                line,
                state,
            }
        })
        .collect()
}

/// A hook script of `skill` that exists in its folder and cannot be run: its path relative to the
/// folder and the event it belongs to.
pub(crate) fn non_executable_hooks(skill: &Skill) -> Vec<(String, String)> {
    let dir = skill.source_dir();
    let mut out = Vec::new();
    for (event, hook) in skill.frontmatter.hooks.iter().flatten() {
        if Path::new(&hook.command).is_absolute() {
            continue;
        }
        let script = dir.join(&hook.command);
        if script.is_file() && !is_executable(&script) {
            out.push((hook.command.clone(), event.clone()));
        }
    }
    out
}

/// What a sync does with the files around each SKILL.md: info for every file it leaves out, a
/// warning for every relative link in SKILL.md whose target would not be installed, an error for
/// a hook script that is not executable (the parser already drops a skill whose script is missing).
pub fn lint_files(skills: &[Skill]) -> LintResult {
    let mut result = LintResult::default();
    for skill in folder_skills(skills) {
        let name = skill.name();
        let dir = skill.source_dir();
        let found = inventory(dir);
        for skipped in &found.skipped {
            let reason = skipped.reason.describe();
            result.found(
                LintLevel::Info,
                name,
                Detail {
                    reason: Some(reason.clone()),
                    ..Detail::new("file-not-copied", Consequence::NotCopied).file(&skipped.rel)
                },
                format!("{} is not installed by a sync: {reason}", skipped.rel),
            );
        }
        for found_link in links_of(skill, &found) {
            let Link {
                target,
                line,
                state,
            } = found_link;
            let link = |code| {
                Detail {
                    target: Some(target.clone()),
                    ..Detail::new(code, Consequence::DeadLink).file(SKILL_FILE)
                }
                .line(Some(line))
            };
            match state {
                LinkState::Installed => {}
                LinkState::OutsideFolder => result.found(
                    LintLevel::Warning,
                    name,
                    Detail {
                        fixable: super::movelink::movable_source(skill, &target)
                            .is_ok()
                            .then_some(super::movelink::FIX_MOVE_LINK),
                        ..link("link-outside-folder")
                    },
                    format!("Link '{target}' in SKILL.md leaves the skill folder and would not be installed."),
                ),
                LinkState::Missing => result.found(
                    LintLevel::Warning,
                    name,
                    link("link-missing"),
                    format!("Link '{target}' in SKILL.md points to a file that does not exist."),
                ),
                LinkState::NotInstalled(why) => result.found(
                    LintLevel::Warning,
                    name,
                    link("link-not-installed"),
                    format!("Link '{target}' in SKILL.md points to a file that would not be installed ({why})."),
                ),
            }
        }
        for (script, event) in non_executable_hooks(skill) {
            result.found(
                LintLevel::Error,
                name,
                Detail {
                    fixable: Some(FIX_CHMOD),
                    ..Detail::new("hook-not-executable", Consequence::HookWontRun).file(&script)
                },
                format!("Hook script '{script}' of the {event} hook is not executable."),
            );
        }
    }
    result
}

pub const FIX_CHMOD: &str = "chmod";

/// The line of the SKILL.md source that holds `text`, to point a rejection of the emitted file at
/// the file the author edits.
fn source_line(skill: &Skill, text: &str) -> Option<usize> {
    std::fs::read_to_string(&skill.source_path)
        .ok()?
        .lines()
        .position(|line| line.trim() == text.trim())
        .map(|index| index + 1)
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
                let line = match &reason {
                    Reason::UnindentedContinuation { line, text } => {
                        source_line(skill, text).or(Some(*line))
                    }
                    _ => None,
                };
                result.found(
                    LintLevel::Error,
                    skill.name(),
                    Detail {
                        reason: Some(reason.code().to_string()),
                        ..Detail::new("frontmatter-rejected", Consequence::Hidden).file(SKILL_FILE)
                    }
                    .line(line),
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
