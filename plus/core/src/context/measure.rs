//! `context measure`: the real size of what a folder loads, read from Claude Code itself. One
//! minimal headless request in the folder; its first request's input (`input + cache_creation +
//! cache_read`) is the number. The instruction rows of `context loads` are estimates, so the part
//! of the measured total they do not cover is reported as "not itemised" instead of being
//! hidden: Claude Code's own prompt, tool definitions, the skill list, plugin parts and the error
//! of the estimate. A request costs model tokens, so it starts only with an explicit yes; an
//! answer from the cache costs nothing.
//!
//! Cache: `<data dir>/plus/cache/measure/<sha256(cwd|version|model)>.json`. An entry is stale when
//! the Claude Code version or a settings file of the folder changed; a stale entry is still served,
//! flagged, until a fresh run is asked for.

use super::bundle_io;
use super::fsx;
use super::hash::sha256_hex;
use super::loads::WhatLoads;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const PROMPT: &str = "Reply with the single word ok";
pub const DEFAULT_MODEL: &str = "haiku";
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
const SETTINGS_CAP: usize = 1 << 20;
const WINDOW_DEFAULT: u64 = 200_000;
const WINDOW_LARGE: u64 = 1_000_000;
const AS_IS: &str = "as-is";

pub fn window_for_model(model: &str) -> u64 {
    if model.contains("[1m]") {
        WINDOW_LARGE
    } else {
        WINDOW_DEFAULT
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parts {
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub total: u64,
    pub parts: Parts,
    pub model: String,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub plugins: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    cwd: String,
    claude_code_version: String,
    requested_model: String,
    variant: String,
    measured_at: String,
    settings_stamp: String,
    run: Run,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MeasureError {
    pub code: &'static str,
    pub message: String,
}

fn failed(code: &'static str, message: impl Into<String>) -> MeasureError {
    MeasureError {
        code,
        message: message.into(),
    }
}

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait Launcher {
    fn version(&self) -> Result<String, String>;
    fn request(&self, cwd: &Path, model: &str) -> Result<Output, String>;
}

pub struct ProcessLauncher {
    bin: String,
}

impl ProcessLauncher {
    pub fn from_env() -> Self {
        let bin = std::env::var("TOOLPORT_CLAUDE_BIN")
            .ok()
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "claude".to_string());
        Self { bin }
    }

    pub fn with_bin(bin: impl Into<String>) -> Self {
        Self { bin: bin.into() }
    }

    fn run(&self, mut command: Command, timeout: Duration) -> Result<Output, String> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("claude not found ({})", self.bin),
            _ => format!("could not start {}: {e}", self.bin),
        })?;
        let drain = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            })
        };
        let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
        let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() > timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("claude timed out after {} s", timeout.as_secs()));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => return Err(e.to_string()),
            }
        };
        Ok(Output {
            code: status.code().unwrap_or(-1),
            stdout: out.join().unwrap_or_default(),
            stderr: err.join().unwrap_or_default(),
        })
    }
}

impl Launcher for ProcessLauncher {
    fn version(&self) -> Result<String, String> {
        let mut command = Command::new(&self.bin);
        command.arg("--version");
        let out = self.run(command, VERSION_TIMEOUT)?;
        let version = out.stdout.split_whitespace().next().unwrap_or("");
        if out.code != 0 || version.is_empty() {
            return Err("claude --version failed".into());
        }
        Ok(version.to_string())
    }

