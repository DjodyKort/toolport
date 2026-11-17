//! `context measure`: the real size of what a folder loads, read from Claude Code itself. One
//! minimal headless request in the folder; its first request's input (`input + cache_creation +
//! cache_read`) is the number. The instruction rows of `context loads` are estimates, so the part
//! of the measured total they do not cover is reported as "not itemised" instead of being
//! hidden: Claude Code's own prompt, tool definitions, the skill list, plugin parts and the error
//! of the estimate. A request costs model tokens, so it starts only with an explicit yes; an
//! answer from the cache costs nothing.
//!
//! One request per variant: as is, `--without plugin:<id>`, `--without skill:<name>` and
//! `--bundle <name>`. The variants reach Claude Code through a temporary `--settings` file, so
//! nothing is written in the folder. The as-is request also carries a throwaway
//! `InstructionsLoaded` hook; the files Claude Code reports verify the rows of `context loads`.
//! The estimate at the time of an as-is run is stored with it, which gives the folder's
//! calibration factor (measured / estimated).
//!
//! Cache: `<data dir>/plus/cache/measure/<sha256(cwd|version|model|variant)>.json`. An entry is stale when
//! the Claude Code version or a settings file of the folder changed; a stale entry is still served,
//! flagged, until a fresh run is asked for.

use super::bundle_apply::{self, Desired};
use super::bundle_store;
use super::fsx;
use super::globs::glob_match;
use super::hash::sha256_hex;
use super::loads::{LoadSource, WhatLoads};
use super::roots::Roots;
use super::verify::{self, Logged, Measured};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    estimated_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instructions: Option<Vec<Logged>>,
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
    fn request(&self, cwd: &Path, model: &str, settings: Option<&Path>) -> Result<Output, String>;
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

    fn request(&self, cwd: &Path, model: &str, settings: Option<&Path>) -> Result<Output, String> {
        let mut command = Command::new(&self.bin);
        command.args([
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
        ]);
        if let Some(file) = settings {
            command.arg("--settings").arg(file);
        }
        command.current_dir(cwd);
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

fn entry_file(dir: &Path, cwd: &str, version: &str, model: &str, variant: &str) -> PathBuf {
    dir.join(format!(
        "{}.json",
        sha256_hex(format!("{cwd}|{version}|{model}|{variant}"))
    ))
}

fn read_entry(path: &Path) -> Option<Entry> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn entries(dir: &Path) -> Vec<(PathBuf, Entry)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| read_entry(&p).map(|e| (p, e)))
        .collect()
}

/// The entry for this exact version, else the newest one an older version left behind.
fn lookup(
    dir: &Path,
    cwd: &str,
    version: Option<&str>,
    model: &str,
    variant: &str,
) -> Option<Entry> {
    let same = |e: &Entry| e.cwd == cwd && e.requested_model == model && e.variant == variant;
    if let Some(version) = version {
        if let Some(entry) = read_entry(&entry_file(dir, cwd, version, model, variant)) {
            if same(&entry) && entry.claude_code_version == version {
                return Some(entry);
            }
        }
    }
    entries(dir)
        .into_iter()
        .map(|(_, e)| e)
        .filter(same)
        .max_by(|a, b| a.measured_at.cmp(&b.measured_at))
}

fn store(dir: &Path, entry: &Entry) -> Result<(), MeasureError> {
    for (path, prior) in entries(dir) {
        let same = prior.cwd == entry.cwd
            && prior.requested_model == entry.requested_model
            && prior.variant == entry.variant;
        if same && prior.claude_code_version != entry.claude_code_version {
            let _ = std::fs::remove_file(path);
        }
    }
    let path = entry_file(
        dir,
        &entry.cwd,
        &entry.claude_code_version,
        &entry.requested_model,
        &entry.variant,
    );
    let text =
        serde_json::to_string_pretty(entry).map_err(|e| failed("cache_failed", e.to_string()))?;
    crate::fsx::atomic_write(&path, &text, crate::fsx::Mode::KeepOrDefault)
        .map_err(|e| failed("cache_failed", format!("{}: {e}", path.display())))
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub force: bool,
    pub confirm: bool,
    /// `plugin:<id>` or `skill:<name>` (a skill glob is allowed).
    pub without: Vec<String>,
    pub bundle: Option<String>,
}

