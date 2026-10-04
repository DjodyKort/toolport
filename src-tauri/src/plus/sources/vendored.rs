//! `vendored`: `SKILL.md` folders inside a checkout that are not its own content, found under
//! `repos/` and under the paths `.gitmodules` lists. A bounded walk that never enters a folder
//! holding a `SKILL.md`, a symlink or a dependency or build folder.

use super::fsx::{self, Kind};
use super::item::{self, Placement};
use super::model::{Item, Origin, SourceMeta};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SKIP: [&str; 9] = [
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "target",
    "dist",
    "build",
    ".tox",
];
const CONTENT_NAMES: [&str; 5] = ["skills", ".claude", "commands", "agents", "rules"];

fn submodule_paths(repo: &Path) -> Vec<PathBuf> {
    let Some(text) = fsx::read_text(&repo.join(".gitmodules"), 64 * 1024) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("path"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .map(|p| repo.join(p.trim()))
        .collect()
}

fn find(dir: &Path, depth: usize, ctx: &ScanCtx, out: &mut Vec<PathBuf>) {
    if fsx::is_file(&dir.join("SKILL.md")) {
        out.push(dir.to_path_buf());
        return;
    }
    for entry in fsx::list_dir(dir) {
        if !ctx.budget.tick() {
            return;
        }
        if entry.kind != Kind::Dir || entry.symlink || SKIP.contains(&entry.name.as_str()) {
            continue;
        }
        if entry.name.starts_with('.') && entry.name != ".claude" {
            continue;
        }
        if depth + 1 > ctx.budget.max_depth() {
            if CONTENT_NAMES.contains(&entry.name.as_str()) {
                ctx.budget.depth_cut(&entry.path);
            }
            continue;
        }
        find(&entry.path, depth + 1, ctx, out);
    }
}

pub struct VendoredDetector;

impl SourceDetector for VendoredDetector {
    fn id(&self) -> &'static str {
        "vendored"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let mut taken: Vec<String> = Vec::new();
        for repo in &ctx.scope.repos {
            let mut bases = vec![repo.path.join("repos")];
            bases.extend(submodule_paths(&repo.path));
            let mut skill_dirs: Vec<PathBuf> = Vec::new();
            for base in bases {
                if ctx.budget.spent() {
                    break;
                }
                if fsx::is_dir(&base) {
                    find(&base, 0, ctx, &mut skill_dirs);
                }
            }
            skill_dirs.sort();
            skill_dirs.dedup();
            let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
            for dir in skill_dirs {
                let group = match dir.parent() {
                    Some(parent) if parent.starts_with(&repo.path) && parent != repo.path => {
                        parent.to_path_buf()
                    }
                    _ => dir.clone(),
                };
                groups.entry(group).or_default().push(dir);
            }
            for (group, dirs) in groups {
                let rel = group
                    .strip_prefix(&repo.path)
                    .map(|r| r.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut id = format!("vendored:{rel}");
                if taken.contains(&id) {
                    id = format!("{id}@{}", super::repo::dir_name(&repo.path));
                }
                taken.push(id.clone());
                let origin = Origin::new("vendored", rel.clone());
                let place = Placement {
                    source_id: &id,
                    origin: &origin,
                    writable: false,
                    audited: false,
                    memory_lazy: false,
                };
                let mut items: Vec<Item> = Vec::new();
                for dir in dirs {
                    let file = dir.join("SKILL.md");
                    if let Some(parsed) = item::parse_file(ctx.cache, "skill", &file) {
                        let fallback = super::repo::dir_name(&dir);
                        items.push(item::make_item(
                            &place,
                            "skill",
                            &fallback,
                            fsx::display(&file),
                            &parsed,
                        ));
                    }
                }
                let meta = SourceMeta {
                    id: id.clone(),
                    origin,
                    detector: "vendored",
                    root: Some(fsx::display(&group)),
                    owner: "third-party",
                    writable: false,
                    managed_by: None,
                    state: "ok",
                    detail: format!("{} skill folder(s) under {rel}", items.len()),
                    freshness: None,
                    warnings: Vec::new(),
                    enabled: None,
                };
                out.sources.push(meta.finish(&items, ctx.now));
                out.items.extend(items);
            }
        }
        out
    }
}
