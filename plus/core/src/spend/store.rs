//! `events.jsonl` (capped, oldest first) and `daily.jsonl` (one line per day, project and
//! model, kept when events are pruned).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::event::Event;
use super::sample::{self, Sample};
use super::{day_of, MS_PER_DAY};

pub const EVENTS_FILE: &str = "events.jsonl";
pub const DAILY_FILE: &str = "daily.jsonl";
const LOCK_FILE: &str = "store.lock";
const LOCK_WAIT: Duration = Duration::from_secs(5);
pub const UNKNOWN_PROJECT: &str = "Unknown";
pub const UNKNOWN_MODEL: &str = "unknown";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyLine {
    pub day: String,
    pub project: String,
    pub model: String,
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_creation: u64,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub cost_estimated: f64,
    #[serde(default)]
    pub requests: u64,
    #[serde(default)]
    pub sessions: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneReport {
    pub removed_events: usize,
    pub removed_bytes: u64,
    pub kept_events: usize,
    pub kept_bytes: u64,
    pub folded_days: usize,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub events: usize,
    pub bytes: u64,
    pub skipped_lines: usize,
    pub oldest_ms: Option<i64>,
    pub newest_ms: Option<i64>,
    pub daily_lines: usize,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn events_path(&self) -> PathBuf {
        self.dir.join(EVENTS_FILE)
    }

    pub fn daily_path(&self) -> PathBuf {
        self.dir.join(DAILY_FILE)
    }

    fn lock(&self) -> Option<fs::File> {
        crate::tasks::store::lock(&self.dir.join(LOCK_FILE), LOCK_WAIT)
    }

    pub fn append(&self, events: &[Event]) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }
        let mut text = String::new();
        for event in events {
            text.push_str(&serde_json::to_string(event).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let _lock = self.lock().ok_or("the spend store is locked")?;
        let mut file = open_private_append(&self.events_path()).map_err(|e| e.to_string())?;
        file.write_all(text.as_bytes()).map_err(|e| e.to_string())
    }

    /// Every readable event; a line that does not parse is skipped and counted.
    pub fn read_events(&self) -> (Vec<Event>, usize) {
        let text = fs::read_to_string(self.events_path()).unwrap_or_default();
        let mut skipped = 0;
        let events = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| match serde_json::from_str::<Event>(line) {
                Ok(event) => Some(event),
                Err(_) => {
                    skipped += 1;
                    None
                }
            })
            .collect();
        (events, skipped)
    }

    pub fn read_daily(&self) -> Vec<DailyLine> {
        fs::read_to_string(self.daily_path())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    pub fn delete_daily(&self) -> Result<bool, String> {
        match fs::remove_file(self.daily_path()) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn stats(&self) -> Stats {
        let (events, skipped) = self.read_events();
        Stats {
            events: events.len(),
            bytes: fs::metadata(self.events_path()).map_or(0, |m| m.len()),
            skipped_lines: skipped,
            oldest_ms: events.iter().map(|e| e.ts).min(),
            newest_ms: events.iter().map(|e| e.ts).max(),
            daily_lines: self.read_daily().len(),
        }
    }

    /// Removes events older than `retention_days`, then the oldest ones until the file fits
    /// `max_bytes`. What is removed is folded into the daily totals first, so the totals
    /// survive. `project_of` names the folder of a session whose events carry none.
    pub fn prune(
        &self,
        now: i64,
        retention_days: u32,
        max_bytes: u64,
        project_of: &dyn Fn(&str) -> Option<String>,
        dry_run: bool,
    ) -> Result<PruneReport, String> {
        let _lock = self.lock().ok_or("the spend store is locked")?;
        let text = fs::read_to_string(self.events_path()).unwrap_or_default();
        let lines: Vec<(&str, Option<Event>)> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| (l, serde_json::from_str::<Event>(l).ok()))
            .collect();
        let cutoff = now - i64::from(retention_days) * MS_PER_DAY;
        let mut drop: BTreeSet<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, (_, e))| e.as_ref().is_none_or(|e| e.ts < cutoff))
            .map(|(i, _)| i)
            .collect();
        let mut kept_bytes: u64 = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| !drop.contains(i))
            .map(|(_, (l, _))| l.len() as u64 + 1)
            .sum();
        if kept_bytes > max_bytes {
            let mut by_age: Vec<usize> = (0..lines.len()).filter(|i| !drop.contains(i)).collect();
            by_age.sort_by_key(|&i| lines[i].1.as_ref().map_or(0, |e| e.ts));
            for i in by_age {
                if kept_bytes <= max_bytes {
                    break;
                }
                kept_bytes -= lines[i].0.len() as u64 + 1;
                drop.insert(i);
            }
        }
        let removed: Vec<Event> = drop.iter().filter_map(|&i| lines[i].1.clone()).collect();
        let mut daily = self.read_daily();
        fold(&mut daily, &removed, project_of);
        let report = PruneReport {
            removed_events: drop.len(),
            removed_bytes: lines
                .iter()
                .enumerate()
                .filter(|(i, _)| drop.contains(i))
                .map(|(_, (l, _))| l.len() as u64 + 1)
                .sum(),
            kept_events: lines.len() - drop.len(),
            kept_bytes,
            folded_days: removed
                .iter()
                .map(|e| day_of(e.ts))
                .collect::<BTreeSet<_>>()
                .len(),
            dry_run,
        };
        if dry_run || drop.is_empty() {
            return Ok(report);
        }
        let mut kept = String::new();
        for (i, (line, _)) in lines.iter().enumerate() {
            if !drop.contains(&i) {
                kept.push_str(line);
                kept.push('\n');
            }
        }
        self.write_daily(&daily)?;
        crate::fsx::atomic_write(&self.events_path(), &kept, crate::fsx::Mode::Private)
            .map_err(|e| format!("cannot write {}: {e}", self.events_path().display()))?;
        Ok(report)
    }

    fn write_daily(&self, daily: &[DailyLine]) -> Result<(), String> {
        let mut text = String::new();
        for line in daily {
            text.push_str(&serde_json::to_string(line).map_err(|e| e.to_string())?);
            text.push('\n');
        }
        crate::fsx::atomic_write(&self.daily_path(), &text, crate::fsx::Mode::Private)
            .map_err(|e| format!("cannot write {}: {e}", self.daily_path().display()))
    }
}

