//! The sync orchestration: transpile, write, copy assets, record the lock, clean stale files,
//! resolve collisions. Mirrors mcpm's `sync_skills` step order.

use super::assets::{compute_skill_hash, copy_skill_assets, rel_string};
use super::clock::Clock;
use super::collisions::{detect_collisions, resolve_collisions, resolve_mode, CollisionSummary};
use super::lock::{get_entry, get_entry_mut, load_lockfile, set_entry, LockEntry, LockFile};
use super::parser::{Skill, SkillType};
use super::pyfs::write_text;
use super::transpiler::{Transpiler, TranspilerRegistry};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct SyncOptions<'a> {
    pub output_root: PathBuf,
    /// Directory holding `mcpm-skills.lock`: the project root, or the user config dir in global mode.
    pub lock_dir: PathBuf,
    pub global_mode: bool,
    pub dry_run: bool,
    pub migrate: Option<bool>,
    pub client_keys: Option<Vec<String>>,
    pub clock: &'a dyn Clock,
}

#[derive(Debug)]
pub struct SyncResult {
    pub lockfile: LockFile,
    pub cleaned: Vec<PathBuf>,
    pub collisions: CollisionSummary,
    pub output_root: PathBuf,
}

pub(crate) fn rel_or_abs(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => rel_string(rel),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Files a previous sync recorded under skills/rules that the new lock no longer lists.
/// Only the lock's own relative entries are honoured; `..` segments and absolute paths are
/// refused so a tampered lock cannot reach outside the output root.
pub fn collect_stale_files(
    previous: &LockFile,
    new_lock: &LockFile,
    output_root: &Path,
) -> Vec<PathBuf> {
    let mut stale = Vec::new();
    for (prev, new) in [
        (&previous.skills, &new_lock.skills),
        (&previous.rules, &new_lock.rules),
    ] {
        for (name, entry) in prev {
            if get_entry(new, name).is_some() {
                continue;
            }
            for (_, rels) in &entry.output_files {
                for rel in rels {
                    if Path::new(rel)
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
                    {
                        continue;
                    }
                    stale.push(output_root.join(rel));
                }
            }
        }
    }
    stale
}

/// Deletes the files and prunes up to two now-empty parent directories per file.
pub fn delete_stale(stale: &[PathBuf]) -> Vec<PathBuf> {
    let mut deleted = Vec::new();
    for path in stale {
        if !path.exists() {
            continue;
        }
        if fs::remove_file(path).is_err() {
            continue;
        }
        deleted.push(path.clone());
        let mut parent = path.parent().map(Path::to_path_buf);
        for _ in 0..2 {
            let Some(dir) = parent.take() else { break };
            let empty = dir.is_dir()
                && fs::read_dir(&dir)
                    .map(|mut r| r.next().is_none())
                    .unwrap_or(false);
            if !empty || fs::remove_dir(&dir).is_err() {
                break;
            }
            parent = dir.parent().map(Path::to_path_buf);
        }
    }
    deleted
}

fn bucket<'a>(lock: &'a mut LockFile, skill: &Skill) -> &'a mut Vec<(String, LockEntry)> {
    match skill.skill_type {
        SkillType::Rule => &mut lock.rules,
        SkillType::Skill => &mut lock.skills,
    }
}

pub fn sync_skills(
    skills: &[Skill],
    registry: &TranspilerRegistry,
    opts: &SyncOptions<'_>,
) -> Result<SyncResult, String> {
    let output_root = opts.output_root.as_path();
    let delivered_elsewhere = |s: &Skill| {
        s.skill_type == SkillType::Rule
            && crate::plus::context::layer_spec::is_folder_rule(&s.source_path)
    };
    let kept: Vec<Skill>;
    let skills = if skills.iter().any(delivered_elsewhere) {
        kept = skills.iter().filter(|s| !delivered_elsewhere(s)).cloned().collect();
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

    for skill in skills {
        let mut entry = LockEntry::new(skill.version(), compute_skill_hash(skill)?);
        for t in &per_file {
            let key = t.client_key();
            if let Err(e) = write_one(*t, skill, output_root, opts.dry_run, &mut entry) {
                entry
                    .warnings
                    .push(format!("{key}: transpilation failed: {e}"));
            }
        }
        let name = skill.name().to_string();
        set_entry(bucket(&mut lock, skill), &name, entry);
    }

    for t in &append {
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
            write_text(&result.output_path, &result.content)?;
        }
    }

    let mut cleaned = Vec::new();
    if let Some(prev) = &previous {
        let stale = collect_stale_files(prev, &lock, output_root);
        if opts.dry_run {
            cleaned = stale;
        } else {
            cleaned = delete_stale(&stale);
            revoke_stale_hooks(prev, &lock, &selected, output_root);
        }
    }

    let found = detect_collisions(skills, &per_file, output_root);
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
    })
}

fn write_one(
    t: &dyn Transpiler,
    skill: &Skill,
    output_root: &Path,
    dry_run: bool,
    entry: &mut LockEntry,
) -> Result<(), String> {
    let key = t.client_key();
    let result = t.transpile(skill, output_root)?;
    entry.clients_synced.push(key.to_string());
    entry.warnings.extend(result.warnings.iter().cloned());
    if !dry_run {
        write_text(&result.output_path, &result.content)?;
    }
    entry.push_output(key, rel_or_abs(&result.output_path, output_root));

    let is_skill_file = result
        .output_path
        .file_name()
        .is_some_and(|n| n == "SKILL.md");
    if !dry_run && skill.skill_type == SkillType::Skill && is_skill_file {
        let dst_dir = result.output_path.parent().unwrap_or(output_root);
        let assets = copy_skill_assets(skill.source_dir(), dst_dir, output_root)?;
        if !assets.is_empty() {
            entry.extend_outputs(key, assets);
        }
    }

    let has_hooks = skill
        .frontmatter
        .hooks
        .as_ref()
        .is_some_and(|h| !h.is_empty());
    if !dry_run && skill.skill_type == SkillType::Skill && has_hooks {
        match t.install_hooks(skill, output_root) {
            Ok(ids) if !ids.is_empty() => entry.set_hooks(key, ids),
            Ok(_) => {}
            Err(e) => entry
                .warnings
                .push(format!("{key}: hook install failed: {e}")),
        }
    }
    Ok(())
}

fn revoke_stale_hooks(
    previous: &LockFile,
    new_lock: &LockFile,
    transpilers: &[&dyn Transpiler],
    output_root: &Path,
) {
    for (prev, new) in [
        (&previous.skills, &new_lock.skills),
        (&previous.rules, &new_lock.rules),
    ] {
        for (name, prev_entry) in prev {
            let new_entry = get_entry(new, name);
            for (client, prev_ids) in &prev_entry.hooks_installed {
                let kept: &[String] = new_entry
                    .and_then(|e| e.hooks_installed.iter().find(|(k, _)| k == client))
                    .map_or(&[], |(_, ids)| ids.as_slice());
                let remove: Vec<String> = prev_ids
                    .iter()
                    .filter(|id| !kept.contains(id))
                    .cloned()
                    .collect();
                if remove.is_empty() {
                    continue;
                }
                if let Some(t) = transpilers.iter().find(|t| t.client_key() == client) {
                    let _ = t.uninstall_hooks(output_root, &remove);
                }
            }
        }
    }
}
