//! What a Claude Code session in a folder loads, as rows with an estimated cost. This is the seam
//! the bundle and leave-out code reads: the instruction files are itemised by [`InstructionLoads`];
//! rows for skills, plugins and agents join through another [`LoadSource`].

use super::fsx;
use super::globs::glob_match;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Origin {
    pub kind: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadItem {
    /// `memory`, `import`, `rule`, `skill`, `agent`, ...
    pub kind: &'static str,
    pub name: String,
    pub path: Option<String>,
    /// `policy`, `org`, `user`, `project`, `local`, `plugin`.
    pub source: &'static str,
    pub origin: Origin,
    pub loaded: bool,
    pub lazy: bool,
    pub reason: String,
    pub tokens: u64,
    /// Who keeps a left-out row out: `toolport` or `settings`.
    pub left_out_by: Option<&'static str>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WhatLoads {
    pub items: Vec<LoadItem>,
    pub total_tokens: u64,
}

pub trait LoadSource {
    fn what_loads(&self, cwd: &Path) -> Result<WhatLoads, String>;

    /// Skill names a session in `cwd` is offered, when a measurement knows them.
    fn visible_skills(&self, _cwd: &Path) -> Vec<String> {
        Vec::new()
    }
}

pub struct NoLoads;

impl LoadSource for NoLoads {
    fn what_loads(&self, _cwd: &Path) -> Result<WhatLoads, String> {
        Ok(WhatLoads::default())
    }
}

const MAX_IMPORT_HOPS: usize = 4;
const MAX_ITEMS: usize = 400;
const MEMORY_INDEX_LINES: usize = 200;
const MEMORY_INDEX_BYTES: usize = 25 * 1024;
pub const POLICY_CLAUDE_MD: &str = "/Library/Application Support/ClaudeCode/CLAUDE.md";
pub const AUTO_MEMORY_OFF: &str = "auto memory is off (autoMemoryEnabled)";

/// Itemises the instruction files a session in a folder loads: user, parent folders and the
/// folder's own `CLAUDE.md`, `CLAUDE.local.md`, rules and `@imports`, and the auto memory index.
/// Skills, plugins, agents and tools are not rows here; they stay in the part of a measured total
/// that is not itemised.
pub struct InstructionLoads<'a> {
    pub home: &'a Path,
    pub policy_file: Option<&'a Path>,
    pub count_tokens: &'a dyn Fn(&str) -> u64,
}

#[derive(Default)]
struct Walk {
    items: Vec<LoadItem>,
    seen: BTreeSet<PathBuf>,
    settings: Settings,
}

#[derive(Default)]
struct Settings {
    excludes: Vec<String>,
    memory: Option<bool>,
}

fn read_settings(path: &Path, into: &mut Settings) {
    let Some(text) = fsx::read_text(path, fsx::TEXT_CAP) else {
        return;
    };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    if let Some(list) = map.get("claudeMdExcludes").and_then(Value::as_array) {
        into.excludes
            .extend(list.iter().filter_map(|v| v.as_str().map(String::from)));
    }
    if let Some(on) = map.get("autoMemoryEnabled").and_then(Value::as_bool) {
        into.memory = Some(on);
    }
}

fn has_paths_front_matter(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("---") else {
        return false;
    };
    rest.lines()
        .skip(1)
        .take_while(|line| line.trim_end() != "---")
        .any(|line| {
            line.trim_start().starts_with("paths:") || line.trim_start().starts_with("globs:")
        })
}

fn import_tokens(text: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for word in line.split_whitespace() {
            if let Some(path) = word.strip_prefix('@') {
                let path = path.trim_end_matches(['.', ',', ';', ')']);
                if path.starts_with('/')
                    || path.starts_with("~/")
                    || path.starts_with("./")
                    || path.contains('/')
                {
                    out.push(path.to_string());
                }
            }
        }
    }
    out
}

fn markdown_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    for entry in fsx::list_dir(dir) {
        match entry.kind {
            fsx::Kind::Dir => markdown_files(&entry.path, depth + 1, out),
            fsx::Kind::File if entry.name.ends_with(".md") => out.push(entry.path),
            _ => {}
        }
    }
}

