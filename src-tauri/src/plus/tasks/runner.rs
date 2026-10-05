//! Runs one task run to its end: steps in order, a `needs-you` step pauses the run (`waiting`)
//! until `task resume`, its `waitFor` condition or its timeout. The run record is the only
//! channel between this runner and `task resume|cancel`. Captured values live in this
//! function's memory and in the `Redactor`, never in a record.

use super::host::Host;
use super::model::{Step, Task, WaitKind};
use super::redactor::Redactor;
use super::store::{self, cap, Run, Status};
use crate::plus::op::OpError;
use crate::plus::redact::sensitive_key;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CHILD_ENV_DROP: [&str; 3] = ["TOOLPORT_SECRET_KEY", "CONDUIT_SECRET_KEY", "TOOLPORT_HTTP_TOKEN"];
const READ_CAP: usize = 64 * 1024;

enum Outcome {
    Ok(String),
    Failed(String),
    Cancelled,
}

fn poll() -> Duration {
    Duration::from_millis(std::env::var("TOOLPORT_TASK_POLL_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(100))
}

fn now() -> String {
    super::cron::rfc3339(super::cron::now())
}

fn cancelled(run_id: &str) -> bool {
    store::load_run(run_id).map(|r| r.cancel_requested).unwrap_or(false)
}

fn set_step(run_id: &str, index: usize, change: impl FnOnce(&mut store::StepRun)) -> Result<Run, OpError> {
    store::update_run(run_id, |r| change(&mut r.steps[index])).map(|(r, ())| r)
}

pub fn kill_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}

pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, 0) == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

pub fn abandon(run_id: &str) -> Result<Run, OpError> {
    let started = store::load_run(run_id).ok().and_then(|r| super::cron::parse_rfc3339(&r.started_at));
    mark_cancelled(run_id, 0, Instant::now())?;
    let ms = started.map(|s| ((super::cron::now() - s).max(0) as u64) * 1000);
    store::update_run(run_id, |r| r.duration_ms = ms).map(|(r, ())| r)
}

fn read_capped(mut from: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = from.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut b = sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if b.len() < READ_CAP {
                b.extend_from_slice(&chunk[..n]);
            }
        }
    });
    buf
}

fn run_child(run_id: &str, mut cmd: Command, red: &Redactor) -> Outcome {
    for name in CHILD_ENV_DROP {
        cmd.env_remove(name);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("cannot start {:?}: {e}", cmd.get_program())),
    };
    let pid = child.id();
    let _ = store::update_run(run_id, |r| r.child_pid = Some(pid));
    let out = child.stdout.take().map(read_capped);
    let err = child.stderr.take().map(read_capped);
    let mut last_check = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if last_check.elapsed() >= poll() {
            last_check = Instant::now();
            if cancelled(run_id) {
                kill_group(pid);
                let _ = child.kill();
                let _ = child.wait();
                let _ = store::update_run(run_id, |r| r.child_pid = None);
                return Outcome::Cancelled;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let _ = store::update_run(run_id, |r| r.child_pid = None);
    let text = |b: Option<Arc<Mutex<Vec<u8>>>>| b.map(|b| String::from_utf8_lossy(&b.lock().unwrap_or_else(std::sync::PoisonError::into_inner)).into_owned()).unwrap_or_default();
    std::thread::sleep(Duration::from_millis(30));
    let output = cap(&red.scrub(format!("{}{}", text(out), text(err)).trim()));
    match status {
        Some(s) if s.success() => Outcome::Ok(output),
        Some(s) => Outcome::Failed(format!("exited with {}\n{output}", s.code().map_or("a signal".into(), |c| c.to_string()))),
        None => Outcome::Failed("could not wait for the process".into()),
    }
}

fn result_text(result: &Value) -> String {
    match result["content"].as_array() {
        Some(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        None => result.to_string(),
    }
}

fn as_text(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), String::from)
}

