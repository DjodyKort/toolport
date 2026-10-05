//! Task definitions under `<data dir>/plus/tasks/<id>.json`, run records under
//! `<data dir>/plus/task-runs/<run-id>.json` and the attention records a schedule or a failed
//! login raises under `<data dir>/plus/attention-tasks.json` (read later by `attention ls`).

use super::cron::{now, rfc3339};
use super::model::{self, Task};
use crate::plus::op::OpError;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

pub const OUTPUT_CAP: usize = 4096;

pub fn plus_dir() -> Result<PathBuf, OpError> {
    crate::registry::conduit_dir().map(|d| d.join("plus")).ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))
}

pub fn tasks_dir() -> Result<PathBuf, OpError> {
    Ok(plus_dir()?.join("tasks"))
}

pub fn runs_dir() -> Result<PathBuf, OpError> {
    Ok(plus_dir()?.join("task-runs"))
}

fn check_id(id: &str) -> Result<(), OpError> {
    if model::valid_id(id) {
        Ok(())
    } else {
        Err(OpError::usage(format!("{id:?} is not a valid id: use 1-64 characters of a-z, 0-9 and '-'")))
    }
}

pub fn task_path(id: &str) -> Result<PathBuf, OpError> {
    check_id(id)?;
    Ok(tasks_dir()?.join(format!("{id}.json")))
}

pub fn write(path: &PathBuf, text: &str) -> Result<(), OpError> {
    crate::registry::atomic_write(path, text).map_err(|e| OpError::failed("write", format!("cannot write {}: {e}", path.display())))
}

pub fn save_task(task: &Task) -> Result<PathBuf, OpError> {
    let path = task_path(&task.id)?;
    write(&path, &(serde_json::to_string_pretty(task).unwrap_or_default() + "\n"))?;
    Ok(path)
}

pub fn load_task(id: &str) -> Result<Task, OpError> {
    let path = task_path(id)?;
    let text = fs::read_to_string(&path).map_err(|_| OpError::not_found(format!("no task {id:?} (looked in {})", path.display())))?;
    let task = model::parse(&text).map_err(|e| OpError::failed("invalid_task", format!("{}: {e}", path.display())))?;
    if task.id != id {
        return Err(OpError::failed("invalid_task", format!("{} holds the task {:?}, not {id:?}", path.display(), task.id)));
    }
    Ok(task)
}

pub fn list_tasks() -> Result<Vec<Result<Task, (String, String)>>, OpError> {
    let dir = tasks_dir()?;
    let mut ids: Vec<String> = fs::read_dir(&dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from)).collect();
    ids.sort();
    Ok(ids.into_iter().map(|id| load_task(&id).map_err(|e| (id, e.message))).collect())
}

pub fn backup_task(id: &str) -> Result<Option<PathBuf>, OpError> {
    let path = task_path(id)?;
    let Ok(text) = fs::read_to_string(&path) else { return Ok(None) };
    let dir = tasks_dir()?.join(".backups");
    let backup = dir.join(format!("{id}.{}.json", now()));
    write(&backup, &text)?;
    Ok(Some(backup))
}

pub fn remove_task(id: &str) -> Result<(), OpError> {
    fs::remove_file(task_path(id)?).map_err(|e| OpError::failed("write", format!("cannot remove task {id:?}: {e}")))
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
}

pub fn valid_run_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn new_run_id() -> String {
    format!("run-{}-{}", now(), &crate::approval::new_correlation_id()[..8])
}

pub fn new_run(task: &Task, trigger: &str) -> Run {
    Run {
        id: new_run_id(),
        task: task.id.clone(),
        trigger: trigger.into(),
        status: Status::Running,
        started_at: rfc3339(now()),
        ended_at: None,
        duration_ms: None,
        steps: task.steps.iter().map(|s| StepRun { id: s.id().into(), title: s.title().into(), kind: s.kind().into(), status: Status::Pending, started_at: None, ended_at: None, output: String::new(), instructions: None }).collect(),
        error: None,
        cancel_requested: false,
        resume_requested: false,
        runner_pid: None,
        child_pid: None,
    }
}

fn run_path(id: &str) -> Result<PathBuf, OpError> {
    if !valid_run_id(id) {
        return Err(OpError::usage(format!("{id:?} is not a run id")));
    }
    Ok(runs_dir()?.join(format!("{id}.json")))
}

pub fn load_run(id: &str) -> Result<Run, OpError> {
    let path = run_path(id)?;
    let text = fs::read_to_string(&path).map_err(|_| OpError::not_found(format!("no run {id:?}")))?;
    serde_json::from_str(&text).map_err(|e| OpError::failed("invalid_run", format!("{}: {e}", path.display())))
}

pub fn update_run<T>(id: &str, change: impl FnOnce(&mut Run) -> T) -> Result<(Run, T), OpError> {
    let path = run_path(id)?;
    let dir = runs_dir()?;
    fs::create_dir_all(&dir).map_err(|e| OpError::failed("write", e.to_string()))?;
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!(".{id}.lock"))).map_err(|e| OpError::failed("write", e.to_string()))?;
    lock.lock_exclusive().map_err(|e| OpError::failed("write", e.to_string()))?;
    let mut run = load_run(id)?;
    let out = change(&mut run);
    write(&path, &(serde_json::to_string_pretty(&run).unwrap_or_default() + "\n"))?;
    Ok((run, out))
}

pub fn create_run(run: &Run) -> Result<(), OpError> {
    write(&run_path(&run.id)?, &(serde_json::to_string_pretty(run).unwrap_or_default() + "\n"))
}

pub fn list_runs(task: Option<&str>) -> Result<Vec<Run>, OpError> {
    let dir = runs_dir()?;
    let mut runs: Vec<Run> = fs::read_dir(&dir).into_iter().flatten().flatten().filter(|e| e.file_name().to_str().is_some_and(|n| n.ends_with(".json") && !n.starts_with('.'))).filter_map(|e| fs::read_to_string(e.path()).ok()).filter_map(|t| serde_json::from_str::<Run>(&t).ok()).filter(|r| task.is_none_or(|t| r.task == t)).collect();
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
    Ok(runs)
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

fn attention_path() -> Result<PathBuf, OpError> {
    Ok(plus_dir()?.join("attention-tasks.json"))
}

pub fn attention() -> Vec<AttentionRecord> {
    attention_path().ok().and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<AttentionFile>(&t).ok()).map(|f| f.items).unwrap_or_default()
}

fn edit_attention(change: impl FnOnce(&mut Vec<AttentionRecord>)) -> Result<(), OpError> {
    let path = attention_path()?;
    let mut items = attention();
    change(&mut items);
    write(&path, &(serde_json::to_string_pretty(&AttentionFile { items }).unwrap_or_default() + "\n"))
}

pub fn raise_attention(record: AttentionRecord) -> Result<bool, OpError> {
    let mut fresh = false;
    edit_attention(|items| {
        if !items.iter().any(|r| r.id == record.id) {
            items.push(record);
            fresh = true;
        }
    })?;
    Ok(fresh)
}

pub fn clear_attention(task: &str) -> Result<(), OpError> {
    edit_attention(|items| items.retain(|r| r.task != task))
}
