//! The sync orchestration: transpile, plan every file against the disk and the previous lock,
//! write what is safe, hold what needs a decision, record the lock, resolve collisions.
//!
//! What a sync does with one installed file:
//!
//! - missing: added; identical and already in the lock: unchanged; identical but not in the lock:
//!   adopted (recorded, nothing written); different and in the lock: updated.
//! - different and in the lock, but the file no longer matches the hash the lock recorded for it:
//!   held as "edited" (someone changed it after the sync installed it). A lock without hashes
//!   records them on this sync and holds nothing.
//! - different and not in the lock: held as "differs" (the file is not ours to replace).
//! - in the lock, no longer in the source: held as a removal. Files the lock never recorded are
//!   never touched.
//! - a hook a skill declares that no earlier sync installed: held until confirmed.
//!
//! Held changes are applied only with `apply_held`; a differing, edited or removed edited file is
//! moved to the backup folder first. `take` applies that to single files, and a file the person
//! chose to keep (`skills keep`) stops being held for as long as its content stays as it was.

use super::assets::{compute_skill_hash, inventory, is_plain_relative, rel_string, SkillFile};
use super::client_scope::carry_retired_clients;
use super::clock::Clock;
use super::collisions::{
    backup_path_aside, detect_collisions, resolve_collisions, resolve_mode, CollisionSummary,
    ResolutionMode,
};
use super::keep::{self, Kept};
use super::lock::{get_entry, get_entry_mut, load_lockfile, set_entry, LockEntry, LockFile};
use super::origin::{is_tap, read_origins};
use super::parser::{Skill, SkillType};
use super::pyfs::write_text;
use super::snapshot::Journal;
use super::sync_extras::{sync_extras, Extras};
use super::transpiler::{claude_folder, Transpiler, TranspilerRegistry};
use crate::hashing::lock_hash;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use toolport_plus_api::SettingsStore;

pub struct SyncOptions<'a> {
    pub output_root: PathBuf,
    /// Directory holding `mcpm-skills.lock`: the project root, or the user config dir in global mode.
    pub lock_dir: PathBuf,
    pub global_mode: bool,
    pub dry_run: bool,
    pub migrate: Option<bool>,
    pub client_keys: Option<Vec<String>>,
    /// Also apply removals, hooks that are new and replacements of files that differ.
    pub apply_held: bool,
    /// With `apply_held`, also install the hooks of skills whose lock source is a tap.
    pub allow_tap_hooks: bool,
    pub clock: &'a dyn Clock,
    /// Where hooks are installed and revoked; `None` for a run that never writes settings.
    pub settings: Option<&'a dyn SettingsStore>,
    /// A run nobody watches: a file the lock tracks but holds no hash for is held when it differs,
    /// as it may have been edited by hand.
    pub unattended: bool,
    /// Told about every file before the run writes or removes it; `None` keeps no snapshot.
    pub journal: Option<&'a Journal>,
    /// Files to replace with the library copy although they are held: `<skill>/<path in the
    /// skill folder>`, backed up first.
    pub take: Vec<String>,
    /// Only these skills and rules are synced; every other entry of the lock stays as it is.
    pub only: Option<Vec<String>>,
    /// The library's agents and output styles, synced in the same run; `None` leaves them out.
    pub extras: Option<Extras>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Update,
    Adopt,
    Unchanged,
    Differs,
    Edited,
    Remove,
    Hook,
    /// A file that differs and that the person chose to keep as it is.
    Kept,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Update => "update",
            Self::Adopt => "adopt",
            Self::Unchanged => "unchanged",
            Self::Differs => "differs",
            Self::Edited => "edited",
            Self::Remove => "remove",
            Self::Hook => "hook",
            Self::Kept => "kept",
        }
    }

    /// Changes whose effect is not visible until later wait for a decision.
    pub fn needs_decision(self) -> bool {
        matches!(
            self,
            Self::Differs | Self::Edited | Self::Remove | Self::Hook
        )
    }
}

