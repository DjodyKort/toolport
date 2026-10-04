//! `org`: what the corporate tools clone (D-040) syncs into `~/.claude`: CLAUDE.md and the org
//! commands. It only compares hashes; nothing here is written, and Toolport never edits these
//! files. An org command that shares a name with a library skill wins (D-073); the engine's
//! shadow pass records that.

use super::fsx;
use super::gitx;
use super::item::{self, Placement};
use super::layout;
use super::model::{Freshness, Item, Origin, SourceMeta};
use super::repo::dir_name;
use super::{DetectorOutput, ScanCtx, SourceDetector};
use crate::plus::hashing::sha256_hex;
use serde_json::json;
use std::path::Path;

const HASH_CAP: usize = 8 * 1024 * 1024;
const DEFAULT_SYNC_SECS: u64 = 14_400;

pub(super) fn sha(ctx: &ScanCtx, path: &Path) -> Option<String> {
    let stamp = fsx::stamp(path)?;
    let key = format!("sha:{}", path.display());
    if let Some(hit) = ctx.cache.get(&key, &stamp.key()) {
        return hit.as_str().map(String::from);
    }
    let hex = sha256_hex(fsx::read_bytes(path, HASH_CAP)?);
    ctx.cache.put(&key, &stamp.key(), json!(hex));
    Some(hex)
}

fn interval_text() -> String {
    let secs = std::env::var("CLAUDE_SYNC_INTERVAL")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_SYNC_SECS);
    if secs % 3600 == 0 {
        format!("{} h", secs / 3600)
    } else {
        format!("{} min", secs.div_ceil(60))
    }
}

fn stamp_secs(path: &Path) -> Option<i64> {
    fsx::read_text(path, 64)
        .and_then(|t| t.trim().parse::<i64>().ok())
        .or_else(|| fsx::mtime_secs(path))
}

pub struct OrgDetector;

impl SourceDetector for OrgDetector {
    fn id(&self) -> &'static str {
        "org"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let clone = ctx.roots.resolve_corp_tools_dir(ctx.config);
        let claude = clone.join("claude");
        if !fsx::is_dir(&claude) {
            return out;
        }
        let name = dir_name(&clone);
        let origin = Origin::new("org", name.clone());
        let place = Placement {
            source_id: "org",
            origin: &origin,
            writable: false,
            audited: false,
            memory_lazy: false,
        };
        let mut items: Vec<Item> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let mut stale = 0;

        let user_md = ctx.roots.claude_home.join("CLAUDE.md");
        let clone_hashes: Vec<String> = ["CLAUDE.md", "CLAUDE.uncompressed.md"]
            .iter()
            .filter_map(|f| sha(ctx, &claude.join(f)))
            .collect();
        if fsx::is_file(&user_md) && !clone_hashes.is_empty() {
            if let Some(parsed) = item::parse_file(ctx.cache, "memory", &user_md) {
                let current = sha(ctx, &user_md);
                if !current.as_ref().is_some_and(|h| clone_hashes.contains(h)) {
                    stale += 1;
                    warnings.push("~/.claude/CLAUDE.md differs from the clone (not synced yet, or edited by hand)".into());
                }
                items.push(item::make_item(
                    &place,
                    "memory",
                    "CLAUDE.md",
                    fsx::display(&user_md),
                    &parsed,
                ));
            }
        }

        let mut undeployed = 0;
        for found in layout::scan_layout(&claude, ctx.budget)
            .into_iter()
            .filter(|f| f.kind == "command")
        {
            let deployed = ctx.roots.claude_home.join(&found.rel);
            if !fsx::is_file(&deployed) {
                undeployed += 1;
                continue;
            }
            let Some(parsed) = item::parse_file(ctx.cache, "command", &deployed) else {
                continue;
            };
            if sha(ctx, &deployed) != sha(ctx, &found.path) {
                stale += 1;
                warnings.push(format!(
                    "command {} differs from the clone (not synced yet, or edited by hand)",
                    found.fallback
                ));
            }
            items.push(item::make_item(
                &place,
                "command",
                &found.fallback,
                fsx::display(&deployed),
                &parsed,
            ));
        }

        let wrapper = claude.join("shell-wrapper.sh");
        if let Some(baseline) = ctx
            .config
            .cf_wrapper_hash
            .as_deref()
            .filter(|h| !h.is_empty())
        {
            if sha(ctx, &wrapper).as_deref() != Some(baseline) {
                warnings.push(
                    "shell-wrapper.sh changed since the hash recorded in context.json".into(),
                );
            }
        }

        let last_sync = stamp_secs(&ctx.roots.claude_home.join(".last_auto_sync")).map(fsx::zulu);
        let remote = gitx::open(&clone, ctx.budget.remaining()).and_then(|git| {
            let remote = git.remote_default()?;
            let counts = git.counts(&remote.sha);
            Some((remote, counts))
        });
        let (reference, ahead, behind) = match &remote {
            Some((remote, counts)) => {
                let (ahead, behind) = counts.unwrap_or((0, 0));
                (remote.name.clone(), ahead, behind)
            }
            None => ("local".to_string(), 0, 0),
        };
        let mut notes = Vec::new();
        if stale > 0 {
            notes.push(format!("{stale} file(s) differ from the clone"));
        }
        if behind > 0 {
            notes.push(format!(
                "the clone is {behind} commit(s) behind {reference}"
            ));
        }
        if undeployed > 0 {
            notes.push(format!("{undeployed} clone command(s) are not deployed"));
        }
        let (state, detail) = if stale > 0 {
            ("stale", notes.join("; "))
        } else if behind > 0 {
            ("behind", notes.join("; "))
        } else if notes.is_empty() {
            ("ok", format!("{} item(s) match the clone", items.len()))
        } else {
            ("ok", notes.join("; "))
        };
        let meta = SourceMeta {
            id: "org".into(),
            origin: origin.clone(),
            detector: "org",
            root: Some(fsx::display(&clone)),
            owner: "org",
            writable: false,
            managed_by: Some(format!("{name} sync (about every {})", interval_text())),
            state,
            detail,
            freshness: Some(Freshness {
                reference,
                behind,
                ahead,
                in_checkout: true,
                last_sync,
            }),
            warnings,
            enabled: None,
        };
        out.sources.push(meta.finish(&items, ctx.now));
        out.items = items;
        out
    }
}
