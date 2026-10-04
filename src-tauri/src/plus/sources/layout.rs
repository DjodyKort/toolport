//! The `.claude`-style layout shared by repos, plugins and the user's own folder:
//! `skills/<name>/SKILL.md`, `commands/**/<name>.md`, `agents/**/<name>.md`, `rules/**/<name>.md`.

use super::budget::Budget;
use super::fsx::{self, Kind};
use std::path::{Path, PathBuf};

pub const CONTENT_DIRS: [&str; 4] = ["skills", "commands", "agents", "rules"];
const NESTED_DEPTH: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub kind: &'static str,
    pub fallback: String,
    pub rel: String,
    pub path: PathBuf,
}

/// The kind and name of a path relative to a `.claude`-style folder, or `None` when it is not
/// an item: a script beside a skill, a `.md.retired-...` backup, a README outside the layout.
pub fn classify(rel: &str) -> Option<(&'static str, String)> {
    let parts: Vec<&str> = rel.split('/').collect();
    let file = parts.last()?;
    match parts.first().copied()? {
        "skills" if parts.len() == 3 && *file == "SKILL.md" => {
            Some(("skill", parts[1].to_string()))
        }
        "commands" if parts.len() >= 2 && parts.len() <= NESTED_DEPTH + 1 => {
            let stem = file.strip_suffix(".md")?;
            let mut names: Vec<&str> = parts[1..parts.len() - 1].to_vec();
            names.push(stem);
            Some(("command", names.join(":")))
        }
        "agents" if parts.len() >= 2 && parts.len() <= NESTED_DEPTH + 1 => {
            Some(("agent", file.strip_suffix(".md")?.to_string()))
        }
        "rules" if parts.len() >= 2 && parts.len() <= NESTED_DEPTH + 1 => {
            let stem = file.strip_suffix(".md")?;
            let mut names: Vec<&str> = parts[1..parts.len() - 1].to_vec();
            names.push(stem);
            Some(("rule", names.join("/")))
        }
        _ => None,
    }
}

fn walk(base: &Path, rel: &str, depth: usize, max: usize, budget: &Budget, out: &mut Vec<Found>) {
    for entry in fsx::list_dir(&base.join(rel)) {
        if !budget.tick() {
            return;
        }
        let child = format!("{rel}/{}", entry.name);
        match entry.kind {
            Kind::File => {
                if let Some((kind, fallback)) = classify(&child) {
                    out.push(Found {
                        kind,
                        fallback,
                        rel: child,
                        path: entry.path,
                    });
                }
            }
            Kind::Dir if depth + 1 < max => walk(base, &child, depth + 1, max, budget, out),
            _ => {}
        }
    }
}

/// The items below `base` (a folder holding `skills/`, `commands/`, `agents/`, `rules/`), in
/// kind and name order. Skill folders are entered one level only; nothing else is searched.
pub fn scan_layout(base: &Path, budget: &Budget) -> Vec<Found> {
    let mut out = Vec::new();
    for top in CONTENT_DIRS {
        if budget.spent() || !fsx::is_dir(&base.join(top)) {
            continue;
        }
        let max = if top == "skills" { 2 } else { NESTED_DEPTH };
        walk(base, top, 0, max, budget, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_names_every_kind_and_ignores_the_rest() {
        let cases = [
            ("skills/odh/SKILL.md", Some(("skill", "odh"))),
            ("skills/odh/references/a.md", None),
            ("commands/up.md", Some(("command", "up"))),
            ("commands/team/up.md", Some(("command", "team:up"))),
            ("commands/up.md.retired-20260101", None),
            ("agents/reviewer.md", Some(("agent", "reviewer"))),
            ("rules/style/py.md", Some(("rule", "style/py"))),
            ("settings.json", None),
        ];
        for (rel, want) in cases {
            assert_eq!(
                classify(rel),
                want.map(|(k, n)| (k, n.to_string())),
                "{rel}"
            );
        }
    }
}
