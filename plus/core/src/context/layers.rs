//! The rule layers of the skills repository (`rules/<name>/SKILL.md`) a bundle can add to a
//! folder's `CLAUDE.local.md`, and the git exclude line that keeps the local files out of commits.

use super::fsx;
use super::roots::Roots;
use serde_yaml::Value as Yaml;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Layer {
    pub name: String,
    pub path: PathBuf,
}

/// The text between the opening fence and the next line that is only `---`, and what follows it.
fn split_fenced(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---")?;
    let start = rest.find('\n')? + 1;
    let mut at = start;
    for line in rest[start..].split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[start..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    None
}

fn declared_name(text: &str) -> Option<String> {
    let (yaml, _) = split_fenced(text)?;
    match serde_yaml::from_str::<Yaml>(yaml).ok()? {
        Yaml::Mapping(map) => map
            .get(Yaml::String("name".into()))
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => None,
    }
}

pub fn list_layers(roots: &Roots) -> Vec<Layer> {
    fsx::list_dir(&roots.rules_dir())
        .into_iter()
        .filter(|entry| entry.kind == fsx::Kind::Dir)
        .filter_map(|entry| {
            let path = entry.path.join("SKILL.md");
            let text = fs::read_to_string(&path).ok()?;
            Some(Layer {
                name: declared_name(&text).unwrap_or(entry.name),
                path,
            })
        })
        .collect()
}

pub fn body_of(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    match split_fenced(&text) {
        Some((_, after)) => after.to_string(),
        None => text,
    }
}

/// Adds `line` to the repository's `info/exclude` (shared by a linked worktree's main checkout); true when it was not there yet.
pub fn ensure_exclude_line(repo: &Path, line: &str) -> Result<bool, String> {
    let Some(layout) = super::gitlayout::at(repo) else {
        return Ok(false);
    };
    let exclude = layout.exclude_file();
    let text = fs::read_to_string(&exclude).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.contains(&line) {
        return Ok(false);
    }
    lines.push(line);
    crate::fsx::atomic_write(
        &exclude,
        &format!("{}\n", lines.join("\n")),
        crate::fsx::Mode::KeepOrDefault,
    )
    .map_err(|e| format!("{}: {e}", exclude.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_starts_after_the_closing_fence() {
        let (front, body) = split_fenced("---\nname: a\n---\nBody\n").unwrap();
        assert_eq!(front, "name: a\n");
        assert_eq!(body, "Body\n");
        assert!(split_fenced("no fence").is_none());
    }

    #[test]
    fn a_layer_takes_its_name_from_the_front_matter_or_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let roots = Roots::from_home(dir.path());
        let rules = roots.rules_dir();
        fs::create_dir_all(rules.join("one")).unwrap();
        fs::create_dir_all(rules.join("two")).unwrap();
        fs::write(rules.join("one/SKILL.md"), "---\nname: first\n---\nA").unwrap();
        fs::write(rules.join("two/SKILL.md"), "B").unwrap();
        let names: Vec<String> = list_layers(&roots).into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["first", "two"]);
    }

    #[test]
    fn the_exclude_line_is_added_once_and_only_in_a_git_repository() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!ensure_exclude_line(dir.path(), "x").unwrap());
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        assert!(ensure_exclude_line(dir.path(), "x").unwrap());
        assert!(!ensure_exclude_line(dir.path(), "x").unwrap());
    }
}