fn captured(result: &Value, names: &[String]) -> Result<Vec<(String, String)>, String> {
    let text = result_text(result);
    let parsed: Option<Value> = serde_json::from_str(&text).ok();
    names
        .iter()
        .map(|name| {
            let found = result.get("structuredContent").and_then(|s| s.get(name)).or_else(|| result.get(name)).or_else(|| parsed.as_ref().and_then(|p| p.get(name)));
            match found {
                Some(v) => Ok((name.clone(), as_text(v))),
                None if names.len() == 1 && parsed.is_none() && !text.trim().is_empty() => Ok((name.clone(), text.trim().to_string())),
                None => Err(format!("the result has no value named {name:?}")),
            }
        })
        .collect()
}

struct State<'a> {
    task: &'a Task,
    host: &'a dyn Host,
    red: Redactor,
    values: HashMap<String, String>,
    sensitive: BTreeSet<String>,
}

impl State<'_> {
    fn keep(&mut self, pairs: Vec<(String, String)>) {
        for (name, value) in pairs {
            if self.sensitive.contains(&name) || sensitive_key(&name) {
                self.red.add(&value);
            }
            self.values.insert(name, value);
        }
    }

    fn wait(&mut self, run_id: &str, index: usize, instructions: &str, wait_for: &Option<super::model::WaitFor>) -> Outcome {
        let deadline = wait_for.as_ref().map(|w| Instant::now() + Duration::from_secs(w.timeout_sec));
        let shown = self.red.scrub(instructions);
        let _ = store::update_run(run_id, |r| {
            r.status = Status::Waiting;
            r.steps[index].status = Status::Waiting;
            r.steps[index].instructions = Some(shown);
        });
        loop {
            let Ok(run) = store::load_run(run_id) else { return Outcome::Failed("the run record disappeared".into()) };
            if run.cancel_requested {
                return Outcome::Cancelled;
            }
            if run.resume_requested {
                let _ = store::update_run(run_id, |r| r.resume_requested = false);
                return Outcome::Ok("resumed".into());
            }
            if wait_for.as_ref().is_some_and(|w| w.kind == WaitKind::SecretUnset) && !self.task.writes_secrets.is_empty() && self.task.writes_secrets.iter().all(|s| self.host.secret_is_set(&s.server, &s.key)) {
                return Outcome::Ok("the secrets this task writes are set".into());
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Outcome::Failed(format!("timed out after {}s waiting for you", wait_for.as_ref().map_or(0, |w| w.timeout_sec)));
            }
            std::thread::sleep(poll());
        }
    }

    fn step(&mut self, run_id: &str, index: usize, step: &Step) -> Outcome {
        match step {
            Step::NeedsYou { instructions, wait_for, .. } => self.wait(run_id, index, instructions, wait_for),
            Step::Mcp { server, tool, args, capture, .. } => {
                if !self.task.requires.servers.contains(server) {
                    return Outcome::Failed(format!("server {server:?} is not in requires.servers"));
                }
                match self.host.call_tool(server, tool, args.clone()) {
                    Err(e) => Outcome::Failed(self.red.scrub(&e)),
                    Ok(result) => self.finish_value(&result, capture, result["isError"] == Value::Bool(true)),
                }
            }
            Step::Routine { routine_id, script, args, capture, .. } => match self.host.run_routine(routine_id.as_deref(), script.as_deref(), args.clone().unwrap_or(Value::Null)) {
                Err(e) => Outcome::Failed(self.red.scrub(&e)),
                Ok(value) => self.finish_value(&value, capture, false),
            },
            Step::Exec { program, args, .. } => {
                if !self.task.requires.commands.contains(program) {
                    return Outcome::Failed(format!("program {program:?} is not in requires.commands"));
                }
                let mut cmd = Command::new(program);
                cmd.args(args);
                run_child(run_id, cmd, &self.red)
            }
            Step::Prompt { prompt, allowed_tools, model, .. } => {
                let mut cmd = Command::new(self.host.claude_program());
                cmd.args(["-p", prompt, "--output-format", "text"]);
                if !allowed_tools.is_empty() {
                    cmd.arg("--allowedTools").arg(allowed_tools.join(","));
                }
                if let Some(model) = model {
                    cmd.arg("--model").arg(model);
                }
                run_child(run_id, cmd, &self.red)
            }
            Step::SecretSet { server, key, from, .. } => {
                if !self.task.declares(server, key) {
                    return Outcome::Failed(format!("{server}/{key} is not listed in writesSecrets; nothing was written"));
                }
                let Some(value) = self.values.get(from).cloned() else { return Outcome::Failed(format!("no captured value named {from:?}")) };
                self.red.add(&value);
                match self.host.set_secret(server, key, &value) {
                    Ok(()) => Outcome::Ok(format!("stored {key} for {server}")),
                    Err(e) => Outcome::Failed(self.red.scrub(&e)),
                }
            }
            Step::RestartServer { server, .. } => match self.host.restart_server(server) {
                Ok(note) => Outcome::Ok(note),
                Err(e) => Outcome::Failed(self.red.scrub(&e)),
            },
        }
    }

    fn finish_value(&mut self, result: &Value, capture: &[String], is_error: bool) -> Outcome {
        let pairs = match captured(result, capture) {
            Ok(p) => p,
            Err(e) if !is_error => return Outcome::Failed(e),
            Err(_) => Vec::new(),
        };
        self.keep(pairs);
        let text = cap(&self.red.scrub(&result_text(result)));
        if is_error { Outcome::Failed(text) } else { Outcome::Ok(text) }
    }
}

