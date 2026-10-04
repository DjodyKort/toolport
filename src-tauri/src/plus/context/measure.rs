//! `context measure` (D-065): the real size of what a folder loads, read from Claude Code itself.
//!
//! One minimal headless request per variant (as is, `--without plugin:<id>`, `--without
//! skill:<name>`, `--bundle <name>`) started in the folder; the variants reach Claude Code as a
//! temporary `--settings` file, so nothing is written in the folder. The first request's input
//! (`input + cache_creation + cache_read`) is the number; the `bytes / 4` estimate of
//! `context loads` stays an ordering aid. A request costs model tokens, so a caller must approve
//! before any request starts; answers read from the cache cost nothing.
//!
//! Cache: `<data dir>/plus/cache/measure/<sha256(cwd|version|model|variant)>.json`. An entry is
//! stale when the Claude Code version or a settings file of the folder changed; a stale entry is
//! still served (flagged) until the caller asks for a fresh run.

use super::config::ContextConfig;
use super::globs::glob_match;
use super::loads::{what_loads_with, LoadsOptions};
use super::loads_extra::git_root;
use super::roots::Roots;
use crate::plus::exec::{is_not_found, run_command, CmdOutput};
use crate::plus::hashing::sha256_hex;
use crate::plus::sources::fsx;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

pub const PROMPT: &str = "Reply with the single word ok";
pub const DEFAULT_MODEL: &str = "haiku";
const AS_IS: &str = "as-is";
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
const SETTINGS_CAP: usize = 1 << 20;
const WINDOW_DEFAULT: u64 = 200_000;
const WINDOW_LARGE: u64 = 1_000_000;

