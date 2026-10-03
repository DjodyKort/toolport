//! Taps (named git sources of skills) and the cross-machine skills repo sync. mcpm keeps this in
//! its remote-sync plugin, which is not part of the fork's tree; the shape here is the minimum
//! the local commands rely on: `taps.json`, `skills_sync.json` with `local_path`, and a clone or
//! fast-forward pull per source through a `GitRunner`.

use super::git::GitRunner;
use super::json::{self, J};
use super::ops::SKILLS_SYNC_CONFIG;
use crate::registry::atomic_write;
use std::fs;
use std::path::{Path, PathBuf};

pub const TAPS_FILE: &str = "taps.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tap {
    pub name: String,
    pub url: String,
}

fn valid_tap_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn load_taps(config_dir: &Path) -> Vec<Tap> {
    let Ok(text) = fs::read_to_string(config_dir.join(TAPS_FILE)) else {
        return Vec::new();
    };
    let Ok(J::Obj(items)) = json::parse(&text) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|(name, v)| {
            Some(Tap {
                name: name.clone(),
                url: v.get("url")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn save_taps(config_dir: &Path, taps: &[Tap]) -> Result<(), String> {
    let doc = J::Obj(
        taps.iter()
            .map(|t| (t.name.clone(), J::Obj(vec![("url".into(), J::str(&t.url))])))
            .collect(),
    );
    atomic_write(&config_dir.join(TAPS_FILE), &doc.dumps())
}

pub fn add_tap(config_dir: &Path, name: &str, url: &str) -> Result<(), String> {
    if !valid_tap_name(name) {
        return Err(format!("invalid tap name {name:?}"));
    }
    if url.trim().is_empty() || url.starts_with('-') {
        return Err(format!("invalid tap url {url:?}"));
    }
    let mut taps = load_taps(config_dir);
    match taps.iter_mut().find(|t| t.name == name) {
        Some(t) => t.url = url.to_string(),
        None => taps.push(Tap {
            name: name.to_string(),
            url: url.to_string(),
        }),
    }
    save_taps(config_dir, &taps)
}

pub fn remove_tap(config_dir: &Path, name: &str) -> Result<bool, String> {
    let mut taps = load_taps(config_dir);
    let before = taps.len();
    taps.retain(|t| t.name != name);
    if taps.len() == before {
        return Ok(false);
    }
    save_taps(config_dir, &taps)?;
    Ok(true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapOutcome {
    Cloned { head: String },
    Updated { head: String },
    Failed(String),
}

pub fn tap_dir(taps_root: &Path, tap: &Tap) -> PathBuf {
    taps_root.join(&tap.name)
}

/// Clones missing taps and fast-forwards existing ones; one failing tap never stops the rest.
pub fn sync_taps(
    git: &dyn GitRunner,
    config_dir: &Path,
    taps_root: &Path,
) -> Vec<(Tap, TapOutcome)> {
    load_taps(config_dir)
        .into_iter()
        .map(|tap| {
            let dir = tap_dir(taps_root, &tap);
            let outcome = if git.is_repo(&dir) {
                match git.pull(&dir) {
                    Ok(head) => TapOutcome::Updated { head },
                    Err(e) => TapOutcome::Failed(e),
                }
            } else {
                let cloned = fs::create_dir_all(taps_root)
                    .map_err(|e| e.to_string())
                    .and_then(|_| git.clone_repo(&tap.url, &dir))
                    .and_then(|_| git.head(&dir));
                match cloned {
                    Ok(head) => TapOutcome::Cloned { head },
                    Err(e) => TapOutcome::Failed(e),
                }
            };
            (tap, outcome)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillsSyncConfig {
    pub remote: String,
    pub local_path: PathBuf,
}

pub fn load_sync_config(config_dir: &Path) -> Option<SkillsSyncConfig> {
    let text = fs::read_to_string(config_dir.join(SKILLS_SYNC_CONFIG)).ok()?;
    let doc = json::parse(&text).ok()?;
    Some(SkillsSyncConfig {
        remote: doc.get("remote")?.as_str()?.to_string(),
        local_path: PathBuf::from(doc.get("local_path")?.as_str()?),
    })
}

pub fn save_sync_config(config_dir: &Path, cfg: &SkillsSyncConfig) -> Result<(), String> {
    let doc = J::Obj(vec![
        ("remote".into(), J::str(&cfg.remote)),
        (
            "local_path".into(),
            J::str(cfg.local_path.to_string_lossy()),
        ),
    ]);
    atomic_write(&config_dir.join(SKILLS_SYNC_CONFIG), &doc.dumps())
}

/// Brings the canonical skills repo up to date and returns its HEAD.
pub fn sync_skills_repo(git: &dyn GitRunner, cfg: &SkillsSyncConfig) -> Result<TapOutcome, String> {
    if git.is_repo(&cfg.local_path) {
        return git
            .pull(&cfg.local_path)
            .map(|head| TapOutcome::Updated { head });
    }
    if let Some(parent) = cfg.local_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    git.clone_repo(&cfg.remote, &cfg.local_path)?;
    let head = git.head(&cfg.local_path)?;
    Ok(TapOutcome::Cloned { head })
}
