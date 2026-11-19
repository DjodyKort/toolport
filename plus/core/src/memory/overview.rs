//! Every repository at once: auto memory, instruction files and sessions in one row, plus the
//! transcript folders that belong to no repository Toolport knows. Repositories come from the
//! folders Claude Code has run in (`~/.claude.json`) and the source roots of `context.json`.

use super::claude_json::Known;
use super::inventory;
use super::resolve::{Layers, Repo};
use super::sessions::{self, Retention};
use super::World;
use crate::context::fsx;
use crate::context::loads::{InstructionLoads, LoadSource};
use crate::context::Roots;
use crate::op::OpError;
use crate::sources::{config, scope, Places};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const LARGE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_REPOS: usize = 400;
const OTHER_ITEMS: usize = 60;
const STALE_DAYS: i64 = 90;
pub const FILTERS: [&str; 4] = ["memory-off", "has-local-file", "expiring", "large"];
pub const SORTS: [&str; 5] = ["name", "memory", "sessions", "size", "active"];

fn candidate_paths(world: &World, known: &Known) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = known
        .folders
        .iter()
        .filter(|p| fsx::is_dir(p))
        .cloned()
        .collect();
    let roots = Roots::from_home(world.home);
    let cfg = config::load(&roots.context_config_path());
    let places = Places::from_env(&roots, &cfg);
    let mut source_roots = scope::default_roots(&places);
    source_roots.extend(scope::config_roots(&roots, &cfg));
    for root in source_roots.into_iter().filter(|p| fsx::is_dir(p)) {
        if scope::has_dot_git(&root) {
            out.push(root.clone());
        }
        for child in fsx::list_dir(&root) {
            if child.kind == fsx::Kind::Dir
                && !child.symlink
                && !child.name.starts_with('.')
                && scope::has_dot_git(&child.path)
            {
                out.push(child.path);
            }
        }
    }
    out
}

/// The repositories Toolport knows of, by main checkout, sorted by name.
pub fn discover(world: &World, known: &Known) -> Vec<Repo> {
    let mut seen = BTreeSet::new();
    let mut repos = Vec::new();
    for path in candidate_paths(world, known) {
        let repo = Repo::of(&path);
        if !repo.git || !seen.insert(repo.root.clone()) {
            continue;
        }
        repos.push(Repo::of(&repo.root));
        if repos.len() >= MAX_REPOS {
            break;
        }
    }
    repos.sort_by(|a, b| a.name().cmp(&b.name()).then_with(|| a.root.cmp(&b.root)));
    repos
}

pub fn repositories_with_own_switch(world: &World) -> usize {
    let known = Known::read(world.claude_json);
    discover(world, &known)
        .iter()
        .filter(|repo| world.layers(repo).switch().is_some())
        .count()
}

struct Built {
    row: Value,
    memory_bytes: u64,
    memory_files: usize,
    sessions: usize,
    session_bytes: u64,
    expiring: usize,
    dirs: Vec<PathBuf>,
    off: bool,
    local: bool,
    last_active: Option<i64>,
    total: u64,
}

