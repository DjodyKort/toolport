//! The rule layers of the skills repository as the deploy sees them: `rules/<name>/SKILL.md` with
//! a `scope` (`global`, `glob` or `folder`), the `folders` a folder layer lands in, `imports` and
//! the `delivery` of those imports. Global and glob layers are rules that `skills sync` already
//! writes to `~/.claude/rules`; only a folder layer is delivered here, into `CLAUDE.local.md`.

use super::bundle_use::bind_folders;
use super::fsx;
use super::globs::glob_match;
use super::layers::{body_of, split_fenced};
use super::roots::Roots;
use serde_yaml::{Mapping, Value as Yaml};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Glob,
    Folder,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Glob => "glob",
            Self::Folder => "folder",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The imported text is written into the layer's block and refreshed by every sync.
    Copy,
    /// The block holds an `@path` line; Claude Code follows it itself.
    Import,
}

impl Delivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Import => "import",
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayerDef {
    pub name: String,
    pub path: PathBuf,
    pub scope: Scope,
    pub globs: Vec<String>,
    pub folders: Vec<String>,
    pub imports: Vec<String>,
    pub delivery: Delivery,
    pub body: String,
    /// What is wrong with the frontmatter; the layer still lists, with these defaults.
    pub problems: Vec<String>,
}

fn list_of(map: &Mapping, key: &str) -> Vec<String> {
    match map.get(Yaml::String(key.into())) {
        Some(Yaml::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(String::from)
            .collect(),
        Some(Yaml::Sequence(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

fn text_of(map: &Mapping, key: &str) -> Option<String> {
    map.get(Yaml::String(key.into()))
        .and_then(Yaml::as_str)
        .map(String::from)
}

pub fn parse(dir_name: &str, path: &Path, text: &str) -> LayerDef {
    let mut problems = Vec::new();
    let front = split_fenced(text)
        .and_then(|(yaml, _)| match serde_yaml::from_str::<Yaml>(yaml) {
            Ok(Yaml::Mapping(map)) => Some(map),
            Ok(_) => None,
            Err(e) => {
                problems.push(format!("the front matter is not valid YAML: {e}"));
                None
            }
        })
        .unwrap_or_default();
    let globs = list_of(&front, "globs");
    let scope = match text_of(&front, "scope").as_deref() {
        Some("global") => Scope::Global,
        Some("glob") => Scope::Glob,
        Some("folder") => Scope::Folder,
        Some(other) => {
            problems.push(format!(
                "scope {other:?} is not global, glob or folder; the layer is read as {}",
                if globs.is_empty() { "global" } else { "glob" }
            ));
            if globs.is_empty() {
                Scope::Global
            } else {
                Scope::Glob
            }
        }
        None if globs.is_empty() => Scope::Global,
        None => Scope::Glob,
    };
    let delivery = match text_of(&front, "delivery").as_deref() {
        Some("import") => Delivery::Import,
        Some("copy") | None => Delivery::Copy,
        Some(other) => {
            problems.push(format!(
                "delivery {other:?} is not copy or import; the layer is delivered as copy"
            ));
            Delivery::Copy
        }
    };
    let folders = list_of(&front, "folders");
    if scope == Scope::Folder && folders.is_empty() {
        problems.push("a folder layer needs `folders:`; it is deployed nowhere".into());
    }
    LayerDef {
        name: text_of(&front, "name").unwrap_or_else(|| dir_name.to_string()),
        path: path.to_path_buf(),
        scope,
        globs,
        folders,
        imports: list_of(&front, "imports"),
        delivery,
        body: body_of(path),
        problems,
    }
}

pub fn list(roots: &Roots) -> Vec<LayerDef> {
    fsx::list_dir(&roots.rules_dir())
        .into_iter()
        .filter(|entry| entry.kind == fsx::Kind::Dir)
        .filter_map(|entry| {
            let path = entry.path.join("SKILL.md");
            let text = fsx::read_text(&path, fsx::TEXT_CAP)?;
            Some(parse(&entry.name, &path, &text))
        })
        .collect()
}

fn expanded(roots: &Roots, pattern: &str) -> String {
    fsx::display(&roots.expand_user(pattern))
}

/// The folders a folder layer lands in. A pattern with `**` is matched against the folders
/// directly under the clients root; any other pattern names folders the way a bundle's `bind`
/// does (`*` stands for one folder name). Only folders that exist count.
pub fn resolve_folders(layer: &LayerDef, roots: &Roots, clients_root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for pattern in &layer.folders {
        let found: Vec<PathBuf> = if pattern.contains("**") {
            let wanted = expanded(roots, pattern);
            fsx::list_dir(clients_root)
                .into_iter()
                .filter(|entry| entry.kind == fsx::Kind::Dir)
                .map(|entry| entry.path)
                .filter(|path| glob_match(&wanted, &format!("{}/", fsx::display(path))))
                .collect()
        } else {
            bind_folders(pattern, &roots.home)
        };
        for folder in found {
            if !out.contains(&folder) {
                out.push(folder);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(front: &str) -> LayerDef {
        parse(
            "client-alpha",
            Path::new("/none/SKILL.md"),
            &format!("---\n{front}\n---\nbody\n"),
        )
    }

    #[test]
    fn test01_scope_defaults_follow_the_globs_and_a_bad_value_is_reported() {
        assert_eq!(layer("name: a").scope, Scope::Global);
        assert_eq!(layer("globs: \"**/x/**\"").scope, Scope::Glob);
        assert_eq!(layer("scope: folder\nfolders: [~/a]").scope, Scope::Folder);
        let bad = layer("scope: everywhere");
        assert_eq!(bad.scope, Scope::Global);
        assert!(bad.problems[0].contains("everywhere"), "{:?}", bad.problems);
    }

    #[test]
    fn test02_lists_take_a_yaml_list_or_a_comma_text() {
        let def =
            layer("scope: folder\nfolders: \"~/a, ~/b\"\nimports:\n  - ~/x.md\ndelivery: import");
        assert_eq!(def.folders, ["~/a", "~/b"]);
        assert_eq!(def.imports, ["~/x.md"]);
        assert_eq!(def.delivery, Delivery::Import);
    }

    #[test]
    fn test03_a_folder_layer_without_folders_says_it_lands_nowhere() {
        let def = layer("scope: folder");
        assert!(
            def.problems[0].contains("deployed nowhere"),
            "{:?}",
            def.problems
        );
    }
}