/// What a plan row is about: a skill or rule, an agent or an output style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Skill,
    Agent,
    Style,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Agent => "agent",
            Self::Style => "style",
        }
    }

    /// How `--take`, `--only` and a kept file name an agent or a style: skills and rules go bare.
    pub fn scoped(self, name: &str) -> String {
        match self {
            Self::Skill => name.to_string(),
            other => format!("{}:{name}", other.as_str()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub kind: Kind,
    pub skill: String,
    pub client: String,
    /// Relative to the output root; absolute when the file lies outside it.
    pub path: String,
    pub op: Op,
    /// Needs a decision and was not applied.
    pub held: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedEntry {
    pub skill: String,
    pub path: String,
    pub reason: String,
}

/// One step of a run that did not complete; the rest of the run went on without it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: Kind,
    pub name: String,
    pub client: String,
    /// `files` or `hooks`.
    pub step: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub skipped: Vec<SkippedEntry>,
    pub failed: Vec<Failure>,
}

impl Plan {
    pub fn count(&self, op: Op) -> usize {
        self.changes.iter().filter(|c| c.op == op).count()
    }

    pub fn held(&self) -> impl Iterator<Item = &Change> {
        self.changes.iter().filter(|c| c.held)
    }
}

#[derive(Debug)]
pub struct SyncResult {
    pub lockfile: LockFile,
    /// Files removed, or that a dry run or `apply_held` would remove.
    pub cleaned: Vec<PathBuf>,
    pub collisions: CollisionSummary,
    pub output_root: PathBuf,
    pub plan: Plan,
}

pub(crate) fn rel_or_abs(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => rel_string(rel),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Deletes the files and prunes the directories they leave empty, never `stop_at` or above it.
pub fn delete_stale(stale: &[PathBuf], stop_at: &Path) -> Vec<PathBuf> {
    let mut deleted = Vec::new();
    for path in stale {
        if fs::symlink_metadata(path).is_err() || fs::remove_file(path).is_err() {
            continue;
        }
        deleted.push(path.clone());
        prune_empty_parents(path, stop_at);
    }
    deleted
}

fn prune_empty_parents(path: &Path, stop_at: &Path) {
    // A moved Claude folder (`CLAUDE_CONFIG_DIR`) is the floor for what lies in it, so pruning
    // never climbs out of it, whether it is inside the home or not.
    let claude = claude_folder(stop_at);
    let floor = if claude != stop_at.join(".claude") && path.starts_with(&claude) {
        claude
    } else {
        stop_at.to_path_buf()
    };
    let mut parent = path.parent().map(Path::to_path_buf);
    while let Some(dir) = parent.take() {
        let empty = dir != floor
            && dir.starts_with(&floor)
            && dir.is_dir()
            && fs::read_dir(&dir).is_ok_and(|mut r| r.next().is_none());
        if !empty || fs::remove_dir(&dir).is_err() {
            break;
        }
        parent = dir.parent().map(Path::to_path_buf);
    }
}

fn bucket<'a>(lock: &'a mut LockFile, skill: &Skill) -> &'a mut Vec<(String, LockEntry)> {
    match skill.skill_type {
        SkillType::Rule => &mut lock.rules,
        SkillType::Skill => &mut lock.skills,
    }
}

enum Source {
    Text(String),
    File(PathBuf),
    Link(PathBuf),
}

pub(crate) struct Desired {
    pub(crate) abs: PathBuf,
    pub(crate) rel: String,
    source: Source,
}

#[derive(PartialEq)]
pub(crate) enum Found {
    Missing,
    Same,
    /// The content matches and the executable bits of the library copy do not.
    Mode,
    Different,
}

impl Desired {
    pub(crate) fn text(abs: PathBuf, rel: String, text: String) -> Self {
        Self {
            abs,
            rel,
            source: Source::Text(text),
        }
    }

    fn content_hash(&self) -> String {
        match &self.source {
            Source::Text(text) => lock_hash(text),
            Source::File(src) => fs::read(src).map(lock_hash).unwrap_or_default(),
            Source::Link(text) => lock_hash(format!("link:{}", text.display())),
        }
    }
}

/// The hash a file would have been recorded with: links by their target text, files by content.
pub(crate) fn installed_hash(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        return Some(lock_hash(format!(
            "link:{}",
            fs::read_link(path).ok()?.display()
        )));
    }
    meta.is_file().then(|| fs::read(path).ok().map(lock_hash))?
}

pub(crate) fn compare(desired: &Desired) -> Found {
    let Ok(meta) = fs::symlink_metadata(&desired.abs) else {
        return Found::Missing;
    };
    let same = match &desired.source {
        Source::Text(text) => {
            meta.is_file() && fs::read(&desired.abs).is_ok_and(|b| b == text.as_bytes())
        }
        Source::File(src) => {
            meta.is_file()
                && matches!((fs::read(&desired.abs), fs::read(src)), (Ok(a), Ok(b)) if a == b)
        }
        Source::Link(text) => {
            meta.file_type().is_symlink() && fs::read_link(&desired.abs).is_ok_and(|t| &t == text)
        }
    };
    match (same, &desired.source) {
        (false, _) => Found::Different,
        (true, Source::File(src)) if executable_differs(src, &meta) => Found::Mode,
        (true, _) => Found::Same,
    }
}

