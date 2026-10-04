//! `repo` and `client`: the `.claude` content and CLAUDE.md of a git checkout, read from its git
//! trees. HEAD and the remote default branch are listed with `git ls-tree`, so a checkout that
//! lags its remote (ODH, 114 commits behind) still shows what `origin/main` has, flagged
//! `inCheckout: false`. A fixed set of paths in the checkout is listed too, without recursion
//! and without reading a file that HEAD already has, so an untracked skill is not missed.
//! Linked worktrees are never entered.

use super::fsx;
use super::gitx::{self, GitDir, Remote, TreeEntry};
use super::item::{self, Parsed, Placement};
use super::layout;
use super::model::{Freshness, Item, Origin, Source, SourceMeta};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SPECS: [&str; 6] = [
    ".claude/skills",
    ".claude/commands",
    ".claude/agents",
    ".claude/rules",
    "CLAUDE.md",
    ".claude/CLAUDE.md",
];

pub(super) struct Spec {
    pub root: PathBuf,
    pub id: String,
    pub name: String,
    pub detector: &'static str,
    pub kind: &'static str,
    pub keep_empty: bool,
}

enum Body {
    Tree(TreeEntry),
    Disk(PathBuf),
}

struct Cand {
    kind: &'static str,
    fallback: String,
    body: Body,
    in_checkout: bool,
}

fn classify_tree_path(rel: &str) -> Option<(&'static str, String)> {
    match rel {
        "CLAUDE.md" | ".claude/CLAUDE.md" => Some(("memory", rel.to_string())),
        _ => layout::classify(rel.strip_prefix(".claude/")?),
    }
}

fn tree(ctx: &ScanCtx, git: &GitDir, sha: &str) -> Vec<TreeEntry> {
    let key = format!("tree:{}:{sha}", git.path().display());
    if let Some(cached) = ctx.cache.get(&key, sha) {
        let rows: Vec<TreeEntry> = cached
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                Some(TreeEntry {
                    path: row[0].as_str()?.to_string(),
                    oid: row[1].as_str()?.to_string(),
                    size: row[2].as_u64()?,
                })
            })
            .collect();
        return rows;
    }
    let entries = git.tree(sha, &SPECS);
    let rows: Vec<_> = entries
        .iter()
        .map(|e| json!([e.path, e.oid, e.size]))
        .collect();
    ctx.cache.put(&key, sha, json!(rows));
    entries
}

fn add_tree(cands: &mut BTreeMap<String, Cand>, entries: Vec<TreeEntry>, in_checkout: bool) {
    for entry in entries {
        if let Some((kind, fallback)) = classify_tree_path(&entry.path) {
            cands.entry(entry.path.clone()).or_insert(Cand {
                kind,
                fallback,
                body: Body::Tree(entry),
                in_checkout,
            });
        }
    }
}

fn behind_ahead(git: &GitDir, head: Option<&str>, remote: &Remote) -> (u64, u64) {
    match head {
        Some(_) => git.counts(&remote.sha).unwrap_or((0, 0)),
        None => (0, 0),
    }
}