pub struct Env<'a> {
    pub roots: &'a Roots,
    pub data_dir: &'a Path,
    pub launcher: &'a dyn Launcher,
    pub rows: &'a WhatLoads,
}

enum Kind {
    AsIs,
    Plugin(String),
    Skill(String),
    Bundle(Box<Desired>),
}

struct Variant {
    label: String,
    key: String,
    kind: Kind,
}

struct Slot {
    variant: Variant,
    found: Option<Entry>,
    fresh: bool,
}

struct VisibleSkills(Vec<String>);

impl LoadSource for VisibleSkills {
    fn what_loads(&self, _cwd: &Path) -> Result<WhatLoads, String> {
        Ok(WhatLoads::default())
    }

    fn visible_skills(&self, _cwd: &Path) -> Vec<String> {
        self.0.clone()
    }
}

fn variants(
    env: &Env,
    req: &Request,
    cwd: &Path,
    visible: &[String],
) -> Result<Vec<Variant>, MeasureError> {
    let mut out = vec![Variant {
        label: "as is".into(),
        key: AS_IS.into(),
        kind: Kind::AsIs,
    }];
    for spec in &req.without {
        let kind = match spec.split_once(':') {
            Some(("plugin", id)) if !id.is_empty() => Kind::Plugin(id.to_string()),
            Some(("skill", name)) if !name.is_empty() => Kind::Skill(name.to_string()),
            _ => {
                return Err(failed(
                    "usage",
                    format!("--without takes plugin:<id> or skill:<name>, not {spec}"),
                ))
            }
        };
        out.push(Variant {
            label: format!("without {spec}"),
            key: format!("without:{spec}"),
            kind,
        });
    }
    if let Some(name) = &req.bundle {
        let loaded = bundle_store::load(env.roots, name).map_err(|e| failed("usage", e.message))?;
        let source = VisibleSkills(visible.to_vec());
        let world = bundle_apply::World {
            roots: env.roots,
            data_dir: env.data_dir,
            loads: &source,
        };
        out.push(Variant {
            label: format!("bundle {name}"),
            key: format!("bundle:{name}:{}", &sha256_hex(&loaded.text)[..12]),
            kind: Kind::Bundle(Box::new(bundle_apply::desired(&world, &loaded.bundle, cwd))),
        });
    }
    Ok(out)
}

fn expand_skills(spec: &str, known: &[String]) -> Vec<String> {
    if spec.contains(['*', '?', '[', '{']) {
        known
            .iter()
            .filter(|n| !n.contains(':') && glob_match(spec, n))
            .cloned()
            .collect()
    } else {
        vec![spec.to_string()]
    }
}

fn skill_overrides(names: &[String]) -> Value {
    Value::Object(
        names
            .iter()
            .map(|n| (n.clone(), Value::String("off".into())))
            .collect::<Map<_, _>>(),
    )
}

/// A bundle as the `--settings` keys it stands for. Layers and extra folders have no settings key
/// (they ride `CLAUDE.local.md` and `--add-dir`), so they are said and left out.
fn bundle_settings(desired: &Desired, notes: &mut Vec<String>) -> Value {
    let mut settings = Map::new();
    if !desired.overrides.is_empty() {
        settings.insert(
            "skillOverrides".into(),
            Value::Object(
                desired
                    .overrides
                    .iter()
                    .map(|(n, m)| (n.clone(), json!(m)))
                    .collect(),
            ),
        );
    }
    if !desired.plugins.is_empty() {
        settings.insert(
            "enabledPlugins".into(),
            Value::Object(
                desired
                    .plugins
                    .iter()
                    .map(|id| (id.clone(), Value::Bool(false)))
                    .collect(),
            ),
        );
    }
    if !desired.excludes.is_empty() {
        settings.insert("claudeMdExcludes".into(), json!(desired.excludes));
    }
    if desired.memory_off {
        settings.insert("autoMemoryEnabled".into(), Value::Bool(false));
    }
    if !desired.env.is_empty() {
        settings.insert(
            "env".into(),
            Value::Object(
                desired
                    .env
                    .iter()
                    .map(|(n, v)| (n.clone(), json!(v)))
                    .collect(),
            ),
        );
    }
    if !desired.deny_servers.is_empty() {
        settings.insert(
            "deniedMcpServers".into(),
            Value::Array(
                desired
                    .deny_servers
                    .iter()
                    .map(|name| json!({ "serverName": name }))
                    .collect(),
            ),
        );
    }
    if !desired.denies.is_empty() {
        settings.insert("permissions".into(), json!({ "deny": desired.denies }));
    }
    if !desired.dirs.is_empty() {
        notes.push(format!(
            "dirs.add ({}) is passed to Claude Code as --add-dir by `context bundle launch`, not through settings, so it is not part of the measurement",
            desired.dirs.join(", ")
        ));
    }
    if !desired.layers.is_empty() {
        let names: Vec<&str> = desired.layers.iter().map(|(n, _)| n.as_str()).collect();
        notes.push(format!(
            "layers.add ({}) is delivered through CLAUDE.local.md, not settings, so it is not part of the measurement",
            names.join(", ")
        ));
    }
    notes.extend(desired.warnings.iter().cloned());
    Value::Object(settings)
}

