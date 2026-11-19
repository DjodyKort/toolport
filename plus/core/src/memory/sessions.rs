//! Sessions (transcripts) per repository: Claude Code keeps them per working folder, so a
//! repository's sessions are the folders of its worktrees and the subfolders it has run in.
//! Toolport reads names, sizes and times; it never opens a transcript, and it only moves files.

use super::claude_json::Known;
use super::resolve::{project_dirs, Repo};
use crate::context::fsx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const DEFAULT_RETENTION_DAYS: u64 = 30;
pub const EXPIRING_DAYS: i64 = 7;
const DAY: i64 = 86_400;

pub struct Retention {
    pub days: u64,
    pub from_settings: bool,
    pub file: PathBuf,
}

pub fn retention(claude_dir: &Path) -> Retention {
    let file = claude_dir.join("settings.json");
    let days = fsx::read_text(&file, fsx::TEXT_CAP)
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("cleanupPeriodDays")?.as_u64())
        .filter(|d| *d >= 1);
    Retention {
        days: days.unwrap_or(DEFAULT_RETENTION_DAYS),
        from_settings: days.is_some(),
        file,
    }
}

pub fn retention_json(r: &Retention) -> Value {
    json!({
        "days": r.days,
        "default": DEFAULT_RETENTION_DAYS,
        "min": 1,
        "source": if r.from_settings { "user settings" } else { "default" },
        "file": fsx::display(&r.file),
        "perRepository": false,
    })
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub folder: PathBuf,
    pub project_dir: PathBuf,
    /// The transcript and the per-session folder, whichever exist.
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
    pub modified: i64,
}

impl Session {
    pub fn expires_at(&self, days: u64) -> i64 {
        self.modified + days as i64 * DAY
    }

    pub fn days_left(&self, days: u64, now: i64) -> i64 {
        let left = self.expires_at(days) - now;
        left.div_euclid(DAY) + i64::from(left.rem_euclid(DAY) > 0)
    }

    pub fn expiring(&self, days: u64, now: i64) -> bool {
        let left = self.expires_at(days) - now;
        left <= EXPIRING_DAYS * DAY
    }