fn build(world: &World, repo: &Repo, known: &Known, retention: &Retention) -> Built {
    let layers = world.layers(repo);
    let (enabled, by) = layers.effective();
    let (dir, custom) = layers.directory(world.home, world.claude_dir, repo);
    let files = inventory::files(&dir);
    let memory = inventory::summary(&files);
    let loads = InstructionLoads {
        home: world.home,
        claude_dir: world.claude_dir,
        policy_file: world.policy_claude_md,
        count_tokens: world.count_tokens,
    };
    let found = loads.what_loads(&repo.root).unwrap_or_default();
    let counted = found.items.iter().filter(|i| i.kind != "memory-index");
    let (mut excluded, mut local, mut imports, mut count) = (0, 0, 0, 0);
    for item in counted {
        count += 1;
        excluded += usize::from(!item.loaded && !item.lazy && item.reason.starts_with("excluded"));
        local += usize::from(item.source == "local");
        imports += usize::from(item.kind == "import");
    }
    let folders = sessions::folders_of(world.claude_dir, repo, known);
    let all = folders.iter().flat_map(|f| f.sessions.iter());
    let totals = sessions::totals(all, retention.days, world.now);
    let mut flags: Vec<&str> = Vec::new();
    if local > 0 {
        flags.push("local file");
    }
    if excluded > 0 {
        flags.push("excluded");
    }
    if imports > 0 {
        flags.push("imports");
    }
    let last_active = [
        memory.last_written,
        folders
            .iter()
            .flat_map(|f| f.sessions.iter())
            .map(|s| s.modified)
            .max(),
    ]
    .into_iter()
    .flatten()
    .max();
    let total = memory.bytes + totals.bytes;
    let row = json!({
        "name": repo.name(),
        "root": fsx::display(&repo.root),
        "key": repo.slug(),
        "worktrees": repo.worktrees.len().saturating_sub(1),
        "memory": {
            "enabled": enabled,
            "switch": super::switch::Position::of(layers.switch()).as_str(),
            "setBy": by.as_str(),
            "locked": layers.managed.enabled.is_some(),
            "directory": fsx::display(&dir),
            "custom": custom,
            "exists": !files.is_empty(),
            "files": memory.count,
            "bytes": memory.bytes,
            "lastWritten": memory.last_written.map(fsx::zulu),
        },
        "instructions": {
            "files": count,
            "tokens": found.total_tokens,
            "excluded": excluded,
            "localFiles": local,
            "imports": imports,
            "flags": flags,
            "basis": "estimate",
        },
        "sessions": sessions::totals_json(&totals),
        "bytes": total,
        "lastActive": last_active.map(fsx::zulu),
    });
    Built {
        row,
        memory_bytes: memory.bytes,
        memory_files: memory.count,
        sessions: totals.count,
        session_bytes: totals.bytes,
        expiring: totals.expiring,
        dirs: folders.iter().filter_map(|f| f.dir.clone()).collect(),
        off: !enabled,
        local: local > 0,
        last_active,
        total,
    }
}

fn keep(filter: Option<&str>, b: &Built) -> bool {
    match filter {
        None => true,
        Some("memory-off") => b.off,
        Some("has-local-file") => b.local,
        Some("expiring") => b.expiring > 0,
        Some("large") => b.total >= LARGE_BYTES,
        Some(_) => true,
    }
}

fn other_folders(world: &World, claimed: &BTreeSet<PathBuf>) -> (Value, u64, usize, u64) {
    let projects = world.claude_dir.join("projects");
    let (mut items, mut count, mut bytes, mut memory) = (Vec::new(), 0usize, 0u64, 0u64);
    for entry in fsx::list_dir(&projects) {
        if entry.kind != fsx::Kind::Dir || claimed.contains(&entry.path) {
            continue;
        }
        let found = sessions::scan_dir(&entry.path, &entry.path);
        let session_bytes: u64 = found.iter().map(|s| s.bytes).sum();
        let memory_bytes = super::store::size_of(&entry.path.join("memory"));
        count += found.len();
        bytes += session_bytes;
        memory += memory_bytes;
        if items.len() < OTHER_ITEMS {
            items.push(json!({
                "key": entry.name,
                "sessions": found.len(),
                "bytes": session_bytes,
                "memoryBytes": memory_bytes,
                "oldest": found.iter().map(|s| s.modified).min().map(fsx::zulu),
            }));
        }
    }
    (json!(items), bytes, count, memory)
}

pub struct Options<'a> {
    pub all: bool,
    pub cwd: Option<&'a Path>,
    pub sort: &'a str,
    pub filter: Option<&'a str>,
}

