//! `inert`: CLAUDE.md and SKILL.md files under folder names no load rule reaches (`_claude/**`
//! and the patterns in `inertPatterns`). A bounded name walk inside the configured folders.

use super::fsx::{self, Kind};
use super::item::{self, Placement};
use super::model::{Item, Origin, SourceMeta};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use std::path::{Path, PathBuf};

const DEFAULT_PATTERNS: [&str; 1] = ["_claude/**"];
const SKIP: [&str; 10] = [
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "target",
    "dist",
    "build",
    ".tox",
    "repos",
];
const INSIDE_DEPTH: usize = 3;

fn glob(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

fn dir_patterns(ctx: &ScanCtx) -> Vec<String> {
    DEFAULT_PATTERNS
        .iter()
        .map(|s| s.to_string())
        .chain(ctx.config.inert_patterns.iter().cloned())
        .map(|p| p.trim_end_matches("/**").trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn find(dir: &Path, depth: usize, patterns: &[String], ctx: &ScanCtx, out: &mut Vec<PathBuf>) {
    for entry in fsx::list_dir(dir) {
        if !ctx.budget.tick() {
            return;
        }
        if entry.kind != Kind::Dir || entry.symlink || SKIP.contains(&entry.name.as_str()) {
            continue;
        }
        if patterns.iter().any(|p| glob(p, &entry.name)) {
            out.push(entry.path);
        } else if depth + 1 < ctx.budget.max_depth() {
            find(&entry.path, depth + 1, patterns, ctx, out);
        }
    }
}

fn contents(
    dir: &Path,
    depth: usize,
    ctx: &ScanCtx,
    out: &mut Vec<(&'static str, String, PathBuf)>,
) {
    for entry in fsx::list_dir(dir) {
        if !ctx.budget.tick() {
            return;
        }
        match (entry.kind, entry.name.as_str()) {
            (Kind::File, "CLAUDE.md") => out.push(("memory", "CLAUDE.md".into(), entry.path)),
            (Kind::File, "SKILL.md") => {
                let name = dir
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                out.push(("skill", name, entry.path));
            }
            (Kind::Dir, _) if depth + 1 < INSIDE_DEPTH && !SKIP.contains(&entry.name.as_str()) => {
                contents(&entry.path, depth + 1, ctx, out)
            }
            _ => {}
        }
    }
}

pub struct InertDetector;

impl SourceDetector for InertDetector {
    fn id(&self) -> &'static str {
        "inert"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let patterns = dir_patterns(ctx);
        let origin = Origin::new("inert", "_claude");
        let place = Placement {
            source_id: "inert",
            origin: &origin,
            writable: false,
            audited: false,
            memory_lazy: false,
        };
        let mut dirs: Vec<PathBuf> = Vec::new();
        for folder in &ctx.scope.folders {
            find(folder, 0, &patterns, ctx, &mut dirs);
        }
        let mut seen: Vec<PathBuf> = Vec::new();
        let mut items: Vec<Item> = Vec::new();
        for dir in dirs {
            let canon = fsx::canonical(&dir);
            if seen.contains(&canon) {
                continue;
            }
            seen.push(canon);
            let mut found = Vec::new();
            contents(&dir, 0, ctx, &mut found);
            for (kind, fallback, path) in found {
                if let Some(parsed) = item::parse_file(ctx.cache, kind, &path) {
                    items.push(item::make_item(
                        &place,
                        kind,
                        &fallback,
                        fsx::display(&path),
                        &parsed,
                    ));
                }
            }
        }
        if items.is_empty() {
            return out;
        }
        let meta = SourceMeta {
            id: "inert".into(),
            origin: origin.clone(),
            detector: "inert",
            root: None,
            owner: "project",
            writable: false,
            managed_by: None,
            state: "ok",
            detail: format!(
                "{} file(s) under {} that no load rule reaches",
                items.len(),
                patterns.join(", ")
            ),
            freshness: None,
            warnings: Vec::new(),
            enabled: None,
        };
        out.sources.push(meta.finish(&items, ctx.now));
        out.items = items;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::glob;

    #[test]
    fn name_globs_match_whole_names_with_star_wildcards() {
        assert!(glob("_claude", "_claude"));
        assert!(!glob("_claude", "x_claude"));
        assert!(glob("*.disabled", "notes.disabled"));
        assert!(glob("old-*", "old-skills"));
        assert!(!glob("old-*", "new-skills"));
        assert!(glob("a*c", "abbc"));
    }
}
