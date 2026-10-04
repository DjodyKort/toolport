//! `library`: the skills repository (skills, rules, agents, commands), found through the sync
//! config. Skill and rule items say whether Claude Code accepts what Toolport emits for them and,
//! when a copy is deployed, that copy too; this is where a rejected multi-line description shows.
//! A second clone of the same remote is flagged `duplicate`.

use super::fsx::{self, Kind};
use super::gitx;
use super::item::{self, Placement};
use super::layout::{self, Found};
use super::model::{Freshness, Item, Origin, SourceMeta, Tokens};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use crate::plus::hashing::sha256_hex;
use crate::plus::skills::agents::parse_agent_file;
use crate::plus::skills::frontmatter::output_accepted;
use crate::plus::skills::parser::{parse_skill_file, Skill, SkillType};
use crate::plus::skills::transpilers::registry_with_home;
use crate::plus::skills::TranspilerRegistry;
use crate::savings::estimated_tokens;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn sync_path(dir: &Path) -> Option<PathBuf> {
    let text = fsx::read_text(&dir.join("skills_sync.json"), 64 * 1024)?;
    let doc: Value = serde_json::from_str(&text).ok()?;
    doc.get("local_path")?.as_str().map(PathBuf::from)
}

/// The clones that could be the library, the one the sync config names first.
pub(super) fn candidates(ctx: &ScanCtx) -> Vec<PathBuf> {
    let mut raw: Vec<PathBuf> = Vec::new();
    if let Some(data) = ctx.data_dir {
        raw.extend(sync_path(data).map(|p| ctx.roots.expand_user(&p.to_string_lossy())));
    }
    raw.push(ctx.roots.skills_repo_path());
    let mut out: Vec<PathBuf> = Vec::new();
    for path in raw {
        let canon = fsx::canonical(&path);
        if fsx::is_dir(&path) && !out.iter().any(|p| fsx::canonical(p) == canon) {
            out.push(path);
        }
    }
    out
}

/// `skills/<n>/SKILL.md`, `rules/<n>/SKILL.md`, `agents/<n>/AGENT.md` and `commands/<n>.md`.
pub(super) fn scan_skills_repo(root: &Path, ctx: &ScanCtx) -> Vec<Found> {
    let mut out = Vec::new();
    for (dir, file, kind) in [
        ("skills", "SKILL.md", "skill"),
        ("rules", "SKILL.md", "rule"),
        ("agents", "AGENT.md", "agent"),
    ] {
        for entry in fsx::list_dir(&root.join(dir)) {
            if !ctx.budget.tick() {
                return out;
            }
            let path = entry.path.join(file);
            if entry.kind == Kind::Dir && fsx::is_file(&path) {
                out.push(Found {
                    kind,
                    fallback: entry.name.clone(),
                    rel: format!("{dir}/{}/{file}", entry.name),
                    path,
                });
            }
        }
    }
    let commands = root.join("commands");
    if fsx::is_dir(&commands) {
        out.extend(
            layout::scan_layout(root, ctx.budget)
                .into_iter()
                .filter(|f| f.kind == "command"),
        );
    }
    out
}

fn looks_like_skills_repo(dir: &Path) -> bool {
    layout::CONTENT_DIRS
        .iter()
        .chain(["styles"].iter())
        .any(|d| fsx::is_dir(&dir.join(d)))
}

