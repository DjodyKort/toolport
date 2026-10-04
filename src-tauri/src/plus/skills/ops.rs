//! Read-mostly operations over the lockfile and the output tree: repo discovery, diff, status,
//! clean and collision triage. Mirrors the data side of mcpm's `skills|agents|styles` commands;
//! rendering and prompting stay with the caller.

use super::agents::{Agent, AgentTranspiler};
use super::assets::compute_skill_hash;
use super::clock::Clock;
use super::collisions::{
    detect_collisions, resolve_collisions, resolve_mode, Collision, CollisionSummary,
};
use super::json;
use super::lock::{get_entry, load_lockfile, lockfile_path, save_lockfile, LockFile, OrderedMap};
use super::parser::{valid_name, Skill, SkillType};
use super::pyfs::text_hash;
use super::styles::{all_style_transpilers, Style, Tier};
use super::transpiler::{Transpiler, TranspilerRegistry};
use crate::registry;
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

/// Where a sync keeps its lock and writes its outputs: user level (the lock beside the registry,
/// outputs under `~/`) or inside the repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub lock_dir: PathBuf,
    pub output_root: PathBuf,
}

pub fn lock_dir(global: bool, repo: &Path) -> PathBuf {
    if global {
        registry::conduit_dir().unwrap_or_else(|| repo.to_path_buf())
    } else {
        repo.to_path_buf()
    }
}