/// The context window a model name implies; the `[1m]` suffix selects the 1M-token window.
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
pub struct PluginRef {
    pub name: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRef {
    pub name: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasureRun {
    pub label: String,
    pub total: u64,
    pub parts: Parts,
    pub skills: usize,
    pub agents: usize,
    pub slash_commands: usize,
    pub plugins: Vec<PluginRef>,
    pub mcp_servers: Vec<ServerRef>,
    pub skill_names: Vec<String>,
    pub agent_names: Vec<String>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Invisible {
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Delta {
    pub label: String,
    pub tokens: i64,
    pub percent: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasureReport {
    pub cwd: String,
    pub claude_code_version: String,
    pub model: String,
    pub measured_at: String,
    pub cached: bool,
    pub stale: bool,
    pub runs: Vec<MeasureRun>,
    pub deltas: Vec<Delta>,
    pub visible_skills: Vec<String>,
    pub invisible_skills: Vec<Invisible>,
    pub notes: Vec<String>,
}

/// Where a cached as-is measurement came from; `context loads --measured` carries it next to the
/// run so a screen can say how old and how trustworthy the number is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredInfo {
    pub claude_code_version: String,
    pub model: String,
    pub measured_at: String,
    pub stale: bool,
    #[serde(skip)]
    pub context_window: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    cwd: String,
    claude_code_version: String,
    requested_model: String,
    model: String,
    variant: String,
    measured_at: String,
    settings_stamp: String,
    slash_command_names: Vec<String>,
    run: MeasureRun,
}

#[derive(Debug)]
pub enum MeasureError {
    Usage(String),
    Failed { code: &'static str, message: String },
    /// The caller did not approve the requests this call needs.
    Declined(usize),
}

fn failed(code: &'static str, message: impl Into<String>) -> MeasureError {
    MeasureError::Failed {
        code,
        message: message.into(),
    }
}

pub trait Launcher {
    fn version(&self) -> Result<String, String>;
    fn request(
        &self,
        cwd: &Path,
        model: &str,
        settings: Option<&Path>,
    ) -> Result<CmdOutput, String>;
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

    fn run(&self, cmd: Command, timeout: Duration) -> Result<CmdOutput, String> {
        run_command(cmd, timeout).map_err(|e| {
            if is_not_found(&e) {
                format!("claude not found ({})", self.bin)
            } else {
                e
            }
        })
    }
}

impl Launcher for ProcessLauncher {
    fn version(&self) -> Result<String, String> {
        let mut cmd = Command::new(&self.bin);
        cmd.arg("--version");
        let out = self.run(cmd, VERSION_TIMEOUT)?;
        let text = out.stdout.trim();
        let version = text.split_whitespace().next().unwrap_or("");
        if !out.ok() || version.is_empty() {
            return Err(format!(
                "claude --version failed: {}",
                out.first_error_line()
            ));
        }
        Ok(version.to_string())
    }

    fn request(
        &self,
        cwd: &Path,
        model: &str,
        settings: Option<&Path>,
    ) -> Result<CmdOutput, String> {
        let mut cmd = Command::new(&self.bin);
        cmd.args([
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
            cmd.arg("--settings").arg(file);
        }
        cmd.current_dir(cwd);
        self.run(cmd, REQUEST_TIMEOUT)
    }
}

pub struct Env<'a> {
    pub roots: &'a Roots,
    pub config: &'a ContextConfig,
    pub data_dir: Option<&'a Path>,
    pub launcher: &'a dyn Launcher,
}

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub cwd: PathBuf,
    pub without: Vec<String>,
    pub bundle: Option<String>,
    pub model: Option<String>,
    pub force: bool,
}

#[derive(Debug)]
pub(super) struct Parsed {
    pub(super) run: MeasureRun,
    pub(super) model: String,
    pub(super) slash_commands: Vec<String>,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn count(usage: &Value, key: &str) -> u64 {
    usage[key].as_u64().unwrap_or(0)
}

pub(super) fn parse_stream(label: &str, stdout: &str, wall_ms: u64) -> Result<Parsed, String> {
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
        return Err(text.lines().next().unwrap_or("the request failed").to_string());
    }
    let init = init.ok_or("claude printed no init event")?;
    let usage = &result["usage"];
    let parts = Parts {
        input: count(usage, "input_tokens"),
        cache_creation: count(usage, "cache_creation_input_tokens"),
        cache_read: count(usage, "cache_read_input_tokens"),
    };
    let skill_names = strings(&init["skills"]);
    let agent_names = strings(&init["agents"]);
    let slash_commands = strings(&init["slash_commands"]);
    let run = MeasureRun {
        label: label.to_string(),
        total: parts.input + parts.cache_creation + parts.cache_read,
        parts,
        skills: skill_names.len(),
        agents: agent_names.len(),
        slash_commands: slash_commands.len(),
        plugins: init["plugins"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| PluginRef {
                name: p["name"].as_str().unwrap_or("").to_string(),
                source: p["source"].as_str().unwrap_or("").to_string(),
            })
            .collect(),
        mcp_servers: init["mcp_servers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| ServerRef {
                name: s["name"].as_str().unwrap_or("").to_string(),
                status: s["status"].as_str().unwrap_or("").to_string(),
            })
            .collect(),
        skill_names,
        agent_names,
        duration_ms: result["duration_ms"].as_u64().unwrap_or(wall_ms),
    };
    Ok(Parsed {
        run,
        model: init["model"].as_str().unwrap_or("").to_string(),
        slash_commands,
    })
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SkillRules {
    off: Vec<String>,
    name_only: Vec<String>,
    allow: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SkillsDef {
    Allow(Vec<String>),
    Rules(SkillRules),
}

impl Default for SkillsDef {
    fn default() -> Self {
        SkillsDef::Rules(SkillRules::default())
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Switches {
    off: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Mcp {
    deny: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Layers {
    add: Vec<String>,
    exclude: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Bundle {
    skills: SkillsDef,
    plugins: Switches,
    mcp: Mcp,
    layers: Layers,
    agents: Switches,
}

fn bundle_path(roots: &Roots, name: &str) -> Result<PathBuf, MeasureError> {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with('.');
    if !plain {
        return Err(MeasureError::Usage(format!("not a bundle name: {name}")));
    }
    let path = roots
        .skills_repo_path()
        .join("profiles")
        .join(format!("{name}.yaml"));
    if fsx::is_file(&path) {
        Ok(path)
    } else {
        Err(MeasureError::Usage(format!(
            "unknown bundle: {name} (no {})",
            path.display()
        )))
    }
}

fn library_skills(roots: &Roots) -> BTreeSet<String> {
    let dir = roots.skills_repo_path().join("skills");
    fsx::list_dir(&dir)
        .into_iter()
        .filter(|e| e.kind == fsx::Kind::Dir)
        .map(|e| e.name)
        .collect()
}

fn expand(patterns: &[String], known: &BTreeSet<String>) -> Vec<String> {
    let mut out = BTreeSet::new();
    for pattern in patterns {
        if pattern.contains(['*', '?', '[', '{']) {
            out.extend(
                known
                    .iter()
                    .filter(|n| !n.contains(':') && glob_match(pattern, n))
                    .cloned(),
            );
        } else {
            out.insert(pattern.clone());
        }
    }
    out.into_iter().collect()
}

fn overrides(names: &[String], mode: &str) -> Map<String, Value> {
    names
        .iter()
        .map(|n| (n.clone(), Value::String(mode.to_string())))
        .collect()
}

/// A bundle as the `--settings` keys it stands for. `layers.add` has no settings key (it rides
/// `CLAUDE.local.md`), so it is reported and left out of the measurement.
fn bundle_settings(
    def: &Bundle,
    library: &BTreeSet<String>,
    visible: &BTreeSet<String>,
    notes: &mut Vec<String>,
) -> Value {
    let known: BTreeSet<String> = library.union(visible).cloned().collect();
    let mut skills = Map::new();
    match &def.skills {
        SkillsDef::Allow(allow) => {
            let keep: BTreeSet<String> = expand(allow, &known).into_iter().collect();
            let hide: Vec<String> = library.difference(&keep).cloned().collect();
            skills.extend(overrides(&hide, "off"));
        }
        SkillsDef::Rules(rules) => {
            if !rules.allow.is_empty() {
                let keep: BTreeSet<String> = expand(&rules.allow, &known).into_iter().collect();
                let hide: Vec<String> = library.difference(&keep).cloned().collect();
                skills.extend(overrides(&hide, "off"));
            }
            skills.extend(overrides(&expand(&rules.off, &known), "off"));
            skills.extend(overrides(&expand(&rules.name_only, &known), "name-only"));
        }
    }
    let mut settings = Map::new();
    if !skills.is_empty() {
        settings.insert("skillOverrides".into(), Value::Object(skills));
    }
    if !def.plugins.off.is_empty() {
        settings.insert(
            "enabledPlugins".into(),
            Value::Object(
                def.plugins
                    .off
                    .iter()
                    .map(|id| (id.clone(), Value::Bool(false)))
                    .collect(),
            ),
        );
    }
    if !def.layers.exclude.is_empty() {
        settings.insert("claudeMdExcludes".into(), json!(def.layers.exclude));
    }
    if !def.mcp.deny.is_empty() {
        settings.insert(
            "deniedMcpServers".into(),
            Value::Array(
                def.mcp
                    .deny
                    .iter()
                    .map(|name| json!({ "serverName": name }))
                    .collect(),
            ),
        );
    }
    if !def.agents.off.is_empty() {
        settings.insert(
            "permissions".into(),
            json!({ "deny": def.agents.off.iter().map(|a| format!("Agent({a})")).collect::<Vec<_>>() }),
        );
    }
    if !def.layers.add.is_empty() {
        notes.push(format!(
            "layers.add ({}) is delivered through CLAUDE.local.md, not settings, so it is not part of the measurement",
            def.layers.add.join(", ")
        ));
    }
    Value::Object(settings)
}

enum Kind {
    AsIs,
    Plugin(String),
    Skill(String),
    Bundle { def: Box<Bundle> },
}

struct Variant {
    label: String,
    key: String,
    kind: Kind,
}

impl Variant {
    fn as_is() -> Self {
        Self {
            label: "as is".into(),
            key: AS_IS.into(),
            kind: Kind::AsIs,
        }
    }
}

fn variants(env: &Env, req: &Request) -> Result<Vec<Variant>, MeasureError> {
    let mut out = vec![Variant::as_is()];
    for spec in &req.without {
        let kind = match spec.split_once(':') {
            Some(("plugin", id)) if !id.is_empty() => Kind::Plugin(id.to_string()),
            Some(("skill", name)) if !name.is_empty() => Kind::Skill(name.to_string()),
            _ => {
                return Err(MeasureError::Usage(format!(
                    "--without takes plugin:<id> or skill:<name>, not {spec}"
                )))
            }
        };
        out.push(Variant {
            label: format!("without {spec}"),
            key: format!("without:{spec}"),
            kind,
        });
    }
    if let Some(name) = &req.bundle {
        let path = bundle_path(env.roots, name)?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| failed("bundle_unreadable", format!("{}: {e}", path.display())))?;
        let def: Bundle = serde_yaml::from_str(&text)
            .map_err(|e| MeasureError::Usage(format!("bundle {name}: {e}")))?;
        out.push(Variant {
            label: format!("bundle {name}"),
            key: format!("bundle:{name}:{}", &sha256_hex(&text)[..12]),
            kind: Kind::Bundle { def: Box::new(def) },
        });
    }
    Ok(out)
}

fn settings_stamp(cwd: &Path) -> String {
    let mut dirs = vec![cwd.to_path_buf()];
    if let Some(root) = git_root(cwd).filter(|r| r != cwd) {
        dirs.push(root);
    }
    let mut seen = Vec::new();
    for dir in dirs {
        for file in ["settings.json", "settings.local.json"] {
            let path = dir.join(".claude").join(file);
            if let Some(bytes) = fsx::read_bytes(&path, SETTINGS_CAP) {
                seen.extend_from_slice(path.to_string_lossy().as_bytes());
                seen.push(0);
                seen.extend_from_slice(&bytes);
                seen.push(0);
            }
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
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| read_entry(&p).map(|e| (p, e)))
        .collect()
}

/// The entry for this exact version, else the newest one an older version left behind.
fn lookup(
    dir: &Path,
    cwd: &str,
    version: &str,
    model: &str,
    variant: &str,
) -> Option<Entry> {
    let same = |e: &Entry| e.cwd == cwd && e.requested_model == model && e.variant == variant;
    if let Some(e) = read_entry(&entry_file(dir, cwd, version, model, variant)) {
        if same(&e) && e.claude_code_version == version {
            return Some(e);
        }
    }
    entries(dir)
        .into_iter()
        .map(|(_, e)| e)
        .filter(same)
        .max_by(|a, b| a.measured_at.cmp(&b.measured_at))
}

fn store(dir: &Path, entry: &Entry) {
    for (path, old) in entries(dir) {
        let same = old.cwd == entry.cwd
            && old.requested_model == entry.requested_model
            && old.variant == entry.variant;
        if same && old.claude_code_version != entry.claude_code_version {
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
    if let Ok(text) = serde_json::to_string_pretty(entry) {
        let _ = crate::registry::atomic_write(&path, &text);
    }
}

static TEMP_SEQ: AtomicUsize = AtomicUsize::new(0);

struct TempSettings(PathBuf);

impl TempSettings {
    fn write(value: &Value) -> Result<Self, MeasureError> {
        let path = std::env::temp_dir().join(format!(
            "toolport-measure-{}-{}.json",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, value.to_string())
            .map_err(|e| failed("temp_settings", format!("{}: {e}", path.display())))?;
        Ok(Self(path))
    }
}

impl Drop for TempSettings {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Slot {
    variant: Variant,
    found: Option<Entry>,
}

fn run_variant(
    env: &Env,
    cwd: &Path,
    model: &str,
    version: &str,
    stamp: &str,
    variant: &Variant,
    settings: Option<&Value>,
) -> Result<Entry, MeasureError> {
    let temp = settings.map(TempSettings::write).transpose()?;
    let started = Instant::now();
    let out = env
        .launcher
        .request(cwd, model, temp.as_ref().map(|t| t.0.as_path()))
        .map_err(|e| failed("claude_failed", e))?;
    let wall = started.elapsed().as_millis() as u64;
    let parsed = parse_stream(&variant.label, &out.stdout, wall).map_err(|why| {
        let detail = if out.ok() {
            why
        } else {
            format!("{why} (exit {}: {})", out.code, out.first_error_line())
        };
        failed("claude_failed", format!("{}: {detail}", variant.label))
    })?;
    Ok(Entry {
        cwd: fsx::display(cwd),
        claude_code_version: version.to_string(),
        requested_model: model.to_string(),
        model: parsed.model,
        variant: variant.key.clone(),
        measured_at: fsx::now_zulu(),
        settings_stamp: stamp.to_string(),
        slash_command_names: parsed.slash_commands,
        run: parsed.run,
    })
}

fn percent(delta: i64, base: u64) -> f64 {
    if base == 0 {
        return 0.0;
    }
    (delta as f64 / base as f64 * 1000.0).round() / 10.0
}

fn invisible_skills(
    env: &Env,
    cwd: &Path,
    listed: &[String],
    slash_commands: &[String],
) -> Vec<Invisible> {
    let options = LoadsOptions {
        no_lazy: true,
        ..Default::default()
    };
    let Ok(report) = what_loads_with(env.roots, env.config, None, cwd, &options) else {
        return Vec::new();
    };
    let listed: BTreeSet<&str> = listed.iter().map(String::as_str).collect();
    let commands: BTreeSet<&str> = slash_commands.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in report.items.iter().filter(|i| i.kind == "skill") {
        if listed.contains(item.name.as_str())
            || !commands.contains(item.name.as_str())
            || !seen.insert(item.name.clone())
        {
            continue;
        }
        let reason = if item.visible == Some(false) {
            item.reason.clone()
        } else {
            "listed as a command but not offered to the model".to_string()
        };
        out.push(Invisible {
            name: item.name.clone(),
            reason,
        });
    }
    out
}

/// Measures `req.cwd` as is and once per variant. Requests are only made for what the cache cannot
/// answer (everything with `force`); `approve` gets the number of requests first and returns
/// whether they may start.
pub fn measure(
    env: &Env,
    req: &Request,
    approve: &mut dyn FnMut(usize) -> bool,
) -> Result<MeasureReport, MeasureError> {
    if !req.cwd.is_dir() {
        return Err(MeasureError::Usage(format!(
            "--cwd is not a folder: {}",
            req.cwd.display()
        )));
    }
    let cwd = fsx::canonical(&req.cwd);
    let model = req
        .model
        .clone()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let variants = variants(env, req)?;
    let version = env
        .launcher
        .version()
        .map_err(|e| failed("claude_unavailable", e))?;
    let stamp = settings_stamp(&cwd);
    let key = fsx::display(&cwd);
    let dir = env.data_dir.map(cache_dir);
    let mut slots: Vec<Slot> = variants
        .into_iter()
        .map(|variant| {
            let found = match (&dir, req.force) {
                (Some(dir), false) => lookup(dir, &key, &version, &model, &variant.key),
                _ => None,
            };
            Slot { variant, found }
        })
        .collect();
    let needed = slots.iter().filter(|s| s.found.is_none()).count();
    if needed > 0 && !approve(needed) {
        return Err(MeasureError::Declined(needed));
    }

    let library = library_skills(env.roots);
    let mut notes = Vec::new();
    let mut skipped = BTreeSet::new();
    for index in 0..slots.len() {
        if slots[index].found.is_some() {
            continue;
        }
        let as_is = slots[0].found.clone();
        let visible: BTreeSet<String> = as_is
            .iter()
            .flat_map(|e| e.run.skill_names.iter().cloned())
            .collect();
        let settings = match &slots[index].variant.kind {
            Kind::AsIs => None,
            Kind::Plugin(id) => Some(json!({ "enabledPlugins": { id: false } })),
            Kind::Skill(spec) => {
                let names = expand(std::slice::from_ref(spec), &visible);
                if names.is_empty() {
                    notes.push(format!(
                        "{}: no listed skill matches, nothing to measure",
                        slots[index].variant.label
                    ));
                    skipped.insert(index);
                    continue;
                }
                Some(json!({ "skillOverrides": overrides(&names, "off") }))
            }
            Kind::Bundle { def } => Some(bundle_settings(def, &library, &visible, &mut notes)),
        };
        let entry = run_variant(
            env,
            &cwd,
            &model,
            &version,
            &stamp,
            &slots[index].variant,
            settings.as_ref(),
        )?;
        if let Some(dir) = &dir {
            store(dir, &entry);
        }
        slots[index].found = Some(entry);
    }

    let made = needed - skipped.len();
    let served: Vec<&Entry> = slots.iter().filter_map(|s| s.found.as_ref()).collect();
    let stale = served
        .iter()
        .any(|e| e.claude_code_version != version || e.settings_stamp != stamp);
    let as_is = slots[0].found.as_ref().expect("the as-is run is measured first");
    let base = as_is.run.total;
    let runs: Vec<MeasureRun> = served.iter().map(|e| e.run.clone()).collect();
    let deltas = runs
        .iter()
        .skip(1)
        .map(|r| {
            let tokens = r.total as i64 - base as i64;
            Delta {
                label: r.label.clone(),
                tokens,
                percent: percent(tokens, base),
            }
        })
        .collect();
    let invisible = invisible_skills(env, &cwd, &as_is.run.skill_names, &as_is.slash_command_names);
    Ok(MeasureReport {
        cwd: key,
        claude_code_version: as_is.claude_code_version.clone(),
        model: as_is.model.clone(),
        measured_at: as_is.measured_at.clone(),
        cached: made == 0,
        stale,
        visible_skills: as_is.run.skill_names.clone(),
        invisible_skills: invisible,
        runs,
        deltas,
        notes,
    })
}

/// The newest cached as-is run for `cwd` (any model), without starting anything. `version` is the
/// installed Claude Code when it could be asked.
pub fn cached_as_is(
    data_dir: &Path,
    cwd: &Path,
    version: Option<&str>,
) -> Option<(MeasureRun, MeasuredInfo)> {
    let cwd = fsx::canonical(cwd);
    let key = fsx::display(&cwd);
    let newest = entries(&cache_dir(data_dir))
        .into_iter()
        .map(|(_, e)| e)
        .filter(|e| e.cwd == key && e.variant == AS_IS)
        .max_by(|a, b| a.measured_at.cmp(&b.measured_at))?;
    let stale = newest.settings_stamp != settings_stamp(&cwd)
        || version.is_some_and(|v| v != newest.claude_code_version);
    let info = MeasuredInfo {
        context_window: window_for_model(&newest.requested_model).max(window_for_model(&newest.model)),
        claude_code_version: newest.claude_code_version,
        model: newest.model,
        measured_at: newest.measured_at,
        stale,
    };
    Some((newest.run, info))
}