fn duplicates(ctx: &ScanCtx, root: &Path, remote: &str, others: &[PathBuf]) -> Vec<PathBuf> {
    let own = fsx::canonical(root);
    let mut pool: Vec<PathBuf> = others.to_vec();
    for row in ctx
        .scope
        .listing
        .iter()
        .filter(|r| r.exists && r.origin != "default")
    {
        pool.push(row.path.clone());
        pool.extend(
            fsx::list_dir(&row.path)
                .into_iter()
                .filter(|e| e.kind == Kind::Dir)
                .map(|e| e.path),
        );
    }
    if let Some(parent) = root.parent() {
        pool.extend(
            fsx::list_dir(parent)
                .into_iter()
                .filter(|e| e.kind == Kind::Dir)
                .map(|e| e.path),
        );
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in pool {
        if !ctx.budget.tick() {
            break;
        }
        let canon = fsx::canonical(&dir);
        if canon == own
            || found.iter().any(|p| fsx::canonical(p) == canon)
            || !looks_like_skills_repo(&dir)
        {
            continue;
        }
        let same = gitx::open(&dir, ctx.budget.remaining())
            .and_then(|git| git.remote_url())
            .is_some_and(|url| gitx::normalize_url(&url) == remote);
        if same {
            found.push(dir);
        }
    }
    found
}

struct Record {
    kind: &'static str,
    name: String,
    path: String,
    description: String,
    activation: Option<String>,
    tokens: u64,
    visible: Option<bool>,
    reason: Option<String>,
}

/// Why Claude Code would not list `skill`: what Toolport emits for it, or the copy already
/// deployed under `home`, fails the strict frontmatter check. `None` when it is accepted.
pub fn invisible_reason(
    registry: &TranspilerRegistry,
    skill: &Skill,
    home: &Path,
) -> Option<String> {
    let claude = registry.get("claude-code")?;
    let out = claude.transpile(skill, home).ok()?;
    if let Err(why) = output_accepted("claude-code", &out.content) {
        return Some(why.to_string());
    }
    let deployed = fsx::read_text(&out.output_path, fsx::TEXT_CAP)?;
    output_accepted("claude-code", &deployed)
        .err()
        .map(|why| format!("the deployed copy is rejected: {why}"))
}

fn emission(ctx: &ScanCtx, registry: &TranspilerRegistry, file: &Found) -> Result<Record, String> {
    let skill = parse_skill_file(&file.path)?;
    let reason = invisible_reason(registry, &skill, &ctx.roots.home);
    let kind = if skill.skill_type == SkillType::Rule {
        "rule"
    } else {
        "skill"
    };
    let size = fsx::stamp(&file.path).map_or(0, |s| s.len);
    let name = skill.name().to_string();
    Ok(Record {
        kind,
        tokens: if kind == "rule" {
            estimated_tokens(size)
        } else {
            estimated_tokens((name.len() + skill.frontmatter.description.len()) as u64)
        },
        path: fsx::display(&file.path),
        description: skill.frontmatter.description.clone(),
        activation: Some(skill.frontmatter.activation.as_str().to_string()),
        visible: Some(reason.is_none()),
        reason,
        name,
    })
}

fn record_json(r: &Record) -> Value {
    json!({
        "kind": r.kind, "name": r.name, "path": r.path, "description": r.description,
        "activation": r.activation, "tokens": r.tokens, "visible": r.visible, "reason": r.reason,
    })
}

fn record_from(v: &Value) -> Option<Record> {
    let kind = match v["kind"].as_str()? {
        "skill" => "skill",
        "rule" => "rule",
        "agent" => "agent",
        _ => return None,
    };
    Some(Record {
        kind,
        name: v["name"].as_str()?.to_string(),
        path: v["path"].as_str()?.to_string(),
        description: v["description"].as_str()?.to_string(),
        activation: v["activation"].as_str().map(String::from),
        tokens: v["tokens"].as_u64()?,
        visible: v["visible"].as_bool(),
        reason: v["reason"].as_str().map(String::from),
    })
}

/// The skill, rule and agent records of the library, from the cache while no file or deployed
/// copy changed.
fn records(ctx: &ScanCtx, root: &Path, files: &[Found]) -> (Vec<Record>, Vec<String>) {
    let mut stamp = String::new();
    for f in files.iter().filter(|f| f.kind != "command") {
        let stamped = fsx::stamp(&f.path).map(|s| s.key()).unwrap_or_default();
        stamp.push_str(&format!("{}={stamped};", f.rel));
        if f.kind != "agent" {
            let deployed = ctx
                .roots
                .claude_home
                .join(format!("skills/{}/SKILL.md", f.fallback));
            let rule = ctx
                .roots
                .claude_home
                .join(format!("rules/{}.md", f.fallback));
            for p in [deployed, rule] {
                stamp.push_str(&fsx::stamp(&p).map(|s| s.key()).unwrap_or_default());
            }
        }
    }
    let stamp = sha256_hex(&stamp);
    let key = format!("library:{}", root.display());
    if let Some(hit) = ctx.cache.get(&key, &stamp) {
        let recs: Vec<Record> = hit["records"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(record_from)
            .collect();
        let warns = hit["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| w.as_str().map(String::from))
            .collect();
        return (recs, warns);
    }
    let mut recs = Vec::new();
    let mut warns = Vec::new();
    let registry = registry_with_home(Some(ctx.roots.home.clone()));
    for file in files.iter().filter(|f| f.kind != "command") {
        if file.kind == "agent" {
            match parse_agent_file(&file.path) {
                Ok(agent) => recs.push(Record {
                    kind: "agent",
                    tokens: estimated_tokens(
                        (agent.frontmatter.name.len() + agent.frontmatter.description.len()) as u64,
                    ),
                    name: agent.frontmatter.name.clone(),
                    path: fsx::display(&file.path),
                    description: agent.frontmatter.description.clone(),
                    activation: None,
                    visible: None,
                    reason: None,
                }),
                Err(e) => warns.push(format!("Failed to parse {}: {e}", file.path.display())),
            }
            continue;
        }
        match emission(ctx, &registry, file) {
            Ok(rec) => recs.push(rec),
            Err(e) => warns.push(format!("Failed to parse {}: {e}", file.path.display())),
        }
    }
    ctx.cache.put(
        &key,
        &stamp,
        json!({"records": recs.iter().map(record_json).collect::<Vec<_>>(), "warnings": warns}),
    );
    (recs, warns)
}

pub struct LibraryDetector;

impl SourceDetector for LibraryDetector {
    fn id(&self) -> &'static str {
        "library"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let found = candidates(ctx);
        let Some(root) = found.first().cloned() else {
            return out;
        };
        let name = super::repo::dir_name(&root);
        let origin = Origin::new("library", name);
        let place = Placement {
            source_id: "library",
            origin: &origin,
            writable: true,
            audited: false,
            memory_lazy: false,
        };
        let files = scan_skills_repo(&root, ctx);
        let (recs, mut warnings) = records(ctx, &root, &files);
        let mut items: Vec<Item> = recs
            .into_iter()
            .map(|r| Item {
                kind: if r.kind == "skill" && r.activation.as_deref() == Some("always") {
                    "rule"
                } else {
                    r.kind
                },
                lazy: r.kind != "rule" && r.activation.as_deref() != Some("always"),
                name: r.name,
                path: r.path,
                source_id: "library".into(),
                origin: origin.clone(),
                writable: true,
                shadowed_by: None,
                audit: "unchecked",
                tokens: Tokens::estimate(r.tokens),
                description: r.description,
                activation: r.activation,
                visible: r.visible,
                invisible_reason: r.reason,
                in_checkout: true,
            })
            .collect();
        for file in files.iter().filter(|f| f.kind == "command") {
            if let Some(parsed) = item::parse_file(ctx.cache, "command", &file.path) {
                items.push(item::make_item(
                    &place,
                    "command",
                    &file.fallback,
                    fsx::display(&file.path),
                    &parsed,
                ));
            }
        }
        let hidden = items.iter().filter(|i| i.visible == Some(false)).count();
        if hidden > 0 {
            warnings.push(format!(
                "{hidden} skill(s) are hidden from Claude Code; see the item reasons"
            ));
        }

        let git = gitx::open(&root, ctx.budget.remaining());
        let remote = git.as_ref().and_then(|g| g.remote_default());
        let url = git.as_ref().and_then(|g| g.remote_url());
        let counts = match (&git, &remote) {
            (Some(g), Some(r)) if g.head().is_some() => g.counts(&r.sha),
            _ => None,
        };
        let (ahead, behind) = counts.unwrap_or((0, 0));
        if ahead > 0 {
            warnings.push(format!("{ahead} commit(s) not pushed"));
        }
        let twins = url
            .as_deref()
            .map(|u| duplicates(ctx, &root, &gitx::normalize_url(u), &found[1..]))
            .unwrap_or_default();
        for twin in &twins {
            warnings.push(format!(
                "another clone of the same remote at {}",
                twin.display()
            ));
        }
        let (state, detail) = if !twins.is_empty() {
            (
                "duplicate",
                format!("{} other clone(s) of the same remote", twins.len()),
            )
        } else if behind > 0 {
            (
                "behind",
                format!(
                    "{behind} commit(s) behind {}",
                    remote.as_ref().map_or("remote", |r| r.name.as_str())
                ),
            )
        } else {
            ("ok", "clone is current".to_string())
        };
        let meta = SourceMeta {
            id: "library".into(),
            origin: origin.clone(),
            detector: "library",
            root: Some(fsx::display(&root)),
            owner: "me",
            writable: true,
            managed_by: None,
            state,
            detail,
            freshness: remote.as_ref().map(|r| Freshness {
                reference: r.name.clone(),
                behind,
                ahead,
                in_checkout: true,
                last_sync: git
                    .as_ref()
                    .and_then(|g| g.last_fetch_secs())
                    .map(fsx::zulu),
            }),
            warnings,
            enabled: None,
        };
        out.sources.push(meta.finish(&items, ctx.now));
        out.items = items;
        out
    }
}