fn project_slug(cwd: &Path) -> String {
    fsx::display(cwd)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

impl InstructionLoads<'_> {
    fn row(
        &self,
        kind: &'static str,
        source: &'static str,
        path: &Path,
        text: &str,
        lazy: bool,
        settings: &Settings,
    ) -> LoadItem {
        let shown = fsx::display(path);
        let excluded =
            source != "policy" && settings.excludes.iter().any(|g| glob_match(g, &shown));
        LoadItem {
            kind,
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: Some(shown),
            source,
            origin: Origin {
                kind: source.into(),
                name: String::new(),
            },
            loaded: !excluded && !lazy,
            lazy: lazy && !excluded,
            reason: if excluded {
                "excluded by claudeMdExcludes".into()
            } else {
                String::new()
            },
            tokens: (self.count_tokens)(text),
            left_out_by: None,
        }
    }

    fn push_file(
        &self,
        walk: &mut Walk,
        kind: &'static str,
        source: &'static str,
        path: &Path,
        hop: usize,
    ) {
        if walk.items.len() >= MAX_ITEMS {
            return;
        }
        let canonical = fsx::canonical(path);
        if !fsx::is_file(path) || !walk.seen.insert(canonical) {
            return;
        }
        let Some(text) = fsx::read_text(path, fsx::TEXT_CAP) else {
            return;
        };
        let lazy = kind == "rule" && has_paths_front_matter(&text);
        let row = self.row(kind, source, path, &text, lazy, &walk.settings);
        let followed = row.loaded;
        walk.items.push(row);
        if !followed || hop >= MAX_IMPORT_HOPS {
            return;
        }
        let base = path.parent().unwrap_or(Path::new("/"));
        for token in import_tokens(&text) {
            let target = match token.strip_prefix("~/") {
                Some(rest) => self.home.join(rest),
                None if token.starts_with('/') => PathBuf::from(&token),
                None => base.join(&token),
            };
            self.push_file(walk, "import", source, &target, hop + 1);
        }
    }
}

