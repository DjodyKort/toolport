//! Read-mostly operations over the lockfile and the output tree: repo discovery, diff, status,
//! clean and collision triage. Mirrors the data side of mcpm's `skills|agents|styles` commands;
//! rendering and prompting stay with the caller.

use super::agents::AgentTranspiler;
use super::assets::compute_skill_hash;
#[cfg(test)]
use super::clock::Clock;
#[cfg(test)]
use super::collisions::{
    detect_collisions, resolve_collisions, resolve_mode, Collision, CollisionSummary,
};
use super::json;
#[cfg(test)]
use super::lock::lockfile_path;
use super::lock::{get_entry, LockFile, OrderedMap};
use super::parser::{valid_name, Skill};
#[cfg(test)]
use super::parser::SkillType;
use super::styles::{all_style_transpilers, Tier};
#[cfg(test)]
use super::transpiler::{
    Transpiler, TranspilerRegistry, APPEND_MODE_TRANSPILERS, PROJECT_ONLY_TRANSPILERS,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Where mcpm keeps `skills_sync.json`, whose `local_path` is the git-sync clone fallback.
pub const SKILLS_SYNC_CONFIG: &str = "skills_sync.json";

fn resolve(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn looks_like_repo(dir: &Path) -> bool {
    dir.join("mcpm-skills.yaml").exists()
        || ["skills", "rules", "agents", "styles"]
            .iter()
            .any(|d| dir.join(d).is_dir())
}

/// Walks up from `start` (or `cwd`) for a manifest or a content directory. The git-sync clone
/// from `<config_dir>/skills_sync.json` is only consulted when no explicit start was given.
pub fn find_skills_repo(start: Option<&Path>, cwd: &Path, config_dir: &Path) -> Option<PathBuf> {
    let mut current = resolve(start.unwrap_or(cwd));
    for _ in 0..20 {
        if looks_like_repo(&current) {
            return Some(current);
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => break,
        }
    }
    if start.is_some() {
        return None;
    }
    let text = fs::read_to_string(config_dir.join(SKILLS_SYNC_CONFIG)).ok()?;
    let doc = json::parse(&text).ok()?;
    let local = PathBuf::from(doc.get("local_path")?.as_str()?);
    local.exists().then_some(local)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffReport {
    pub no_lockfile: bool,
    pub new: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
    pub unchanged: usize,
}

impl DiffReport {
    pub fn is_clean(&self) -> bool {
        self.new.is_empty() && self.modified.is_empty() && self.removed.is_empty()
    }
}

/// Compares the current skills with the lockfile by name and content hash.
pub fn diff_skills(skills: &[Skill], lock: Option<&LockFile>) -> Result<DiffReport, String> {
    let current: BTreeSet<&str> = skills.iter().map(Skill::name).collect();
    let Some(lock) = lock else {
        return Ok(DiffReport {
            no_lockfile: true,
            new: current.iter().map(|s| s.to_string()).collect(),
            ..DiffReport::default()
        });
    };
    let locked: BTreeSet<&str> = lock
        .skills
        .iter()
        .chain(lock.rules.iter())
        .map(|(k, _)| k.as_str())
        .collect();
    let mut report = DiffReport::default();
    report.new = current.difference(&locked).map(|s| s.to_string()).collect();
    report.removed = locked.difference(&current).map(|s| s.to_string()).collect();
    let mut modified = BTreeSet::new();
    for skill in skills {
        let entry =
            get_entry(&lock.rules, skill.name()).or_else(|| get_entry(&lock.skills, skill.name()));
        if let Some(entry) = entry {
            if compute_skill_hash(skill)? != entry.hash {
                modified.insert(skill.name().to_string());
            }
        }
    }
    let common = current.intersection(&locked).count();
    report.unchanged = common - modified.len();
    report.modified = modified.into_iter().collect();
    Ok(report)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusRow {
    pub name: String,
    pub client: String,
    pub present: bool,
}

#[cfg(test)]
pub fn has_drift(rows: &[StatusRow]) -> bool {
    rows.iter().any(|r| !r.present)
}

/// Per skill and synced client: does the primary output file still exist?
#[cfg(test)]
pub fn skills_status(
    lock: &LockFile,
    registry: &TranspilerRegistry,
    root: &Path,
) -> Vec<StatusRow> {
    let mut rows = Vec::new();
    for (name, entry) in lock.skills.iter().chain(lock.rules.iter()) {
        let skill_type = if get_entry(&lock.rules, name).is_some() {
            SkillType::Rule
        } else {
            SkillType::Skill
        };
        for client in &entry.clients_synced {
            let Some(t) = registry.get(client) else {
                continue;
            };
            let path = t.get_output_path(&Skill::placeholder(name, skill_type), root);
            rows.push(StatusRow {
                name: name.clone(),
                client: client.clone(),
                present: path.exists(),
            });
        }
    }
    rows
}

pub fn agents_status(
    lock: &LockFile,
    transpilers: &[Box<dyn AgentTranspiler>],
    root: &Path,
) -> Vec<StatusRow> {
    let mut rows = Vec::new();
    for (name, entry) in &lock.agents {
        for client in &entry.clients_synced {
            let Some(t) = transpilers.iter().find(|t| t.client_key() == client) else {
                continue;
            };
            let path = t.get_output_path(&super::agents::Agent::placeholder(name), root);
            rows.push(StatusRow {
                name: name.clone(),
                client: client.clone(),
                present: path.exists(),
            });
        }
    }
    rows
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleStatus {
    /// Tier-1 client key and the styles the lock says were synced to it.
    pub native: Vec<(String, Vec<String>)>,
    /// Tier-2 client key and its active style, if any.
    pub active: Vec<(String, Option<String>)>,
}

pub fn styles_status(lock: &LockFile) -> StyleStatus {
    let mut status = StyleStatus {
        native: Vec::new(),
        active: Vec::new(),
    };
    for t in all_style_transpilers() {
        let key = t.client_key().to_string();
        match t.tier() {
            Tier::Native => {
                let synced = lock
                    .styles
                    .iter()
                    .filter(|(_, e)| e.clients_synced.contains(&key))
                    .map(|(n, _)| n.clone())
                    .collect();
                status.native.push((key, synced));
            }
            Tier::ApplyRemove => {
                let active = lock
                    .active_styles
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.clone());
                status.active.push((key, active));
            }
        }
    }
    status
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanOutcome {
    pub removed: Vec<PathBuf>,
    pub lockfile_removed: bool,
    pub skipped: Vec<(String, String)>,
}

/// `mcpm skills clean`: every transpiler (or one `client`) removes what the lock says it wrote;
/// the lockfile itself goes away only on a full clean.
#[cfg(test)]
pub fn clean_skills(
    lock_dir: &Path,
    clean_root: &Path,
    registry: &TranspilerRegistry,
    client: Option<&str>,
    lock: Option<&LockFile>,
) -> CleanOutcome {
    let mut out = CleanOutcome::default();
    let Some(lock) = lock else {
        return out;
    };
    let managed: Vec<String> = lock
        .skills
        .iter()
        .chain(lock.rules.iter())
        .map(|(k, _)| k.clone())
        .filter(|k| valid_name(k).is_ok())
        .collect();
    if managed.is_empty() {
        return out;
    }
    for t in registry
        .all()
        .filter(|t| client.is_none_or(|c| c == t.client_key()))
    {
        match t.clean(clean_root, &managed) {
            Ok(removed) => out.removed.extend(removed),
            Err(e) => out.skipped.push((t.client_key().to_string(), e)),
        }
    }
    let path = lockfile_path(lock_dir);
    if client.is_none() && path.exists() && fs::remove_file(&path).is_ok() {
        out.lockfile_removed = true;
    }
    out
}

/// `mcpm agents clean`: removes agent outputs but, unlike skills, leaves the lockfile alone.
pub fn clean_agents(
    clean_root: &Path,
    transpilers: &[Box<dyn AgentTranspiler>],
    client: Option<&str>,
    lock: Option<&LockFile>,
) -> CleanOutcome {
    let mut out = CleanOutcome::default();
    let Some(lock) = lock else {
        return out;
    };
    let managed: Vec<String> = lock
        .agents
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| valid_name(k).is_ok())
        .collect();
    if managed.is_empty() {
        return out;
    }
    for t in transpilers
        .iter()
        .filter(|t| client.is_none_or(|c| c == t.client_key()))
    {
        match t.clean(clean_root, &managed) {
            Ok(removed) => out.removed.extend(removed),
            Err(e) => out.skipped.push((t.client_key().to_string(), e)),
        }
    }
    out
}

/// `mcpm styles clean`: every style transpiler cleans, then the lock forgets styles entirely.
pub fn clean_styles(root: &Path, lock: Option<&mut LockFile>) -> CleanOutcome {
    let mut out = CleanOutcome::default();
    let managed: Vec<String> = lock
        .as_ref()
        .map(|l| {
            l.styles
                .iter()
                .map(|(k, _)| k.clone())
                .filter(|k| valid_name(k).is_ok())
                .collect()
        })
        .unwrap_or_default();
    for t in all_style_transpilers() {
        match t.clean(root, &managed) {
            Ok(removed) => out.removed.extend(removed),
            Err(e) => out.skipped.push((t.client_key().to_string(), e)),
        }
    }
    if let Some(lock) = lock {
        lock.styles = OrderedMap::new();
        lock.active_styles = OrderedMap::new();
    }
    out
}

#[cfg(test)]
pub struct ResolveRequest<'a> {
    pub client: Option<&'a str>,
    pub global_mode: bool,
    pub dry_run: bool,
    pub migrate: Option<bool>,
    pub output_root: &'a Path,
    pub clock: &'a dyn Clock,
}

/// `mcpm skills resolve`: per-file clients only, project-only clients dropped in global mode.
#[cfg(test)]
pub fn resolve_skill_collisions(
    skills: &[Skill],
    registry: &TranspilerRegistry,
    req: &ResolveRequest<'_>,
) -> Result<(Vec<Collision>, CollisionSummary), String> {
    let transpilers: Vec<&dyn Transpiler> = registry
        .all()
        .filter(|t| req.client.is_none_or(|c| c == t.client_key()))
        .filter(|t| !(req.global_mode && PROJECT_ONLY_TRANSPILERS.contains(&t.client_key())))
        .filter(|t| !APPEND_MODE_TRANSPILERS.contains(&t.client_key()))
        .collect();
    let found = detect_collisions(skills, &transpilers, req.output_root);
    let summary = resolve_collisions(
        found.clone(),
        req.output_root,
        resolve_mode(req.migrate),
        req.dry_run,
        req.clock,
    )?;
    Ok((found, summary))
}