fn finish(run_id: &str, status: Status, error: Option<String>, started: Instant) -> Result<Run, OpError> {
    store::update_run(run_id, |r| {
        r.status = status;
        r.error = error;
        r.ended_at = Some(now());
        r.duration_ms = Some(started.elapsed().as_millis() as u64);
        r.child_pid = None;
        r.resume_requested = false;
    })
    .map(|(r, ())| r)
}

pub fn execute(run_id: &str, host: &dyn Host) -> Result<Run, OpError> {
    let started = Instant::now();
    let run = store::load_run(run_id)?;
    let task = store::load_task(&run.task)?;
    let _ = store::update_run(run_id, |r| r.runner_pid = Some(std::process::id()));
    let mut state = State { task: &task, host, red: Redactor::default(), values: HashMap::new(), sensitive: task.secret_sources().into_iter().map(String::from).collect() };
    for (index, step) in task.steps.iter().enumerate() {
        if cancelled(run_id) {
            return mark_cancelled(run_id, index, started);
        }
        set_step(run_id, index, |s| {
            s.status = Status::Running;
            s.started_at = Some(now());
        })?;
        let _ = store::update_run(run_id, |r| r.status = Status::Running);
        let outcome = state.step(run_id, index, step);
        let (status, output) = match outcome {
            Outcome::Ok(o) => (Status::Ok, o),
            Outcome::Failed(_) if cancelled(run_id) => return mark_cancelled(run_id, index, started),
            Outcome::Failed(o) => (Status::Failed, o),
            Outcome::Cancelled => return mark_cancelled(run_id, index, started),
        };
        set_step(run_id, index, |s| {
            s.status = status;
            s.ended_at = Some(now());
            s.output = state.red.scrub(&output);
        })?;
        if status == Status::Failed {
            let _ = store::update_run(run_id, |r| r.steps[index + 1..].iter_mut().for_each(|s| s.status = Status::Skipped));
            return finish(run_id, Status::Failed, Some(format!("step {:?} failed", step.id())), started);
        }
    }
    let _ = store::clear_attention(&task.id);
    finish(run_id, Status::Ok, None, started)
}

fn mark_cancelled(run_id: &str, from: usize, started: Instant) -> Result<Run, OpError> {
    let _ = store::update_run(run_id, |r| {
        for s in r.steps[from..].iter_mut().filter(|s| !s.status.finished() && s.status != Status::Ok) {
            s.status = if s.status == Status::Pending { Status::Skipped } else { Status::Cancelled };
            s.ended_at = Some(now());
        }
    });
    finish(run_id, Status::Cancelled, Some("cancelled".into()), started)
}