static TEMP_SEQ: AtomicUsize = AtomicUsize::new(0);

struct TempFile(PathBuf);

impl TempFile {
    fn new(stem: &str, ext: &str) -> Self {
        Self(std::env::temp_dir().join(format!(
            "toolport-{stem}-{}-{}.{ext}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn write(stem: &str, value: &Value) -> Result<Self, MeasureError> {
        let file = Self::new(stem, "json");
        std::fs::write(&file.0, value.to_string())
            .map_err(|e| failed("temp_settings", format!("{}: {e}", file.0.display())))?;
        Ok(file)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Ctx<'a> {
    cwd: &'a Path,
    model: &'a str,
    version: &'a str,
    stamp: &'a str,
}

fn run_variant(
    env: &Env,
    ctx: &Ctx,
    variant: &Variant,
    settings: Option<&Value>,
) -> Result<Entry, MeasureError> {
    let as_is = variant.key == AS_IS;
    let log = as_is.then(|| TempFile::new("instructions", "log"));
    let hook = log.as_ref().map(|l| verify::hook_settings(&l.0));
    let temp = settings
        .or(hook.as_ref())
        .map(|value| TempFile::write("measure", value))
        .transpose()?;
    let started = Instant::now();
    let out = env
        .launcher
        .request(ctx.cwd, ctx.model, temp.as_ref().map(|t| t.0.as_path()))
        .map_err(|e| failed("request_failed", e))?;
    if out.code != 0 && out.stdout.trim().is_empty() {
        let line = out
            .stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("claude failed");
        return Err(failed(
            "request_failed",
            format!("{}: {}", variant.label, line.trim()),
        ));
    }
    let run = parse_stream(&out.stdout, started.elapsed().as_millis() as u64)
        .map_err(|e| failed("request_failed", format!("{}: {e}", variant.label)))?;
    Ok(Entry {
        cwd: fsx::display(ctx.cwd),
        claude_code_version: ctx.version.to_string(),
        requested_model: ctx.model.to_string(),
        variant: variant.key.clone(),
        measured_at: fsx::now_zulu(),
        settings_stamp: ctx.stamp.to_string(),
        run,
        estimated_tokens: as_is.then_some(env.rows.total_tokens),
        instructions: log.map(|l| {
            std::fs::read_to_string(&l.0)
                .map(|text| verify::parse_log(&text))
                .unwrap_or_default()
        }),
    })
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64 / whole as f64 * 1000.0).round() / 10.0
    }
}

fn signed_percent(delta: i64, base: u64) -> f64 {
    if base == 0 {
        0.0
    } else {
        (delta as f64 / base as f64 * 1000.0).round() / 10.0
    }
}

/// What the stored as-is run says about a folder's estimate: the factor that turns the estimated
/// instruction tokens into the measured total, as it stood when the run was made.
#[derive(Clone, Debug, PartialEq)]
pub struct Calibration {
    pub factor: f64,
    pub measured: u64,
    pub estimated: u64,
    pub measured_at: String,
    pub stale: bool,
}

pub fn calibration_value(calibration: Option<&Calibration>, estimate_now: u64) -> Value {
    match calibration {
        None => Value::Null,
        Some(c) => json!({
            "factor": c.factor,
            "measured": c.measured,
            "estimated": c.estimated,
            "measuredAt": c.measured_at,
            "stale": c.stale,
            "calibratedTotal": (estimate_now as f64 * c.factor).round() as u64,
            "meaning": "the last measured total divided by the estimate of the instruction rows at that time; the rows themselves stay estimates",
        }),
    }
}

fn newest_as_is(data_dir: &Path, cwd: &Path) -> Option<Entry> {
    let key = fsx::display(&fsx::canonical(cwd));
    entries(&cache_dir(data_dir))
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| e.cwd == key && e.variant == AS_IS)
        .max_by(|a, b| a.measured_at.cmp(&b.measured_at))
}

fn is_stale(entry: &Entry, cwd: &Path, version: Option<&str>) -> bool {
    entry.settings_stamp != settings_stamp(&fsx::canonical(cwd))
        || version.is_some_and(|v| v != entry.claude_code_version)
}

pub fn calibration(data_dir: &Path, cwd: &Path, version: Option<&str>) -> Option<Calibration> {
    let entry = newest_as_is(data_dir, cwd)?;
    let estimated = entry.estimated_tokens.filter(|e| *e > 0)?;
    Some(Calibration {
        factor: (entry.run.total as f64 / estimated as f64 * 1000.0).round() / 1000.0,
        measured: entry.run.total,
        estimated,
        measured_at: entry.measured_at.clone(),
        stale: is_stale(&entry, cwd, version),
    })
}

/// The newest stored as-is run of a folder, for `context loads` and `context status`.
pub struct Stored {
    pub entry_total: u64,
    pub window_percent: f64,
    pub model: String,
    pub claude_code_version: String,
    pub measured_at: String,
    pub stale: bool,
    instructions: Option<Vec<Logged>>,
}

impl Stored {
    pub fn summary(&self) -> Value {
        json!({
            "total": self.entry_total,
            "windowPercent": self.window_percent,
            "model": self.model,
            "claudeCodeVersion": self.claude_code_version,
            "measuredAt": self.measured_at,
            "stale": self.stale,
        })
    }

