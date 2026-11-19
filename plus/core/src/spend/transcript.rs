//! Backfill from Claude Code's transcripts under `~/.claude/projects`, read-only. Tokens are
//! the billed usage written in the file; cost is estimated from list prices.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde_json::Value;

use super::prices;
use super::sample::Sample;

const SYNTHETIC_MODEL: &str = "<synthetic>";

#[derive(Clone, Debug, Default)]
pub struct TranscriptSession {
    pub session: String,
    pub cwd: Option<String>,
    pub first_ts: i64,
    pub last_ts: i64,
    pub samples: Vec<Sample>,
}

type CacheKey = (u64, SystemTime);

type Cache = HashMap<PathBuf, (CacheKey, Option<TranscriptSession>)>;

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

pub fn roots_under(home: &Path, config_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for base in config_dir
        .into_iter()
        .chain([crate::fsx::claude_dir_from(None, home).as_path()])
    {
        let projects = base.join("projects");
        if !roots.contains(&projects) {
            roots.push(projects);
        }
    }
    roots
}

fn transcript_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        let Ok(folders) = std::fs::read_dir(root) else {
            continue;
        };
        for folder in folders.flatten() {
            let Ok(entries) = std::fs::read_dir(folder.path()) else {
                continue;
            };
            files.extend(
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "jsonl")),
            );
        }
    }
    files.sort();
    files
}

fn key_of(path: &Path) -> Option<CacheKey> {
    let meta = std::fs::metadata(path).ok()?;
    Some((
        meta.len(),
        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}

fn cached(path: &Path, key: CacheKey) -> Option<Option<TranscriptSession>> {
    let guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref()?.get(path) {
        Some((k, parsed)) if *k == key => Some(parsed.clone()),
        _ => None,
    }
}

/// Transcript files found and how many of them are already parsed and cached, for the
/// first-run progress of the Spend page. The lock is not held while a file parses, so this can
/// be asked while `scan` runs.
pub fn progress(roots: &[PathBuf]) -> (usize, usize) {
    let files = transcript_files(roots);
    let done = files
        .iter()
        .filter(|p| key_of(p).is_some_and(|k| cached(p, k).is_some()))
        .count();
    (files.len(), done)
}

/// Every session found, keyed by session id. Files are parsed once per size and modification
/// time; a later call reuses the result.
pub fn scan(roots: &[PathBuf]) -> BTreeMap<String, TranscriptSession> {
    let mut out = BTreeMap::new();
    for path in transcript_files(roots) {
        let Some(key) = key_of(&path) else {
            continue;
        };
        let parsed = cached(&path, key).unwrap_or_else(|| {
            let parsed = parse_file(&path);
            CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .insert(path.clone(), (key, parsed.clone()));
            parsed
        });
        if let Some(session) = parsed {
            out.insert(session.session.clone(), session);
        }
    }
    out
}

/// How many of the most recently written transcripts [`latest`] looks at.
const LATEST_LOOKED_AT: usize = 40;

/// The session of the most recently written transcript, optionally the newest one of a folder.
/// Only the newest files are parsed, so the answer does not wait for the whole history.
pub fn latest(roots: &[PathBuf], cwd: Option<&Path>) -> Option<TranscriptSession> {
    let mut files: Vec<(SystemTime, PathBuf)> = transcript_files(roots)
        .into_iter()
        .filter_map(|path| Some((key_of(&path)?.1, path)))
        .collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.0));
    let wanted = cwd.map(|dir| std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()));
    for (_, path) in files.into_iter().take(LATEST_LOOKED_AT) {
        let Some(key) = key_of(&path) else { continue };
        let parsed = cached(&path, key).unwrap_or_else(|| {
            let parsed = parse_file(&path);
            CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .insert(path.clone(), (key, parsed.clone()));
            parsed
        });
        let Some(session) = parsed else { continue };
        let same_folder = wanted.as_ref().is_none_or(|dir| {
            session.cwd.as_deref().is_some_and(|c| {
                let given = Path::new(c);
                std::fs::canonicalize(given).unwrap_or_else(|_| given.to_path_buf()) == *dir
            })
        });
        if same_folder {
            return Some(session);
        }
    }
    None
}

fn parse_file(path: &Path) -> Option<TranscriptSession> {
    let text = std::fs::read_to_string(path).ok()?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let mut session = TranscriptSession {
        session: stem,
        ..TranscriptSession::default()
    };
    let mut by_message: BTreeMap<String, Sample> = BTreeMap::new();
    let mut loose = Vec::new();
    for line in text.lines() {
        if !line.contains("\"usage\"") && !line.contains("\"cwd\"") {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if session.cwd.is_none() {
            session.cwd = record
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Some(id) = record.get("sessionId").and_then(Value::as_str) {
            session.session = id.to_string();
        }
        let Some(message) = record.get("message") else {
            continue;
        };
        let Some(usage) = message.get("usage").filter(|u| u.is_object()) else {
            continue;
        };
        let model = message.get("model").and_then(Value::as_str);
        if model == Some(SYNTHETIC_MODEL) {
            continue;
        }
        let number = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        let ts = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(super::parse_ts)
            .unwrap_or(0);
        let mut sample = Sample {
            ts,
            session: None,
            project: None,
            model: model.map(str::to_string),
            input: number("input_tokens"),
            output: number("output_tokens"),
            cache_read: number("cache_read_input_tokens"),
            cache_creation: number("cache_creation_input_tokens"),
            requests: 1,
            transcript: true,
            ..Sample::default()
        };
        sample.cost_estimated = prices::cost(
            model,
            sample.input,
            sample.output,
            sample.cache_read,
            sample.cache_creation,
        );
        match message.get("id").and_then(Value::as_str) {
            Some(id) => {
                by_message.insert(id.to_string(), sample);
            }
            None => loose.push(sample),
        }
    }
    session.samples = by_message.into_values().chain(loose).collect();
    if session.samples.is_empty() {
        return None;
    }
    for sample in &mut session.samples {
        sample.session = Some(session.session.clone());
        sample.project = session.cwd.clone();
    }
    session.first_ts = session.samples.iter().map(|s| s.ts).min().unwrap_or(0);
    session.last_ts = session.samples.iter().map(|s| s.ts).max().unwrap_or(0);
    Some(session)
}

/// The folder that groups worktrees and clones of one repository: the main checkout when `cwd`
/// is inside a git worktree, otherwise the working folder itself.
pub fn project_root(cwd: &str) -> String {
    let mut dir = Some(Path::new(cwd));
    while let Some(d) = dir {
        let git = d.join(".git");
        if git.is_dir() {
            return d.to_string_lossy().into_owned();
        }
        if let Ok(text) = std::fs::read_to_string(&git) {
            let target = text.trim().strip_prefix("gitdir:").map(str::trim);
            if let Some(main) = target.and_then(|t| t.split("/.git/worktrees/").next()) {
                if target.is_some_and(|t| t.contains("/.git/worktrees/")) {
                    return main.to_string();
                }
            }
            return d.to_string_lossy().into_owned();
        }
        dir = d.parent();
    }
    cwd.to_string()
}