#[cfg(unix)]
fn executable_differs(source: &Path, installed: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(source)
        .is_ok_and(|m| m.permissions().mode() & 0o111 != installed.permissions().mode() & 0o111)
}

#[cfg(not(unix))]
fn executable_differs(_source: &Path, _installed: &fs::Metadata) -> bool {
    false
}

fn install(desired: &Desired) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("{}: {e}", desired.abs.display());
    match &desired.source {
        Source::Text(text) => write_text(&desired.abs, text),
        Source::File(src) => {
            if let Some(parent) = desired.abs.parent() {
                fs::create_dir_all(parent).map_err(fail)?;
            }
            if fs::symlink_metadata(&desired.abs).is_ok_and(|m| m.file_type().is_symlink()) {
                fs::remove_file(&desired.abs).map_err(fail)?;
            }
            fs::copy(src, &desired.abs).map(drop).map_err(fail)
        }
        Source::Link(text) => {
            if let Some(parent) = desired.abs.parent() {
                fs::create_dir_all(parent).map_err(fail)?;
            }
            let _ = fs::remove_file(&desired.abs);
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(text, &desired.abs).map_err(fail)
            }
            #[cfg(not(unix))]
            {
                let target = desired.abs.parent().unwrap_or(Path::new(".")).join(text);
                fs::copy(target, &desired.abs).map(drop).map_err(fail)
            }
        }
    }
}

/// Everything one skill is installed as for one client: the transpiled file and, next to a
/// SKILL.md, the rest of the folder.
pub(crate) struct Layout {
    pub(crate) files: Vec<Desired>,
    pub(crate) skipped: Vec<SkippedEntry>,
    pub(crate) warnings: Vec<String>,
}

pub(crate) fn desired_for(
    t: &dyn Transpiler,
    skill: &Skill,
    output_root: &Path,
) -> Result<Layout, String> {
    let result = t.transpile(skill, output_root)?;
    let warnings = result.warnings.clone();
    let mut skipped_out = Vec::new();
    let mut out = vec![Desired {
        rel: rel_or_abs(&result.output_path, output_root),
        abs: result.output_path.clone(),
        source: Source::Text(result.content),
    }];
    let is_skill_file = result
        .output_path
        .file_name()
        .is_some_and(|n| n == "SKILL.md");
    if skill.skill_type == SkillType::Skill && is_skill_file {
        let dst_dir = result.output_path.parent().unwrap_or(output_root);
        let found = inventory(skill.source_dir());
        for SkillFile { rel, link } in found.files {
            let abs = dst_dir.join(&rel);
            out.push(Desired {
                rel: rel_or_abs(&abs, output_root),
                source: match link {
                    Some(text) => Source::Link(text),
                    None => Source::File(skill.source_dir().join(&rel)),
                },
                abs,
            });
        }
        for skipped in found.skipped {
            skipped_out.push(SkippedEntry {
                skill: skill.name().to_string(),
                path: skipped.rel,
                reason: skipped.reason.describe(),
            });
        }
    }
    Ok(Layout {
        files: out,
        skipped: skipped_out,
        warnings,
    })
}

fn tracked_rels(previous: Option<&LockFile>, skill: &Skill, client: &str) -> BTreeSet<String> {
    let Some(prev) = previous else {
        return BTreeSet::new();
    };
    let map = match skill.skill_type {
        SkillType::Rule => &prev.rules,
        SkillType::Skill => &prev.skills,
    };
    get_entry(map, skill.name())
        .and_then(|e| e.output_files.iter().find(|(k, _)| k == client))
        .map(|(_, rels)| rels.iter().cloned().collect())
        .unwrap_or_default()
}

fn previous_entry<'a>(previous: Option<&'a LockFile>, skill: &Skill) -> Option<&'a LockEntry> {
    let prev = previous?;
    let map = match skill.skill_type {
        SkillType::Rule => &prev.rules,
        SkillType::Skill => &prev.skills,
    };
    get_entry(map, skill.name())
}

fn previous_hooks(previous: Option<&LockFile>, skill: &Skill, client: &str) -> Vec<String> {
    let Some(prev) = previous else {
        return Vec::new();
    };
    let map = match skill.skill_type {
        SkillType::Rule => &prev.rules,
        SkillType::Skill => &prev.skills,
    };
    get_entry(map, skill.name())
        .and_then(|e| e.hooks_installed.iter().find(|(k, _)| k == client))
        .map(|(_, ids)| ids.clone())
        .unwrap_or_default()
}