    pub fn to_json(&self, days: u64, now: i64) -> Value {
        json!({
            "id": self.id,
            "folder": fsx::display(&self.folder),
            "bytes": self.bytes,
            "modified": fsx::zulu(self.modified),
            "ageDays": (now - self.modified).max(0) / DAY,
            "expiresAt": fsx::zulu(self.expires_at(days)),
            "daysLeft": self.days_left(days, now),
            "expiring": self.expiring(days, now),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub path: PathBuf,
    pub exists: bool,
    pub dir: Option<PathBuf>,
    pub sessions: Vec<Session>,
}

pub fn scan_dir(project_dir: &Path, folder: &Path) -> Vec<Session> {
    let mut by_id: std::collections::BTreeMap<String, Session> = Default::default();
    let mut transcript: std::collections::BTreeSet<String> = Default::default();
    for entry in fsx::list_dir(project_dir) {
        if entry.name == "memory" || entry.symlink {
            continue;
        }
        let (id, is_transcript) = match (entry.kind, entry.name.strip_suffix(".jsonl")) {
            (fsx::Kind::File, Some(id)) => (id.to_string(), true),
            (fsx::Kind::Dir, _) => (entry.name.clone(), false),
            _ => continue,
        };
        let Some(stamp) = fsx::stamp(&entry.path) else {
            continue;
        };
        let modified = (stamp.mtime_ns / 1_000_000_000) as i64;
        let session = by_id.entry(id.clone()).or_insert_with(|| Session {
            id: id.clone(),
            folder: folder.to_path_buf(),
            project_dir: project_dir.to_path_buf(),
            paths: Vec::new(),
            bytes: 0,
            modified,
        });
        session.bytes += super::store::size_of(&entry.path);
        // The transcript's own write time is the session's age; its folder changes with every subagent.
        if is_transcript {
            session.modified = modified;
            transcript.insert(id);
        } else if !transcript.contains(&id) {
            session.modified = session.modified.max(modified);
        }
        session.paths.push(entry.path);
    }
    by_id.into_values().collect()
}

/// The working folders that belong to `repo`: its worktrees, the folder asked about and every
/// folder under them that Claude Code has run in.
pub fn folders_of(claude_dir: &Path, repo: &Repo, known: &Known) -> Vec<Folder> {
    let projects = claude_dir.join("projects");
    let mut paths: Vec<PathBuf> = repo.worktrees.clone();
    paths.push(repo.cwd.clone());
    paths.extend(known.under(&repo.worktrees));
    paths.sort();
    paths.dedup();
    let mut seen_dirs: Vec<PathBuf> = Vec::new();
    paths
        .into_iter()
        .map(|path| {
            let mut sessions = Vec::new();
            let mut dir = None;
            let canonical = fsx::canonical(&path);
            for candidate in [&path, &canonical] {
                for found in project_dirs(&projects, candidate) {
                    if seen_dirs.contains(&found) {
                        continue;
                    }
                    seen_dirs.push(found.clone());
                    sessions.extend(scan_dir(&found, &path));
                    dir = dir.or(Some(found));
                }
            }
            Folder {
                exists: fsx::is_dir(&path),
                path,
                dir,
                sessions,
            }
        })
        .collect()
}

pub struct Totals {
    pub count: usize,
    pub bytes: u64,
    pub oldest: Option<i64>,
    pub expiring: usize,
}

pub fn totals<'a>(sessions: impl Iterator<Item = &'a Session>, days: u64, now: i64) -> Totals {
    let mut t = Totals {
        count: 0,
        bytes: 0,
        oldest: None,
        expiring: 0,
    };
    for s in sessions {
        t.count += 1;
        t.bytes += s.bytes;
        t.oldest = Some(t.oldest.map_or(s.modified, |o| o.min(s.modified)));
        t.expiring += usize::from(s.expiring(days, now));
    }
    t
}

pub fn totals_json(t: &Totals) -> Value {
    json!({
        "count": t.count,
        "bytes": t.bytes,
        "oldest": t.oldest.map(fsx::zulu),
        "expiringThisWeek": t.expiring,
    })
}

pub fn folder_json(f: &Folder, days: u64, now: i64) -> Value {
    json!({
        "path": fsx::display(&f.path),
        "exists": f.exists,
        "projectDir": f.dir.as_deref().map(fsx::display),
        "sessions": f.sessions.len(),
        "bytes": f.sessions.iter().map(|s| s.bytes).sum::<u64>(),
        "oldest": f.sessions.iter().map(|s| s.modified).min().map(fsx::zulu),
        "expiringThisWeek": f.sessions.iter().filter(|s| s.expiring(days, now)).count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_its_transcript_and_its_folder_and_never_the_memory_folder() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("aaa.jsonl"), "12345").unwrap();
        std::fs::create_dir_all(p.join("aaa/subagents")).unwrap();
        std::fs::write(p.join("aaa/subagents/x.jsonl"), "123").unwrap();
        std::fs::write(p.join("bbb.jsonl"), "1").unwrap();
        std::fs::create_dir_all(p.join("memory")).unwrap();
        std::fs::write(p.join("memory/MEMORY.md"), "m").unwrap();
        let sessions = scan_dir(p, Path::new("/f"));
        let ids: Vec<(&str, u64, usize)> = sessions
            .iter()
            .map(|s| (s.id.as_str(), s.bytes, s.paths.len()))
            .collect();
        assert_eq!(ids, [("aaa", 8, 2), ("bbb", 1, 1)]);
    }

    #[test]
    fn a_session_expires_retention_days_after_its_last_write() {
        let s = Session {
            id: "a".into(),
            folder: "/f".into(),
            project_dir: "/p".into(),
            paths: vec![],
            bytes: 0,
            modified: 1_000_000,
        };
        let day = 86_400;
        let now = 1_000_000 + 24 * day + 5;
        assert_eq!(s.days_left(30, now), 6);
        assert!(s.expiring(30, now));
        assert!(!s.expiring(60, now));
        assert!(s.expiring(30, 1_000_000 + 31 * day));
    }
}
