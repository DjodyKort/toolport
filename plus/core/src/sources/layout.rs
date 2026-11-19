//! The `.claude`-style layout shared by repos, plugins and the user's own folder:
//! `skills/<name>/SKILL.md`, `commands/**/<name>.md`, `agents/**/<name>.md`, `rules/**/<name>.md`.

use super::budget::Budget;
use crate::context::fsx::{self, Kind};
use crate::untrusted::{self, Kind as LinkKind};
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

/// Whose folder is being read. Links in the user's own folders are theirs and are followed;
/// links in a checkout somebody else wrote (a tap, a plugin) are never followed and are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    Own,
    Untrusted,
}

struct Scan<'a> {
    trust: Trust,
    budget: &'a Budget,
    out: Vec<Found>,
    links: &'a mut Vec<String>,
}

impl Scan<'_> {
    fn walk(&mut self, base: &Path, rel: &str, depth: usize, max: usize) {
        match self.trust {
            Trust::Own => {
                for entry in fsx::list_dir(&base.join(rel)) {
                    if !self.budget.tick() {
                        return;
                    }
                    self.visit(base, rel, depth, max, &entry.name, entry.path, entry.kind);
                }
            }
            Trust::Untrusted => {
                for entry in untrusted::list_dir(&base.join(rel)) {
                    if !self.budget.tick() {
                        return;
                    }
                    let kind = match entry.kind {
                        LinkKind::File => Kind::File,
                        LinkKind::Dir => Kind::Dir,
                        LinkKind::Link => {
                            self.links.push(format!("{rel}/{}", entry.name));
                            continue;
                        }
                        LinkKind::Other => Kind::Other,
                    };
                    self.visit(base, rel, depth, max, &entry.name, entry.path, kind);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        &mut self,
        base: &Path,
        rel: &str,
        depth: usize,
        max: usize,
        name: &str,
        path: PathBuf,
        kind: Kind,
    ) {
        let child = format!("{rel}/{name}");
        match kind {
            Kind::File => {
                if let Some((kind, fallback)) = classify(&child) {
                    self.out.push(Found {
                        kind,
                        fallback,
                        rel: child,
                        path,
                    });
                }
            }
            Kind::Dir if depth + 1 < max => self.walk(base, &child, depth + 1, max),
            _ => {}
        }
    }
}

/// The items below `base` (a folder holding `skills/`, `commands/`, `agents/`, `rules/`), in
/// kind and name order. Skill folders are entered one level only; nothing else is searched.
pub fn scan_layout(base: &Path, budget: &Budget) -> Vec<Found> {
    scan_layout_as(base, budget, Trust::Own, &mut Vec::new())
}

/// [`scan_layout`] for a checkout somebody else wrote: links are not followed, and every one that
/// was left out is named in `links`.
pub fn scan_layout_untrusted(base: &Path, budget: &Budget, links: &mut Vec<String>) -> Vec<Found> {
    scan_layout_as(base, budget, Trust::Untrusted, links)
}

pub fn scan_layout_as(
    base: &Path,
    budget: &Budget,
    trust: Trust,
    links: &mut Vec<String>,
) -> Vec<Found> {
    let mut scan = Scan {
        trust,
        budget,
        out: Vec::new(),
        links,
    };
    for top in CONTENT_DIRS {
        let path = base.join(top);
        let present = match trust {
            Trust::Own => fsx::is_dir(&path),
            Trust::Untrusted => {
                if untrusted::kind(&path) == Some(LinkKind::Link) {
                    scan.links.push(top.to_string());
                }
                untrusted::is_dir(&path)
            }
        };
        if budget.spent() || !present {
            continue;
        }
        let max = if top == "skills" { 2 } else { NESTED_DEPTH };
        scan.walk(base, top, 0, max);
    }
    scan.out
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