/// The path of an installed file inside the skill's folder, the way the screen and `--take` name
/// it: the files of a skill sit next to its first output.
pub(crate) fn rel_in_skill(abs: &Path, desired: &[Desired]) -> String {
    let dir = desired.first().and_then(|d| d.abs.parent());
    dir.and_then(|dir| abs.strip_prefix(dir).ok())
        .map_or_else(|| rel_string(abs), rel_string)
}

fn sync_one(
    t: &dyn Transpiler,
    skill: &Skill,
    ctx: &mut Ctx<'_>,
    entry: &mut LockEntry,
) -> Result<(), String> {
    let key = t.client_key().to_string();
    let layout = desired_for(t, skill, ctx.output_root)?;
    let desired = layout.files;
    ctx.plan.skipped.extend(layout.skipped);
    entry.clients_synced.push(key.clone());
    entry.warnings.extend(layout.warnings);
    let tracked = tracked_rels(ctx.previous, skill, &key);
    let previous = previous_entry(ctx.previous, skill);
    place_files(
        ctx,
        Placed {
            kind: Kind::Skill,
            name: skill.name(),
            client: &key,
        },
        &desired,
        &tracked,
        previous,
        entry,
    )?;
    sync_hooks(t, skill, ctx, entry)
}

pub(crate) struct Placed<'a> {
    pub(crate) kind: Kind,
    pub(crate) name: &'a str,
    pub(crate) client: &'a str,
}

/// Plans and writes the files one item installs for one client: each file is added, updated,
/// adopted, held for a decision or kept, and the lock entry learns what the item now owns.
pub(crate) fn place_files(
    ctx: &mut Ctx<'_>,
    item: Placed<'_>,
    desired: &[Desired],
    tracked: &BTreeSet<String>,
    previous: Option<&LockEntry>,
    entry: &mut LockEntry,
) -> Result<(), String> {
    let Placed {
        kind,
        name,
        client: key,
    } = item;
    let mut kept = Vec::new();
    for d in desired {
        ctx.desired_abs.insert(d.abs.clone());
        let found = compare(d);
        let is_tracked = tracked.contains(&d.rel);
        let recorded = previous.and_then(|e| e.file_hash(key, &d.rel));
        let edited_since_install = is_tracked
            && recorded.is_some_and(|hash| installed_hash(&d.abs).as_deref() != Some(hash));
        let unverifiable = ctx.unattended && is_tracked && recorded.is_none();
        let mut op = match (found, is_tracked) {
            (Found::Missing, _) => Op::Add,
            (Found::Same, true) => Op::Unchanged,
            (Found::Same, false) | (Found::Mode, false) => Op::Adopt,
            (Found::Mode, true) => Op::Update,
            (Found::Different, true) if edited_since_install || unverifiable => Op::Edited,
            (Found::Different, true) => Op::Update,
            (Found::Different, false) => Op::Differs,
        };
        let in_skill = rel_in_skill(&d.abs, desired);
        let scoped = kind.scoped(name);
        let taken = ctx.take.contains(&format!("{scoped}/{in_skill}"));
        let replace_held = ctx.apply_held || taken;
        if matches!(op, Op::Differs | Op::Edited)
            && !replace_held
            && ctx.is_kept(key, &scoped, &in_skill, &d.abs)
        {
            op = Op::Kept;
        }
        let mut held = false;
        let mut owned = true;
        let mut hash = d.content_hash();
        match op {
            Op::Add | Op::Update if !ctx.dry_run => {
                ctx.touch(&d.abs)?;
                install(d)?;
            }
            Op::Differs | Op::Edited if replace_held => {
                if !ctx.dry_run {
                    ctx.touch(&d.abs)?;
                    backup_path_aside(&d.abs, ctx.output_root, ctx.clock)?;
                    install(d)?;
                }
            }
            Op::Differs => {
                held = true;
                owned = false;
                entry.warnings.push(format!(
                    "{key}: {} differs from the library copy and is not tracked, held",
                    d.rel
                ));
            }
            Op::Edited => {
                held = true;
                hash = recorded.unwrap_or_default().to_string();
                entry.warnings.push(format!(
                    "{key}: {} was edited after the last sync, held",
                    d.rel
                ));
            }
            Op::Kept => {
                owned = is_tracked;
                hash = recorded.unwrap_or_default().to_string();
            }
            _ => {}
        }
        ctx.plan.changes.push(Change {
            kind,
            skill: name.to_string(),
            client: key.to_string(),
            path: d.rel.clone(),
            op,
            held,
        });
        if owned {
            if !hash.is_empty() {
                entry.set_file_hash(key, &d.rel, hash);
            }
            kept.push(d.rel.clone());
        }
    }
    entry.extend_outputs(key, kept);
    Ok(())
}