    fn request(&self, cwd: &Path, model: &str) -> Result<Output, String> {
        let mut command = Command::new(&self.bin);
        command
            .args([
                "-p",
                PROMPT,
                "--output-format",
                "stream-json",
                "--verbose",
                "--max-turns",
                "1",
                "--no-session-persistence",
                "--model",
                model,
            ])
            .current_dir(cwd);
        self.run(command, REQUEST_TIMEOUT)
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn names(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["name"].as_str().map(str::to_string))
        .collect()
}

pub fn parse_stream(stdout: &str, wall_ms: u64) -> Result<Run, String> {
    let mut init: Option<Value> = None;
    let mut result: Option<Value> = None;
    for line in stdout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match (event["type"].as_str(), event["subtype"].as_str()) {
            (Some("system"), Some("init")) if init.is_none() => init = Some(event),
            (Some("result"), _) => result = Some(event),
            _ => {}
        }
    }
    let result = result.ok_or("claude printed no result event")?;
    if result["is_error"] == Value::Bool(true) {
        let text = result["result"].as_str().unwrap_or("the request failed");
        return Err(text
            .lines()
            .next()
            .unwrap_or("the request failed")
            .to_string());
    }
    let init = init.ok_or("claude printed no init event")?;
    let count = |key: &str| result["usage"][key].as_u64().unwrap_or(0);
    let parts = Parts {
        input: count("input_tokens"),
        cache_creation: count("cache_creation_input_tokens"),
        cache_read: count("cache_read_input_tokens"),
    };
    Ok(Run {
        total: parts.input + parts.cache_creation + parts.cache_read,
        parts,
        model: init["model"].as_str().unwrap_or("").to_string(),
        skills: strings(&init["skills"]),
        agents: strings(&init["agents"]),
        plugins: names(&init["plugins"]),
        mcp_servers: names(&init["mcp_servers"]),
        duration_ms: result["duration_ms"].as_u64().unwrap_or(wall_ms),
    })
}

fn settings_stamp(cwd: &Path) -> String {
    let mut seen = Vec::new();
    for file in ["settings.json", "settings.local.json"] {
        let path = cwd.join(".claude").join(file);
        if let Some(bytes) = fsx::read_bytes(&path, SETTINGS_CAP) {
            seen.extend_from_slice(path.to_string_lossy().as_bytes());
            seen.push(0);
            seen.extend_from_slice(&bytes);
            seen.push(0);
        }
    }
    sha256_hex(&seen)
}

fn cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("cache").join("measure")
}

fn entry_file(dir: &Path, cwd: &str, version: &str, model: &str) -> PathBuf {
    dir.join(format!(
        "{}.json",
        sha256_hex(format!("{cwd}|{version}|{model}"))
    ))
}

fn read_entry(path: &Path) -> Option<Entry> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The entry for this exact version, else the newest one an older version left behind.
fn lookup(dir: &Path, cwd: &str, version: Option<&str>, model: &str) -> Option<Entry> {
    if let Some(version) = version {
        if let Some(entry) = read_entry(&entry_file(dir, cwd, version, model)) {
            if entry.cwd == cwd && entry.claude_code_version == version {
                return Some(entry);
            }
        }
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| read_entry(&e.path()))
        .filter(|e| e.cwd == cwd && e.requested_model == model && e.variant == AS_IS)
        .max_by(|a, b| a.measured_at.cmp(&b.measured_at))
}

fn store(dir: &Path, entry: &Entry) -> Result<(), MeasureError> {
    for old in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = old.path();
        if let Some(prior) = read_entry(&path) {
            let same = prior.cwd == entry.cwd && prior.requested_model == entry.requested_model;
            if same && prior.claude_code_version != entry.claude_code_version {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let path = entry_file(
        dir,
        &entry.cwd,
        &entry.claude_code_version,
        &entry.requested_model,
    );
    let text =
        serde_json::to_string_pretty(entry).map_err(|e| failed("cache_failed", e.to_string()))?;
    bundle_io::write_atomic(&path, text.as_bytes())
        .map_err(|e| failed("cache_failed", format!("{}: {e}", path.display())))
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub force: bool,
    pub confirm: bool,
}

pub struct Env<'a> {
    pub data_dir: &'a Path,
    pub launcher: &'a dyn Launcher,
    pub rows: &'a WhatLoads,
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64 * 1000.0).round() / 10.0
    }
}

