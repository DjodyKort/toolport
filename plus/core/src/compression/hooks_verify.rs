//! `compression hooks verify`: runs the command of each hook entry the way Claude Code does,
//! with a synthetic PreToolUse event on stdin, in a temporary directory and without network, and
//! says what came back. An entry that Toolport wrapped runs through its wrapper, so the whole
//! chain is exercised; the wrapper counts nothing and ignores a pause while it is verified.

use super::hooks::{find, is_wrapped, read_hooks, HookProvider, HOOK_PROVIDERS, VERIFY_ENV};
use super::ledger::now_ms;
use super::store::Paths;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use toolport_plus_api::SettingsStore;

pub const TIMEOUT: Duration = Duration::from_secs(10);
const BASH_COMMAND: &str = "git status";
const SHOWN_OUTPUT: usize = 400;

pub const PAYLOAD_NOTE: &str = "A synthetic PreToolUse event for Bash (rtk) or Read (read-once) on stdin, a temporary working directory, no network. The wrapper counts nothing while a hook is verified, and `toolportctl` resolves to the binary that sits next to the one running this check.";

pub struct Options {
    pub timeout: Duration,
    /// Directory put in front of `PATH` for the hook command.
    pub path_prepend: Option<PathBuf>,
}

impl Options {
    /// Hook commands start `toolportctl`; the one beside this binary answers to that name.
    pub fn system() -> Self {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .filter(|dir| dir.join("toolportctl").is_file());
        Self {
            path_prepend: beside,
            ..Self::default()
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: TIMEOUT,
            path_prepend: None,
        }
    }
}

struct Run {
    exit: Option<i32>,
    stdout: String,
    stderr: String,
    latency_ms: u64,
    timed_out: bool,
    spawn_error: Option<String>,
}

impl Run {
    fn answered(&self) -> bool {
        self.exit == Some(2) || !self.stdout.trim().is_empty()
    }
}

#[cfg(unix)]
const SHELL: (&str, &str) = ("/bin/sh", "-c");
#[cfg(not(unix))]
const SHELL: (&str, &str) = ("cmd", "/C");

fn read_all(mut source: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = source.read_to_end(&mut bytes);
        bytes
    })
}

fn shown(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    match text.char_indices().nth(SHOWN_OUTPUT) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_string(),
    }
}

fn run(command: &str, stdin: &[u8], dir: &Path, options: &Options) -> Run {
    let mut cmd = Command::new(SHELL.0);
    cmd.arg(SHELL.1)
        .arg(command)
        .current_dir(dir)
        .env(VERIFY_ENV, "1")
        .env("CLAUDE_PROJECT_DIR", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(front) = &options.path_prepend {
        let mut dirs = vec![front.clone()];
        dirs.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        if let Ok(joined) = std::env::join_paths(dirs) {
            cmd.env("PATH", joined);
        }
    }
    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Run {
                exit: None,
                stdout: String::new(),
                stderr: String::new(),
                latency_ms: 0,
                timed_out: false,
                spawn_error: Some(e.to_string()),
            }
        }
    };
    if let Some(mut input) = child.stdin.take() {
        let bytes = stdin.to_vec();
        std::thread::spawn(move || {
            let _ = input.write_all(&bytes);
        });
    }
    let out = child.stdout.take().map(read_all);
    let err = child.stderr.take().map(read_all);
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if started.elapsed() >= options.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => break None,
        }
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    let collect = |handle: Option<std::thread::JoinHandle<Vec<u8>>>| {
        handle
            .filter(|_| !timed_out)
            .and_then(|h| h.join().ok())
            .map(|b| shown(&b))
            .unwrap_or_default()
    };
    Run {
        exit: status,
        stdout: collect(out),
        stderr: collect(err),
        latency_ms,
        timed_out,
        spawn_error: None,
    }
}

fn event(dir: &Path, session: &str, tool: &str, input: Value) -> Vec<u8> {
    json!({
        "session_id": session,
        "transcript_path": dir.join("transcript.jsonl"),
        "cwd": dir,
        "permission_mode": "default",
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": input,
        "tool_use_id": "toolu_toolport_verify",
    })
    .to_string()
    .into_bytes()
}

fn why_not_run(run: &Run) -> Option<String> {
    if let Some(error) = &run.spawn_error {
        return Some(format!("the command could not start: {error}"));
    }
    if run.timed_out {
        return Some("the command did not finish in time".into());
    }
    match run.exit {
        Some(127) => Some(format!(
            "the shell did not find a command ({})",
            run.stderr.lines().next().unwrap_or("exit 127")
        )),
        None => Some("the command was stopped by a signal".into()),
        _ => None,
    }
}

fn rewrite_of(stdout: &str) -> Option<String> {
    let answer: Value = serde_json::from_str(stdout.trim()).ok()?;
    let rewritten = answer
        .pointer("/hookSpecificOutput/updatedInput/command")?
        .as_str()?
        .to_string();
    (rewritten != BASH_COMMAND).then_some(rewritten)
}

type Steps = Vec<(String, Run)>;
type Judge = fn(&[&Run]) -> Verdict;

struct Verdict {
    pass: bool,
    reason: Option<String>,
    returned: Option<String>,
}

fn fail(reason: impl Into<String>) -> Verdict {
    Verdict {
        pass: false,
        reason: Some(reason.into()),
        returned: None,
    }
}