    /// Mark the rows the `InstructionsLoaded` log covers; `None` when the run holds no log.
    pub fn verify(&self, items: &mut [super::loads::LoadItem]) -> Option<verify::Verified> {
        verify::compare(
            items,
            &Measured {
                instructions: self.instructions.as_deref(),
                measured_at: &self.measured_at,
                claude_code_version: &self.claude_code_version,
                stale: self.stale,
            },
        )
    }
}

pub fn stored(data_dir: &Path, cwd: &Path, version: Option<&str>) -> Option<Stored> {
    let entry = newest_as_is(data_dir, cwd)?;
    let stale = is_stale(&entry, cwd, version);
    let window = window_for_model(&entry.requested_model).max(window_for_model(&entry.run.model));
    Some(Stored {
        entry_total: entry.run.total,
        window_percent: percent(entry.run.total, window),
        model: entry.run.model.clone(),
        claude_code_version: entry.claude_code_version,
        measured_at: entry.measured_at,
        stale,
        instructions: entry.instructions,
    })
}

fn report(
    entry: &Entry,
    rows: &WhatLoads,
    cached: bool,
    stale: bool,
    extra: (Vec<Value>, Vec<String>),
    verified: Option<verify::Verified>,
    calibration: Option<&Calibration>,
) -> Value {
    let run = &entry.run;
    let window = window_for_model(&run.model);
    let itemised = rows.total_tokens;
    let rest = run.total.saturating_sub(itemised);
    let (variants, mut notes) = extra;
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
        "variants": variants,
        "verified": verified,
        "calibration": calibration_value(calibration, itemised),
        "notes": notes,
    })
}

