//! The rows `context loads` adds on top of the memory/rules/settings/MCP/skill walk: `@imports`,
//! the auto-memory index, account and plugin skills, user and project commands and agents, and
//! the files below the working directory that load only when Claude reads there. Plugin and
//! account contents come from the `sources` detectors (MIG-SRC-1), which are read-only.

use super::layers::is_managed_local;
use super::loads::{excluded, show, skill_dirs, text_tokens, Ctx, LoadItem};
use crate::plus::sources::budget::{Budget, DETECTOR_TIME, MAX_DEPTH};
use crate::plus::sources::cache::Cache;
use crate::plus::sources::fsx::{self, Kind};
use crate::plus::sources::item;
use crate::plus::sources::layout;
use crate::plus::sources::model::Origin;
use crate::plus::sources::scope::RootSet;
use crate::plus::sources::{scan_one, DetectorOutput, ScanCtx};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

const MAX_HOPS: usize = 4;
const MAX_IMPORTS: usize = 200;
const MEMORY_INDEX_LINES: usize = 200;
const MEMORY_INDEX_BYTES: usize = 25 * 1024;
const NESTED_ENTRIES: usize = 20_000;
const SKIP_DIRS: [&str; 9] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "venv",
    "__pycache__",
    "repos",
    "vendor",
    "site-packages",
];
const DEFAULT_INERT: &str = "_claude";

/// The nearest folder at or above `dir` that holds a `.git` entry (a directory, or the file a
/// linked worktree has).
pub(super) fn git_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

pub(super) fn dir_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

pub(super) fn carries_managed_marker(text: &str) -> bool {
    is_managed_local(text)
}

/// True when the corporate tools clone ships a CLAUDE.md, so `~/.claude/CLAUDE.md` is theirs.
pub(super) fn org_provides_claude_md(ctx: &Ctx) -> bool {
    let claude = ctx.roots.resolve_corp_tools_dir(ctx.config).join("claude");
    ["CLAUDE.md", "CLAUDE.uncompressed.md"]
        .iter()
        .any(|f| fsx::is_file(&claude.join(f)))
}

fn strip_code(text: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let mut inline = false;
        for c in line.chars() {
            if c == '`' {
                inline = !inline;
            } else if !inline {
                out.push(c);
            }
        }
        out.push('\n');
    }
    out
}