pub fn overview(world: &World, opts: &Options) -> Result<Value, OpError> {
    if !SORTS.contains(&opts.sort) {
        return Err(OpError::usage(format!(
            "--sort must be one of {}",
            SORTS.join(", ")
        )));
    }
    if let Some(filter) = opts.filter.filter(|f| !FILTERS.contains(f)) {
        return Err(OpError::usage(format!(
            "unknown filter {filter}; use one of {}",
            FILTERS.join(", ")
        )));
    }
    let known = Known::read(world.claude_json);
    let retention = sessions::retention(world.claude_dir);
    let repos = match (opts.all, opts.cwd) {
        (false, Some(cwd)) => {
            if let Some(problem) = fsx::folder_problem(cwd) {
                return Err(OpError::usage(format!("cwd {problem}")));
            }
            vec![Repo::of(cwd)]
        }
        _ => discover(world, &known),
    };
    let mut built: Vec<Built> = repos
        .iter()
        .map(|repo| build(world, repo, &known, &retention))
        .collect();
    let claimed: BTreeSet<PathBuf> = built.iter().flat_map(|b| b.dirs.iter().cloned()).collect();
    match opts.sort {
        "memory" => built.sort_by_key(|b| std::cmp::Reverse(b.memory_bytes)),
        "sessions" => built.sort_by_key(|b| std::cmp::Reverse(b.session_bytes)),
        "size" => built.sort_by_key(|b| std::cmp::Reverse(b.total)),
        "active" => built.sort_by_key(|b| std::cmp::Reverse(b.last_active)),
        _ => {}
    }
    let listed: Vec<&Built> = built.iter().filter(|b| keep(opts.filter, b)).collect();
    let mut totals = json!({
        "repositories": listed.len(),
        "memoryFiles": listed.iter().map(|b| b.memory_files).sum::<usize>(),
        "memoryBytes": listed.iter().map(|b| b.memory_bytes).sum::<u64>(),
        "sessions": listed.iter().map(|b| b.sessions).sum::<usize>(),
        "sessionBytes": listed.iter().map(|b| b.session_bytes).sum::<u64>(),
        "expiringThisWeek": listed.iter().map(|b| b.expiring).sum::<usize>(),
        "bytes": listed.iter().map(|b| b.total).sum::<u64>(),
    });
    let mut other = json!(null);
    if opts.all {
        let (items, bytes, count, memory) = other_folders(world, &claimed);
        other = json!({
            "folders": items.as_array().map_or(0, Vec::len),
            "sessions": count,
            "sessionBytes": bytes,
            "memoryBytes": memory,
            "items": items,
        });
    }
    let layers = Layers::load(
        world.claude_dir,
        &Repo::of(world.home),
        world.managed_settings,
        world.env_disable.clone(),
    );
    totals["asOf"] = json!(fsx::zulu(world.now));
    Ok(json!({
        "scope": if opts.all { "all" } else { "repository" },
        "rows": listed.iter().map(|b| b.row.clone()).collect::<Vec<_>>(),
        "other": other,
        "totals": totals,
        "retention": sessions::retention_json(&retention),
        "global": { "enabled": layers.user.enabled, "file": fsx::display(&layers.user_file) },
        "sort": opts.sort,
        "filter": opts.filter,
        "filters": FILTERS,
        "sorts": SORTS,
        "staleDays": STALE_DAYS,
    }))
}

/// Every repository with auto memory kept, for the all-folders sheet.
pub fn memory_all(world: &World) -> Value {
    let known = Known::read(world.claude_json);
    let repos = discover(world, &known);
    let mut claimed: BTreeSet<PathBuf> = BTreeSet::new();
    let mut rows = Vec::new();
    let mut own = 0;
    for repo in &repos {
        let layers = world.layers(repo);
        let (dir, custom) = layers.directory(world.home, world.claude_dir, repo);
        claimed.insert(dir.clone());
        own += usize::from(layers.switch().is_some());
        let files = inventory::files(&dir);
        if files.is_empty() && layers.switch().is_none() {
            continue;
        }
        let memory = inventory::summary(&files);
        let (enabled, by) = layers.effective();
        rows.push(json!({
            "name": repo.name(),
            "root": fsx::display(&repo.root),
            "key": repo.slug(),
            "directory": fsx::display(&dir),
            "custom": custom,
            "exists": !files.is_empty(),
            "files": memory.count,
            "bytes": memory.bytes,
            "lastWritten": memory.last_written.map(fsx::zulu),
            "enabled": enabled,
            "switch": super::switch::Position::of(layers.switch()).as_str(),
            "setBy": by.as_str(),
            "ownSwitch": layers.switch().is_some(),
        }));
    }
    let projects = world.claude_dir.join("projects");
    let mut other = Vec::new();
    for entry in fsx::list_dir(&projects) {
        let memory_dir = entry.path.join("memory");
        if entry.kind != fsx::Kind::Dir || claimed.contains(&memory_dir) {
            continue;
        }
        let files = inventory::files(&memory_dir);
        if files.is_empty() {
            continue;
        }
        let memory = inventory::summary(&files);
        other.push(json!({
            "key": entry.name,
            "files": memory.count,
            "bytes": memory.bytes,
            "lastWritten": memory.last_written.map(fsx::zulu),
        }));
    }
    let global = Layers::load(world.claude_dir, &Repo::of(world.home), None, None);
    json!({
        "repositories": rows,
        "otherFolders": other,
        "global": {
            "enabled": global.user.enabled,
            "default": global.user.enabled.unwrap_or(true),
            "file": fsx::display(&global.user_file),
            "overrides": own,
        },
    })
}