pub(super) fn scan_checkout(ctx: &ScanCtx, spec: &Spec) -> Option<(Source, Vec<Item>)> {
    let git = gitx::open(&spec.root, ctx.budget.remaining())?;
    let head = git.head();
    let remote = git.remote_default();
    let mut cands: BTreeMap<String, Cand> = BTreeMap::new();
    if let Some(sha) = &head {
        add_tree(&mut cands, tree(ctx, &git, sha), true);
    }
    let mut disk: Vec<(String, &'static str, String, PathBuf)> = Vec::new();
    let dot_claude = spec.root.join(".claude");
    for found in layout::scan_layout(&dot_claude, ctx.budget) {
        disk.push((
            format!(".claude/{}", found.rel),
            found.kind,
            found.fallback,
            found.path,
        ));
    }
    for rel in ["CLAUDE.md", ".claude/CLAUDE.md"] {
        let path = spec.root.join(rel);
        if fsx::is_file(&path) {
            disk.push((rel.to_string(), "memory", rel.to_string(), path));
        }
    }
    for (rel, kind, fallback, path) in disk {
        cands.entry(rel).or_insert(Cand {
            kind,
            fallback,
            body: Body::Disk(path),
            in_checkout: true,
        });
    }
    if let Some(remote) = &remote {
        add_tree(&mut cands, tree(ctx, &git, &remote.sha), false);
    }

    let mut parsed: BTreeMap<String, Parsed> = BTreeMap::new();
    let mut missing: Vec<(String, TreeEntry)> = Vec::new();
    for (rel, cand) in &cands {
        match &cand.body {
            Body::Disk(path) => {
                if let Some(p) = item::parse_file(ctx.cache, cand.kind, path) {
                    parsed.insert(rel.clone(), p);
                }
            }
            Body::Tree(entry) => match item::cached_blob(ctx.cache, cand.kind, &entry.oid) {
                Some(p) => {
                    parsed.insert(rel.clone(), p);
                }
                None if entry.size > fsx::TEXT_CAP as u64 => {
                    parsed.insert(rel.clone(), item::oversized(entry.size));
                }
                None => missing.push((rel.clone(), entry.clone())),
            },
        }
    }
    if !missing.is_empty() && !ctx.budget.spent() {
        let mut oids: Vec<String> = missing.iter().map(|(_, e)| e.oid.clone()).collect();
        oids.sort();
        oids.dedup();
        let blobs = git.blobs(&oids);
        for (rel, entry) in missing {
            if let Some(bytes) = blobs.get(&entry.oid) {
                let kind = cands[&rel].kind;
                parsed.insert(
                    rel,
                    item::store_blob(ctx.cache, kind, &entry.oid, entry.size, bytes),
                );
            }
        }
    }

    let origin = Origin::new(spec.kind, spec.name.clone());
    let place = Placement {
        source_id: &spec.id,
        origin: &origin,
        writable: false,
        audited: true,
        memory_lazy: false,
    };
    let mut items = Vec::new();
    let mut only_remote = 0;
    for (rel, cand) in &cands {
        let Some(p) = parsed.get(rel) else { continue };
        let path = if cand.in_checkout {
            fsx::display(&spec.root.join(rel))
        } else {
            only_remote += 1;
            format!(
                "{}:{rel}",
                remote.as_ref().map_or("origin", |r| r.name.as_str())
            )
        };
        let mut built = item::make_item(&place, cand.kind, &cand.fallback, path, p);
        built.in_checkout = cand.in_checkout;
        items.push(built);
    }
    if items.is_empty() && !spec.keep_empty {
        return None;
    }

    let mut warnings = Vec::new();
    let high = items.iter().filter(|i| i.audit == "high").count();
    if high > 0 {
        warnings.push(format!(
            "{high} item(s) with high-severity audit findings; nothing was changed"
        ));
    }
    let (state, detail, freshness) = match &remote {
        Some(remote) => {
            let (ahead, behind) = behind_ahead(&git, head.as_deref(), remote);
            let detail = if behind > 0 {
                format!(
                    "{behind} commit(s) behind {}; {only_remote} item(s) only on {}",
                    remote.name, remote.name
                )
            } else {
                format!("up to date with {}", remote.name)
            };
            (
                if behind > 0 { "behind" } else { "ok" },
                detail,
                Some(Freshness {
                    reference: remote.name.clone(),
                    behind,
                    ahead,
                    in_checkout: only_remote == 0,
                    last_sync: git.last_fetch_secs().map(fsx::zulu),
                }),
            )
        }
        None => ("ok", "no remote branch to compare with".to_string(), None),
    };
    if only_remote > 0 {
        warnings.push(format!(
            "{only_remote} item(s) exist only on the remote branch, not in the checkout"
        ));
    }
    let meta = SourceMeta {
        id: spec.id.clone(),
        origin,
        detector: spec.detector,
        root: Some(fsx::display(&spec.root)),
        owner: "project",
        writable: false,
        managed_by: None,
        state,
        detail,
        freshness,
        warnings,
        enabled: None,
    };
    Some((meta.finish(&items, ctx.now), items))
}

pub(super) fn dir_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| "repo".to_string(), |n| n.to_string_lossy().into_owned())
}

pub struct RepoDetector;

impl SourceDetector for RepoDetector {
    fn id(&self) -> &'static str {
        "repo"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        for repo in &ctx.scope.repos {
            if ctx.budget.spent() {
                break;
            }
            let name = dir_name(&repo.path);
            let spec = Spec {
                root: repo.path.clone(),
                id: format!("repo:{}", name.to_lowercase()),
                name,
                detector: "repo",
                kind: "repo",
                keep_empty: repo.explicit,
            };
            if let Some((source, items)) = scan_checkout(ctx, &spec) {
                out.sources.push(source);
                out.items.extend(items);
            }
        }
        out
    }
}
