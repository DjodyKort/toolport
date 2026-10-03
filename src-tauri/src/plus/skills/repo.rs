//! Skills repository management ported from mcpm's `skills init|add|audit`: repo discovery,
//! the manifest and directory skeleton, and skill scaffolding. Bundles live in `bundle.rs`.

use super::audit::{audit_skills, AuditResult};
use super::ops::find_skills_repo;
use super::parser::{discover_skills, valid_name};
use super::pyfs::write_text;
use crate::registry;
use serde_yaml::{Mapping, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub const MANIFEST_NAME: &str = "mcpm-skills.yaml";
const REPO_DIRS: [&str; 5] = ["skills", "rules", "agents", "styles", "profiles"];

const PROGRESSIVE_STUBS: [(&str, &str); 3] = [
    (
        "modules/example.md",
        "# Module: Example\n\n**Status:** Optional.\n\n## Section heading\n\n`## Example`\n\n## Trigger signals\n\n- When this module belongs in the spec.\n\n## Default content shape\n\nThe body of this section when included.\n",
    ),
    (
        "reference/example.md",
        "# Example reference\n\nDeep content the SKILL.md links to but does not inline.\n\nKeep one level deep, no nested references.\n",
    ),
    (
        "templates/example.md",
        "# Example template\n\nReusable scaffolding the skill renders with task-specific values.\n",
    ),
];

/// Python `Path.resolve()`: symlinks resolve for the part that exists, the rest is appended.
pub fn resolve_path(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut existing = abs.as_path();
    let mut tail = Vec::new();
    loop {
        if let Ok(mut real) = existing.canonicalize() {
            real.extend(tail.iter().rev());
            return real;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_owned());
                existing = parent;
            }
            _ => return abs,
        }
    }
}

/// `mcpm`'s repo discovery: walk up from `start` (or the working directory); only without an
/// explicit `start` does the git-sync clone from `skills_sync.json` count.
pub fn find_repo(start: Option<&Path>) -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config = registry::conduit_dir().unwrap_or_else(|| cwd.clone());
    find_skills_repo(start, &cwd, &config)
}

pub fn manifest_text(name: &str) -> String {
    let mut map = Mapping::new();
    for (key, value) in [
        ("name", name),
        ("description", ""),
        ("author", ""),
        ("version", "1.0.0"),
        ("license", ""),
    ] {
        map.insert(Value::String(key.into()), Value::String(value.into()));
    }
    serde_yaml::to_string(&map).unwrap_or_default()
}

#[derive(Debug, PartialEq, Eq)]
pub struct InitReport {
    pub repo: PathBuf,
    pub name: String,
    pub already_exists: bool,
    pub created: Vec<String>,
}

/// Creates the manifest and the content directories; an existing repo (manifest or `skills/`)
/// is left alone. Nothing is written on `dry_run`.
pub fn init_repo(path: &Path, name: Option<&str>, dry_run: bool) -> Result<InitReport, String> {
    let repo = resolve_path(path);
    let name = match name {
        Some(n) => n.to_string(),
        None => repo
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    if name.is_empty() {
        return Err("repository name is empty; pass --name".into());
    }
    if repo.join(MANIFEST_NAME).exists() || repo.join("skills").is_dir() {
        return Ok(InitReport {
            repo,
            name,
            already_exists: true,
            created: Vec::new(),
        });
    }
    let created: Vec<String> = std::iter::once(MANIFEST_NAME.to_string())
        .chain(REPO_DIRS.iter().map(|d| format!("{d}/")))
        .collect();
    if !dry_run {
        for dir in REPO_DIRS {
            let path = repo.join(dir);
            fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        write_text(&repo.join(MANIFEST_NAME), &manifest_text(&name))?;
    }
    Ok(InitReport {
        repo,
        name,
        already_exists: false,
        created,
    })
}

pub fn skill_bucket(skill_type: &str) -> &'static str {
    if skill_type == "rule" {
        "rules"
    } else {
        "skills"
    }
}

pub fn skill_template(name: &str, skill_type: &str) -> String {
    let activation = if skill_type == "rule" {
        "always"
    } else {
        "auto"
    };
    format!(
        "---\nname: {name}\ndescription: \"TODO: Describe what this {skill_type} does and when to use it.\"\nactivation: {activation}\n---\n\nTODO: Add {skill_type} instructions here.\n"
    )
}

pub struct AddRequest<'a> {
    pub repo: &'a Path,
    pub name: &'a str,
    pub skill_type: &'a str,
    pub with_progressive: bool,
    pub dry_run: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AddReport {
    pub repo: PathBuf,
    pub skill_type: String,
    pub skill_file: PathBuf,
    pub files: Vec<PathBuf>,
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Writes `<bucket>/<name>/SKILL.md` from the template, plus the progressive-disclosure stubs.
/// Unlike the self-MCP scaffold tool this works in a directory that is not a repo yet.
pub fn add_skill(req: &AddRequest<'_>) -> Result<AddReport, String> {
    if !["skill", "rule"].contains(&req.skill_type) {
        return Err("type must be skill or rule".into());
    }
    valid_name(req.name).map_err(|e| format!("Invalid skill name '{}': {e}", req.name))?;
    if req.with_progressive && req.skill_type != "skill" {
        return Err("--with-progressive is only valid for skills, not rules.".into());
    }
    let repo = resolve_path(req.repo);
    let dir = repo.join(skill_bucket(req.skill_type)).join(req.name);
    if dir.exists() {
        return Err(format!(
            "{} '{}' already exists.",
            capitalize(req.skill_type),
            req.name
        ));
    }
    let skill_file = dir.join("SKILL.md");
    let mut files = vec![skill_file.clone()];
    if req.with_progressive {
        files.extend(PROGRESSIVE_STUBS.iter().map(|(rel, _)| dir.join(rel)));
    }
    if !req.dry_run {
        write_text(&skill_file, &skill_template(req.name, req.skill_type))?;
        if req.with_progressive {
            for (rel, content) in PROGRESSIVE_STUBS {
                write_text(&dir.join(rel), content)?;
            }
        }
    }
    Ok(AddReport {
        repo,
        skill_type: req.skill_type.to_string(),
        skill_file,
        files,
    })
}

pub struct AuditReport {
    pub repo: PathBuf,
    pub skill_count: usize,
    pub result: AuditResult,
}

pub fn audit_repo(start: Option<&Path>) -> Result<AuditReport, String> {
    let start = start.map(resolve_path);
    let repo = find_repo(start.as_deref()).ok_or("no skills repository found")?;
    let skills = discover_skills(&repo);
    let result = audit_skills(&skills);
    Ok(AuditReport {
        repo,
        skill_count: skills.len(),
        result,
    })
}
