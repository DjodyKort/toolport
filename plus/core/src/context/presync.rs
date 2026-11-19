//! What runs before a `claude` launch: the org file pull when its interval has passed, then
//! `context sync`. A failure is recorded and only warns; Claude starts regardless. The record in
//! the data directory is what the Rules and launch tab shows as the last run.

use super::bundle_io;
use super::fsx;
use super::layer_deploy;
use super::ops::{self, SyncOpts};
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use toolport_plus_api::Host;

pub const DEFAULT_INTERVAL_SECS: u64 = 14_400;
const OUTPUT_CAP: usize = 4 * 1024;
const PULL_TIMEOUT: Duration = Duration::from_secs(120);
const PYTHON_CALL: &str = "import sys; sys.path.insert(0, sys.argv[1]); from cfdevtools.claude import sync_claude_files, auto_update_mcp_repos; sync_claude_files(); auto_update_mcp_repos()";

/// `orgPull.intervalSecs` from `context.json`, else the org tool's own `CLAUDE_SYNC_INTERVAL`,
/// else four hours.
pub fn interval_secs(config: &crate::sources::config::Config) -> u64 {
    config
        .org_pull
        .as_ref()
        .ok()
        .and_then(|pull| pull.interval_secs)
        .or_else(|| {
            std::env::var("CLAUDE_SYNC_INTERVAL")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|s| *s > 0)
        })
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

/// What pulls the org file: the built-in call into the org tool, or the argv from `context.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrgCommand {
    BuiltIn,
    Argv(Vec<String>),
}

impl OrgCommand {
    pub fn from_config(config: &crate::sources::config::Config) -> Result<Self, String> {
        match config.org_pull.clone()?.command {
            Some(argv) => Ok(Self::Argv(argv)),
            None => Ok(Self::BuiltIn),
        }
    }
}

/// An argv for display: items with a space or quote are wrapped in quotes.
pub fn command_text(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
                format!("{a:?}")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub last_attempt_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ok_at: Option<i64>,
    pub duration_ms: u64,
    pub ok: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// What the org tool or the sync said when it failed, scrubbed and cut short.
    #[serde(default)]
    pub output: String,
}

fn record_path(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("context-presync.json")
}

pub fn read(data_dir: &Path) -> Option<Record> {
    let text = bundle_io::read_exact(&record_path(data_dir)).ok()??;
    serde_json::from_str(&text).ok()
}

fn write(data_dir: &Path, record: &Record) -> Result<(), String> {
    let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())? + "\n";
    crate::fsx::atomic_write(
        &record_path(data_dir),
        &text,
        crate::fsx::Mode::KeepOrDefault,
    )
    .map_err(|e| format!("{}: {e}", record_path(data_dir).display()))
}

/// The corporate tool that syncs the org file into `~/.claude`.
pub trait OrgTool {
    fn pull(&self, dir: &Path, command: &OrgCommand) -> Result<(), String>;
}

pub struct ProcessOrgTool;

fn finish(mut child: std::process::Child, what: &str) -> Result<(), String> {
    let deadline = Instant::now() + PULL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut said = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let mut buf = Vec::new();
                    let _ = err.by_ref().take(OUTPUT_CAP as u64).read_to_end(&mut buf);
                    said = String::from_utf8_lossy(&buf).trim().to_string();
                }
                return Err(if said.is_empty() {
                    format!("{what} exited with {status}")
                } else {
                    format!("{what} exited with {status}: {said}")
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{what} did not finish in {} s",
                    PULL_TIMEOUT.as_secs()
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("{what}: {e}")),
        }
    }
}

impl OrgTool for ProcessOrgTool {
    fn pull(&self, dir: &Path, command: &OrgCommand) -> Result<(), String> {
        let OrgCommand::Argv(argv) = command else {
            return built_in_pull(dir);
        };
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| "the org pull command is empty".to_string())?;
        let child = Command::new(program)
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{program}: {e}"))?;
        finish(child, program)
    }
}