fn report(entry: &Entry, rows: &WhatLoads, cached: bool, stale: bool) -> Value {
    let run = &entry.run;
    let window = window_for_model(&run.model);
    let itemised = rows.total_tokens;
    let rest = run.total.saturating_sub(itemised);
    let mut notes = Vec::new();
    if itemised > run.total {
        notes.push("the estimate of the instruction rows is above the measured total; the estimate runs high here".to_string());
    }
    if stale {
        notes.push("measured with another Claude Code version or other settings; measure again for a current number".into());
    }
    json!({
        "ran": true,
        "cwd": entry.cwd,
        "claudeCodeVersion": entry.claude_code_version,
        "model": run.model,
        "requestedModel": entry.requested_model,
        "measuredAt": entry.measured_at,
        "cached": cached,
        "stale": stale,
        "basis": "measurement",
        "total": run.total,
        "parts": run.parts,
        "contextWindow": window,
        "windowPercent": percent(run.total, window),
        "skills": run.skills.len(),
        "agents": run.agents.len(),
        "plugins": run.plugins,
        "mcpServers": run.mcp_servers,
        "itemised": { "tokens": itemised, "basis": "estimate", "rows": rows.items.iter().filter(|i| i.loaded).count() },
        "notItemised": {
            "tokens": rest,
            "percent": percent(rest, run.total),
            "includes": "Claude Code's own prompt and built-in tools, tool definitions, the skill list, plugin parts and the error of the estimate",
        },
        "notes": notes,
    })
}