/// The recorded origin of a skill that did not start in the library; the origins file sits at the
/// repository root, two levels above the skill's folder.
fn library_origin(skill: &Skill) -> Option<String> {
    let repo = skill.source_dir().parent()?.parent()?;
    read_origins(repo).remove(skill.name())
}

fn sync_hooks(
    t: &dyn Transpiler,
    skill: &Skill,
    ctx: &mut Ctx<'_>,
    entry: &mut LockEntry,
) -> Result<(), String> {
    let key = t.client_key();
    let has_hooks = skill
        .frontmatter
        .hooks
        .as_ref()
        .is_some_and(|h| !h.is_empty());
    if skill.skill_type != SkillType::Skill || !has_hooks {
        return Ok(());
    }
    let known = previous_hooks(ctx.previous, skill, key);
    let wanted = t.hook_ids(skill, ctx.output_root);
    let new: Vec<&String> = wanted.iter().filter(|id| !known.contains(id)).collect();
    let tap_hooks_wait = is_tap(&entry.source) && !ctx.allow_tap_hooks;
    if !new.is_empty() && (!ctx.apply_held || tap_hooks_wait) {
        if ctx.apply_held {
            entry.warnings.push(format!(
                "{key}: hooks of a skill from {} wait for confirmation of tap hooks",
                entry.source
            ));
        }
        for id in new {
            ctx.plan.changes.push(Change {
                kind: Kind::Skill,
                skill: skill.name().to_string(),
                client: key.to_string(),
                path: id.clone(),
                op: Op::Hook,
                held: true,
            });
        }
        if !known.is_empty() {
            entry.set_hooks(key, known);
        }
        return Ok(());
    }
    for id in new {
        ctx.plan.changes.push(Change {
            kind: Kind::Skill,
            skill: skill.name().to_string(),
            client: key.to_string(),
            path: id.clone(),
            op: Op::Hook,
            held: false,
        });
    }
    if ctx.dry_run {
        return Ok(());
    }
    let Some(store) = ctx.settings else {
        entry
            .warnings
            .push(format!("{key}: hook install failed: no settings store"));
        return Ok(());
    };
    for id in &wanted {
        ctx.touch(Path::new(id))?;
    }
    match t.install_hooks(skill, ctx.output_root, store) {
        Ok(ids) if !ids.is_empty() => entry.set_hooks(key, ids),
        Ok(_) => {}
        Err(e) => {
            if !known.is_empty() {
                entry.set_hooks(key, known);
            }
            entry
                .warnings
                .push(format!("{key}: hook install failed: {e}"));
            ctx.plan.failed.push(Failure {
                kind: Kind::Skill,
                name: skill.name().to_string(),
                client: key.to_string(),
                step: "hooks",
                message: e,
            });
        }
    }
    Ok(())
}

pub(crate) struct Ctx<'a> {
    pub(crate) only: Option<&'a [String]>,
    pub(crate) take: &'a [String],
    pub(crate) kept: Kept,
    pub(crate) output_root: &'a Path,
    pub(crate) previous: Option<&'a LockFile>,
    pub(crate) dry_run: bool,
    pub(crate) apply_held: bool,
    pub(crate) allow_tap_hooks: bool,
    pub(crate) clock: &'a dyn Clock,
    pub(crate) settings: Option<&'a dyn SettingsStore>,
    pub(crate) journal: Option<&'a Journal>,
    pub(crate) unattended: bool,
    pub(crate) plan: Plan,
    pub(crate) desired_abs: BTreeSet<PathBuf>,
}

impl Ctx<'_> {
    /// A skill the run was not asked to sync.
    pub(crate) fn frozen(&self, name: &str) -> bool {
        self.only
            .is_some_and(|only| !only.iter().any(|n| n == name))
    }

    pub(crate) fn is_kept(&self, client: &str, skill: &str, in_skill: &str, abs: &Path) -> bool {
        self.kept
            .get(&keep::key(client, skill, in_skill))
            .is_some_and(|hash| installed_hash(abs).as_deref() == Some(hash))
    }

    pub(crate) fn touch(&self, path: &Path) -> Result<(), String> {
        match self.journal {
            Some(journal) if !self.dry_run => journal.before_change(path),
            _ => Ok(()),
        }
    }
}

