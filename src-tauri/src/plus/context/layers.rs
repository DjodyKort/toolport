//! Context layers are mcpm rules: `rules/<name>/SKILL.md` with `activation: always` (optionally
//! `globs:` for path scoping) in the canonical skills repo. This module scaffolds them (never
//! overwriting an existing body) and deploys client layers as `CLAUDE.local.md` into client repos.

use super::roots::Roots;
use super::Report;
use crate::plus::skills::parser::find_fence_line;
use crate::plus::skills::pyfs::write_text;
use serde_yaml::Value as Yaml;
use std::fs;
use std::path::{Path, PathBuf};

pub const PERSONAL_RULE_NAME: &str = "personal";

pub const MANAGED_LOCAL_HEADER: &str = "<!-- Managed by `toolportctl context` — edit the canonical layer (skills_repo/rules/client-*/SKILL.md); `toolportctl context sync` regenerates. -->";

/// Header mcpm wrote before the port; files carrying it are still ours and get rewritten.
const LEGACY_MANAGED_LOCAL_HEADER: &str = "<!-- Managed by `mcpm context` — edit the canonical layer (skills_repo/rules/client-*/SKILL.md); `mcpm context sync` regenerates. -->";

pub fn is_managed_local(text: &str) -> bool {
    text.contains(MANAGED_LOCAL_HEADER) || text.contains(LEGACY_MANAGED_LOCAL_HEADER)
}

const PERSONAL_TEMPLATE: &str = "---\nname: personal\ndescription: \"Personal always-on layer on top of the org CLAUDE.md\"\nactivation: always\n---\n\n## Personal preferences\n\n<!-- Your personal layer. Survives corp-dev-tools syncs (it never touches rules/).\n     Keep it additive to the org CLAUDE.md — rules load after user memory. -->\n";

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub path: PathBuf,
    pub globs: Vec<String>,
    pub description: String,
}

/// Creates `rules/personal/SKILL.md`; `None` when it already exists.
pub fn scaffold_personal_rule(roots: &Roots) -> Result<Option<PathBuf>, String> {
    let target = roots.rules_dir().join(PERSONAL_RULE_NAME).join("SKILL.md");
    if target.exists() {
        return Ok(None);
    }
    write_text(&target, PERSONAL_TEMPLATE)?;
    Ok(Some(target))
}

/// Skill names allow lowercase alphanumerics and single hyphens, while client dirs may carry
/// underscores (`v18_arp`): the rule name is slugged, the raw name stays in the glob.
pub fn slug(name: &str) -> String {
    let mapped: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let mut collapsed = String::new();
    for c in mapped.chars() {
        if !(c == '-' && collapsed.ends_with('-')) {
            collapsed.push(c);
        }
    }
    let trimmed = collapsed.trim_matches('-');
    if trimmed.is_empty() {
        "client".into()
    } else {
        trimmed.into()
    }
}

