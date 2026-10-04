//! `account`: skills the Anthropic account syncs into `~/.claude/skills/synced/<uuid>/`. The
//! `manifest.json` lists them; a skill with a folder on disk is read from its `SKILL.md`, one
//! only in the manifest counts by the name and description it carries.

use super::fsx::{self, Kind};
use super::item::{self, Placement};
use super::layout;
use super::model::{Item, Origin, SourceMeta, Tokens};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use crate::savings::estimated_tokens;
use serde_json::Value;
use std::collections::BTreeSet;

fn manifest_skills(doc: &Value) -> Vec<(String, String)> {
    let list = match doc {
        Value::Array(list) => list.clone(),
        Value::Object(map) => match map.get("skills") {
            Some(Value::Array(list)) => list.clone(),
            Some(Value::Object(named)) => named
                .iter()
                .map(|(k, v)| match v {
                    Value::Object(_) => {
                        let mut v = v.clone();
                        v["name"] = Value::String(k.clone());
                        v
                    }
                    other => serde_json::json!({"name": k, "description": other}),
                })
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    list.iter()
        .filter_map(|entry| match entry {
            Value::String(name) => Some((name.clone(), String::new())),
            Value::Object(map) => Some((
                map.get("name")?.as_str()?.to_string(),
                map.get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )),
            _ => None,
        })
        .collect()
}

pub struct AccountDetector;

impl SourceDetector for AccountDetector {
    fn id(&self) -> &'static str {
        "account"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let synced = ctx.roots.claude_home.join("skills/synced");
        if !fsx::is_dir(&synced) {
            return out;
        }
        let origin = Origin::new("account", "account");
        let place = Placement {
            source_id: "account",
            origin: &origin,
            writable: false,
            audited: false,
            memory_lazy: false,
        };
        let mut items: Vec<Item> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut accounts = 0;
        for account in fsx::list_dir(&synced) {
            if !ctx.budget.tick() {
                break;
            }
            if account.kind != Kind::Dir {
                continue;
            }
            accounts += 1;
            let mut files: Vec<(String, std::path::PathBuf)> =
                layout::scan_layout(&account.path, ctx.budget)
                    .into_iter()
                    .filter(|f| f.kind == "skill")
                    .map(|f| (f.fallback, f.path))
                    .collect();
            for child in fsx::list_dir(&account.path) {
                let skill = child.path.join("SKILL.md");
                if child.kind == Kind::Dir && child.name != "skills" && fsx::is_file(&skill) {
                    files.push((child.name, skill));
                }
            }
            for (fallback, path) in files {
                if let Some(parsed) = item::parse_file(ctx.cache, "skill", &path) {
                    let built =
                        item::make_item(&place, "skill", &fallback, fsx::display(&path), &parsed);
                    seen.insert(built.name.clone());
                    items.push(built);
                }
            }
            let manifest = account.path.join("manifest.json");
            let doc = fsx::read_text(&manifest, 4 * 1024 * 1024)
                .and_then(|t| serde_json::from_str::<Value>(&t).ok());
            for (name, description) in doc.as_ref().map(manifest_skills).unwrap_or_default() {
                if !seen.insert(name.clone()) {
                    continue;
                }
                items.push(Item {
                    kind: "skill",
                    tokens: Tokens::estimate(estimated_tokens(
                        (name.len() + description.len()) as u64,
                    )),
                    path: fsx::display(&manifest),
                    source_id: "account".into(),
                    origin: origin.clone(),
                    writable: false,
                    lazy: true,
                    shadowed_by: None,
                    audit: "unchecked",
                    description,
                    activation: None,
                    visible: None,
                    invisible_reason: None,
                    in_checkout: true,
                    name,
                });
            }
        }
        if accounts == 0 {
            return out;
        }
        let meta = SourceMeta {
            id: "account".into(),
            origin,
            detector: "account",
            root: Some(fsx::display(&synced)),
            owner: "anthropic",
            writable: false,
            managed_by: Some("Claude account sync".into()),
            state: "ok",
            detail: format!("{} skill(s) from {accounts} account folder(s)", items.len()),
            freshness: None,
            warnings: Vec::new(),
            enabled: None,
        };
        out.sources.push(meta.finish(&items, ctx.now));
        out.items = items;
        out
    }
}