pub fn measure(env: &Env, req: &Request) -> Result<Value, MeasureError> {
    if let Some(problem) = fsx::folder_problem(&req.cwd) {
        return Err(failed("usage", problem));
    }
    let cwd = fsx::canonical(&req.cwd);
    let key = fsx::display(&cwd);
    let model = req
        .model
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let dir = cache_dir(env.data_dir);
    let stamp = settings_stamp(&cwd);
    let version = env.launcher.version().ok();
    let known_skills: Vec<String> = newest_as_is(env.data_dir, &cwd)
        .map(|e| e.run.skills)
        .unwrap_or_default();
    let mut slots: Vec<Slot> = variants(env, req, &cwd, &known_skills)?
        .into_iter()
        .map(|variant| {
            let found = if req.force {
                None
            } else {
                lookup(&dir, &key, version.as_deref(), &model, &variant.key)
                    .filter(|e| variant.key != AS_IS || e.instructions.is_some())
            };
            Slot {
                variant,
                found,
                fresh: false,
            }
        })
        .collect();
    let needed = slots.iter().filter(|s| s.found.is_none()).count();
    if needed > 0 && !req.confirm {
        let labels: Vec<&str> = slots
            .iter()
            .filter(|s| s.found.is_none())
            .map(|s| s.variant.label.as_str())
            .collect();
        return Ok(json!({
            "ran": false,
            "cwd": key,
            "plan": {
                "requests": needed,
                "variants": labels,
                "model": model,
                "cost": "about as many input tokens as the folder's stack, on the chosen model, once per request, roughly a minute each",
                "sends": "the instruction files go to the model like in any session",
                "writes": "nothing in the folder; the answers are cached",
            },
            "start": "run again with --yes (start: true in the tool) to start the requests",
        }));
    }
    let mut notes = Vec::new();
    let mut skipped = BTreeSet::new();
    let mut made = 0;
    if needed > 0 {
        let version = version.as_deref().ok_or_else(|| {
            failed(
                "claude_not_found",
                "claude --version failed; is Claude Code installed?",
            )
        })?;
        let ctx = Ctx {
            cwd: &cwd,
            model: &model,
            version,
            stamp: &stamp,
        };
        for index in 0..slots.len() {
            if slots[index].found.is_some() {
                continue;
            }
            let visible: Vec<String> = slots[0]
                .found
                .as_ref()
                .map(|e| e.run.skills.clone())
                .unwrap_or_default();
            let settings = match &slots[index].variant.kind {
                Kind::AsIs => None,
                Kind::Plugin(id) => Some(json!({ "enabledPlugins": { id: false } })),
                Kind::Skill(spec) => {
                    let names = expand_skills(spec, &visible);
                    if names.is_empty() {
                        notes.push(format!(
                            "{}: no listed skill matches, nothing to measure",
                            slots[index].variant.label
                        ));
                        skipped.insert(index);
                        continue;
                    }
                    Some(json!({ "skillOverrides": skill_overrides(&names) }))
                }
                Kind::Bundle(desired) => Some(bundle_settings(desired, &mut notes)),
            };
            let entry = run_variant(env, &ctx, &slots[index].variant, settings.as_ref())?;
            store(&dir, &entry)?;
            slots[index].found = Some(entry);
            slots[index].fresh = true;
            made += 1;
        }
    }
    let served: Vec<&Slot> = slots
        .iter()
        .enumerate()
        .filter(|(i, s)| s.found.is_some() && !skipped.contains(i))
        .map(|(_, s)| s)
        .collect();
    let stale = served.iter().any(|s| {
        let e = s.found.as_ref().expect("served slots are found");
        e.settings_stamp != stamp
            || version
                .as_deref()
                .is_some_and(|v| v != e.claude_code_version)
    });
    let as_is = slots[0]
        .found
        .as_ref()
        .expect("the as-is run is measured first");
    let base = as_is.run.total;
    let variants: Vec<Value> = served
        .iter()
        .skip(1)
        .map(|s| {
            let e = s.found.as_ref().expect("served slots are found");
            let delta = e.run.total as i64 - base as i64;
            json!({
                "label": s.variant.label,
                "total": e.run.total,
                "deltaTokens": delta,
                "deltaPercent": signed_percent(delta, base),
                "cached": !s.fresh,
            })
        })
        .collect();
    let mut rows = env.rows.clone();
    let verified = verify::compare(
        &mut rows.items,
        &Measured {
            instructions: as_is.instructions.as_deref(),
            measured_at: &as_is.measured_at,
            claude_code_version: &as_is.claude_code_version,
            stale,
        },
    );
    let calibration = calibration(env.data_dir, &cwd, version.as_deref());
    Ok(report(
        as_is,
        &rows,
        made == 0,
        stale,
        (variants, notes),
        verified,
        calibration.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::loads::LoadItem;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct Stub {
        version: &'static str,
        requests: Cell<u32>,
        settings_seen: RefCell<Vec<String>>,
        loaded: Vec<String>,
    }

    impl Stub {
        fn new(version: &'static str) -> Self {
            Self {
                version,
                ..Self::default()
            }
        }
    }

    fn stream(total_cache_read: u64, skills: &[&str]) -> String {
        let names = serde_json::to_string(skills).unwrap();
        [
            format!(
                r#"{{"type":"system","subtype":"init","model":"claude-haiku","skills":{names},"agents":["x"],"plugins":[{{"name":"p@m"}}],"mcp_servers":[{{"name":"toolport"}}]}}"#
            ),
            format!(
                r#"{{"type":"result","subtype":"success","duration_ms":1200,"usage":{{"input_tokens":10,"cache_creation_input_tokens":4000,"cache_read_input_tokens":{total_cache_read}}}}}"#
            ),
        ]
        .join("\n")
    }

    impl Launcher for Stub {
        fn version(&self) -> Result<String, String> {
            Ok(self.version.to_string())
        }

        fn request(
            &self,
            _cwd: &Path,
            _model: &str,
            settings: Option<&Path>,
        ) -> Result<Output, String> {
            self.requests.set(self.requests.get() + 1);
            let text = settings
                .map(|p| std::fs::read_to_string(p).unwrap())
                .unwrap_or_default();
            self.settings_seen.borrow_mut().push(text.clone());
            let mut read = 6000;
            if text.contains("\"enabledPlugins\"") {
                read = 3000;
            } else if text.contains("\"skillOverrides\"") {
                read = 5500;
            } else if let Some(at) = text.find("cat >> '") {
                let log = text[at + 8..].split('\'').next().unwrap();
                let lines: String = self
                    .loaded
                    .iter()
                    .map(|p| {
                        format!(
                            "{{\"file_path\":\"{p}\",\"memory_type\":\"User\",\"load_reason\":\"session_start\"}}\n"
                        )
                    })
                    .collect();
                std::fs::write(log, lines).unwrap();
            }
            Ok(Output {
                code: 0,
                stdout: stream(read, &["alpha", "beta", "scratch-1"]),
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

    struct World {
        dir: tempfile::TempDir,
        roots: Roots,
        cwd: PathBuf,
    }

    impl World {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let base = fsx::canonical(dir.path());
            let cwd = base.join("proj");
            std::fs::create_dir_all(cwd.join(".claude")).unwrap();
            Self {
                roots: Roots::from_home(&base.join("home")),
                cwd,
                dir,
            }
        }

        fn data(&self) -> PathBuf {
            self.dir.path().join("data")
        }

        fn env<'a>(&'a self, stub: &'a Stub, rows: &'a WhatLoads, data: &'a Path) -> Env<'a> {
            Env {
                roots: &self.roots,
                data_dir: data,
                launcher: stub,
                rows,
            }
        }

        fn request(&self, confirm: bool) -> Request {
            Request {
                cwd: self.cwd.clone(),
                confirm,
                ..Default::default()
            }
        }
    }

    #[test]
    fn nothing_is_requested_without_a_yes_and_a_cached_answer_costs_nothing() {
        let w = World::new();
        let data = w.data();
        let stub = Stub::new("2.1.0");
        let rows = rows(2000);
        let env = w.env(&stub, &rows, &data);

        let plan = measure(&env, &w.request(false)).unwrap();
        assert_eq!(plan["ran"], false);
        assert_eq!(plan["plan"]["variants"], json!(["as is"]));
        assert_eq!(stub.requests.get(), 0);

        let measured = measure(&env, &w.request(true)).unwrap();
        assert_eq!(measured["total"], 10_010);
        assert_eq!(measured["cached"], false);
        assert_eq!(measured["notItemised"]["tokens"], 8_010);
        assert_eq!(measured["itemised"]["basis"], "estimate");
        assert_eq!(stub.requests.get(), 1);

        let again = measure(&env, &w.request(false)).unwrap();
        assert_eq!(again["cached"], true);
        assert_eq!(again["stale"], false);
        assert_eq!(stub.requests.get(), 1);
    }

    #[test]
    fn a_new_claude_code_version_or_a_changed_settings_file_serves_the_number_stale() {
        let w = World::new();
        let data = w.data();
        let rows = rows(0);
        let first = Stub::new("2.1.0");
        measure(&w.env(&first, &rows, &data), &w.request(true)).unwrap();

        std::fs::write(w.cwd.join(".claude/settings.local.json"), "{}").unwrap();
        let same_version = measure(&w.env(&first, &rows, &data), &w.request(false)).unwrap();
        assert_eq!(
            (
                same_version["cached"].as_bool(),
                same_version["stale"].as_bool()
            ),
            (Some(true), Some(true))
        );

        let newer = Stub::new("2.2.0");
        let served = measure(&w.env(&newer, &rows, &data), &w.request(false)).unwrap();
        assert_eq!(served["stale"], true);
        assert_eq!(served["claudeCodeVersion"], "2.1.0");
        assert_eq!(newer.requests.get(), 0);
    }

    #[test]
    fn an_estimate_above_the_measured_total_is_said_not_hidden() {
        let w = World::new();
        let data = w.data();
        let stub = Stub::new("2.1.0");
        let rows = rows(50_000);
        let out = measure(&w.env(&stub, &rows, &data), &w.request(true)).unwrap();
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

    #[test]
    fn a_plugin_and_a_skill_variant_each_cost_one_request_and_report_their_delta() {
        let w = World::new();
        let data = w.data();
        let stub = Stub::new("2.1.0");
        let rows = rows(2000);
        let env = w.env(&stub, &rows, &data);
        let mut request = w.request(false);
        request.without = vec!["plugin:kit@market".into(), "skill:scratch-*".into()];
        let plan = measure(&env, &request).unwrap();
        assert_eq!(plan["plan"]["requests"], 3);
        assert_eq!(stub.requests.get(), 0);

        request.confirm = true;
        let out = measure(&env, &request).unwrap();
        assert_eq!(stub.requests.get(), 3);
        assert_eq!(out["total"], 10_010);
        assert_eq!(out["variants"][0]["label"], "without plugin:kit@market");
        assert_eq!(out["variants"][0]["deltaTokens"], -3000);
        assert_eq!(out["variants"][0]["deltaPercent"], -30.0);
        assert_eq!(out["variants"][1]["deltaTokens"], -500);
        let seen = stub.settings_seen.borrow();
        assert!(seen[1].contains("\"kit@market\":false"), "{}", seen[1]);
        assert!(seen[2].contains("\"scratch-1\":\"off\""), "{}", seen[2]);
        assert!(!seen[2].contains("alpha"));
        drop(seen);

        let again = measure(&env, &request).unwrap();
        assert_eq!(
            stub.requests.get(),
            3,
            "every variant answers from the cache"
        );
        assert_eq!(again["variants"][0]["cached"], true);
        assert_eq!(again["cached"], true);
    }

    #[test]
    fn a_skill_that_is_not_listed_is_noted_and_costs_no_request() {
        let w = World::new();
        let data = w.data();
        let stub = Stub::new("2.1.0");
        let rows = rows(1);
        let mut request = w.request(true);
        request.without = vec!["skill:nothing-*".into()];
        let out = measure(&w.env(&stub, &rows, &data), &request).unwrap();
        assert_eq!(stub.requests.get(), 1);
        assert_eq!(out["variants"], json!([]));
        assert!(out["notes"][0]
            .as_str()
            .unwrap()
            .contains("no listed skill matches"));
    }

    #[test]
    fn a_bundle_variant_is_measured_through_settings_and_its_edge_cases_are_said() {
        let w = World::new();
        let data = w.data();
        let profiles = w.roots.skills_repo_path().join("profiles");
        std::fs::create_dir_all(&profiles).unwrap();
        std::fs::write(
            profiles.join("lean.yaml"),
            "format: 1\nname: lean\nplugins:\n  off: [\"kit@market\"]\nlayers:\n  add: [acme]\n",
        )
        .unwrap();
        let layer = w.roots.skills_repo_path().join("rules/acme/SKILL.md");
        std::fs::create_dir_all(layer.parent().unwrap()).unwrap();
        std::fs::write(
            layer,
            "---\nname: acme\ndescription: d\nactivation: always\n---\n\nRules.\n",
        )
        .unwrap();
        let stub = Stub::new("2.1.0");
        let rows = rows(2000);
        let mut request = w.request(true);
        request.bundle = Some("lean".into());
        let out = measure(&w.env(&stub, &rows, &data), &request).unwrap();
        assert_eq!(out["variants"][0]["label"], "bundle lean");
        assert_eq!(out["variants"][0]["deltaTokens"], -3000);
        let notes: Vec<&str> = out["notes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            notes.iter().any(|n| n.contains("layers.add (acme)")),
            "{notes:?}"
        );
        assert!(!w.cwd.join(".claude/settings.local.json").exists());

        request.bundle = Some("missing".into());
        let err = measure(&w.env(&stub, &rows, &data), &request).unwrap_err();
        assert_eq!(err.code, "usage");
        request.bundle = None;
        request.without = vec!["plugin".into()];
        let err = measure(&w.env(&stub, &rows, &data), &request).unwrap_err();
        assert!(err
            .message
            .contains("--without takes plugin:<id> or skill:<name>"));
    }

    #[test]
    fn the_instructions_log_marks_the_rows_loaded_absent_or_mismatch() {
        let w = World::new();
        let data = w.data();
        let (kept, left_out, wrong, extra) = (
            w.cwd.join("CLAUDE.md"),
            w.cwd.join("left.md"),
            w.cwd.join("wrong.md"),
            w.cwd.join("extra.md"),
        );
        for file in [&kept, &left_out, &wrong, &extra] {
            std::fs::write(file, "x").unwrap();
        }
        let item = |path: &Path, loaded: bool, reason: &str| LoadItem {
            kind: "memory",
            path: Some(fsx::display(path)),
            loaded,
            reason: reason.to_string(),
            tokens: 10,
            ..Default::default()
        };
        let rows = WhatLoads {
            items: vec![
                item(&kept, true, "loaded"),
                item(&left_out, false, "excluded by claudeMdExcludes"),
                item(&wrong, true, "loaded"),
            ],
            total_tokens: 30,
        };
        let mut stub = Stub::new("2.1.0");
        stub.loaded = vec![fsx::display(&kept), fsx::display(&extra)];
        let out = measure(&w.env(&stub, &rows, &data), &w.request(true)).unwrap();
        let verified = &out["verified"];
        assert_eq!(verified["status"], "mismatch");
        assert_eq!(
            (verified["loaded"].as_u64(), verified["absent"].as_u64()),
            (Some(1), Some(1))
        );
        assert_eq!(verified["mismatches"][0]["expected"], "loaded");
        assert_eq!(verified["mismatches"][0]["actual"], "absent");
        assert_eq!(verified["unknown"], json!([fsx::display(&extra)]));

        let stored = stored(&data, &w.cwd, Some("2.1.0")).unwrap();
        let mut items = rows.items.clone();
        let again = stored.verify(&mut items).unwrap();
        assert_eq!(again.status, "mismatch");
        assert_eq!(
            items.iter().map(|i| i.confirmed).collect::<Vec<_>>(),
            [Some("loaded"), Some("absent"), Some("mismatch")]
        );
    }

    #[test]
    fn the_calibration_factor_is_the_measured_total_over_the_estimate_at_measure_time() {
        let w = World::new();
        let data = w.data();
        assert!(calibration(&data, &w.cwd, None).is_none());
        let stub = Stub::new("2.1.0");
        let rows = rows(2000);
        let out = measure(&w.env(&stub, &rows, &data), &w.request(true)).unwrap();
        assert_eq!(out["calibration"]["factor"], 5.005);
        assert_eq!(out["calibration"]["estimated"], 2000);
        assert_eq!(out["calibration"]["calibratedTotal"], 10_010);

        let c = calibration(&data, &w.cwd, Some("2.1.0")).unwrap();
        assert_eq!((c.measured, c.estimated, c.stale), (10_010, 2000, false));
        assert!(calibration(&data, &w.cwd, Some("2.9.0")).unwrap().stale);
        let value = calibration_value(Some(&c), 1000);
        assert_eq!(value["calibratedTotal"], 5005);
    }
}
