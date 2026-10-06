//! Scheduled update watch (MIG-UPD-7): one pass of the read-only update check over every server,
//! remembered in `<data dir>/plus/update-watch.json`. A finding is raised once per state: the same
//! commits or version never fire again, a new count, a new version or a state that went away and
//! came back do. The attention feed reads the stored findings; nothing here starts a network call
//! except `tick`.

use super::{Ctx, Item, Level};
use crate::plus::tasks::cron;
use crate::plus::update::{self, Env, Mode, Options, Report, ServerReport, Status};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DEFAULT_INTERVAL_HOURS: u32 = 24;
pub const MAX_INTERVAL_HOURS: u32 = 24 * 30;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub enabled: bool,
    pub interval_hours: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { enabled: true, interval_hours: DEFAULT_INTERVAL_HOURS }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub server: String,
    pub kind: String,
    pub signature: String,
    pub title: String,
    pub detail: String,
    #[serde(default)]
    pub since: i64,
}

impl Finding {
    fn key(&self) -> String {
        format!("{}:{}", self.server, self.kind)
    }

    pub fn id(&self) -> String {
        format!("updates:{}:{}:{}", self.server, self.kind, self.signature)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    #[serde(default)]
    settings: Settings,
    #[serde(default)]
    last_run_at: Option<i64>,
    #[serde(default)]
    active: BTreeMap<String, Finding>,
}

pub fn path() -> Option<PathBuf> {
    crate::registry::conduit_dir().map(|d| d.join("plus").join("update-watch.json"))
}

fn read() -> File {
    path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write(file: &File) -> Result<(), String> {
    let path = path().ok_or("the data directory could not be resolved")?;
    let text = serde_json::to_string_pretty(file).unwrap_or_default() + "\n";
    crate::registry::atomic_write(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn settings() -> Settings {
    read().settings
}

pub fn set_settings(enabled: Option<bool>, interval_hours: Option<u32>) -> Result<Settings, String> {
    if let Some(hours) = interval_hours {
        if !(1..=MAX_INTERVAL_HOURS).contains(&hours) {
            return Err(format!("intervalHours must be between 1 and {MAX_INTERVAL_HOURS}, got {hours}"));
        }
    }
    let mut file = read();
    if let Some(enabled) = enabled {
        file.settings.enabled = enabled;
    }
    if let Some(hours) = interval_hours {
        file.settings.interval_hours = hours;
    }
    write(&file)?;
    Ok(file.settings)
}

fn finding(rep: &ServerReport, kind: &str, signature: String, title: String, detail: String) -> Finding {
    Finding { server: rep.id.clone(), kind: kind.into(), signature, title, detail, since: 0 }
}

/// What a report says is worth knowing: the fork remote or upstream has new commits, a package or
/// release has a newer version, or a fast-forward would conflict.
pub fn findings_of(rep: &ServerReport) -> Vec<Finding> {
    let mut out = Vec::new();
    let id = &rep.id;
    if rep.kind == "git" {
        let behind = rep.behind.unwrap_or(0);
        let ahead = rep.ahead.unwrap_or(0);
        let remote = rep.latest.as_deref().unwrap_or("its remote");
        if behind > 0 && ahead > 0 {
            out.push(finding(
                rep,
                "conflict",
                format!("{ahead}+{behind}"),
                format!("{id}: a sync would conflict"),
                format!("{id} has {ahead} local commit(s) and is {behind} behind {remote}; it cannot be fast-forwarded."),
            ));
        } else if behind > 0 {
            out.push(finding(
                rep,
                "fork",
                behind.to_string(),
                format!("{id}: {behind} new commit(s) on {remote}"),
                format!("{id} is {behind} commit(s) behind {remote}."),
            ));
        }
        if let Some(up) = rep.upstream.as_ref().filter(|u| u.behind > 0) {
            out.push(finding(
                rep,
                "upstream",
                up.behind.to_string(),
                format!("{id}: {} new commit(s) on {}", up.behind, up.remote_ref),
                format!("{id} is {} commit(s) behind {}.", up.behind, up.remote_ref),
            ));
        }
    } else if rep.status == Status::UpdateAvailable {
        let latest = rep.latest.as_deref().unwrap_or("a newer version");
        let current = rep.current.as_deref().unwrap_or("unknown");
        out.push(finding(
            rep,
            "package",
            format!("{current}>{latest}"),
            format!("{id}: {latest} is available"),
            format!("{id} ({}) runs {current}; {latest} is available.", rep.kind),
        ));
    }
    out
}

#[derive(Debug)]
pub struct Outcome {
    pub ran: bool,
    pub reason: Option<&'static str>,
    pub raised: Vec<Finding>,
    pub active: usize,
    pub errors: usize,
}

impl Outcome {
    pub fn to_value(&self) -> Value {
        json!({"ran": self.ran, "reason": self.reason, "raised": self.raised, "active": self.active, "errors": self.errors})
    }

    fn skipped(reason: &'static str, active: usize) -> Outcome {
        Outcome { ran: false, reason: Some(reason), raised: Vec::new(), active, errors: 0 }
    }
}

/// Folds one report into the stored state. A server whose check errored keeps what it had, so a
/// flaky network neither clears a finding nor makes it fire again when the network returns.
fn fold(file: &mut File, report: &Report, now: i64) -> Vec<Finding> {
    let checked: Vec<&str> = report.servers.iter().map(|s| s.id.as_str()).collect();
    let errored: Vec<&str> = report.servers.iter().filter(|s| s.status == Status::Error).map(|s| s.id.as_str()).collect();
    let mut next: BTreeMap<String, Finding> = file
        .active
        .iter()
        .filter(|(_, f)| errored.contains(&f.server.as_str()))
        .map(|(k, f)| (k.clone(), f.clone()))
        .collect();
    let mut raised = Vec::new();
    for rep in report.servers.iter().filter(|s| s.status != Status::Error) {
        for mut found in findings_of(rep) {
            match file.active.get(&found.key()) {
                Some(old) if old.signature == found.signature => found.since = old.since,
                _ => {
                    found.since = now;
                    raised.push(found.clone());
                }
            }
            next.insert(found.key(), found);
        }
    }
    next.retain(|_, f| checked.contains(&f.server.as_str()));
    file.active = next;
    raised
}

pub fn tick_with(env: &Env, now: i64, force: bool) -> Result<Outcome, String> {
    let mut file = read();
    if !file.settings.enabled {
        return Ok(Outcome::skipped("disabled", file.active.len()));
    }
    let interval = i64::from(file.settings.interval_hours) * 3600;
    if !force && file.last_run_at.is_some_and(|last| now < last + interval) {
        return Ok(Outcome::skipped("not-due", file.active.len()));
    }
    let report = update::execute_with(env, &Options::new(Mode::Check))?;
    let raised = fold(&mut file, &report, now);
    file.last_run_at = Some(now);
    write(&file)?;
    Ok(Outcome { ran: true, reason: None, raised, active: file.active.len(), errors: report.count("error") })
}

pub fn tick(force: bool) -> Result<Outcome, String> {
    tick_with(&Env::system(), cron::now(), force)
}

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let file = read();
    if !file.settings.enabled {
        return Vec::new();
    }
    file.active
        .values()
        .map(|f| {
            let level = if f.kind == "conflict" { Level::NeedsYou } else { Level::Look };
            Item::new(ctx, f.id(), level, "update", f.title.clone(), f.detail.clone())
                .target("servers", &[("server", &f.server)])
                .since_epoch(f.since)
        })
        .collect()
}

pub fn settings_handler(args: Value) -> Result<Value, String> {
    let enabled = args.get("enabled").and_then(Value::as_bool);
    let hours = match args.get("intervalHours") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("intervalHours must be a whole number of hours")?),
    };
    let settings = if enabled.is_some() || hours.is_some() { set_settings(enabled, hours)? } else { settings() };
    serde_json::to_value(settings).map_err(|e| e.to_string())
}

pub fn tick_handler(args: Value) -> Result<Value, String> {
    let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
    tick(force).map(|o| o.to_value())
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