fn import_tokens(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for word in strip_code(text).split_whitespace() {
        let Some(rest) = word.strip_prefix('@') else {
            continue;
        };
        let rest = rest.trim_end_matches(['.', ',', ';', ':', ')', '!', '?', '"', '\'']);
        let pathish = rest.contains('/') || rest.contains('.') || rest.starts_with('~');
        if !rest.is_empty() && pathish && !found.iter().any(|f| f == rest) {
            found.push(rest.to_string());
        }
    }
    found
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn resolve_import(ctx: &Ctx, parent: &Path, token: &str) -> PathBuf {
    if let Some(rest) = token.strip_prefix("~/") {
        return normalize(&ctx.roots.home.join(rest));
    }
    let raw = Path::new(token);
    if raw.is_absolute() {
        return normalize(raw);
    }
    normalize(&parent.parent().unwrap_or(parent).join(raw))
}

/// Lists the files `parent` pulls in with `@path`, then the files those pull in, up to four hops.
/// Each row's `via` is the chain of files above it, outermost first, ending with its importer.
pub(super) fn follow_imports(
    ctx: &mut Ctx,
    parent: &Path,
    text: &str,
    via: &[String],
    source: &'static str,
    owner: &(Origin, bool),
) {
    if via.len() >= MAX_HOPS {
        return;
    }
    let mut chain = via.to_vec();
    chain.push(show(parent));
    for token in import_tokens(text) {
        if ctx.items.iter().filter(|i| i.kind == "import").count() >= MAX_IMPORTS {
            ctx.notes
                .push(format!("stopped following @imports after {MAX_IMPORTS}"));
            return;
        }
        let target = resolve_import(ctx, parent, &token);
        if chain.iter().any(|c| Path::new(c) == target) || !fsx::is_file(&target) {
            continue;
        }
        let Some(body) = fsx::read_text(&target, fsx::TEXT_CAP) else {
            continue;
        };
        ctx.push(
            LoadItem::new(
                "import",
                token,
                show(&target),
                source,
                format!("@import, hop {}", chain.len()),
                text_tokens(&body),
            )
            .from(owner.0.clone(), owner.1)
            .via(chain.clone()),
        );
        follow_imports(ctx, &target, &body, &chain, source, owner);
    }
}

fn slug(path: &Path) -> String {
    show(path)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The auto-memory index of the project: `MEMORY.md`, of which Claude Code loads the first 200
/// lines or 25 KB.
pub(super) fn memory_index(ctx: &mut Ctx) {
    let project = git_root(ctx.cwd).unwrap_or_else(|| ctx.cwd.to_path_buf());
    let mut keys = vec![project.clone(), fsx::canonical(&project), ctx.cwd.to_path_buf()];
    keys.dedup();
    for key in keys {
        let path = ctx
            .roots
            .claude_home
            .join("projects")
            .join(slug(&key))
            .join("memory")
            .join("MEMORY.md");
        let Some(text) = fsx::read_text(&path, fsx::TEXT_CAP) else {
            continue;
        };
        let total = text.lines().count();
        let mut kept = String::new();
        let mut lines = 0;
        for line in text.lines().take(MEMORY_INDEX_LINES) {
            if kept.len() + line.len() + 1 > MEMORY_INDEX_BYTES {
                break;
            }
            kept.push_str(line);
            kept.push('\n');
            lines += 1;
        }
        let origin = Origin::new("user", show(&ctx.roots.claude_home));
        ctx.push(
            LoadItem::new(
                "memory-index",
                "MEMORY.md",
                show(&path),
                "user",
                format!(
                    "first {lines} of {total} lines (Claude Code loads at most {MEMORY_INDEX_LINES} lines or 25 KB)"
                ),
                text_tokens(&kept),
            )
            .from(origin, false),
        );
        return;
    }
}

/// The `plugin` detector's view of the installed plugins: file reads only, with the detector
/// budget. Whether a plugin is switched on is decided by the settings layers, not by the detector.
pub(super) fn scan_plugins(ctx: &mut Ctx) -> DetectorOutput {
    let cache = Cache::memory();
    let scope = RootSet::default();
    let now = fsx::now_zulu();
    let budget = Budget::new(DETECTOR_TIME, MAX_DEPTH, None);
    let out = scan_one(
        "plugin",
        &ScanCtx {
            roots: ctx.roots,
            config: ctx.config,
            data_dir: None,
            cwd: None,
            scope: &scope,
            budget: &budget,
            cache: &cache,
            now: &now,
        },
    );
    ctx.partial |= budget.hit().is_some_and(|r| !r.starts_with("depth"));
    out
}

/// One row per installed plugin. Returns the (name, tokens) of the skills of every enabled plugin,
/// which the skill-list budget needs.
pub(super) fn plugins(ctx: &mut Ctx, found: &DetectorOutput) -> Vec<(String, u64)> {
    let mut listed = Vec::new();
    for source in &found.sources {
        let id = source.origin.name.clone();
        let mine: Vec<_> = found
            .items
            .iter()
            .filter(|i| i.source_id == source.id)
            .collect();
        let count = |kind: &str| mine.iter().filter(|i| i.kind == kind).count();
        let tokens: u64 = mine
            .iter()
            .filter(|i| matches!(i.kind, "skill" | "agent" | "command"))
            .map(|i| i.tokens.value)
            .sum();
        let switch = ctx.plugin_switches.get(&id).cloned();
        let mut row = LoadItem::new(
            "plugin",
            id.clone(),
            source.root.clone().unwrap_or_default(),
            "plugin",
            format!(
                "{} skills, {} agents, {} commands",
                count("skill"),
                count("agent"),
                count("command")
            ),
            tokens,
        )
        .from(source.origin.clone(), false);
        if source.status.state == "unreachable" {
            row.loaded = false;
            row.reason = source.status.detail.clone();
        } else {
            match switch {
                Some((true, _)) => {
                    for item in mine.iter().filter(|i| i.kind == "skill") {
                        listed.push((format!("{id}:{}", item.name), item.tokens.value));
                    }
                }
                Some((false, layer)) => {
                    row.loaded = false;
                    row.reason = format!("turned off in {layer}");
                }
                None => {
                    row.loaded = false;
                    row.reason = "not enabled in any settings file".into();
                }
            }
        }
        ctx.push(row);
    }
    listed
}

/// What the skills library carries: `commands/<n>.md` and `agents/<n>/`, as `kind:name`.
fn library_names(library: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for entry in fsx::list_dir(&library.join("commands")) {
        if let Some(stem) = entry.name.strip_suffix(".md") {
            names.insert(format!("command:{stem}"));
        }
    }
    for entry in fsx::list_dir(&library.join("agents")) {
        if entry.kind == Kind::Dir {
            names.insert(format!("agent:{}", entry.name));
        }
    }
    names
}

/// Commands and agents: `~/.claude/{commands,agents}` and the `.claude` folders on the way down
/// to the working directory. A user-level file nobody manages is `loose`, never `org`.
pub(super) fn commands_and_agents(ctx: &mut Ctx) {
    let budget = Budget::new(DETECTOR_TIME, MAX_DEPTH, None);
    let clone = ctx.roots.resolve_corp_tools_dir(ctx.config).join("claude");
    let org_commands: BTreeSet<String> = layout::scan_layout(&clone, &budget)
        .into_iter()
        .filter(|f| f.kind == "command")
        .map(|f| f.rel)
        .collect();
    let library = library_names(&ctx.roots.skills_repo_path());
    let mut bases: Vec<(PathBuf, bool)> = vec![(ctx.roots.claude_home.clone(), true)];
    for dir in ctx.cwd.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if dir != ctx.roots.home {
            bases.push((dir.join(".claude"), false));
        }
    }
    for (base, user) in bases {
        for found in layout::scan_layout(&base, &budget) {
            if !matches!(found.kind, "command" | "agent") {
                continue;
            }
            let Some(text) = fsx::read_text(&found.path, fsx::TEXT_CAP) else {
                continue;
            };
            let parsed = item::parse_text(&text, text.len() as u64, true);
            let name = item::name_for(found.kind, &parsed, &found.fallback);
            let tokens = item::tokens_for(found.kind, &name, &parsed);
            let key = format!("{}:{}", found.kind, found.fallback);
            let (source, origin, writable) = if !user {
                let (origin, writable) = ctx.project_origin(&found.path);
                ("project", origin, writable)
            } else if found.kind == "command" && org_commands.contains(&found.rel) {
                ("org", Origin::new("org", ctx.corp_name.clone()), false)
            } else if library.contains(&key) {
                (
                    "personal",
                    Origin::new("library", ctx.library_name.clone()),
                    true,
                )
            } else if carries_managed_marker(&text) {
                ("personal", Origin::new("managed", "toolportctl"), false)
            } else {
                (
                    "loose",
                    Origin::new("loose", show(&ctx.roots.claude_home)),
                    true,
                )
            };
            let reason = if found.kind == "command" {
                "description loads at start; body when the command runs"
            } else {
                "name and description load at start; body when the agent runs"
            };
            ctx.push(
                LoadItem::new(found.kind, name, show(&found.path), source, reason, tokens)
                    .from(origin, writable),
            );
        }
    }
    ctx.partial |= budget.hit().is_some_and(|r| !r.starts_with("depth"));
}

fn inert(ctx: &Ctx, name: &str) -> bool {
    name == DEFAULT_INERT
        || ctx
            .config
            .inert_patterns
            .iter()
            .map(|p| p.trim_end_matches("/**").trim_end_matches('/'))
            .any(|p| p == name)
}

fn walk(ctx: &Ctx, dir: &Path, depth: usize, budget: &Budget, out: &mut Vec<PathBuf>) {
    for entry in fsx::list_dir(dir) {
        if !budget.tick() {
            return;
        }
        if entry.kind != Kind::Dir
            || entry.symlink
            || entry.name.starts_with('.')
            || SKIP_DIRS.contains(&entry.name.as_str())
            || inert(ctx, &entry.name)
        {
            continue;
        }
        out.push(entry.path.clone());
        if depth + 1 < MAX_DEPTH {
            walk(ctx, &entry.path, depth + 1, budget, out);
        }
    }
}

fn relative(ctx: &Ctx, path: &Path) -> String {
    path.strip_prefix(ctx.cwd)
        .map_or_else(|_| show(path), |p| p.display().to_string())
}

/// Files below the working directory: a CLAUDE.md or a project skill there loads when Claude
/// reads a file in that folder. They are listed, never counted in the total.
pub(super) fn nested(ctx: &mut Ctx, effective: &Map<String, Value>) {
    let budget = Budget::new(DETECTOR_TIME, MAX_DEPTH, Some(NESTED_ENTRIES));
    let mut dirs = Vec::new();
    walk(ctx, ctx.cwd, 0, &budget, &mut dirs);
    for dir in dirs {
        for rel in ["CLAUDE.md", ".claude/CLAUDE.md"] {
            let path = dir.join(rel);
            let Some(text) = fsx::read_text(&path, fsx::TEXT_CAP) else {
                continue;
            };
            let (origin, writable) = ctx.project_origin(&path);
            let mut row = LoadItem::new(
                "memory",
                relative(ctx, &path),
                show(&path),
                "project",
                "",
                text_tokens(&text),
            )
            .from(origin, writable)
            .on_demand(format!("loads when Claude reads a file in {}", relative(ctx, &dir)));
            if excluded(effective, &path) {
                row.lazy = false;
                row.reason = "excluded by claudeMdExcludes".into();
            }
            ctx.push(row);
        }
        for (dirname, skill_dir) in skill_dirs(&dir.join(".claude/skills")) {
            let skill_md = skill_dir.join("SKILL.md");
            let Some(text) = fsx::read_text(&skill_md, fsx::TEXT_CAP) else {
                continue;
            };
            let parsed = item::parse_text(&text, text.len() as u64, false);
            let name = item::name_for("skill", &parsed, &dirname);
            let tokens = item::tokens_for("skill", &name, &parsed);
            let (origin, writable) = ctx.project_origin(&skill_md);
            ctx.push(
                LoadItem::new("skill", name, show(&skill_md), "project", "", tokens)
                    .from(origin, writable)
                    .visible(Some(parsed.accepted))
                    .on_demand(format!(
                        "loads when Claude reads a file in {}",
                        relative(ctx, &dir)
                    )),
            );
        }
    }
    if budget.hit().is_some_and(|r| !r.starts_with("depth")) {
        ctx.partial = true;
        ctx.notes.push(format!(
            "stopped looking for nested files below {}",
            show(ctx.cwd)
        ));
    }
}