pub fn measure(env: &Env, req: &Request) -> Result<Value, MeasureError> {
    if let Some(problem) = fsx::folder_problem(&req.cwd) {
        return Err(failed("usage", problem));
    }
    let cwd = fsx::display(&fsx::canonical(&req.cwd));
    let model = req
        .model
        .clone()
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let dir = cache_dir(env.data_dir);
    let stamp = settings_stamp(Path::new(&cwd));
    let version = env.launcher.version().ok();
    if !req.force {
        if let Some(entry) = lookup(&dir, &cwd, version.as_deref(), &model) {
            let stale = version.as_deref() != Some(entry.claude_code_version.as_str())
                || entry.settings_stamp != stamp;
            return Ok(report(&entry, env.rows, true, stale));
        }
    }
    if !req.confirm {
        return Ok(json!({
            "ran": false,
            "cwd": cwd,
            "plan": {
                "requests": 1,
                "model": model,
                "cost": "about as many input tokens as the folder's stack, on the chosen model, roughly a minute",
                "sends": "the instruction files go to the model like in any session",
                "writes": "nothing in the folder; the answer is cached",
            },
            "start": "run again with --yes (start: true in the tool) to start the request",
        }));
    }
    let version = version.ok_or_else(|| {
        failed(
            "claude_not_found",
            "claude --version failed; is Claude Code installed?",
        )
    })?;
    let started = Instant::now();
    let out = env
        .launcher
        .request(Path::new(&cwd), &model)
        .map_err(|e| failed("request_failed", e))?;
    if out.code != 0 && out.stdout.trim().is_empty() {
        let line = out
            .stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("claude failed");
        return Err(failed("request_failed", line.trim().to_string()));
    }
    let run = parse_stream(&out.stdout, started.elapsed().as_millis() as u64)
        .map_err(|e| failed("request_failed", e))?;
    let entry = Entry {
        cwd,
        claude_code_version: version,
        requested_model: model,
        variant: AS_IS.into(),
        measured_at: fsx::now_zulu(),
        settings_stamp: stamp,
        run,
    };
    store(&dir, &entry)?;
    Ok(report(&entry, env.rows, false, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::loads::{LoadItem, WhatLoads};
    use std::cell::Cell;

    struct Stub {
        version: &'static str,
        requests: Cell<u32>,
    }

    impl Launcher for Stub {
        fn version(&self) -> Result<String, String> {
            Ok(self.version.to_string())
        }
        fn request(&self, _cwd: &Path, _model: &str) -> Result<Output, String> {
            self.requests.set(self.requests.get() + 1);
            Ok(Output {
                code: 0,
                stdout: [
                    r#"{"type":"system","subtype":"init","model":"claude-haiku","skills":["a","b"],"agents":["x"],"plugins":[{"name":"p@m"}],"mcp_servers":[{"name":"toolport"}]}"#,
                    r#"{"type":"result","subtype":"success","duration_ms":1200,"usage":{"input_tokens":10,"cache_creation_input_tokens":4000,"cache_read_input_tokens":6000}}"#,
                ]
                .join("\n"),
                stderr: String::new(),
            })
        }
    }

    fn rows(tokens: u64) -> WhatLoads {
        WhatLoads {
            items: vec![LoadItem {
                loaded: true,
                tokens,
                ..Default::default()
            }],
            total_tokens: tokens,
        }
    }

    fn request(cwd: &Path, confirm: bool) -> Request {
        Request {
            cwd: cwd.to_path_buf(),
            confirm,
            ..Default::default()
        }
    }

    #[test]
    fn nothing_is_requested_without_a_yes_and_a_cached_answer_costs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let cwd = dir.path().join("proj");
        std::fs::create_dir_all(&cwd).unwrap();
        let stub = Stub {
            version: "2.1.0",
            requests: Cell::new(0),
        };
        let rows = rows(2000);
        let env = Env {
            data_dir: &data,
            launcher: &stub,
            rows: &rows,
        };

        let plan = measure(&env, &request(&cwd, false)).unwrap();
        assert_eq!(plan["ran"], false);
        assert_eq!(stub.requests.get(), 0);

        let measured = measure(&env, &request(&cwd, true)).unwrap();
        assert_eq!(measured["total"], 10_010);
        assert_eq!(measured["cached"], false);
        assert_eq!(measured["notItemised"]["tokens"], 8_010);
        assert_eq!(measured["itemised"]["basis"], "estimate");
        assert_eq!(stub.requests.get(), 1);

        let again = measure(&env, &request(&cwd, false)).unwrap();
        assert_eq!(again["cached"], true);
        assert_eq!(again["stale"], false);
        assert_eq!(stub.requests.get(), 1);
    }

    #[test]
    fn a_new_claude_code_version_or_a_changed_settings_file_serves_the_number_stale() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let cwd = dir.path().join("proj");
        std::fs::create_dir_all(cwd.join(".claude")).unwrap();
        let rows = rows(0);
        let first = Stub {
            version: "2.1.0",
            requests: Cell::new(0),
        };
        measure(
            &Env {
                data_dir: &data,
                launcher: &first,
                rows: &rows,
            },
            &request(&cwd, true),
        )
        .unwrap();

        std::fs::write(cwd.join(".claude/settings.local.json"), "{}").unwrap();
        let same_version = measure(
            &Env {
                data_dir: &data,
                launcher: &first,
                rows: &rows,
            },
            &request(&cwd, false),
        )
        .unwrap();
        assert_eq!(
            (
                same_version["cached"].as_bool(),
                same_version["stale"].as_bool()
            ),
            (Some(true), Some(true))
        );

        let newer = Stub {
            version: "2.2.0",
            requests: Cell::new(0),
        };
        let served = measure(
            &Env {
                data_dir: &data,
                launcher: &newer,
                rows: &rows,
            },
            &request(&cwd, false),
        )
        .unwrap();
        assert_eq!(served["stale"], true);
        assert_eq!(served["claudeCodeVersion"], "2.1.0");
        assert_eq!(newer.requests.get(), 0);
    }

    #[test]
    fn an_estimate_above_the_measured_total_is_said_not_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("proj");
        std::fs::create_dir_all(&cwd).unwrap();
        let stub = Stub {
            version: "2.1.0",
            requests: Cell::new(0),
        };
        let rows = rows(50_000);
        let env = Env {
            data_dir: &dir.path().join("data"),
            launcher: &stub,
            rows: &rows,
        };
        let out = measure(&env, &request(&cwd, true)).unwrap();
        assert_eq!(out["notItemised"]["tokens"], 0);
        assert!(out["notes"][0].as_str().unwrap().contains("runs high"));
    }

    #[test]
    fn a_failed_request_names_the_cause_and_leaves_no_cache() {
        let result = parse_stream(
            r#"{"type":"result","is_error":true,"result":"Not logged in\nmore"}"#,
            5,
        );
        assert_eq!(result.unwrap_err(), "Not logged in");
        assert!(parse_stream("", 5).is_err());
    }
}