impl LoadSource for InstructionLoads<'_> {
    fn what_loads(&self, cwd: &Path) -> Result<WhatLoads, String> {
        if let Some(problem) = fsx::folder_problem(cwd) {
            return Err(problem);
        }
        let cwd = fsx::canonical(cwd);
        let user_claude = self.home.join(".claude");
        let mut settings = Settings::default();
        read_settings(&user_claude.join("settings.json"), &mut settings);
        read_settings(&cwd.join(".claude/settings.json"), &mut settings);
        read_settings(&cwd.join(".claude/settings.local.json"), &mut settings);
        let mut walk = Walk {
            items: Vec::new(),
            seen: BTreeSet::new(),
            settings,
        };
        if let Some(policy) = self.policy_file {
            self.push_file(&mut walk, "memory", "policy", policy, 0);
        }
        self.push_file(
            &mut walk,
            "memory",
            "user",
            &user_claude.join("CLAUDE.md"),
            0,
        );
        let mut rules = Vec::new();
        markdown_files(&user_claude.join("rules"), 0, &mut rules);
        for rule in rules {
            self.push_file(&mut walk, "rule", "user", &rule, 0);
        }
        let mut chain: Vec<&Path> = cwd.ancestors().collect();
        chain.reverse();
        for dir in chain {
            for (file, source) in [
                ("CLAUDE.md", "project"),
                (".claude/CLAUDE.md", "project"),
                ("CLAUDE.local.md", "local"),
            ] {
                self.push_file(&mut walk, "memory", source, &dir.join(file), 0);
            }
            let mut rules = Vec::new();
            markdown_files(&dir.join(".claude/rules"), 0, &mut rules);
            for rule in rules {
                self.push_file(&mut walk, "rule", "project", &rule, 0);
            }
        }
        let Walk {
            mut items,
            settings,
            ..
        } = walk;
        let index = user_claude
            .join("projects")
            .join(project_slug(&cwd))
            .join("memory/MEMORY.md");
        if let Some(text) = fsx::read_text(&index, MEMORY_INDEX_BYTES) {
            let text: String = text
                .lines()
                .take(MEMORY_INDEX_LINES)
                .collect::<Vec<_>>()
                .join("\n");
            let off = settings.memory == Some(false);
            items.push(LoadItem {
                kind: "memory-index",
                name: "MEMORY.md".into(),
                path: Some(fsx::display(&index)),
                source: "user",
                origin: Origin {
                    kind: "user".into(),
                    name: String::new(),
                },
                loaded: !off,
                lazy: false,
                reason: if off {
                    AUTO_MEMORY_OFF.into()
                } else {
                    String::new()
                },
                tokens: (self.count_tokens)(&text),
                left_out_by: None,
            });
        }
        let total_tokens = items.iter().filter(|i| i.loaded).map(|i| i.tokens).sum();
        Ok(WhatLoads {
            items,
            total_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn loads(home: &Path, cwd: &Path) -> WhatLoads {
        let count = |text: &str| text.len() as u64;
        InstructionLoads {
            home,
            policy_file: None,
            count_tokens: &count,
        }
        .what_loads(cwd)
        .unwrap()
    }

    #[test]
    fn rows_follow_load_order_and_imports_count_with_their_parent() {
        let dir = tempfile::tempdir().unwrap();
        let home = fsx::canonical(dir.path()).join("home");
        let cwd = home.join("work/app");
        put(&home.join(".claude/CLAUDE.md"), "user");
        put(&home.join("work/CLAUDE.md"), "parent @docs/a.md");
        put(&home.join("work/docs/a.md"), "imported");
        put(&cwd.join("CLAUDE.md"), "repo");
        put(
            &cwd.join(".claude/rules/r.md"),
            "---\npaths: src/**\n---\nlazy",
        );
        let report = loads(&home, &cwd);
        let names: Vec<(&str, &str)> = report
            .items
            .iter()
            .map(|i| (i.kind, i.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("memory", "CLAUDE.md"),
                ("memory", "CLAUDE.md"),
                ("import", "a.md"),
                ("memory", "CLAUDE.md"),
                ("rule", "r.md")
            ]
        );
        assert!(report.items[4].lazy && !report.items[4].loaded);
        assert_eq!(
            report.total_tokens,
            ("user".len() + "parent @docs/a.md".len() + "imported".len() + "repo".len()) as u64
        );
    }

    #[test]
    fn claude_md_excludes_remove_a_file_and_say_why() {
        let dir = tempfile::tempdir().unwrap();
        let home = fsx::canonical(dir.path()).join("home");
        let cwd = home.join("app");
        put(&home.join(".claude/CLAUDE.md"), "user");
        put(&cwd.join("CLAUDE.md"), "repo");
        let user_file = fsx::display(&home.join(".claude/CLAUDE.md"));
        put(
            &cwd.join(".claude/settings.local.json"),
            &format!("{{\"claudeMdExcludes\": [\"{user_file}\"], \"autoMemoryEnabled\": false}}"),
        );
        put(
            &home.join(format!(
                ".claude/projects/{}/memory/MEMORY.md",
                project_slug(&cwd)
            )),
            "index",
        );
        let report = loads(&home, &cwd);
        let user = report
            .items
            .iter()
            .find(|i| i.source == "user" && i.kind == "memory")
            .unwrap();
        assert!(!user.loaded && user.reason.starts_with("excluded"));
        let index = report
            .items
            .iter()
            .find(|i| i.kind == "memory-index")
            .unwrap();
        assert!(!index.loaded && index.reason == AUTO_MEMORY_OFF);
        assert_eq!(report.total_tokens, "repo".len() as u64);
    }

    #[test]
    fn an_import_cycle_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let home = fsx::canonical(dir.path()).join("home");
        put(&home.join("CLAUDE.md"), "@./b/x.md");
        put(&home.join("b/x.md"), "@../CLAUDE.md");
        assert_eq!(loads(&home, &home).items.len(), 2);
    }
}
