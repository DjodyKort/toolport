//! The instruction files a session in a folder reads, in Claude Code's documented order: managed
//! policy, the user file and rules, folders above the repository, the project file with its
//! imports under it, the local file and rules; then what loads on demand and what is not read.
//! Toolport edits none of them. The only switch is an exclusion in `claudeMdExcludes`.

use super::claude_json::Known;
use super::resolve::Repo;
use super::World;
use crate::context::fsx;
use crate::context::gitlayout;
use crate::context::leave_out;
use crate::context::loads::{InstructionLoads, LoadItem, LoadSource};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const SKIP_DIRS: [&str; 7] = [
    ".git",
    "node_modules",
    "target",
    ".venv",
    "dist",
    "build",
    "vendor",
];
const MAX_ON_DEMAND: usize = 25;
const MAX_DIRS: usize = 4000;
const NESTED_DEPTH: usize = 4;
const INDEX_CAP: usize = 64 * 1024 * 1024;

/// The names git tracks in one repository, read from its index (format 2 and 3).
#[derive(Default)]
struct Tracked(Option<BTreeSet<String>>);

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_index(path: &Path) -> Option<BTreeSet<String>> {
    let bytes = fsx::read_bytes(path, INDEX_CAP)?;
    if bytes.get(..4)? != b"DIRC" {
        return None;
    }
    let version = be32(&bytes, 4)?;
    if !(2..=3).contains(&version) {
        return None;
    }
    let count = be32(&bytes, 8)? as usize;
    let mut at = 12;
    let mut names = BTreeSet::new();
    for _ in 0..count {
        let flags = u16::from_be_bytes(bytes.get(at + 60..at + 62)?.try_into().ok()?);
        let fixed = if version == 3 && flags & 0x4000 != 0 {
            64
        } else {
            62
        };
        let len = if flags & 0x0fff == 0x0fff {
            bytes.get(at + fixed..)?.iter().position(|b| *b == 0)?
        } else {
            usize::from(flags & 0x0fff)
        };
        let name = bytes.get(at + fixed..at + fixed + len)?;
        names.insert(String::from_utf8_lossy(name).into_owned());
        at += (fixed + len + 8) & !7;
    }
    Some(names)
}

impl Tracked {
    fn contains(&mut self, git_dir: &Path, rel: &str) -> Option<bool> {
        if self.0.is_none() {
            self.0 = Some(read_index(&git_dir.join("index"))?);
        }
        self.0.as_ref().map(|names| names.contains(rel))
    }
}

/// Whether git tracks the file; `None` when it cannot be told (no index, or format 4).
fn in_git(cache: &mut BTreeMap<PathBuf, Tracked>, path: &Path) -> Option<bool> {
    let layout = gitlayout::locate(path.parent()?)?;
    let rel = path.strip_prefix(&layout.work_root).ok()?;
    cache
        .entry(layout.git_dir.clone())
        .or_default()
        .contains(&layout.git_dir, &rel.to_string_lossy())
}

pub struct Report {
    pub rows: Vec<Value>,
    pub total_tokens: u64,
    /// What the auto memory index adds to `total_tokens` when memory is on.
    pub memory_index_tokens: u64,
    pub left_out_tokens: u64,
    pub excluded: usize,
    pub local_files: usize,
    pub imports: usize,
}

fn scope_of(item: &LoadItem, repo: &Repo) -> &'static str {
    match (item.kind, item.source) {
        ("import", _) => "import",
        (_, "policy") => "managed",
        ("rule", _) => "rules",
        (_, "user") => "user",
        (_, "local") => "local",
        _ => match item.path.as_deref() {
            Some(p) if Path::new(p).starts_with(&repo.root) => "project",
            _ => "parent",
        },
    }
}

fn nested_claude_md(root: &Path, skip: &BTreeSet<PathBuf>, out: &mut Vec<PathBuf>) {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut visited = 0;
    while let Some((dir, depth)) = stack.pop() {
        visited += 1;
        if visited > MAX_DIRS || out.len() >= MAX_ON_DEMAND {
            return;
        }
        for entry in fsx::list_dir(&dir) {
            match entry.kind {
                fsx::Kind::Dir if !entry.symlink => {
                    if depth < NESTED_DEPTH && !SKIP_DIRS.contains(&entry.name.as_str()) {
                        stack.push((entry.path, depth + 1));
                    }
                }
                fsx::Kind::File
                    if entry.name == "CLAUDE.md" && dir != root && !skip.contains(&entry.path) =>
                {
                    out.push(entry.path);
                }
                _ => {}
            }
        }
    }
    out.sort();
}