impl Scope {
    pub fn new(global: bool, repo: &Path) -> Result<Self, String> {
        let output_root = if global {
            crate::clients::home().ok_or("home directory unknown")?
        } else {
            repo.to_path_buf()
        };
        Ok(Self {
            lock_dir: lock_dir(global, repo),
            output_root,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockSource {
    Repo,
    UserLevel,
}

/// The lock a read-only command sees: the repository's own (project sync), else the user-level
/// one beside the registry (global sync).
pub fn read_lock(repo: &Path) -> Option<(LockFile, LockSource)> {
    if let Some(lock) = load_lockfile(repo) {
        return Some((lock, LockSource::Repo));
    }
    let lock = load_lockfile(&registry::conduit_dir()?)?;
    Some((lock, LockSource::UserLevel))
}

/// The root a lock's output files were written under.
pub fn lock_output_root(source: LockSource, repo: &Path) -> Result<PathBuf, String> {
    match source {
        LockSource::Repo => Ok(repo.to_path_buf()),
        LockSource::UserLevel => {
            crate::clients::home().ok_or_else(|| "home directory unknown".to_string())
        }
    }
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

/// `mcpm agents diff`: the agents' names and file hashes against the lock's agents section. Without
/// a lock every agent is new, listed in discovery order.
pub fn diff_agents(agents: &[Agent], lock: Option<&LockFile>) -> Result<DiffReport, String> {
    let Some(lock) = lock else {
        return Ok(DiffReport {
            no_lockfile: true,
            new: agents.iter().map(|a| a.name().to_string()).collect(),
            ..DiffReport::default()
        });
    };
    let current: BTreeSet<&str> = agents.iter().map(Agent::name).collect();
    let locked: BTreeSet<&str> = lock.agents.iter().map(|(k, _)| k.as_str()).collect();
    let mut report = DiffReport {
        new: current.difference(&locked).map(|s| s.to_string()).collect(),
        removed: locked.difference(&current).map(|s| s.to_string()).collect(),
        ..DiffReport::default()
    };
    let mut modified = BTreeSet::new();
    for agent in agents {
        if let Some(entry) = get_entry(&lock.agents, agent.name()) {
            if text_hash(&agent.source_path)? != entry.hash {
                modified.insert(agent.name().to_string());
            }
        }
    }
    report.unchanged = current.intersection(&locked).count() - modified.len();
    report.modified = modified.into_iter().collect();
    Ok(report)
}

/// `mcpm styles diff`: the styles' names and file hashes against the lock's styles section. Without
/// a lock every style is new, listed in discovery order.
pub fn diff_styles(styles: &[Style], lock: Option<&LockFile>) -> Result<DiffReport, String> {
    let Some(lock) = lock else {
        return Ok(DiffReport {
            no_lockfile: true,
            new: styles.iter().map(|s| s.name().to_string()).collect(),
            ..DiffReport::default()
        });
    };
    let current: BTreeSet<&str> = styles.iter().map(Style::name).collect();
    let locked: BTreeSet<&str> = lock.styles.iter().map(|(k, _)| k.as_str()).collect();
    let mut report = DiffReport {
        new: current.difference(&locked).map(|s| s.to_string()).collect(),
        removed: locked.difference(&current).map(|s| s.to_string()).collect(),
        ..DiffReport::default()
    };
    let mut modified = BTreeSet::new();
    for style in styles {
        if let Some(entry) = get_entry(&lock.styles, style.name()) {
            if text_hash(&style.source_path)? != entry.hash {
                modified.insert(style.name().to_string());
            }
        }
    }
    report.unchanged = current.intersection(&locked).count() - modified.len();
    report.modified = modified.into_iter().collect();
    Ok(report)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusRow {
    pub name: String,
    pub client: String,
    pub present: bool,
}

pub fn has_drift(rows: &[StatusRow]) -> bool {
    rows.iter().any(|r| !r.present)
}

/// Per skill and synced client: does the primary output file still exist?
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
    for (name, entry) in lock.agents.iter().filter(|(name, _)| valid_name(name).is_ok()) {
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

/// A tier-1 client and the styles the lock says were synced to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeStyleRow {
    pub client: String,
    pub display_name: String,
    pub styles: Vec<String>,
}

/// A tier-2 client and its active style, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveStyleRow {
    pub client: String,
    pub display_name: String,
    pub active: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleStatus {
    pub native: Vec<NativeStyleRow>,
    pub active: Vec<ActiveStyleRow>,
}

pub fn styles_status(lock: &LockFile) -> StyleStatus {
    let mut status = StyleStatus {
        native: Vec::new(),
        active: Vec::new(),
    };
    for t in all_style_transpilers() {
        let client = t.client_key().to_string();
        let display_name = t.display_name().to_string();
        match t.tier() {
            Tier::Native => {
                let styles = lock
                    .styles
                    .iter()
                    .filter(|(_, e)| e.clients_synced.contains(&client))
                    .map(|(n, _)| n.clone())
                    .collect();
                status.native.push(NativeStyleRow {
                    client,
                    display_name,
                    styles,
                });
            }
            Tier::ApplyRemove => {
                let active = lock
                    .active_styles
                    .iter()
                    .find(|(k, _)| *k == client)
                    .map(|(_, v)| v.clone());
                status.active.push(ActiveStyleRow {
                    client,
                    display_name,
                    active,
                });
            }
        }
    }
    status
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CleanOutcome {
    pub managed: Vec<String>,
    pub removed: Vec<PathBuf>,
    pub lockfile_removed: bool,
    pub skipped: Vec<(String, String)>,
    /// Lock entries whose names are not valid skill names; they never reach a transpiler.
    pub ignored: Vec<String>,
}

fn managed_names<'a>(names: impl Iterator<Item = &'a String>) -> (Vec<String>, Vec<String>) {
    let (valid, ignored): (Vec<&String>, Vec<&String>) =
        names.partition(|name| valid_name(name).is_ok());
    let mut managed: Vec<String> = Vec::new();
    for name in valid {
        if !managed.contains(name) {
            managed.push(name.clone());
        }
    }
    (managed, ignored.into_iter().cloned().collect())
}

/// `mcpm skills clean`: every transpiler (or one `client`) removes what the lock says it wrote;
/// the lockfile itself goes away only on a full clean. A dry run reports the same paths and
/// leaves the disk alone.
pub fn clean_skills(
    lock_dir: &Path,
    clean_root: &Path,
    registry: &TranspilerRegistry,
    client: Option<&str>,
    lock: Option<&LockFile>,
    dry_run: bool,
) -> CleanOutcome {
    let mut out = CleanOutcome::default();
    let Some(lock) = lock else {
        return out;
    };
    let (managed, ignored) =
        managed_names(lock.skills.iter().chain(lock.rules.iter()).map(|(k, _)| k));
    out.ignored = ignored;
    out.managed = managed.clone();
    if managed.is_empty() {
        return out;
    }
    for t in registry
        .all()
        .filter(|t| client.is_none_or(|c| c == t.client_key()))
    {
        if dry_run {
            out.removed.extend(t.clean_targets(clean_root, &managed));
            continue;
        }
        match t.clean(clean_root, &managed) {
            Ok(removed) => out.removed.extend(removed),
            Err(e) => out.skipped.push((t.client_key().to_string(), e)),
        }
    }
    let path = lockfile_path(lock_dir);
    if client.is_none() && path.exists() && (dry_run || fs::remove_file(&path).is_ok()) {
        out.lockfile_removed = true;
    }
    out
}

/// `mcpm agents clean`: removes agent outputs but, unlike skills, leaves the lockfile alone. A
/// dry run reports the same paths and leaves the disk alone.
pub fn clean_agents(
    clean_root: &Path,
    transpilers: &[Box<dyn AgentTranspiler>],
    client: Option<&str>,
    lock: Option<&LockFile>,
    dry_run: bool,
) -> CleanOutcome {
    let mut out = CleanOutcome::default();
    let Some(lock) = lock else {
        return out;
    };
    let (managed, ignored) = managed_names(lock.agents.iter().map(|(k, _)| k));
    out.ignored = ignored;
    out.managed = managed.clone();
    if managed.is_empty() {
        return out;
    }
    for t in transpilers
        .iter()
        .filter(|t| client.is_none_or(|c| c == t.client_key()))
    {
        if dry_run {
            out.removed.extend(t.clean_targets(clean_root, &managed));
            continue;
        }
        match t.clean(clean_root, &managed) {
            Ok(removed) => out.removed.extend(removed),
            Err(e) => out.skipped.push((t.client_key().to_string(), e)),
        }
    }
    out
}

/// `mcpm styles clean`: every style transpiler cleans, then the lock forgets styles entirely. A
/// dry run reports the same paths and leaves the disk and the lock alone.
pub fn clean_styles(root: &Path, lock: Option<&mut LockFile>, dry_run: bool) -> CleanOutcome {
    let mut out = CleanOutcome::default();
    let (managed, ignored) = managed_names(
        lock.as_ref()
            .into_iter()
            .flat_map(|l| l.styles.iter().map(|(k, _)| k)),
    );
    out.ignored = ignored;
    out.managed = managed.clone();
    for t in all_style_transpilers() {
        if dry_run {
            out.removed.extend(t.clean_targets(root, &managed));
            continue;
        }
        match t.clean(root, &managed) {
            Ok(removed) => out.removed.extend(removed),
            Err(e) => out.skipped.push((t.client_key().to_string(), e)),
        }
    }
    if let Some(lock) = lock.filter(|_| !dry_run) {
        lock.styles = OrderedMap::new();
        lock.active_styles = OrderedMap::new();
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UninstallOutcome {
    pub source: PathBuf,
    pub outputs: Vec<PathBuf>,
    pub lock_updated: bool,
}

/// The source directory of `name` under the first of `buckets` that has it. Names that are not
/// valid and directories that resolve outside the repository are refused before anything is
/// touched.
fn source_dir(repo: &Path, name: &str, buckets: &[&str], noun: &str) -> Result<PathBuf, String> {
    valid_name(name).map_err(|e| format!("Invalid {noun} name '{name}': {e}"))?;
    let source = buckets
        .iter()
        .map(|dir| repo.join(dir).join(name))
        .find(|path| path.is_dir())
        .ok_or_else(|| format!("{} '{name}' not found.", capitalized(noun)))?;
    let inside = match (source.canonicalize(), repo.canonicalize()) {
        (Ok(dir), Ok(root)) => dir != root && dir.starts_with(&root),
        _ => false,
    };
    if !inside {
        return Err(format!("'{name}' resolves outside the repository"));
    }
    Ok(source)
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// `mcpm skills uninstall`: every transpiler drops its outputs for `name`, the source directory
/// goes away and the lock forgets the entry. Names that are not valid skill names and source
/// directories that resolve outside the repository are refused before anything is touched.
pub fn uninstall_skill(
    repo: &Path,
    name: &str,
    scope: &Scope,
    registry: &TranspilerRegistry,
    dry_run: bool,
) -> Result<UninstallOutcome, String> {
    let source = source_dir(repo, name, &["skills", "rules"], "skill")?;
    let managed = [name.to_string()];
    let mut outputs = Vec::new();
    for t in registry.all() {
        if dry_run {
            outputs.extend(t.clean_targets(&scope.output_root, &managed));
        } else {
            let removed = t
                .clean(&scope.output_root, &managed)
                .map_err(|e| format!("{}: {e}", t.client_key()))?;
            outputs.extend(removed);
        }
    }
    if !dry_run {
        fs::remove_dir_all(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    }
    let mut lock_updated = false;
    if let Some(mut lock) = load_lockfile(&scope.lock_dir) {
        let before = lock.skills.len() + lock.rules.len();
        lock.skills.retain(|(k, _)| k != name);
        lock.rules.retain(|(k, _)| k != name);
        lock_updated = lock.skills.len() + lock.rules.len() != before;
        if lock_updated && !dry_run {
            save_lockfile(&scope.lock_dir, &lock)?;
        }
    }
    Ok(UninstallOutcome {
        source,
        outputs,
        lock_updated,
    })
}

/// `mcpm agents uninstall`: the agent's outputs for every client, its source directory and its
/// lock entry. mcpm passes a keyword the transpilers do not take, so the command raises a
/// TypeError on every call; this is what it was written to do.
pub fn uninstall_agent(
    repo: &Path,
    name: &str,
    scope: &Scope,
    transpilers: &[Box<dyn AgentTranspiler>],
    dry_run: bool,
) -> Result<UninstallOutcome, String> {
    let source = source_dir(repo, name, &["agents"], "agent")?;
    let managed = [name.to_string()];
    let mut outputs = Vec::new();
    for t in transpilers {
        if dry_run {
            outputs.extend(t.clean_targets(&scope.output_root, &managed));
        } else {
            let removed = t
                .clean(&scope.output_root, &managed)
                .map_err(|e| format!("{}: {e}", t.client_key()))?;
            outputs.extend(removed);
        }
    }
    if !dry_run {
        fs::remove_dir_all(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    }
    let mut lock_updated = false;
    if let Some(mut lock) = load_lockfile(&scope.lock_dir) {
        let before = lock.agents.len();
        lock.agents.retain(|(k, _)| k != name);
        lock_updated = lock.agents.len() != before;
        if lock_updated && !dry_run {
            save_lockfile(&scope.lock_dir, &lock)?;
        }
    }
    Ok(UninstallOutcome {
        source,
        outputs,
        lock_updated,
    })
}

pub struct ResolveRequest<'a> {
    pub client: Option<&'a str>,
    pub global_mode: bool,
    pub dry_run: bool,
    pub migrate: Option<bool>,
    pub output_root: &'a Path,
    pub clock: &'a dyn Clock,
}

/// `mcpm skills resolve`: per-file clients only, project-only clients dropped in global mode.
pub fn resolve_skill_collisions(
    skills: &[Skill],
    registry: &TranspilerRegistry,
    req: &ResolveRequest<'_>,
) -> Result<(Vec<Collision>, CollisionSummary), String> {
    let transpilers: Vec<&dyn Transpiler> = registry
        .all()
        .filter(|t| req.client.is_none_or(|c| c == t.client_key()))
        .filter(|t| !(req.global_mode && t.capabilities().project_only))
        .filter(|t| !t.capabilities().append_mode)
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
