//! Task definitions under `<data dir>/plus/tasks/<id>.json`, run records under
//! `<data dir>/plus/task-runs/<run-id>.json` and the attention records a schedule or a failed
//! login raises under `<data dir>/plus/attention-tasks.json` (read later by `attention ls`).

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::cron::rfc3339;
use super::model::{self, Task};
use crate::op::OpError;

pub const OUTPUT_CAP: usize = 4096;
const LOCK_WAIT: Duration = Duration::from_secs(10);

pub fn plus_dir(base: &Path) -> PathBuf {
    base.join("plus")
}

pub fn tasks_dir(base: &Path) -> PathBuf {
    plus_dir(base).join("tasks")
}

pub fn runs_dir(base: &Path) -> PathBuf {
    plus_dir(base).join("task-runs")
}

fn check_id(id: &str) -> Result<(), OpError> {
    if model::valid_id(id) {
        Ok(())
    } else {
        Err(OpError::usage(format!(
            "{id:?} is not a valid id: use 1-64 characters of a-z, 0-9 and '-'"
        )))
    }
}

pub fn task_path(base: &Path, id: &str) -> Result<PathBuf, OpError> {
    check_id(id)?;
    Ok(tasks_dir(base).join(format!("{id}.json")))
}

pub fn write(path: &Path, text: &str) -> Result<(), OpError> {
    atomic_write(path, text)
        .map_err(|e| OpError::failed("write", format!("cannot write {}: {e}", path.display())))
}

fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// An exclusive advisory lock on `path`, released when the returned file is dropped or the
/// holder dies. Waits up to `wait`; `None` when it could not be taken in time.
pub fn lock(path: &Path, wait: Duration) -> Option<File> {
    fs::create_dir_all(path.parent()?).ok()?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    let end = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Some(file),
            Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return None,
        }
    }
}

pub fn save_task(base: &Path, task: &Task) -> Result<PathBuf, OpError> {
    let path = task_path(base, &task.id)?;
    write(
        &path,
        &(serde_json::to_string_pretty(task).unwrap_or_default() + "\n"),
    )?;
    super::builtin::claim(base, &task.id);
    Ok(path)
}

pub fn load_task(base: &Path, id: &str) -> Result<Task, OpError> {
    let path = task_path(base, id)?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => super::builtin::unclaimed_text(base, id)
            .map(String::from)
            .ok_or_else(|| {
                OpError::not_found(format!("no task {id:?} (looked in {})", path.display()))
            })?,
    };
    let task = model::parse(&text)
        .map_err(|e| OpError::failed("invalid_task", format!("{}: {e}", path.display())))?;
    if task.id != id {
        return Err(OpError::failed(
            "invalid_task",
            format!(
                "{} holds the task {:?}, not {id:?}",
                path.display(),
                task.id
            ),
        ));
    }
    Ok(task)
}

pub type TaskEntry = Result<Task, (String, String)>;

pub fn list_tasks(base: &Path) -> Vec<TaskEntry> {
    let mut ids: Vec<String> = fs::read_dir(tasks_dir(base))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .filter(|n| !n.starts_with('.'))
                .map(String::from)
        })
        .collect();
    let shipped: Vec<String> = super::builtin::unclaimed_ids(base)
        .into_iter()
        .map(String::from)
        .filter(|id| !ids.contains(id))
        .collect();
    ids.extend(shipped);
    ids.sort();
    ids.into_iter()
        .map(|id| load_task(base, &id).map_err(|e| (id, e.message)))
        .collect()
}

pub fn backup_task(base: &Path, id: &str) -> Result<Option<PathBuf>, OpError> {
    let path = task_path(base, id)?;
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let backup = tasks_dir(base)
        .join(".backups")
        .join(format!("{millis:013}-{id}.json"));
    write(&backup, &text)?;
    Ok(Some(backup))
}

pub fn remove_task(base: &Path, id: &str) -> Result<(), OpError> {
    fs::remove_file(task_path(base, id)?)
        .map_err(|e| OpError::failed("write", format!("cannot remove task {id:?}: {e}")))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pending,
    Running,
    Waiting,
    Ok,
    Failed,
    Cancelled,
    Skipped,
}

impl Status {
    pub fn finished(self) -> bool {
        matches!(self, Status::Ok | Status::Failed | Status::Cancelled)
    }