struct Removal {
    name: String,
    client: String,
    rel: String,
    abs: PathBuf,
    /// The file no longer matches the hash recorded when it was installed.
    edited: bool,
}

/// Files a previous sync recorded that this run no longer installs, per skill and client, for the
/// clients this run synced. Entries with unsafe paths are skipped, so a tampered lock cannot reach
/// outside the output root.
fn removals(
    previous: &LockFile,
    new_lock: &LockFile,
    synced_clients: &BTreeSet<String>,
    ctx: &Ctx<'_>,
) -> Vec<Removal> {
    let mut out = Vec::new();
    for (prev, new) in [
        (&previous.skills, &new_lock.skills),
        (&previous.rules, &new_lock.rules),
    ] {
        for (name, entry) in prev {
            if ctx.frozen(name) {
                continue;
            }
            let now = get_entry(new, name);
            for (client, rels) in &entry.output_files {
                if !synced_clients.contains(client) {
                    continue;
                }
                let still: BTreeSet<&String> = now
                    .and_then(|e| e.output_files.iter().find(|(k, _)| k == client))
                    .map(|(_, r)| r.iter().collect())
                    .unwrap_or_default();
                for rel in rels {
                    if still.contains(rel) || !is_plain_relative(rel) {
                        continue;
                    }
                    let abs = ctx.output_root.join(rel);
                    if ctx.desired_abs.contains(&abs) {
                        continue;
                    }
                    let edited = entry
                        .file_hash(client, rel)
                        .is_some_and(|hash| installed_hash(&abs).as_deref() != Some(hash));
                    out.push(Removal {
                        name: name.clone(),
                        client: client.clone(),
                        rel: rel.clone(),
                        abs,
                        edited,
                    });
                }
            }
        }
    }
    out
}