fn yaml_double_quoted(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn check_scaffold_text(what: &str, text: &str) -> Result<(), String> {
    if text.chars().any(char::is_control) {
        return Err(format!("{what} must not contain control characters"));
    }
    if text.contains("---") {
        return Err(format!(
            "{what} must not contain '---' (it would end the frontmatter)"
        ));
    }
    Ok(())
}

/// Creates `rules/client-<slug>/SKILL.md` with a path-scoped glob; `None` when present.
/// Names the skills pipeline would reject or that cannot sit in the frontmatter are refused
/// before anything is written.
pub fn scaffold_client_rule(
    roots: &Roots,
    name: &str,
    glob: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let slug = slug(name);
    check_scaffold_text("client name", name)?;
    if let Some(glob) = glob {
        check_scaffold_text("glob", glob)?;
    }
    crate::plus::skills::parser::valid_name(&format!("client-{slug}"))
        .map_err(|e| format!("rule name client-{slug}: {e}"))?;
    let target = roots
        .rules_dir()
        .join(format!("client-{slug}"))
        .join("SKILL.md");
    if target.exists() {
        return Ok(None);
    }
    let globs = glob
        .map(str::to_string)
        .unwrap_or_else(|| format!("**/clients/{name}/**"));
    let (quoted_name, globs) = (yaml_double_quoted(name), yaml_double_quoted(&globs));
    let content = format!(
        "---\nname: client-{slug}\ndescription: \"Client context: {quoted_name}\"\nactivation: always\nglobs: \"{globs}\"\n---\n\n## {name} — client context\n\n<!-- Project knowledge for this client that doesn't belong in the repo's own\n     CLAUDE.md: contacts, conventions, environment quirks, gotchas. -->\n"
    );
    write_text(&target, &content)?;
    Ok(Some(target))
}

/// The text between the opening `---` and the next fence line, and what follows that fence's
/// dashes; a `---` inside a value does not close the frontmatter.
fn split_fenced(text: &str) -> Option<(&str, &str)> {
    if !text.starts_with("---") {
        return None;
    }
    let from = text.find('\n').map_or(text.len(), |i| i + 1);
    let close = find_fence_line(text, from)?;
    Some((&text[3..close], &text[close + 3..]))
}

pub(super) fn frontmatter(path: &Path) -> serde_yaml::Mapping {
    let Ok(text) = fs::read_to_string(path) else {
        return Default::default();
    };
    let Some((yaml, _)) = split_fenced(&text) else {
        return Default::default();
    };
    match serde_yaml::from_str::<Yaml>(yaml) {
        Ok(Yaml::Mapping(m)) => m,
        _ => Default::default(),
    }
}

pub(super) fn yaml_text(value: &Yaml) -> String {
    match value {
        Yaml::String(s) => s.clone(),
        Yaml::Number(n) => n.to_string(),
        Yaml::Bool(b) => if *b { "True" } else { "False" }.into(),
        Yaml::Null => "None".into(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

/// Parses `rules/*/SKILL.md` frontmatter. Invalid YAML falls back to the directory name with no
/// globs, so such a layer still deploys even though the skills pipeline skips it (recorded quirk).
pub fn list_layers(roots: &Roots) -> Vec<Layer> {
    let base = roots.rules_dir();
    let Ok(read) = fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
    dirs.sort();
    let mut layers = Vec::new();
    for dir in dirs {
        let skill_md = dir.join("SKILL.md");
        if !dir.is_dir() || !skill_md.exists() {
            continue;
        }
        let fm = frontmatter(&skill_md);
        let get = |key: &str| fm.get(Yaml::String(key.into()));
        let globs = match get("globs") {
            Some(Yaml::String(s)) => s
                .split(',')
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(String::from)
                .collect(),
            Some(Yaml::Sequence(items)) => items.iter().map(yaml_text).collect(),
            _ => Vec::new(),
        };
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        layers.push(Layer {
            name: get("name").map(yaml_text).unwrap_or(dir_name),
            path: skill_md,
            globs,
            description: get("description").map(yaml_text).unwrap_or_default(),
        });
    }
    layers
}

pub fn body_of(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    match split_fenced(&text) {
        Some((_, after)) => after.to_string(),
        None => text,
    }
}

/// Deploys client layers as `CLAUDE.local.md` into their client repos.
///
/// Rules with `paths:` globs only activate for sessions rooted above the client dir; sessions
/// rooted inside a client repo need project memory, and `CLAUDE.local.md` is the personal,
/// uncommitted variant. Body only, ignored through `.git/info/exclude`, unmanaged files left alone.
pub fn deploy_client_locals(
    roots: &Roots,
    clients_root: &Path,
    report: &mut Report,
    dry_run: bool,
) -> Result<(), String> {
    if !clients_root.is_dir() {
        return Ok(());
    }
    let layers: Vec<Layer> = list_layers(roots)
        .into_iter()
        .filter(|l| l.name.starts_with("client-"))
        .collect();
    let mut children: Vec<PathBuf> = fs::read_dir(clients_root)
        .map_err(|e| format!("{}: {e}", clients_root.display()))?
        .flatten()
        .map(|e| e.path())
        .collect();
    children.sort();
    for child in children {
        if !child.is_dir() {
            continue;
        }
        let child_name = child
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let wanted = format!("client-{}", slug(&child_name));
        let Some(layer) = layers.iter().find(|l| l.name == wanted) else {
            continue;
        };
        let target = child.join("CLAUDE.local.md");
        let content = format!(
            "{MANAGED_LOCAL_HEADER}\n\n{}\n",
            body_of(&layer.path).trim()
        );
        let existing = if target.exists() {
            Some(fs::read(&target).map_err(|e| format!("{}: {e}", target.display()))?)
        } else {
            None
        };
        let existing_text = existing
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned());
        if let Some(text) = &existing_text {
            if !is_managed_local(text) {
                report.warn(format!(
                    "{} exists and is not managed — leaving it alone",
                    target.display()
                ));
                continue;
            }
        }
        if existing_text.as_deref() != Some(content.as_str()) {
            if !dry_run {
                fs::write(&target, &content).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            report.add(format!("deployed {}", target.display()));
        }
        ensure_local_exclude(&child, report, dry_run)?;
    }
    Ok(())
}

/// A `.git` pointer file (worktree/submodule) is skipped; the append drops a missing trailing
/// newline of the previous last line exactly like mcpm.
fn ensure_local_exclude(repo: &Path, report: &mut Report, dry_run: bool) -> Result<(), String> {
    let git_dir = repo.join(".git");
    if !git_dir.is_dir() {
        return Ok(());
    }
    let exclude = git_dir.join("info").join("exclude");
    let text = fs::read_to_string(&exclude).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.contains(&"CLAUDE.local.md") {
        return Ok(());
    }
    lines.push("CLAUDE.local.md");
    if !dry_run {
        write_text(&exclude, &format!("{}\n", lines.join("\n")))?;
    }
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    report.add(format!("added CLAUDE.local.md to {name}/.git/info/exclude"));
    Ok(())
}