fn built_in_pull(dir: &Path) -> Result<(), String> {
    let git = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["pull", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    finish(git, "git pull")?;
    let python = Command::new(dir.join(".venv/bin/python"))
        .args(["-c", PYTHON_CALL])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the org tool's python: {e}"))?;
    finish(python, "the org tool's sync")
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Opts {
    pub force: bool,
}

/// The built-in pull needs the org tool's clone and python; a configured command only needs the
/// folder it runs in.
fn org_installed(dir: &Path, command: &Result<OrgCommand, String>) -> bool {
    match command {
        Ok(OrgCommand::Argv(_)) => fsx::is_dir(dir),
        _ => fsx::is_dir(&dir.join(".git")) && fsx::is_file(&dir.join(".venv/bin/python")),
    }
}

fn cap(text: &str) -> String {
    let scrubbed = crate::redact::scrub_text(text.to_string());
    scrubbed.chars().take(OUTPUT_CAP).collect()
}

fn failed_rows(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.get("action").and_then(Value::as_str) == Some("failed") {
                let what = map
                    .get("layer")
                    .or_else(|| map.get("name"))
                    .or_else(|| map.get("entry"))
                    .and_then(Value::as_str)
                    .unwrap_or("a part");
                let why = map
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("failed");
                out.push(format!("{what}: {why}"));
            }
            if let Some(error) = map.get("error").and_then(Value::as_str) {
                out.push(error.to_string());
            }
            map.values().for_each(|v| failed_rows(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| failed_rows(v, out)),
        _ => {}
    }
}

/// Runs the presync when it is due (or with `force`) and records the outcome. Nothing is written
/// when it is not due.
pub fn run(host: &dyn Host, tool: &dyn OrgTool, opts: Opts) -> Result<Value, OpError> {
    let env = layer_deploy::env(host)?;
    let now = host.now_unix();
    let config = crate::sources::config::load(&env.roots.context_config_path());
    let interval = interval_secs(&config);
    let previous = read(&env.data_dir);
    let places = crate::sources::Places::from_env(&env.roots, &config);
    let command = OrgCommand::from_config(&config);
    let stamp_file = places.claude_home.join(".last_auto_sync");
    let last_org = fsx::read_text(&stamp_file, 64).and_then(|t| t.trim().parse::<i64>().ok());
    let org_due = command.is_err()
        || (org_installed(&places.corp_tools_dir, &command)
            && last_org.is_none_or(|at| now - at >= interval as i64));
    let sync_due = opts.force
        || org_due
        || previous
            .as_ref()
            .and_then(|r| r.last_ok_at)
            .is_none_or(|at| now - at >= interval as i64);
    if !sync_due {
        let next = previous
            .and_then(|r| r.last_ok_at)
            .map(|at| at + interval as i64);
        return Ok(json!({ "ran": false, "reason": "interval not passed", "nextAfter": next }));
    }

    let started = Instant::now();
    let mut warnings = Vec::new();
    let mut output = String::new();
    let mut org_pulled = false;
    let mut pull_failed = false;
    if org_due {
        match &command {
            Ok(command) => {
                org_pulled = true;
                if let Err(why) = tool.pull(&places.corp_tools_dir, command) {
                    pull_failed = true;
                    warnings.push(
                        "the org tool could not be pulled; the files on disk are from its last good run"
                            .to_string(),
                    );
                    output.push_str(&why);
                }
                let _ = crate::fsx::atomic_write(
                    &stamp_file,
                    &format!("{now}\n"),
                    crate::fsx::Mode::KeepOrDefault,
                );
            }
            // No stamp: the pull is tried again at the next launch once context.json is fixed.
            Err(why) => {
                pull_failed = true;
                warnings.push(format!("the org file was not pulled: {why}"));
                output.push_str(why);
            }
        }
    }
    let synced = ops::sync_with(
        host,
        &SyncOpts {
            dry_run: false,
            rules: false,
            attended: false,
        },
    );
    let mut problems = Vec::new();
    match &synced {
        Ok(data) => failed_rows(data, &mut problems),
        Err(error) => problems.push(error.message.clone()),
    }
    let ok = problems.is_empty() && !pull_failed;
    warnings.extend(problems.iter().map(|p| format!("context sync: {p}")));
    if !problems.is_empty() {
        output.push_str(&problems.join("\n"));
    }
    let record = Record {
        last_attempt_at: now,
        last_ok_at: if ok {
            Some(now)
        } else {
            previous.and_then(|r| r.last_ok_at)
        },
        duration_ms: started.elapsed().as_millis() as u64,
        ok,
        warnings: warnings.clone(),
        output: cap(&output),
    };
    write(&env.data_dir, &record).map_err(|e| OpError::failed("presync_record", e))?;
    if ok {
        arm_wrapper_baseline(&env, &places);
    }
    let data = json!({
        "ran": true,
        "ok": ok,
        "orgPulled": org_pulled,
        "durationMs": record.duration_ms,
        "warnings": warnings,
        "nextAfter": now + interval as i64,
    });
    if ok {
        Ok(data)
    } else {
        Err(OpError::failed(
            "presync_failed",
            warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "the presync failed".into()),
        )
        .with_details(data))
    }
}

fn arm_wrapper_baseline(env: &layer_deploy::Env, places: &crate::sources::Places) {
    let wrapper = places
        .corp_tools_dir
        .join("claude")
        .join("shell-wrapper.sh");
    if let Some(bytes) = fsx::read_bytes(&wrapper, 8 * 1024 * 1024) {
        let _ = super::config::arm_wrapper_hash(
            &env.roots.context_config_path(),
            &super::hash::sha256_hex(bytes),
        );
    }
}