pub fn sync_skills(
    skills: &[Skill],
    registry: &TranspilerRegistry,
    opts: &SyncOptions<'_>,
) -> Result<SyncResult, String> {
    let output_root = opts.output_root.as_path();
    let delivered_elsewhere =
        |s: &Skill| s.skill_type == SkillType::Rule && is_folder_rule(&s.source_path);
    let kept: Vec<Skill>;
    let skills = if skills.iter().any(delivered_elsewhere) {
        kept = skills
            .iter()
            .filter(|s| !delivered_elsewhere(s))
            .cloned()
            .collect();
        kept.as_slice()
    } else {
        skills
    };
    let mut lock = LockFile::new(opts.clock.now().isoformat());
    lock.scope = if opts.global_mode {
        "global"
    } else {
        "project"
    }
    .into();
    lock.output_root = output_root.to_string_lossy().into_owned();

    // sync only owns skills/rules; agents and styles belong to other sync paths.
    let previous = load_lockfile(&opts.lock_dir);
    if let Some(prev) = &previous {
        lock.agents = prev.agents.clone();
        lock.styles = prev.styles.clone();
        lock.active_styles = prev.active_styles.clone();
    }

    let selected: Vec<&dyn Transpiler> = registry
        .all()
        .filter(|t| {
            opts.client_keys
                .as_ref()
                .is_none_or(|keys| keys.iter().any(|k| k == t.client_key()))
        })
        .filter(|t| !(opts.global_mode && t.capabilities().project_only))
        .collect();
    let per_file: Vec<&dyn Transpiler> = selected
        .iter()
        .copied()
        .filter(|t| !t.capabilities().append_mode)
        .collect();
    let append: Vec<&dyn Transpiler> = selected
        .iter()
        .copied()
        .filter(|t| t.capabilities().append_mode)
        .collect();

    let mut ctx = Ctx {
        only: opts.only.as_deref(),
        take: &opts.take,
        kept: crate::fsx::data_dir()
            .map(|base| keep::load(&base))
            .unwrap_or_default(),
        output_root,
        previous: previous.as_ref(),
        dry_run: opts.dry_run,
        apply_held: opts.apply_held,
        allow_tap_hooks: opts.allow_tap_hooks,
        clock: opts.clock,
        settings: opts.settings,
        journal: opts.journal,
        unattended: opts.unattended,
        plan: Plan::default(),
        desired_abs: BTreeSet::new(),
    };

    for skill in skills {
        if ctx.frozen(skill.name()) {
            if let Some(entry) = previous_entry(ctx.previous, skill) {
                set_entry(bucket(&mut lock, skill), skill.name(), entry.clone());
            }
            continue;
        }
        let mut entry = LockEntry::new(skill.version(), compute_skill_hash(skill)?);
        if let Some(source) = library_origin(skill) {
            entry.source = source;
        }
        for t in &per_file {
            if let Err(e) = sync_one(*t, skill, &mut ctx, &mut entry) {
                let key = t.client_key();
                entry
                    .warnings
                    .push(format!("{key}: transpilation failed: {e}"));
                ctx.plan.failed.push(Failure {
                    kind: Kind::Skill,
                    name: skill.name().to_string(),
                    client: key.to_string(),
                    step: "files",
                    message: e.clone(),
                });
                let tracked = tracked_rels(ctx.previous, skill, key);
                for rel in &tracked {
                    if let Some(hash) =
                        previous_entry(ctx.previous, skill).and_then(|e| e.file_hash(key, rel))
                    {
                        entry.set_file_hash(key, rel, hash.to_string());
                    }
                }
                entry.extend_outputs(key, tracked.into_iter().collect());
            }
        }
        let name = skill.name().to_string();
        set_entry(bucket(&mut lock, skill), &name, entry);
    }

    if let Some(extras) = &opts.extras {
        sync_extras(
            &mut ctx,
            &mut lock,
            extras,
            opts.global_mode,
            &opts.client_keys,
        )?;
    }

    if let Some(prev) = ctx.previous.filter(|_| ctx.only.is_some()) {
        for (old, new) in [
            (&prev.skills, &mut lock.skills),
            (&prev.rules, &mut lock.rules),
        ] {
            for (name, entry) in old {
                if ctx.frozen(name) && get_entry(new, name).is_none() {
                    set_entry(new, name, entry.clone());
                }
            }
        }
    }

    for t in append.iter().filter(|_| opts.only.is_none()) {
        let Some(result) = t.transpile_all(skills, output_root) else {
            continue;
        };
        let Ok(result) = result else { continue };
        for skill in skills {
            let name = skill.name().to_string();
            let target = bucket(&mut lock, skill);
            if get_entry(target, &name).is_none() {
                set_entry(
                    target,
                    &name,
                    LockEntry::new(skill.version(), compute_skill_hash(skill)?),
                );
            }
            if let Some(e) = get_entry_mut(target, &name) {
                e.clients_synced.push(t.client_key().to_string());
                e.warnings.extend(result.warnings.iter().cloned());
            }
        }
        if !opts.dry_run && !result.content.is_empty() {
            ctx.touch(&result.output_path)?;
            write_text(&result.output_path, &result.content)?;
        }
    }

    let mut cleaned = Vec::new();
    if let Some(prev) = &previous {
        let synced: BTreeSet<String> = per_file
            .iter()
            .map(|t| t.client_key().to_string())
            .collect();
        let found = removals(prev, &lock, &synced, &ctx);
        let mut doomed = Vec::new();
        let mut carried: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        let mut backed_up = Vec::new();
        for Removal {
            name,
            client,
            rel,
            abs,
            edited,
        } in found
        {
            let gone = fs::symlink_metadata(&abs).is_err();
            if gone {
                continue;
            }
            ctx.plan.changes.push(Change {
                kind: Kind::Skill,
                skill: name.clone(),
                client: client.clone(),
                path: rel.clone(),
                op: Op::Remove,
                held: !opts.apply_held,
            });
            if !opts.apply_held {
                carried.entry(name).or_default().push((client, rel));
            } else if edited && !opts.dry_run {
                ctx.touch(&abs)?;
                backup_path_aside(&abs, output_root, opts.clock)?;
                prune_empty_parents(&abs, output_root);
                backed_up.push(abs);
            } else {
                ctx.touch(&abs)?;
                doomed.push(abs);
            }
        }
        cleaned = if opts.dry_run {
            doomed.clone()
        } else {
            let mut gone = delete_stale(&doomed, output_root);
            gone.extend(backed_up);
            gone
        };
        carry_held_removals(prev, &mut lock, &carried, &synced);
        if !opts.dry_run {
            revoke_stale_hooks(
                prev,
                &mut lock,
                &selected,
                output_root,
                opts.settings,
                |name| ctx.frozen(name),
            );
        }
    }

    if let Some(prev) = &previous {
        carry_retired_clients(prev, &mut lock, registry);
    }

    let found = match &opts.only {
        None => detect_collisions(skills, &per_file, output_root),
        Some(_) => {
            let active: Vec<Skill> = skills
                .iter()
                .filter(|s| !ctx.frozen(s.name()))
                .cloned()
                .collect();
            detect_collisions(&active, &per_file, output_root)
        }
    };
    if resolve_mode(opts.migrate) == ResolutionMode::AutoReplace {
        for collision in &found {
            ctx.touch(&collision.collision_path)?;
        }
    }
    let summary = resolve_collisions(
        found,
        output_root,
        resolve_mode(opts.migrate),
        opts.dry_run,
        opts.clock,
    )?;
    for r in summary.kept() {
        let c = &r.collision;
        let in_rules = get_entry(&lock.rules, &c.skill_name).is_some();
        let target = if in_rules {
            &mut lock.rules
        } else {
            &mut lock.skills
        };
        if let Some(entry) = get_entry_mut(target, &c.skill_name) {
            entry.warnings.push(format!(
                "{}: shadowed by existing file at {}",
                c.client_key,
                c.collision_path.display()
            ));
        }
    }

    Ok(SyncResult {
        lockfile: lock,
        cleaned,
        collisions: summary,
        output_root: output_root.to_path_buf(),
        plan: ctx.plan,
    })
}

