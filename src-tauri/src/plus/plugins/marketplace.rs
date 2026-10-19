//! A plugin marketplace inside a tap clone: `.claude-plugin/marketplace.json` and the plugins it
//! lists by relative path. Read-only; nothing that resolves outside the clone is followed.

use crate::plus::jsonfs::read_json;
use crate::plus::sources::fsx::{self, Kind};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const FILE: &str = ".claude-plugin/marketplace.json";
const PLUGIN_FILE: &str = ".claude-plugin/plugin.json";
const MAX_NAME: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub name: String,
    pub dir: PathBuf,
    pub version: Option<String>,
    pub description: Option<String>,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub hooks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marketplace {
    pub name: String,
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<String>,
}

impl Marketplace {
    pub fn plugin(&self, name: &str) -> Option<&Plugin> {
        self.plugins.iter().find(|p| p.name == name)
    }

    pub fn id_of(&self, plugin: &str) -> String {
        format!("{plugin}@{}", self.name)
    }
}

/// A marketplace or plugin name: it ends up in a plugin id (`name@marketplace`) and on the
/// `claude` command line, so `@` and anything that looks like a flag are out.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn is_marketplace(clone: &Path) -> bool {
    fsx::is_file(&clone.join(FILE))
}

/// `None` when the clone has no marketplace file; `Some(Err)` when it has one that cannot be used.
pub fn read(clone: &Path) -> Option<Result<Marketplace, String>> {
    let file = clone.join(FILE);
    fsx::is_file(&file).then(|| parse(clone, &file))
}

fn parse(clone: &Path, file: &Path) -> Result<Marketplace, String> {
    let doc: Value = read_json(file).ok_or_else(|| format!("{FILE} is not valid JSON"))?;
    let name = doc
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| valid_name(n))
        .ok_or_else(|| format!("{FILE} has no usable \"name\""))?;
    let root = fsx::canonical(clone);
    let mut plugins: Vec<Plugin> = Vec::new();
    let mut warnings = Vec::new();
    for entry in doc
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(plugin) = entry
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| valid_name(n))
        else {
            warnings.push("a plugin entry without a usable name is skipped".to_string());
            continue;
        };
        let Some(source) = entry.get("source").and_then(Value::as_str) else {
            warnings.push(format!(
                "{plugin} comes from a remote source, not from this clone, so it is not linked"
            ));
            continue;
        };
        let Some(dir) = inside(&root, source) else {
            warnings.push(format!(
                "{plugin}: source {source:?} is not a folder inside the clone, so it is skipped"
            ));
            continue;
        };
        if plugins.iter().any(|p| p.name == plugin) {
            warnings.push(format!("{plugin} is listed twice; the first entry is kept"));
            continue;
        }
        plugins.push(plugin_at(plugin, dir, entry));
    }
    Ok(Marketplace {
        name: name.to_string(),
        plugins,
        warnings,
    })
}

fn inside(root: &Path, source: &str) -> Option<PathBuf> {
    let rel = source.strip_prefix("./")?;
    if rel.split('/').any(|part| part == "..") {
        return None;
    }
    let dir = fsx::canonical(&root.join(rel));
    (dir.starts_with(root) && fsx::is_dir(&dir)).then_some(dir)
}

fn plugin_at(name: &str, dir: PathBuf, entry: &Value) -> Plugin {
    let manifest: Option<Value> = read_json(&dir.join(PLUGIN_FILE));
    let text = |key: &str| {
        manifest
            .as_ref()
            .and_then(|m| m.get(key))
            .or_else(|| entry.get(key))
            .and_then(Value::as_str)
            .map(String::from)
    };
    let skills = fsx::list_dir(&dir.join("skills"))
        .into_iter()
        .filter(|e| e.kind == Kind::Dir && fsx::is_file(&e.path.join("SKILL.md")))
        .map(|e| e.name)
        .collect();
    let agents = fsx::list_dir(&dir.join("agents"))
        .into_iter()
        .filter(|e| e.kind == Kind::File)
        .filter_map(|e| e.name.strip_suffix(".md").map(String::from))
        .collect();
    Plugin {
        name: name.to_string(),
        version: text("version"),
        description: entry
            .get("description")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| text("description")),
        skills,
        agents,
        hooks: fsx::is_file(&dir.join("hooks/hooks.json")),
        dir,
    }
}
