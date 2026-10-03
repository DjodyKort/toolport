//! Collision detection and backup of hand-written files that shadow a synced skill.

use super::clock::Clock;
use super::json::{self, J};
use super::parser::Skill;
use super::transpiler::Transpiler;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const BACKUP_DIR_NAME: &str = ".mcpm-backups";
pub const BACKUP_INDEX_NAME: &str = "INDEX.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionMode {
    AutoReplace,
    WarnOnly,
}

/// `--migrate` replaces, everything else only warns; the interactive prompt is a UI concern.
pub fn resolve_mode(migrate: Option<bool>) -> ResolutionMode {
    match migrate {
        Some(true) => ResolutionMode::AutoReplace,
        _ => ResolutionMode::WarnOnly,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collision {
    pub skill_name: String,
    pub client_key: String,
    pub synced_path: PathBuf,
    pub synced_content: String,
    pub collision_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Replaced,
    Kept,
    SkippedDryRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub collision: Collision,
    pub action: Action,
    pub backup_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CollisionSummary {
    pub resolutions: Vec<Resolution>,
}

impl CollisionSummary {
    pub fn replaced(&self) -> impl Iterator<Item = &Resolution> {
        self.resolutions
            .iter()
            .filter(|r| r.action == Action::Replaced)
    }

    pub fn kept(&self) -> impl Iterator<Item = &Resolution> {
        self.resolutions.iter().filter(|r| r.action == Action::Kept)
    }
}

pub fn detect_collisions(
    skills: &[Skill],
    transpilers: &[&dyn Transpiler],
    output_root: &Path,
) -> Vec<Collision> {
    let mut out = Vec::new();
    for skill in skills {
        for t in transpilers {
            let synced_path = t.get_output_path(skill, output_root);
            for collision_path in t.get_collision_paths(skill, output_root) {
                if !collision_path.exists() || collision_path == synced_path {
                    continue;
                }
                let synced_content = fs::read_to_string(&synced_path).unwrap_or_default();
                out.push(Collision {
                    skill_name: skill.name().to_string(),
                    client_key: t.client_key().to_string(),
                    synced_path: synced_path.clone(),
                    synced_content,
                    collision_path,
                });
            }
        }
    }
    out
}

fn backup_rel(collision_path: &Path, output_root: &Path) -> PathBuf {
    if let Ok(rel) = collision_path.strip_prefix(output_root) {
        return rel.to_path_buf();
    }
    if collision_path.is_absolute() {
        return collision_path
            .components()
            .filter(|c| !matches!(c, Component::RootDir | Component::Prefix(_)))
            .collect();
    }
    collision_path.to_path_buf()
}

fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
    fs::remove_file(from).map_err(|e| format!("{}: {e}", from.display()))
}

/// Moves the colliding file to `<output_root>/.mcpm-backups/<rel>.<UTC stamp>` and appends to
/// `INDEX.json`. The original disappears only after the move succeeded.
pub fn backup_and_remove(
    collision: &Collision,
    output_root: &Path,
    clock: &dyn Clock,
) -> Result<PathBuf, String> {
    let backup_root = output_root.join(BACKUP_DIR_NAME);
    let rel = backup_rel(&collision.collision_path, output_root);
    let stamp = clock.now().backup_stamp();
    let mut name = rel
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(format!(".{stamp}"));
    let backup_path = backup_root.join(rel.with_file_name(name));
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    move_file(&collision.collision_path, &backup_path)?;
    let p = |path: &Path| J::str(path.to_string_lossy());
    append_index(
        &backup_root,
        J::Obj(vec![
            ("timestamp".into(), J::str(&stamp)),
            ("skill_name".into(), J::str(&collision.skill_name)),
            ("client_key".into(), J::str(&collision.client_key)),
            ("original_path".into(), p(&collision.collision_path)),
            ("backup_path".into(), p(&backup_path)),
            ("synced_path".into(), p(&collision.synced_path)),
            ("reason".into(), J::str("collision-with-synced-skill")),
        ]),
    )?;
    Ok(backup_path)
}

fn append_index(backup_root: &Path, entry: J) -> Result<(), String> {
    let index_path = backup_root.join(BACKUP_INDEX_NAME);
    let mut backups: Vec<J> = fs::read_to_string(&index_path)
        .ok()
        .and_then(|t| json::parse(&t).ok())
        .map(|v| match v.get("backups") {
            Some(J::Arr(items)) => items.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    backups.push(entry);
    let doc = J::Obj(vec![("backups".into(), J::Arr(backups))]);
    crate::registry::atomic_write(&index_path, &doc.dumps())
}

pub fn resolve_collisions(
    collisions: Vec<Collision>,
    output_root: &Path,
    mode: ResolutionMode,
    dry_run: bool,
    clock: &dyn Clock,
) -> Result<CollisionSummary, String> {
    let mut summary = CollisionSummary::default();
    for c in collisions {
        let (action, backup_path) = match mode {
            ResolutionMode::WarnOnly => (Action::Kept, None),
            ResolutionMode::AutoReplace if dry_run => (Action::SkippedDryRun, None),
            ResolutionMode::AutoReplace => (
                Action::Replaced,
                Some(backup_and_remove(&c, output_root, clock)?),
            ),
        };
        summary.resolutions.push(Resolution {
            collision: c,
            action,
            backup_path,
        });
    }
    Ok(summary)
}