pub fn report(world: &World, repo: &Repo, known: &Known) -> Result<Report, String> {
    let loads = InstructionLoads {
        home: world.home,
        policy_file: world.policy_claude_md,
        count_tokens: world.count_tokens,
    };
    let mut found = loads.what_loads(&repo.cwd)?;
    leave_out::mark(&mut found.items, world.data_dir, &repo.cwd);
    let mut cache = BTreeMap::new();
    let mut rows = Vec::new();
    if world.policy_claude_md.is_none_or(|p| !fsx::is_file(p)) {
        rows.push(json!({
            "path": crate::context::loads::POLICY_CLAUDE_MD,
            "name": "CLAUDE.md",
            "kind": "memory",
            "scope": "managed",
            "source": "policy",
            "state": "none",
            "loaded": false,
            "lazy": false,
            "tokens": 0,
            "bytes": 0,
            "canExclude": false,
            "note": "No managed policy file on this Mac. If one exists, it always loads and cannot be excluded.",
        }));
    }
    let root_text = fsx::display(&repo.root);
    let mut left_out_tokens = 0;
    let (mut excluded, mut local_files, mut imports) = (0, 0, 0);
    let mut have_claude_md = false;
    let mut listed: BTreeSet<PathBuf> = BTreeSet::new();
    let memory_index_tokens = found
        .items
        .iter()
        .filter(|i| i.kind == "memory-index" && i.loaded)
        .map(|i| i.tokens)
        .sum();
    for item in found.items.iter().filter(|i| i.kind != "memory-index") {
        let scope = scope_of(item, repo);
        let path = item.path.clone().unwrap_or_default();
        let path_buf = PathBuf::from(&path);
        listed.insert(path_buf.clone());
        let is_excluded = !item.loaded && !item.lazy && item.reason.starts_with("excluded");
        let state = match (item.source, item.loaded, item.lazy, is_excluded) {
            ("policy", ..) => "managed",
            (_, true, ..) => "loads",
            (_, _, true, _) => "on demand",
            (.., true) => "excluded",
            _ => "not read",
        };
        if is_excluded {
            excluded += 1;
            left_out_tokens += item.tokens;
        }
        if item.kind == "memory" && matches!(item.source, "project" | "local") {
            have_claude_md = true;
        }
        if scope == "local" {
            local_files += 1;
        }
        if scope == "import" {
            imports += 1;
        }
        let external =
            item.kind == "import" && item.source != "user" && !path.starts_with(&root_text);
        let note = match (scope, item.source) {
            (_, "policy") => Some("Managed policy file: always loads, cannot be excluded"),
            ("import", _) => Some("Loads with its parent; an import has no switch of its own"),
            _ => None,
        };
        rows.push(json!({
            "path": path,
            "name": item.name,
            "kind": item.kind,
            "scope": scope,
            "source": item.source,
            "state": state,
            "loaded": item.loaded,
            "lazy": item.lazy,
            "reason": item.reason,
            "tokens": item.tokens,
            "bytes": fsx::stamp(&path_buf).map_or(0, |s| s.len),
            "inGit": in_git(&mut cache, &path_buf),
            "leftOutBy": item.left_out_by,
            "canExclude": matches!(item.kind, "memory" | "rule") && item.source != "policy",
            "parent": item.parent,
            "external": external,
            "externalApproved": if external { known.external_imports_approved(&repo.root) } else { None },
            "note": note,
        }));
    }
    let mut nested = Vec::new();
    nested_claude_md(&repo.root, &listed, &mut nested);
    for path in nested {
        let text = fsx::read_text(&path, fsx::TEXT_CAP).unwrap_or_default();
        rows.push(json!({
            "path": fsx::display(&path),
            "name": "CLAUDE.md",
            "kind": "memory",
            "scope": "on-demand",
            "source": "project",
            "state": "on demand",
            "loaded": false,
            "lazy": true,
            "reason": "",
            "tokens": (world.count_tokens)(&text),
            "bytes": fsx::stamp(&path).map_or(0, |s| s.len),
            "inGit": in_git(&mut cache, &path),
            "leftOutBy": null,
            "canExclude": false,
            "note": "Loads when Claude reads, writes or edits a file in that folder",
        }));
    }
    let mut dir_chain: Vec<&Path> = repo.cwd.ancestors().collect();
    dir_chain.reverse();
    for dir in dir_chain {
        let path = dir.join("AGENTS.md");
        if !fsx::is_file(&path) {
            continue;
        }
        let text = fsx::read_text(&path, fsx::TEXT_CAP).unwrap_or_default();
        rows.push(json!({
            "path": fsx::display(&path),
            "name": "AGENTS.md",
            "kind": "memory",
            "scope": "agents",
            "source": "project",
            "state": if have_claude_md { "not read" } else { "loads" },
            "loaded": false,
            "lazy": false,
            "reason": "",
            "tokens": (world.count_tokens)(&text),
            "bytes": fsx::stamp(&path).map_or(0, |s| s.len),
            "inGit": in_git(&mut cache, &path),
            "leftOutBy": null,
            "canExclude": false,
            "note": "Read only when no CLAUDE.md or CLAUDE.local.md exists in the folder or above",
        }));
    }
    Ok(Report {
        rows,
        total_tokens: found.total_tokens,
        memory_index_tokens,
        left_out_tokens,
        excluded,
        local_files,
        imports,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, flags: u16) -> Vec<u8> {
        let mut e = vec![0u8; 60];
        e.extend_from_slice(&flags.to_be_bytes());
        e.extend_from_slice(name.as_bytes());
        let pad = (62 + name.len() + 8) & !7;
        e.resize(pad, 0);
        e
    }

    #[test]
    fn the_git_index_lists_the_tracked_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = b"DIRC".to_vec();
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend(entry("CLAUDE.md", 9));
        bytes.extend(entry("src/a.rs", 8));
        bytes.extend_from_slice(&[0u8; 20]);
        std::fs::write(dir.path().join("index"), bytes).unwrap();
        let names = read_index(&dir.path().join("index")).unwrap();
        assert_eq!(
            names.into_iter().collect::<Vec<_>>(),
            ["CLAUDE.md", "src/a.rs"]
        );
        assert!(read_index(&dir.path().join("none")).is_none());
    }
}
