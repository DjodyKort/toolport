//! SKILL.md parsing: YAML frontmatter plus Markdown body, validated like mcpm's pydantic schema.

use regex::Regex;
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillType {
    Skill,
    Rule,
}

impl SkillType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillType::Skill => "skill",
            SkillType::Rule => "rule",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    Always,
    Auto,
    Agent,
    Manual,
}

impl Activation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Activation::Always => "always",
            Activation::Auto => "auto",
            Activation::Agent => "agent",
            Activation::Manual => "manual",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillHook {
    pub command: String,
    pub matcher: String,
    pub hook_type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillDependencies {
    pub servers: Vec<String>,
    pub skills: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub allowed_tools: Option<String>,
    pub metadata: Mapping,
    pub hooks: Option<Vec<(String, SkillHook)>>,
    pub globs: Option<String>,
    pub activation: Activation,
    pub priority: i64,
    pub dependencies: Option<SkillDependencies>,
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub frontmatter: SkillFrontmatter,
    pub body: String,
    pub source_path: PathBuf,
    pub skill_type: SkillType,
}

impl Skill {
    pub fn name(&self) -> &str {
        &self.frontmatter.name
    }

    /// Stand-in used only to ask a transpiler for an output path, like mcpm's `dummy` configs.
    pub fn placeholder(name: &str, skill_type: SkillType) -> Skill {
        Skill {
            frontmatter: SkillFrontmatter {
                name: name.to_string(),
                description: "dummy".into(),
                license: None,
                compatibility: None,
                allowed_tools: None,
                metadata: Mapping::new(),
                hooks: None,
                globs: None,
                activation: Activation::Auto,
                priority: 0,
                dependencies: None,
            },
            body: String::new(),
            source_path: PathBuf::from("dummy"),
            skill_type,
        }
    }

    pub fn source_dir(&self) -> &Path {
        self.source_path.parent().unwrap_or_else(|| Path::new(""))
    }

    /// `metadata.version` when it is a string; mcpm rejects other types at lock time.
    pub fn version(&self) -> Option<String> {
        self.frontmatter
            .metadata
            .get("version")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

/// A fence is a line holding only `---` plus trailing whitespace; a `---` inside a value never is.
pub(crate) fn is_fence_line(line: &str) -> bool {
    line.starts_with("---") && line.trim() == "---"
}

/// Byte offset of the first fence line starting at or after `from`, which must be a line start.
pub(crate) fn find_fence_line(text: &str, from: usize) -> Option<usize> {
    let mut offset = from;
    for line in text[from..].split_inclusive('\n') {
        if is_fence_line(line) {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Splits `---` fenced YAML from the body, trimmed like Python's `str.strip()`; both fences
/// must sit on their own line.
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let yaml_start = content.find('\n')? + 1;
    if !is_fence_line(&content[..yaml_start]) {
        return None;
    }
    let close = find_fence_line(content, yaml_start)?;
    let after = &content[close..];
    let body = after.find('\n').map_or("", |i| &after[i + 1..]);
    Some((&content[yaml_start..close], body.trim()))
}

/// Frontmatter keys with top-level hyphens normalised to underscores, in document order.
pub fn parse_frontmatter(content: &str) -> Result<(Vec<(String, Value)>, String), String> {
    let Some((yaml, body)) = split_frontmatter(content) else {
        return Ok((Vec::new(), content.to_string()));
    };
    let parsed: Value = serde_yaml::from_str(yaml).map_err(|e| format!("invalid YAML: {e}"))?;
    let map = match parsed {
        Value::Null => return Ok((Vec::new(), body.to_string())),
        Value::Mapping(m) => m,
        _ => return Err("frontmatter must be a mapping".into()),
    };
    let mut out: Vec<(String, Value)> = Vec::new();
    for (k, v) in map {
        let key = scalar_to_string(&k)?.replace('-', "_");
        match out.iter_mut().find(|(ek, _)| *ek == key) {
            Some(slot) => slot.1 = v,
            None => out.push((key, v)),
        }
    }
    Ok((out, body.to_string()))
}

pub(crate) fn scalar_to_string(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(if *b { "True" } else { "False" }.into()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Null => Ok("None".into()),
        _ => Err("unsupported frontmatter key".into()),
    }
}

pub(crate) fn field<'a>(fm: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    fm.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub(crate) fn want_str(v: &Value, name: &str) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        _ => Err(format!("{name}: input should be a valid string")),
    }
}

pub(crate) fn opt_str(fm: &[(String, Value)], name: &str) -> Result<Option<String>, String> {
    match field(fm, name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => want_str(v, name).map(Some),
    }
}

fn str_list(v: Option<&Value>, name: &str) -> Result<Vec<String>, String> {
    match v {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Sequence(items)) => items.iter().map(|i| want_str(i, name)).collect(),
        Some(_) => Err(format!("{name}: input should be a valid list")),
    }
}

pub(crate) fn valid_name(v: &str) -> Result<(), String> {
    let len = v.chars().count();
    if len == 0 || len > 64 {
        return Err("name must be 1-64 characters".into());
    }
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[a-z0-9]([a-z0-9-]*[a-z0-9])?\z").unwrap());
    if !re.is_match(v) {
        return Err(
            "name must be lowercase alphanumeric + hyphens, cannot start/end with hyphen".into(),
        );
    }
    if v.contains("--") {
        return Err("name must not contain consecutive hyphens".into());
    }
    Ok(())
}

pub fn build_frontmatter(fm: &[(String, Value)]) -> Result<SkillFrontmatter, String> {
    let name = want_str(field(fm, "name").ok_or("name: field required")?, "name")?;
    valid_name(&name)?;
    let description = want_str(
        field(fm, "description").ok_or("description: field required")?,
        "description",
    )?;
    let dlen = description.chars().count();
    if dlen == 0 || dlen > 1024 {
        return Err("description must be 1-1024 characters".into());
    }
    let compatibility = opt_str(fm, "compatibility")?;
    if compatibility
        .as_ref()
        .is_some_and(|c| c.chars().count() > 500)
    {
        return Err("compatibility must be at most 500 characters".into());
    }
    let metadata = match field(fm, "metadata") {
        None | Some(Value::Null) => Mapping::new(),
        Some(Value::Mapping(m)) => m.clone(),
        Some(_) => return Err("metadata: input should be a valid dictionary".into()),
    };
    let hooks = match field(fm, "hooks") {
        None | Some(Value::Null) => None,
        Some(Value::Mapping(m)) => {
            let mut out = Vec::new();
            for (event, hook) in m {
                let event = scalar_to_string(event)?;
                out.push((event.clone(), build_hook(&event, hook)?));
            }
            Some(out)
        }
        Some(_) => return Err("hooks: input should be a valid dictionary".into()),
    };
    let activation = match field(fm, "activation") {
        None => Activation::Auto,
        Some(Value::String(s)) => match s.as_str() {
            "always" => Activation::Always,
            "auto" => Activation::Auto,
            "agent" => Activation::Agent,
            "manual" => Activation::Manual,
            other => return Err(format!("activation: invalid value {other:?}")),
        },
        Some(_) => return Err("activation: invalid value".into()),
    };
    let priority = match field(fm, "priority") {
        None => 0,
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .ok_or("priority: input should be a valid integer")?,
        Some(Value::String(s)) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| "priority: input should be a valid integer".to_string())?,
        Some(_) => return Err("priority: input should be a valid integer".into()),
    };
    let dependencies = match field(fm, "dependencies") {
        None | Some(Value::Null) => None,
        Some(Value::Mapping(m)) => Some(SkillDependencies {
            servers: str_list(m.get("servers"), "dependencies.servers")?,
            skills: str_list(m.get("skills"), "dependencies.skills")?,
        }),
        Some(_) => return Err("dependencies: input should be a valid dictionary".into()),
    };
    Ok(SkillFrontmatter {
        name,
        description,
        license: opt_str(fm, "license")?,
        compatibility,
        allowed_tools: opt_str(fm, "allowed_tools")?,
        metadata,
        hooks,
        globs: opt_str(fm, "globs")?,
        activation,
        priority,
        dependencies,
    })
}

fn build_hook(event: &str, v: &Value) -> Result<SkillHook, String> {
    let Value::Mapping(m) = v else {
        return Err(format!("hooks.{event}: input should be a valid dictionary"));
    };
    let command = want_str(
        m.get("command")
            .ok_or_else(|| format!("hooks.{event}.command: field required"))?,
        "command",
    )?;
    if command.trim().is_empty() {
        return Err("hook command must be a non-empty path".into());
    }
    if command.starts_with('/') || command.starts_with('~') {
        return Err(format!(
            "hook command must be a path relative to the skill source dir, got absolute path: {command}"
        ));
    }
    let or_default = |key: &str, default: &str| -> Result<String, String> {
        match m.get(key) {
            None => Ok(default.to_string()),
            Some(v) => want_str(v, key),
        }
    };
    Ok(SkillHook {
        command,
        matcher: or_default("matcher", "*")?,
        hook_type: or_default("type", "command")?,
    })
}

pub fn parse_skill_file(path: &Path) -> Result<Skill, String> {
    if !path.exists() {
        return Err(format!("Skill file not found: {}", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let content = String::from_utf8(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let (fm_data, body) = parse_frontmatter(&content)?;
    if fm_data.is_empty() {
        return Err(format!("No YAML frontmatter found in {}", path.display()));
    }
    let frontmatter = build_frontmatter(&fm_data)?;
    if let Some(hooks) = frontmatter.hooks.as_ref().filter(|h| !h.is_empty()) {
        let skill_dir = path.parent().unwrap_or_else(|| Path::new(""));
        let canon_dir = skill_dir.canonicalize().map_err(|e| e.to_string())?;
        for (event, hook) in hooks {
            let target = skill_dir.join(&hook.command);
            let resolved = target.canonicalize().unwrap_or_else(|_| target.clone());
            if !resolved.starts_with(&canon_dir) {
                return Err(format!(
                    "hook '{event}' command must stay inside the skill directory: {}",
                    hook.command
                ));
            }
            if !resolved.is_file() {
                return Err(format!(
                    "hook '{event}' command not found at {} (relative to {})",
                    resolved.display(),
                    skill_dir.display()
                ));
            }
        }
    }
    let skill_type = infer_skill_type(path, frontmatter.activation);
    Ok(Skill {
        frontmatter,
        body,
        source_path: path.to_path_buf(),
        skill_type,
    })
}

/// Any `rules` component anywhere in the path (including ancestors of the repo) makes a rule.
fn infer_skill_type(path: &Path, activation: Activation) -> SkillType {
    if path.components().any(|c| c.as_os_str() == "rules") {
        return SkillType::Rule;
    }
    if activation == Activation::Always {
        SkillType::Rule
    } else {
        SkillType::Skill
    }
}

#[derive(Debug, Default)]
pub struct Discovery {
    pub skills: Vec<Skill>,
    pub warnings: Vec<String>,
}

/// `skills/` first, then `rules/`; each directory's children in byte order of their names.
fn skill_dirs(repo: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for search in ["skills", "rules"] {
        let dir = repo.join(search);
        if !dir.is_dir() {
            continue;
        }
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
        children.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        dirs.extend(children);
    }
    dirs
}

pub fn discover_skills_report(repo: &Path) -> Discovery {
    let mut out = Discovery::default();
    for skill_dir in skill_dirs(repo) {
        if !skill_dir.is_dir() {
            continue;
        }
        let skill_file = skill_dir.join("SKILL.md");
        if !skill_file.exists() {
            out.warnings.push(format!(
                "Skill directory {} has no SKILL.md, skipping",
                file_name(&skill_dir)
            ));
            continue;
        }
        match parse_skill_file(&skill_file) {
            Ok(skill) => out.skills.push(skill),
            Err(e) => out
                .warnings
                .push(format!("Failed to parse {}: {e}", skill_file.display())),
        }
    }
    out
}

/// The first skill `discover_skills` would list under `name`, without parsing the ones after it.
pub fn find_skill(repo: &Path, name: &str) -> Option<Skill> {
    skill_dirs(repo)
        .into_iter()
        .filter(|dir| dir.is_dir())
        .filter_map(|dir| parse_skill_file(&dir.join("SKILL.md")).ok())
        .find(|skill| skill.name() == name)
}

pub fn discover_skills(repo: &Path) -> Vec<Skill> {
    discover_skills_report(repo).skills
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
