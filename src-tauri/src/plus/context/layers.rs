//! Context layers are mcpm rules: `rules/<name>/SKILL.md` with `activation: always` (optionally
//! `globs:` for path scoping) in the canonical skills repo. This module scaffolds them (never
//! overwriting an existing body) and deploys client layers as `CLAUDE.local.md` into client repos.

use super::layer_spec::{self, SpecEdit, Spec};
use super::roots::Roots;
use super::Report;
use crate::plus::skills::parser::find_fence_line;
use crate::plus::skills::pyfs::write_text;
use serde_yaml::Value as Yaml;
use std::collections::BTreeMap;
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
    pub spec: Spec,
}

pub fn personal_rule_path(roots: &Roots) -> PathBuf {
    roots.rules_dir().join(PERSONAL_RULE_NAME).join("SKILL.md")
}

/// Creates `rules/personal/SKILL.md`; `None` when it already exists.
pub fn scaffold_personal_rule(roots: &Roots) -> Result<Option<PathBuf>, String> {
    let target = personal_rule_path(roots);
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

pub(super) fn yaml_double_quoted(text: &str) -> String {
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

/// Where a client rule goes and what it holds, after the name and glob passed the checks that
/// keep the frontmatter valid and the skills pipeline from rejecting the rule.
pub struct ClientRule {
    pub path: PathBuf,
    pub content: String,
    pub glob: String,
}

pub fn check_client_rule(name: &str, glob: Option<&str>) -> Result<(), String> {
    check_scaffold_text("client name", name)?;
    if let Some(glob) = glob {
        check_scaffold_text("glob", glob)?;
    }
    let slug = slug(name);
    crate::plus::skills::parser::valid_name(&format!("client-{slug}"))
        .map_err(|e| format!("rule name client-{slug}: {e}"))
}

pub fn plan_client_rule(
    roots: &Roots,
    name: &str,
    glob: Option<&str>,
) -> Result<ClientRule, String> {
    plan_client_rule_with(roots, name, glob, &SpecEdit::default())
}

pub fn plan_client_rule_with(
    roots: &Roots,
    name: &str,
    glob: Option<&str>,
    edit: &SpecEdit,
) -> Result<ClientRule, String> {
    for item in edit.folders.iter().chain(edit.imports.iter()).flatten() {
        check_scaffold_text("folder or import", item)?;
    }
    check_client_rule(name, glob)?;
    let slug = slug(name);
    let path = roots
        .rules_dir()
        .join(format!("client-{slug}"))
        .join("SKILL.md");
    let glob = glob
        .map(str::to_string)
        .unwrap_or_else(|| format!("**/clients/{name}/**"));
    let (quoted_name, quoted_glob) = (yaml_double_quoted(name), yaml_double_quoted(&glob));
    let mut content = format!(
        "---\nname: client-{slug}\ndescription: \"Client context: {quoted_name}\"\nactivation: always\nglobs: \"{quoted_glob}\"\n---\n\n## {name} — client context\n\n<!-- Project knowledge for this client that doesn't belong in the repo's own\n     CLAUDE.md: contacts, conventions, environment quirks, gotchas. -->\n"
    );
    let edits: Vec<_> = SpecEdit {
        glob: None,
        ..edit.clone()
    }
    .edits()
    .into_iter()
    .filter(|(_, value)| value.is_some())
    .collect();
    if !edits.is_empty() {
        content = layer_spec::rewrite_frontmatter(&content, &edits).ok_or("scaffold has no frontmatter")?;
    }
    Ok(ClientRule {
        path,
        content,
        glob,
    })
}

/// Creates `rules/client-<slug>/SKILL.md` with a path-scoped glob; `None` when present.
/// Names the skills pipeline would reject or that cannot sit in the frontmatter are refused
/// before anything is written.
pub fn scaffold_client_rule(
    roots: &Roots,
    name: &str,
    glob: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    scaffold_client_rule_with(roots, name, glob, &SpecEdit::default())
}

pub fn scaffold_client_rule_with(
    roots: &Roots,
    name: &str,
    glob: Option<&str>,
    edit: &SpecEdit,
) -> Result<Option<PathBuf>, String> {
    let rule = plan_client_rule_with(roots, name, glob, edit)?;
    if rule.path.exists() {
        return Ok(None);
    }
    write_text(&rule.path, &rule.content)?;
    Ok(Some(rule.path))
}

/// The text between the opening `---` and the next fence line, and what follows that fence's
/// dashes; a `---` inside a value does not close the frontmatter.
pub(super) fn split_fenced(text: &str) -> Option<(&str, &str)> {
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
    frontmatter_of(&text)
}

pub(super) fn frontmatter_of(text: &str) -> serde_yaml::Mapping {
    let Some((yaml, _)) = split_fenced(text) else {
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
    list_layers_in(&roots.rules_dir())
}

pub fn list_layers_in(base: &Path) -> Vec<Layer> {
    let Ok(read) = fs::read_dir(base) else {
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
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        layers.push(layer_from(skill_md.clone(), &frontmatter(&skill_md), dir_name));
    }
    layers
}

pub(super) fn layer_from(path: PathBuf, fm: &serde_yaml::Mapping, dir_name: String) -> Layer {
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
    Layer {
        name: get("name").map(yaml_text).unwrap_or(dir_name),
        path,
        globs,
        description: get("description").map(yaml_text).unwrap_or_default(),
        spec: layer_spec::spec_of(fm),
    }
}

pub fn body_of(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    match split_fenced(&text) {
        Some((_, after)) => after.to_string(),
        None => text,
    }
}

/// One layer's share of a folder's `CLAUDE.local.md`.
pub struct Entry {
    pub name: String,
    pub text: String,
    /// Matched by the client folder's name, not by an explicit `folders` entry.
    pub by_name: bool,
}

/// What `CLAUDE.local.md` of one folder should hold, and what it holds now.
pub struct FolderEffect {
    pub folder: PathBuf,
    pub path: PathBuf,
    pub existing: Option<String>,
    /// `None`: the file should not exist (or is not ours to touch, see `foreign`).
    pub content: Option<String>,
    pub foreign: bool,
    pub layers: Vec<String>,
}

const LAYER_BEGIN: &str = "<!-- toolport:layer:begin ";
const LAYER_END: &str = "<!-- toolport:layer:end ";

fn layer_section(entry: &Entry) -> String {
    format!(
        "{LAYER_BEGIN}{name} -->\n{}\n{LAYER_END}{name} -->\n",
        entry.text.trim(),
        name = entry.name
    )
}

pub fn local_content(entries: &[Entry], carried: Option<&str>) -> Option<String> {
    let mut content = format!("{MANAGED_LOCAL_HEADER}\n\n");
    if entries.is_empty() {
        content.push_str(carried?);
        return Some(content);
    }
    match entries {
        [only] if only.by_name => content.push_str(&format!("{}\n", only.text.trim())),
        many => {
            let sections: Vec<String> = many.iter().map(layer_section).collect();
            content.push_str(&sections.join("\n"));
        }
    }
    if let Some(block) = carried {
        content.push('\n');
        content.push_str(block);
    }
    Some(content)
}

/// Every folder a layer is delivered into as `CLAUDE.local.md`, with the layers that share it:
/// the client folder a `client-<name>` layer is named after, and the explicit `folders` of a layer
/// whose scope is `folder`. Problems with a folder or an import come back as warnings.
pub fn local_targets(
    roots: &Roots,
    clients_root: &Path,
    layers: &[Layer],
    warnings: &mut Vec<String>,
) -> BTreeMap<PathBuf, Vec<Entry>> {
    let mut out: BTreeMap<PathBuf, Vec<Entry>> = BTreeMap::new();
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut text_of = |layer: &Layer, warnings: &mut Vec<String>| -> String {
        texts
            .entry(layer.name.clone())
            .or_insert_with(|| {
                let delivered = layer_spec::deliver(&roots.home, layers, layer);
                warnings.extend(
                    delivered
                        .issues
                        .iter()
                        .map(|i| format!("{}: {}: {}", layer.name, i.key, i.message)),
                );
                delivered.text
            })
            .clone()
    };
    if let Ok(read) = fs::read_dir(clients_root) {
        let mut children: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children.into_iter().filter(|c| c.is_dir()) {
            let child_name = child
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let wanted = format!("client-{}", slug(&child_name));
            let Some(layer) = layers
                .iter()
                .find(|l| l.name == wanted && l.spec.scope == layer_spec::Scope::Glob)
            else {
                continue;
            };
            let text = text_of(layer, warnings);
            out.entry(child).or_default().push(Entry {
                name: layer.name.clone(),
                text,
                by_name: true,
            });
        }
    }
    for layer in layers.iter().filter(|l| l.spec.scope == layer_spec::Scope::Folder) {
        for raw in &layer.spec.folders {
            let folder = layer_spec::expand_user(&roots.home, raw);
            if !folder.is_absolute() {
                warnings.push(format!("{}: folder {raw} is not absolute — skipped", layer.name));
                continue;
            }
            if folder.starts_with(&roots.claude_home) {
                warnings.push(format!("{}: folder {raw} is inside the Claude home folder — skipped", layer.name));
                continue;
            }
            if !folder.is_dir() {
                warnings.push(format!("{}: folder {raw} does not exist — skipped", layer.name));
                continue;
            }
            if !folder.join(".git").exists() {
                warnings.push(format!(
                    "{}: {} has no .git, so CLAUDE.local.md there is not git-ignored",
                    layer.name,
                    folder.display()
                ));
            }
            let text = text_of(layer, warnings);
            out.entry(folder).or_default().push(Entry {
                name: layer.name.clone(),
                text,
                by_name: false,
            });
        }
    }
    for entries in out.values_mut() {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries.dedup_by(|a, b| a.name == b.name);
    }
    out
}

/// The effect on `CLAUDE.local.md` of each of `folders` once `entries` describes what it should
/// hold. A file that is not ours stays as it is, a folder without layers loses its managed file.
pub fn local_effects(
    folders: impl IntoIterator<Item = PathBuf>,
    targets: &BTreeMap<PathBuf, Vec<Entry>>,
) -> Vec<FolderEffect> {
    let mut out = Vec::new();
    for folder in folders {
        let path = folder.join("CLAUDE.local.md");
        let existing = fs::read(&path)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let entries = targets.get(&folder).map_or(&[][..], Vec::as_slice);
        let foreign = existing.as_deref().is_some_and(|t| !is_managed_local(t));
        let content = if foreign {
            None
        } else {
            let carried = existing.as_deref().and_then(super::bundle_apply::carried_block);
            local_content(entries, carried)
        };
        out.push(FolderEffect {
            layers: entries.iter().map(|e| e.name.clone()).collect(),
            folder,
            path,
            existing,
            content,
            foreign,
        });
    }
    out
}

pub fn write_local_effect(effect: &FolderEffect, report: &mut Report, dry_run: bool) -> Result<(), String> {
    if effect.foreign {
        if !effect.layers.is_empty() {
            report.warn(format!(
                "{} exists and is not managed — leaving it alone",
                effect.path.display()
            ));
        }
        return Ok(());
    }
    match (&effect.content, &effect.existing) {
        (Some(content), existing) => {
            if existing.as_deref() != Some(content.as_str()) {
                if !dry_run {
                    fs::write(&effect.path, content)
                        .map_err(|e| format!("{}: {e}", effect.path.display()))?;
                }
                report.add(format!("deployed {}", effect.path.display()));
            }
            ensure_local_exclude(&effect.folder, report, dry_run)?;
        }
        (None, Some(_)) => {
            if !dry_run {
                fs::remove_file(&effect.path)
                    .map_err(|e| format!("{}: {e}", effect.path.display()))?;
            }
            report.add(format!("removed {}", effect.path.display()));
        }
        (None, None) => {}
    }
    Ok(())
}

/// Deploys layers as `CLAUDE.local.md` into the folders they are for.
///
/// Rules with `paths:` globs only activate for sessions rooted above the client dir; sessions
/// rooted inside a client repo need project memory, and `CLAUDE.local.md` is the personal,
/// uncommitted variant. Imports of a copy layer are refreshed here, so a changed imported file
/// reaches the folder on the next `context sync`. Ignored through `.git/info/exclude`, unmanaged
/// files left alone.
pub fn deploy_client_locals(
    roots: &Roots,
    clients_root: &Path,
    report: &mut Report,
    dry_run: bool,
) -> Result<(), String> {
    let layers = list_layers(roots);
    let mut warnings = Vec::new();
    let targets = local_targets(roots, clients_root, &layers, &mut warnings);
    for warning in warnings {
        report.warn(warning);
    }
    for effect in local_effects(targets.keys().cloned().collect::<Vec<_>>(), &targets) {
        write_local_effect(&effect, report, dry_run)?;
    }
    Ok(())
}

/// A `.git` pointer file (worktree/submodule) is skipped; the append drops a missing trailing
/// newline of the previous last line exactly like mcpm.
fn ensure_local_exclude(repo: &Path, report: &mut Report, dry_run: bool) -> Result<(), String> {
    ensure_exclude_line(repo, "CLAUDE.local.md", report, dry_run).map(|_| ())
}

/// Adds `line` to the repo's `.git/info/exclude`; true when it was not there yet.
pub(super) fn ensure_exclude_line(
    repo: &Path,
    line: &str,
    report: &mut Report,
    dry_run: bool,
) -> Result<bool, String> {
    let git_dir = repo.join(".git");
    if !git_dir.is_dir() {
        return Ok(false);
    }
    let exclude = git_dir.join("info").join("exclude");
    let text = fs::read_to_string(&exclude).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.contains(&line) {
        return Ok(false);
    }
    lines.push(line);
    if !dry_run {
        write_text(&exclude, &format!("{}\n", lines.join("\n")))?;
    }
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    report.add(format!("added {line} to {name}/.git/info/exclude"));
    Ok(true)
}