/// A file whose removal is held stays in the lock, so it stays tracked and a later confirmed sync
/// can still remove it. A skill that left the library keeps an entry that lists only what is held,
/// plus whatever clients this run did not sync.
fn carry_held_removals(
    previous: &LockFile,
    lock: &mut LockFile,
    carried: &BTreeMap<String, Vec<(String, String)>>,
    synced: &BTreeSet<String>,
) {
    for (name, files) in carried {
        for (prev_map, new_map) in [
            (&previous.skills, &mut lock.skills),
            (&previous.rules, &mut lock.rules),
        ] {
            let Some(prev_entry) = get_entry(prev_map, name) else {
                continue;
            };
            if get_entry(new_map, name).is_none() {
                let mut entry = prev_entry.clone();
                entry
                    .output_files
                    .retain(|(client, _)| !synced.contains(client));
                entry
                    .file_hashes
                    .retain(|(client, _)| !synced.contains(client));
                entry.hooks_installed.retain(|(client, _)| {
                    !synced.contains(client) || files.iter().any(|(c, _)| c == client)
                });
                entry.warnings = vec!["no longer in the library; removal is held".into()];
                set_entry(new_map, name, entry);
            }
            if let Some(entry) = get_entry_mut(new_map, name) {
                for (client, rel) in files {
                    entry.extend_outputs(client, vec![rel.clone()]);
                    if let Some(hash) = prev_entry.file_hash(client, rel) {
                        entry.set_file_hash(client, rel, hash.to_string());
                    }
                }
            }
        }
    }
}

fn revoke_stale_hooks(
    previous: &LockFile,
    new_lock: &mut LockFile,
    transpilers: &[&dyn Transpiler],
    output_root: &Path,
    settings: Option<&dyn SettingsStore>,
    frozen: impl Fn(&str) -> bool,
) {
    let Some(store) = settings else {
        return;
    };
    for (prev, new) in [
        (&previous.skills, &mut new_lock.skills),
        (&previous.rules, &mut new_lock.rules),
    ] {
        for (name, prev_entry) in prev {
            if frozen(name) {
                continue;
            }
            for (client, prev_ids) in &prev_entry.hooks_installed {
                let kept: Vec<String> = get_entry(new, name)
                    .and_then(|e| e.hooks_installed.iter().find(|(k, _)| k == client))
                    .map_or_else(Vec::new, |(_, ids)| ids.clone());
                let remove: Vec<String> = prev_ids
                    .iter()
                    .filter(|id| !kept.contains(id))
                    .cloned()
                    .collect();
                if remove.is_empty() {
                    continue;
                }
                let Some(t) = transpilers.iter().find(|t| t.client_key() == client) else {
                    continue;
                };
                if let Err(e) = t.uninstall_hooks(output_root, &remove, store) {
                    if get_entry(new, name).is_none() {
                        let mut stub =
                            LockEntry::new(prev_entry.version.clone(), prev_entry.hash.clone());
                        stub.source = prev_entry.source.clone();
                        set_entry(new, name, stub);
                    }
                    if let Some(entry) = get_entry_mut(new, name) {
                        let mut held = kept;
                        held.extend(remove);
                        entry.set_hooks(client, held);
                        entry
                            .warnings
                            .push(format!("{client}: hook removal failed: {e}"));
                    }
                }
            }
        }
    }
}

fn is_folder_rule(source: &Path) -> bool {
    let Ok(text) = fs::read_to_string(source) else {
        return false;
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        return false;
    };
    let front = rest.split("\n---").next().unwrap_or("");
    front.lines().any(|l| l.trim_end() == "scope: folder")
}