fn judge_rtk(runs: &[&Run]) -> Verdict {
    let run = runs[0];
    if let Some(reason) = why_not_run(run) {
        return fail(reason);
    }
    if run.exit != Some(0) {
        return fail(format!("the hook exited with {}", run.exit.unwrap_or(-1)));
    }
    if run.stdout.trim().is_empty() {
        return fail(
            "the hook answered nothing; rtk may be missing or may not rewrite this command",
        );
    }
    match rewrite_of(&run.stdout) {
        Some(rewritten) => Verdict {
            pass: true,
            reason: None,
            returned: Some(format!("rewrote `{BASH_COMMAND}` to `{rewritten}`")),
        },
        None => fail("the hook answered, but not with a rewritten command"),
    }
}

fn judge_read_once(runs: &[&Run]) -> Verdict {
    if let Some(reason) = runs.iter().find_map(|r| why_not_run(r)) {
        return fail(reason);
    }
    let (first, second) = (runs[0], runs[1]);
    if first.answered() {
        return fail(
            "the hook answered the first read of a file, so a repeat cannot be told from it",
        );
    }
    if !second.answered() {
        return fail("the hook let a repeat read of an unchanged file through without an answer");
    }
    let said = if second.stdout.trim().is_empty() {
        format!("exit {}", second.exit.unwrap_or(-1))
    } else {
        second.stdout.clone()
    };
    Verdict {
        pass: true,
        reason: None,
        returned: Some(format!("answered the repeat read: {said}")),
    }
}

fn probe(provider: &HookProvider, command: &str, options: &Options) -> Value {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let session = format!("toolport-verify-{}-{stamp}", std::process::id());
    let dir = std::env::temp_dir().join(session.clone());
    let setup = std::fs::create_dir_all(&dir).and_then(|_| {
        std::fs::write(dir.join("transcript.jsonl"), "")?;
        std::fs::write(dir.join("fixture.txt"), "toolport verify\n")
    });
    if let Err(e) = setup {
        let _ = std::fs::remove_dir_all(&dir);
        return json!({"status": "fail", "reason": format!("no temporary directory: {e}"), "calls": []});
    }
    let (steps, verdict): (Steps, Judge) = if provider.id == "rtk" {
        let input = json!({"command": BASH_COMMAND, "description": "toolport verify"});
        let payload = event(&dir, &session, "Bash", input);
        (
            vec![(
                format!("Bash: {BASH_COMMAND}"),
                run(command, &payload, &dir, options),
            )],
            judge_rtk,
        )
    } else {
        let file = dir.join("fixture.txt");
        let payload = event(&dir, &session, "Read", json!({"file_path": file}));
        let steps = ["first read", "repeat read"]
            .into_iter()
            .map(|label| {
                (
                    format!("Read: {label}"),
                    run(command, &payload, &dir, options),
                )
            })
            .collect();
        (steps, judge_read_once)
    };
    let _ = std::fs::remove_dir_all(&dir);
    let runs: Vec<&Run> = steps.iter().map(|(_, r)| r).collect();
    let verdict = verdict(&runs);
    json!({
        "status": if verdict.pass { "pass" } else { "fail" },
        "reason": verdict.reason,
        "returned": verdict.returned,
        "latencyMs": steps.iter().map(|(_, r)| r.latency_ms).max(),
        "calls": steps.iter().map(|(label, r)| json!({
            "step": label,
            "exitCode": r.exit,
            "latencyMs": r.latency_ms,
            "answered": r.answered(),
            "stdout": r.stdout,
            "stderr": r.stderr,
        })).collect::<Vec<_>>(),
    })
}

/// Verifies the entry of `only`, or of every provider, in every settings file.
pub fn verify(
    store: &dyn SettingsStore,
    paths: &Paths,
    only: Option<&str>,
    options: &Options,
) -> Result<Value, String> {
    let ledger = super::hooks::read_ledger(paths)?;
    let paused = super::hooks::pause_active(&ledger, now_ms());
    let mut rows = Vec::new();
    for file in store.claude_settings_paths() {
        let (_, _, hooks) = read_hooks(store, &file)?;
        for provider in HOOK_PROVIDERS
            .iter()
            .filter(|p| only.is_none_or(|id| id == p.id))
        {
            let mut row = json!({
                "file": file.to_string_lossy(),
                "provider": provider.id,
            });
            match find(&hooks, provider) {
                None => {
                    row["status"] = json!("skipped");
                    row["reason"] = json!("no entry in this settings file");
                    row["calls"] = json!([]);
                }
                Some((_, _, command)) => {
                    let result = probe(provider, &command, options);
                    row["command"] = json!(command);
                    row["wrapped"] = json!(is_wrapped(&command));
                    row["paused"] = json!(paused && is_wrapped(&command));
                    if let (Some(target), Some(source)) = (row.as_object_mut(), result.as_object())
                    {
                        target.extend(source.clone());
                    }
                }
            }
            rows.push(row);
        }
    }
    let count = |status: &str| rows.iter().filter(|r| r["status"] == status).count();
    Ok(json!({
        "payload": PAYLOAD_NOTE,
        "passed": count("pass"),
        "failed": count("fail"),
        "skipped": count("skipped"),
        "paused": paused,
        "rows": rows,
    }))
}
