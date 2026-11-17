//! Runs one task run to its end: steps in order, a `needs-you` step pauses the run (`waiting`)
//! until `task resume`, its `waitFor` condition or its timeout. The run record is the only
//! channel between this runner and `task resume|cancel`. Captured values live in this
//! function's memory and in the `Redactor`, never in a record.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;
use toolport_plus_api::{ProcessOutcome, ProcessSpec, TaskHost};

use super::cron::rfc3339;
use super::model::{Step, Task, WaitFor, WaitKind};
use super::redactor::Redactor;
use super::store::{self, cap, Run, Status};
use crate::op::OpError;
use crate::redact::sensitive_key;

enum Outcome {
    Ok(String),
    Failed(String),
    Cancelled,
}

fn poll() -> Duration {
    Duration::from_millis(
        std::env::var("TOOLPORT_TASK_POLL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100),
    )
}

fn now(host: &dyn TaskHost) -> String {
    rfc3339(host.now())
}

fn cancelled(base: &Path, run_id: &str) -> bool {
    store::load_run(base, run_id)
        .map(|r| r.cancel_requested)
        .unwrap_or(false)
}

fn set_step(
    base: &Path,
    run_id: &str,
    index: usize,
    change: impl FnOnce(&mut store::StepRun),
) -> Result<Run, OpError> {
    store::update_run(base, run_id, |r| change(&mut r.steps[index])).map(|(r, ())| r)
}

pub fn abandon(host: &dyn TaskHost, base: &Path, run_id: &str) -> Result<Run, OpError> {
    let started = store::load_run(base, run_id)
        .ok()
        .and_then(|r| super::cron::parse_rfc3339(&r.started_at));
    mark_cancelled(host, base, run_id, 0, Instant::now())?;
    let ms = started.map(|s| ((host.now() - s).max(0) as u64) * 1000);
    store::update_run(base, run_id, |r| r.duration_ms = ms).map(|(r, ())| r)
}

fn result_text(result: &Value) -> String {
    match result["content"].as_array() {
        Some(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
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
            let found = result
                .get("structuredContent")
                .and_then(|s| s.get(name))
                .or_else(|| result.get(name))
                .or_else(|| parsed.as_ref().and_then(|p| p.get(name)));
            match found {
                Some(v) => Ok((name.clone(), as_text(v))),
                None if names.len() == 1 && parsed.is_none() && !text.trim().is_empty() => {
                    Ok((name.clone(), text.trim().to_string()))
                }
                None => Err(format!("the result has no value named {name:?}")),
            }
        })
        .collect()
}

struct State<'a> {
    base: &'a Path,
    task: &'a Task,
    host: &'a dyn TaskHost,
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

    fn process(&self, run_id: &str, spec: ProcessSpec) -> Outcome {
        let base = self.base;
        let outcome = self
            .host
            .task_run_process(&spec, &|| cancelled(base, run_id), &|pid| {
                let _ = store::update_run(base, run_id, |r| r.child_pid = Some(pid));
            });
        let _ = store::update_run(base, run_id, |r| r.child_pid = None);
        match outcome {
            ProcessOutcome::Cancelled => Outcome::Cancelled,
            ProcessOutcome::SpawnFailed(error) => {
                Outcome::Failed(format!("cannot start {:?}: {error}", spec.program))
            }
            ProcessOutcome::Exited { code, output } => {
                let output = cap(&self.red.scrub(output.trim()));
                if code == Some(0) {
                    Outcome::Ok(output)
                } else {
                    Outcome::Failed(format!(
                        "exited with {}\n{output}",
                        code.map_or("a signal".into(), |c| c.to_string())
                    ))
                }
            }
        }
    }

    fn wait(
        &mut self,
        run_id: &str,
        index: usize,
        instructions: &str,
        wait_for: &Option<WaitFor>,
    ) -> Outcome {
        let deadline = wait_for
            .as_ref()
            .map(|w| Instant::now() + Duration::from_secs(w.timeout_sec));
        let shown = self.red.scrub(instructions);
        let _ = store::update_run(self.base, run_id, |r| {
            r.status = Status::Waiting;
            r.steps[index].status = Status::Waiting;
            r.steps[index].instructions = Some(shown);
        });
        loop {
            let Ok(run) = store::load_run(self.base, run_id) else {
                return Outcome::Failed("the run record disappeared".into());
            };
            if run.cancel_requested {
                return Outcome::Cancelled;
            }
            if run.resume_requested {
                let _ = store::update_run(self.base, run_id, |r| r.resume_requested = false);
                return Outcome::Ok("resumed".into());
            }
            let secrets_set = wait_for
                .as_ref()
                .is_some_and(|w| w.kind == WaitKind::SecretUnset)
                && !self.task.writes_secrets.is_empty()
                && self
                    .task
                    .writes_secrets
                    .iter()
                    .all(|s| self.host.task_secret_is_set(&s.server, &s.key));
            if secrets_set {
                return Outcome::Ok("the secrets this task writes are set".into());
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Outcome::Failed(format!(
                    "timed out after {}s waiting for you",
                    wait_for.as_ref().map_or(0, |w| w.timeout_sec)
                ));
            }
            std::thread::sleep(poll());
        }
    }

    fn step(&mut self, run_id: &str, index: usize, step: &Step) -> Outcome {
        match step {
            Step::NeedsYou {
                instructions,
                wait_for,
                ..
            } => self.wait(run_id, index, instructions, wait_for),
            Step::Mcp {
                server,
                tool,
                args,
                capture,
                ..
            } => {
                if !self.task.requires.servers.contains(server) {
                    return Outcome::Failed(format!(
                        "server {server:?} is not in requires.servers"
                    ));
                }
                match self.host.task_call_tool(server, tool, args.clone()) {
                    Err(e) => Outcome::Failed(self.red.scrub(&e)),
                    Ok(result) => {
                        self.finish_value(&result, capture, result["isError"] == Value::Bool(true))
                    }
                }
            }
            Step::Script {
                script,
                args,
                capture,
                ..
            } => match self.host.task_run_script(
                script,
                args.clone().unwrap_or(Value::Null),
                &self.task.requires.servers,
            ) {
                Err(e) => Outcome::Failed(self.red.scrub(&e)),
                Ok(value) => self.finish_value(&value, capture, false),
            },
            Step::Exec { program, args, .. } => {
                if !self.task.requires.commands.contains(program) {
                    return Outcome::Failed(format!(
                        "program {program:?} is not in requires.commands"
                    ));
                }
                self.process(
                    run_id,
                    ProcessSpec {
                        program: program.clone(),
                        args: args.clone(),
                    },
                )
            }
            Step::Prompt {
                prompt,
                allowed_tools,
                model,
                ..
            } => {
                let mut args = vec![
                    "-p".into(),
                    prompt.clone(),
                    "--output-format".into(),
                    "text".into(),
                ];
                if !allowed_tools.is_empty() {
                    args.push("--allowedTools".into());
                    args.push(allowed_tools.join(","));
                }
                if let Some(model) = model {
                    args.push("--model".into());
                    args.push(model.clone());
                }
                self.process(
                    run_id,
                    ProcessSpec {
                        program: self.host.task_claude_program(),
                        args,
                    },
                )
            }
            Step::SecretSet {
                server, key, from, ..
            } => {
                if !self.task.declares(server, key) {
                    return Outcome::Failed(format!(
                        "{server}/{key} is not listed in writesSecrets; nothing was written"
                    ));
                }
                let Some(value) = self.values.get(from).cloned() else {
                    return Outcome::Failed(format!("no captured value named {from:?}"));
                };
                self.red.add(&value);
                match self.host.task_set_secret(server, key, &value) {
                    Ok(()) => Outcome::Ok(format!("stored {key} for {server}")),
                    Err(e) => Outcome::Failed(self.red.scrub(&e)),
                }
            }
            Step::RestartServer { server, .. } => match self.host.task_restart_server(server) {
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
        if is_error {
            Outcome::Failed(text)
        } else {
            Outcome::Ok(text)
        }
    }
}

fn finish(
    host: &dyn TaskHost,
    base: &Path,
    run_id: &str,
    status: Status,
    error: Option<String>,
    started: Instant,
) -> Result<Run, OpError> {
    let ended = now(host);
    store::update_run(base, run_id, |r| {
        r.status = status;
        r.error = error;
        r.ended_at = Some(ended);
        r.duration_ms = Some(started.elapsed().as_millis() as u64);
        r.child_pid = None;
        r.resume_requested = false;
    })
    .map(|(r, ())| r)
}

fn missing_server(task: &Task, host: &dyn TaskHost) -> Option<(String, Value)> {
    let server = task
        .requires
        .servers
        .iter()
        .find(|s| !host.task_server_installed(s))?;
    let name = if server == "playwright" {
        "Playwright"
    } else {
        server.as_str()
    };
    let notice = format!(
        "install the {name} server: run toolportctl server install {server}, then start the task again"
    );
    let action = serde_json::json!({"label": format!("Install {name}"), "command": ["toolportctl", "server", "install", server]});
    Some((notice, action))
}

pub fn execute(host: &dyn TaskHost, base: &Path, run_id: &str) -> Result<Run, OpError> {
    let started = Instant::now();
    let run = store::load_run(base, run_id)?;
    let task = store::load_task(base, &run.task)?;
    let _ = store::update_run(base, run_id, |r| r.runner_pid = Some(std::process::id()));
    if let (Some((notice, action)), true) = (missing_server(&task, host), !task.steps.is_empty()) {
        set_step(base, run_id, 0, |s| {
            s.status = Status::Failed;
            s.started_at = Some(now(host));
            s.ended_at = Some(now(host));
            s.output = notice.clone();
        })?;
        let _ = store::update_run(base, run_id, |r| {
            r.steps[1..]
                .iter_mut()
                .for_each(|s| s.status = Status::Skipped);
            r.action = Some(action);
        });
        return finish(host, base, run_id, Status::Failed, Some(notice), started);
    }
    let mut state = State {
        base,
        task: &task,
        host,
        red: Redactor::default(),
        values: HashMap::new(),
        sensitive: task
            .secret_sources()
            .into_iter()
            .map(String::from)
            .collect(),
    };
    for (index, step) in task.steps.iter().enumerate() {
        if cancelled(base, run_id) {
            return mark_cancelled(host, base, run_id, index, started);
        }
        set_step(base, run_id, index, |s| {
            s.status = Status::Running;
            s.started_at = Some(now(host));
        })?;
        let _ = store::update_run(base, run_id, |r| r.status = Status::Running);
        let outcome = state.step(run_id, index, step);
        let (status, output) = match outcome {
            Outcome::Ok(o) => (Status::Ok, o),
            Outcome::Failed(_) if cancelled(base, run_id) => {
                return mark_cancelled(host, base, run_id, index, started)
            }
            Outcome::Failed(o) => (Status::Failed, o),
            Outcome::Cancelled => return mark_cancelled(host, base, run_id, index, started),
        };
        let scrubbed = state.red.scrub(&output);
        set_step(base, run_id, index, |s| {
            s.status = status;
            s.ended_at = Some(now(host));
            s.output = scrubbed;
        })?;
        if status == Status::Failed {
            let _ = store::update_run(base, run_id, |r| {
                r.steps[index + 1..]
                    .iter_mut()
                    .for_each(|s| s.status = Status::Skipped)
            });
            return finish(
                host,
                base,
                run_id,
                Status::Failed,
                Some(format!("step {:?} failed", step.id())),
                started,
            );
        }
    }
    let _ = store::clear_attention(base, &task.id);
    finish(host, base, run_id, Status::Ok, None, started)
}

fn mark_cancelled(
    host: &dyn TaskHost,
    base: &Path,
    run_id: &str,
    from: usize,
    started: Instant,
) -> Result<Run, OpError> {
    let ended = now(host);
    let _ = store::update_run(base, run_id, |r| {
        for s in r.steps[from..]
            .iter_mut()
            .filter(|s| !s.status.finished() && s.status != Status::Ok)
        {
            s.status = if s.status == Status::Pending {
                Status::Skipped
            } else {
                Status::Cancelled
            };
            s.ended_at = Some(ended.clone());
        }
    });
    finish(
        host,
        base,
        run_id,
        Status::Cancelled,
        Some("cancelled".into()),
        started,
    )
}