#[cfg(unix)]
fn open_private_append(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private_append(path: &Path) -> std::io::Result<fs::File> {
    OpenOptions::new().create(true).append(true).open(path)
}

pub fn fold(
    daily: &mut Vec<DailyLine>,
    events: &[Event],
    project_of: &dyn Fn(&str) -> Option<String>,
) {
    let mut keyed: BTreeMap<(String, String, String), usize> = daily
        .iter()
        .enumerate()
        .map(|(i, l)| ((l.day.clone(), l.project.clone(), l.model.clone()), i))
        .collect();
    for s in sample::from_events(events) {
        fold_sample(daily, &mut keyed, &s, project_of);
    }
}

fn fold_sample(
    daily: &mut Vec<DailyLine>,
    keyed: &mut BTreeMap<(String, String, String), usize>,
    s: &Sample,
    project_of: &dyn Fn(&str) -> Option<String>,
) {
    let project = s
        .project
        .clone()
        .or_else(|| s.session.as_deref().and_then(project_of))
        .unwrap_or_else(|| UNKNOWN_PROJECT.to_string());
    let model = s.model.clone().unwrap_or_else(|| UNKNOWN_MODEL.to_string());
    let key = (day_of(s.ts), project, model);
    let index = *keyed.entry(key.clone()).or_insert_with(|| {
        daily.push(DailyLine {
            day: key.0.clone(),
            project: key.1.clone(),
            model: key.2.clone(),
            ..DailyLine::default()
        });
        daily.len() - 1
    });
    let line = &mut daily[index];
    line.input += s.input;
    line.output += s.output;
    line.cache_read += s.cache_read;
    line.cache_creation += s.cache_creation;
    line.cost += s.cost;
    line.cost_estimated += s.cost_estimated;
    line.requests += s.requests;
    if let Some(id) = &s.session {
        if !line.sessions.contains(id) {
            line.sessions.push(id.clone());
        }
    }
}