    pub fn live(self) -> bool {
        matches!(self, Status::Running | Status::Waiting)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRun {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: Status,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// Why a catch-up run exists: how many scheduled slots passed with no run, and when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Missed {
    pub slots: u32,
    pub first_due: String,
    pub last_due: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub task: String,
    pub trigger: String,
    pub status: Status,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub steps: Vec<StepRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub resume_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed: Option<Missed>,
}

pub fn valid_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn new_run_id(now: i64) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let digest = Sha256::digest(
        format!(
            "{nanos}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        )
        .as_bytes(),
    );
    let tail: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("run-{now}-{tail}")
}

pub fn new_run(task: &Task, trigger: &str, now: i64) -> Run {
    Run {
        id: new_run_id(now),
        task: task.id.clone(),
        trigger: trigger.into(),
        status: Status::Running,
        started_at: rfc3339(now),
        ended_at: None,
        duration_ms: None,
        steps: task
            .steps
            .iter()
            .map(|s| StepRun {
                id: s.id().into(),
                title: s.title().into(),
                kind: s.kind().into(),
                status: Status::Pending,
                started_at: None,
                ended_at: None,
                output: String::new(),
                instructions: None,
            })
            .collect(),
        error: None,
        cancel_requested: false,
        resume_requested: false,
        runner_pid: None,
        child_pid: None,
        action: None,
        missed: None,
    }
}

pub fn run_path(base: &Path, id: &str) -> Result<PathBuf, OpError> {
    if !valid_run_id(id) {
        return Err(OpError::usage(format!("{id:?} is not a run id")));
    }
    Ok(runs_dir(base).join(format!("{id}.json")))
}

pub fn load_run(base: &Path, id: &str) -> Result<Run, OpError> {
    let path = run_path(base, id)?;
    let text =
        fs::read_to_string(&path).map_err(|_| OpError::not_found(format!("no run {id:?}")))?;
    serde_json::from_str(&text)
        .map_err(|e| OpError::failed("invalid_run", format!("{}: {e}", path.display())))
}

pub fn update_run<T>(
    base: &Path,
    id: &str,
    change: impl FnOnce(&mut Run) -> T,
) -> Result<(Run, T), OpError> {
    let path = run_path(base, id)?;
    let _guard = lock(&runs_dir(base).join(format!(".{id}.lock")), LOCK_WAIT).ok_or_else(|| {
        OpError::failed("write", format!("run {id} is locked by another process"))
    })?;
    let mut run = load_run(base, id)?;
    let out = change(&mut run);
    write(
        &path,
        &(serde_json::to_string_pretty(&run).unwrap_or_default() + "\n"),
    )?;
    Ok((run, out))
}

pub fn create_run(base: &Path, run: &Run) -> Result<(), OpError> {
    write(
        &run_path(base, &run.id)?,
        &(serde_json::to_string_pretty(run).unwrap_or_default() + "\n"),
    )
}

pub fn list_runs(base: &Path, task: Option<&str>) -> Vec<Run> {
    let mut runs: Vec<Run> = fs::read_dir(runs_dir(base))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.ends_with(".json") && !n.starts_with('.'))
        })
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str::<Run>(&t).ok())
        .filter(|r| task.is_none_or(|t| r.task == t))
        .collect();
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
    runs
}

pub fn live_run(base: &Path, task: &str) -> Option<Run> {
    list_runs(base, Some(task))
        .into_iter()
        .find(|r| r.status.live())
}

pub fn cap(text: &str) -> String {
    if text.len() <= OUTPUT_CAP {
        return text.to_string();
    }
    let mut end = OUTPUT_CAP;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &text[..end])
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionRecord {
    pub id: String,
    pub level: String,
    pub title: String,
    pub detail: String,
    pub from: String,
    pub task: String,
    pub since: String,
    pub action: Option<Value>,
}

#[derive(Default, Serialize, Deserialize)]
struct AttentionFile {
    #[serde(default)]
    items: Vec<AttentionRecord>,
}

fn attention_path(base: &Path) -> PathBuf {
    plus_dir(base).join("attention-tasks.json")
}

pub fn attention(base: &Path) -> Vec<AttentionRecord> {
    fs::read_to_string(attention_path(base))
        .ok()
        .and_then(|t| serde_json::from_str::<AttentionFile>(&t).ok())
        .map(|f| f.items)
        .unwrap_or_default()
}

fn edit_attention(
    base: &Path,
    change: impl FnOnce(&mut Vec<AttentionRecord>),
) -> Result<(), OpError> {
    let _guard = lock(&plus_dir(base).join(".attention-tasks.lock"), LOCK_WAIT);
    let mut items = attention(base);
    change(&mut items);
    write(
        &attention_path(base),
        &(serde_json::to_string_pretty(&AttentionFile { items }).unwrap_or_default() + "\n"),
    )
}

pub fn raise_attention(base: &Path, record: AttentionRecord) -> Result<bool, OpError> {
    let mut fresh = false;
    edit_attention(base, |items| {
        if !items.iter().any(|r| r.id == record.id) {
            items.push(record);
            fresh = true;
        }
    })?;
    Ok(fresh)
}

pub fn clear_attention(base: &Path, task: &str) -> Result<(), OpError> {
    edit_attention(base, |items| items.retain(|r| r.task != task))
}
