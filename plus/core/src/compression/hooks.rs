//! Toolport owns the hook entries of the `rtk` and `read-once` providers in Claude Code's
//! `settings.json`. An entry that is already there is wrapped, not replaced:
//! `toolportctl compression hook <id> -- /bin/sh -c '<original>'` runs the original, counts the
//! call and its latency, and passes the answer through untouched. The original command is kept
//! in the ledger, so undo puts back exactly what was there. Entries Toolport did not wrap are
//! never touched.

use super::ledger::{format_ts, now_ms, parse_ts};
use super::model::ProviderName;
use super::store::Paths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use toolport_plus_api::SettingsStore;

pub const LEDGER_FILE: &str = "compression-hooks.json";
pub const COUNTERS_FILE: &str = "compression-hooks.jsonl";
pub const WRAP_PREFIX: &str = "toolportctl compression hook ";
/// Set by `compression hooks verify`: the wrapper then runs even while paused and counts nothing.
pub const VERIFY_ENV: &str = "TOOLPORT_HOOK_VERIFY";
const EVENT: &str = "PreToolUse";
pub const CANNOT_TELL: &str = "Counts hook calls, latency and how often the hook answered. It cannot see the size of the tool output a rewrite later saves; that needs the billed measurement in `compression verify`.";

pub struct HookProvider {
    pub id: &'static str,
    pub matcher: &'static str,
    pub marker: &'static str,
}

pub const HOOK_PROVIDERS: [HookProvider; 2] = [
    HookProvider {
        id: "rtk",
        matcher: "Bash",
        marker: "rtk",
    },
    HookProvider {
        id: "read-once",
        matcher: "Read",
        marker: "read-once",
    },
];

/// What a provider does to the machine, as data: the owner's rule is enforced from this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trust {
    pub closed_engine: bool,
    pub sends_code_off_machine: bool,
    pub installs_global_hooks: bool,
}

impl Trust {
    pub fn needs_egress_consent(self) -> bool {
        self.closed_engine && self.sends_code_off_machine
    }
}

pub fn trust_of(provider: ProviderName) -> Trust {
    let (closed_engine, sends_code_off_machine, installs_global_hooks) = match provider {
        ProviderName::Headroom => (false, false, false),
        ProviderName::RtkOnly => (false, false, true),
        ProviderName::Parsec => (true, true, true),
        ProviderName::None => (false, false, false),
    };
    Trust {
        closed_engine,
        sends_code_off_machine,
        installs_global_hooks,
    }
}

pub fn trust_json(provider: ProviderName) -> Value {
    let t = trust_of(provider);
    json!({
        "closedEngine": t.closed_engine,
        "sendsCodeOffMachine": t.sends_code_off_machine,
        "installsGlobalHooks": t.installs_global_hooks,
        "needsEgressConsent": t.needs_egress_consent(),
    })
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub file: String,
    pub provider: String,
    pub original: String,
    pub wrapped: String,
    pub at: String,
}

/// An edited entry Toolport took over. `previous` is what the ledger held before, so a revert
/// puts it back; without one the ledger had no entry and a revert removes it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Adoption {
    pub file: String,
    pub provider: String,
    pub at: String,
    #[serde(default)]
    pub previous: Option<Previous>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Previous {
    pub original: String,
    pub wrapped: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Epoch {
    pub event: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    #[serde(default)]
    pub entries: Vec<LedgerEntry>,
    #[serde(default)]
    pub paused_since: Option<String>,
    #[serde(default)]
    pub paused_until: Option<String>,
    #[serde(default)]
    pub adopted: Vec<Adoption>,
    #[serde(default)]
    pub epochs: Vec<Epoch>,
    #[serde(default)]
    pub egress_accepted: Vec<Epoch>,
}

fn ledger_path(paths: &Paths) -> PathBuf {
    paths.dir.join(LEDGER_FILE)
}

fn counters_path(paths: &Paths) -> PathBuf {
    paths.dir.join(COUNTERS_FILE)
}

pub fn read_ledger(paths: &Paths) -> Result<Ledger, String> {
    match std::fs::read_to_string(ledger_path(paths)) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{LEDGER_FILE}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_ledger(paths: &Paths, ledger: &Ledger) -> Result<(), String> {
    crate::fsx::write_private(
        &ledger_path(paths),
        &serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())?,
    )
}

pub fn record_egress_consent(paths: &Paths, provider: ProviderName) -> Result<(), String> {
    let mut ledger = read_ledger(paths)?;
    ledger.egress_accepted.push(Epoch {
        event: "accept-egress".into(),
        at: format_ts(now_ms()),
        provider: Some(provider.as_str().to_string()),
    });
    write_ledger(paths, &ledger)
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn wrap(id: &str, original: &str) -> String {
    format!("{WRAP_PREFIX}{id} -- /bin/sh -c {}", shell_quote(original))
}

pub(super) fn is_wrapped(command: &str) -> bool {
    command.starts_with(WRAP_PREFIX)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Absent,
    Foreign,
    Ours,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Absent => "absent",
            State::Foreign => "foreign",
            State::Ours => "ours",
        }
    }
}

/// The command a wrapped entry runs, when the entry still has the shape `wrap` gives it.
pub(super) fn unwrap_original(command: &str, id: &str) -> Option<String> {
    let rest = command.strip_prefix(&format!("{WRAP_PREFIX}{id} -- /bin/sh -c "))?;
    let inner = rest
        .strip_prefix('\'')?
        .strip_suffix('\'')?
        .replace("'\\''", "'");
    (shell_quote(&inner) == rest).then_some(inner)
}

/// Position of the provider's entry in `hooks.PreToolUse`: group, hook, command.
pub(super) fn find(hooks: &Value, provider: &HookProvider) -> Option<(usize, usize, String)> {
    let groups = hooks.get(EVENT)?.as_array()?;
    for (gi, group) in groups.iter().enumerate() {
        if group.get("matcher").and_then(Value::as_str) != Some(provider.matcher) {
            continue;
        }
        let Some(list) = group.get("hooks").and_then(Value::as_array) else {
            continue;
        };
        for (hi, hook) in list.iter().enumerate() {
            let Some(command) = hook.get("command").and_then(Value::as_str) else {
                continue;
            };
            let ours = command.starts_with(&format!("{WRAP_PREFIX}{} ", provider.id));
            if ours
                || command.to_ascii_lowercase().contains(provider.marker) && !is_wrapped(command)
            {
                return Some((gi, hi, command.to_string()));
            }
        }
    }
    None
}

fn set_command(hooks: &mut Value, at: (usize, usize), command: &str) {
    hooks[EVENT][at.0]["hooks"][at.1]["command"] = json!(command);
}

fn add_entry(hooks: &mut Value, provider: &HookProvider, command: &str) {
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let groups = &mut hooks[EVENT];
    if !groups.is_array() {
        *groups = json!([]);
    }
    if let Some(list) = groups.as_array_mut() {
        list.push(json!({
            "matcher": provider.matcher,
            "hooks": [{"type": "command", "command": command}],
        }));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub file: PathBuf,
    pub provider: &'static str,
    pub state: State,
    pub command: Option<String>,
}

impl Row {
    fn to_value(&self, action: Option<&str>) -> Value {
        let mut v = json!({
            "file": self.file.to_string_lossy(),
            "provider": self.provider,
            "state": self.state.as_str(),
            "command": self.command,
        });
        if let Some(action) = action {
            v["action"] = json!(action);
        }
        v
    }
}

pub(super) fn read_hooks(
    store: &dyn SettingsStore,
    file: &Path,
) -> Result<(Value, Option<String>, Value), String> {
    let (root, text) = store.read_settings(file)?;
    let hooks = root.get("hooks").cloned().unwrap_or(Value::Null);
    Ok((root, text, hooks))
}

pub fn rows(store: &dyn SettingsStore) -> Result<Vec<Row>, String> {
    let mut out = Vec::new();
    for file in store.claude_settings_paths() {
        let (_, _, hooks) = read_hooks(store, &file)?;
        for provider in &HOOK_PROVIDERS {
            let found = find(&hooks, provider);
            out.push(Row {
                file: file.clone(),
                provider: provider.id,
                state: match &found {
                    None => State::Absent,
                    Some((_, _, c)) if is_wrapped(c) => State::Ours,
                    Some(_) => State::Foreign,
                },
                command: found.map(|(_, _, c)| c),
            });
        }
    }
    Ok(out)
}

pub fn apply(
    store: &dyn SettingsStore,
    paths: &Paths,
    only: Option<&str>,
    dry_run: bool,
) -> Result<Value, String> {
    let mut ledger = read_ledger(paths)?;
    let mut out = Vec::new();
    for file in store.claude_settings_paths() {
        let (mut root, text, mut hooks) = read_hooks(store, &file)?;
        let mut changed = false;
        for provider in HOOK_PROVIDERS
            .iter()
            .filter(|p| only.is_none_or(|id| id == p.id))
        {
            let found = find(&hooks, provider);
            let row = |state, command: Option<String>| Row {
                file: file.clone(),
                provider: provider.id,
                state,
                command,
            };
            let recorded = ledger
                .entries
                .iter()
                .find(|e| e.file == file.to_string_lossy() && e.provider == provider.id)
                .cloned();
            match (found, recorded) {
                (None, None) => out.push(row(State::Absent, None).to_value(Some("none to wrap"))),
                (None, Some(entry)) => {
                    add_entry(&mut hooks, provider, &entry.wrapped);
                    changed = true;
                    let action = if dry_run {
                        "would re-apply"
                    } else {
                        "re-applied"
                    };
                    out.push(row(State::Absent, None).to_value(Some(action)));
                }
                (Some((gi, hi, c)), Some(entry)) if is_wrapped(&c) && c != entry.wrapped => {
                    set_command(&mut hooks, (gi, hi), &entry.wrapped);
                    changed = true;
                    let action = if dry_run {
                        "would restore ours"
                    } else {
                        "restored ours"
                    };
                    out.push(row(State::Ours, Some(c)).to_value(Some(action)));
                }
                (Some((_, _, c)), _) if is_wrapped(&c) => {
                    out.push(row(State::Ours, Some(c)).to_value(Some("kept")))
                }
                (Some((gi, hi, original)), _) => {
                    let wrapped = wrap(provider.id, &original);
                    set_command(&mut hooks, (gi, hi), &wrapped);
                    changed = true;
                    ledger.entries.retain(|e| {
                        !(e.file == file.to_string_lossy() && e.provider == provider.id)
                    });
                    ledger.adopted.retain(|a| {
                        !(a.file == file.to_string_lossy() && a.provider == provider.id)
                    });
                    ledger.entries.push(LedgerEntry {
                        file: file.to_string_lossy().into_owned(),
                        provider: provider.id.into(),
                        original: original.clone(),
                        wrapped,
                        at: format_ts(now_ms()),
                    });
                    ledger.epochs.push(Epoch {
                        event: "wrap".into(),
                        at: format_ts(now_ms()),
                        provider: Some(provider.id.into()),
                    });
                    let action = if dry_run { "would wrap" } else { "wrapped" };
                    out.push(row(State::Foreign, Some(original)).to_value(Some(action)));
                }
            }
        }
        if changed && !dry_run {
            root["hooks"] = hooks;
            store.write_hooks(&file, text.as_deref(), &root)?;
        }
    }
    if !dry_run && out.iter().any(|r| r["action"] == "wrapped") {
        write_ledger(paths, &ledger)?;
    }
    Ok(json!({"dryRun": dry_run, "rows": out}))
}

pub fn undo(
    store: &dyn SettingsStore,
    paths: &Paths,
    only: Option<&str>,
    dry_run: bool,
) -> Result<Value, String> {
    let mut ledger = read_ledger(paths)?;
    let mut out = Vec::new();
    let mut kept = Vec::new();
    for entry in std::mem::take(&mut ledger.entries) {
        if only.is_some_and(|id| id != entry.provider) {
            kept.push(entry);
            continue;
        }
        let file = PathBuf::from(&entry.file);
        let (mut root, text, mut hooks) = read_hooks(store, &file)?;
        let provider = HOOK_PROVIDERS.iter().find(|p| p.id == entry.provider);
        let at = provider.and_then(|p| find(&hooks, p));
        let mut report = json!({"file": entry.file, "provider": entry.provider});
        match at {
            Some((gi, hi, command)) if command == entry.wrapped => {
                report["action"] = json!(if dry_run { "would restore" } else { "restored" });
                if !dry_run {
                    set_command(&mut hooks, (gi, hi), &entry.original);
                    root["hooks"] = hooks;
                    store.write_hooks(&file, text.as_deref(), &root)?;
                }
                if dry_run {
                    kept.push(entry);
                }
            }
            Some(_) => {
                report["action"] = json!("edited since, left alone");
                kept.push(entry);
            }
            None => {
                report["action"] = json!("gone, dropped from the ledger");
                if dry_run {
                    kept.push(entry);
                }
            }
        }
        out.push(report);
    }
    ledger.entries = kept;
    ledger.adopted.retain(|a| {
        ledger
            .entries
            .iter()
            .any(|e| e.file == a.file && e.provider == a.provider)
    });
    if !dry_run && !out.is_empty() {
        ledger.epochs.push(Epoch {
            event: "undo".into(),
            at: format_ts(now_ms()),
            provider: only.map(String::from),
        });
        write_ledger(paths, &ledger)?;
    }
    Ok(json!({"dryRun": dry_run, "rows": out}))
}

/// Takes over an entry that reads as edited: the ledger records the edit as what Toolport wrote
/// (its health turns ok) and what undo restores is the edit's own command, not the original of
/// before the edit. `revert` puts the ledger back as it was before the adoption.
pub fn adopt_edit(
    store: &dyn SettingsStore,
    paths: &Paths,
    id: &str,
    revert: bool,
) -> Result<Value, String> {
    let provider = HOOK_PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("unknown hook provider '{id}'"))?;
    let mut ledger = read_ledger(paths)?;
    let mut out = Vec::new();
    for file in store.claude_settings_paths() {
        let (_, _, hooks) = read_hooks(store, &file)?;
        let Some((_, _, command)) = find(&hooks, provider) else {
            continue;
        };
        let name = file.to_string_lossy().into_owned();
        let at = ledger
            .entries
            .iter()
            .position(|e| e.file == name && e.provider == id);
        let report = |action: &str| json!({"file": name, "provider": id, "action": action, "command": command});
        if revert {
            let Some(adoption) = ledger
                .adopted
                .iter()
                .position(|a| a.file == name && a.provider == id)
                .map(|i| ledger.adopted.remove(i))
            else {
                continue;
            };
            match (adoption.previous, at) {
                (Some(previous), Some(i)) => {
                    ledger.entries[i].original = previous.original;
                    ledger.entries[i].wrapped = previous.wrapped;
                }
                (None, Some(i)) => {
                    ledger.entries.remove(i);
                    ledger.epochs.push(Epoch {
                        event: "undo".into(),
                        at: format_ts(now_ms()),
                        provider: Some(id.into()),
                    });
                }
                (_, None) => continue,
            }
            out.push(report("reverted"));
            continue;
        }
        let edited =
            is_wrapped(&command) && at.is_none_or(|i| ledger.entries[i].wrapped != command);
        if !edited {
            continue;
        }
        let original = unwrap_original(&command, id).unwrap_or_else(|| command.clone());
        let now = format_ts(now_ms());
        let previous = at.map(|i| Previous {
            original: ledger.entries[i].original.clone(),
            wrapped: ledger.entries[i].wrapped.clone(),
        });
        match at {
            Some(i) => {
                ledger.entries[i].original = original;
                ledger.entries[i].wrapped = command.clone();
            }
            None => {
                ledger.entries.push(LedgerEntry {
                    file: name.clone(),
                    provider: id.into(),
                    original,
                    wrapped: command.clone(),
                    at: now.clone(),
                });
                ledger.epochs.push(Epoch {
                    event: "wrap".into(),
                    at: now.clone(),
                    provider: Some(id.into()),
                });
            }
        }
        ledger
            .adopted
            .retain(|a| !(a.file == name && a.provider == id));
        ledger.adopted.push(Adoption {
            file: name.clone(),
            provider: id.into(),
            at: now,
            previous,
        });
        out.push(report("adopted"));
    }
    if out.is_empty() {
        return Err(if revert {
            format!("no {id} entry holds an adopted edit to revert")
        } else {
            format!("no {id} entry reads as edited")
        });
    }
    write_ledger(paths, &ledger)?;
    Ok(json!({"provider": id, "revert": revert, "rows": out}))
}

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 24 * 60 * MINUTE_MS;
pub const MAX_PAUSE_MS: i64 = 30 * DAY_MS;

/// `1d`, `6h`, `90m`, `1d12h`: whole units of days, hours, minutes and seconds, summed.
pub fn parse_duration(text: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut digits = String::new();
    let mut seen = false;
    for c in text.trim().chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit = match c.to_ascii_lowercase() {
            'd' => DAY_MS,
            'h' => 60 * MINUTE_MS,
            'm' => MINUTE_MS,
            's' => 1000,
            _ => return None,
        };
        total = total.checked_add(digits.parse::<i64>().ok()?.checked_mul(unit)?)?;
        digits.clear();
        seen = true;
    }
    (seen && digits.is_empty() && total > 0).then_some(total)
}

/// An RFC 3339 timestamp with or without seconds, or a date; without an offset the time is UTC.
pub fn parse_until(text: &str) -> Option<i64> {
    let without_seconds = || {
        let (head, tail) = (text.get(..16)?, text.get(16..)?);
        parse_ts(&format!("{head}:00{tail}"))
    };
    parse_ts(text)
        .or_else(without_seconds)
        .or_else(|| parse_ts(&format!("{text}T00:00:00Z")))
}

/// When a pause ends, from `--for` or `--until`; `None` is a pause with no end.
pub fn pause_end(
    duration: Option<&str>,
    until: Option<&str>,
    now: i64,
) -> Result<Option<i64>, String> {
    let end = match (duration, until) {
        (Some(_), Some(_)) => return Err("pass either for or until, not both".into()),
        (Some(text), None) => now
            + parse_duration(text).ok_or_else(|| {
                format!("'{text}' is not a duration (use 1d, 6h, 90m or 1d12h)")
            })?,
        (None, Some(text)) => parse_until(text).ok_or_else(|| {
            format!("'{text}' is not a time (use 2026-11-18T09:12+01:00, 2026-11-18T09:12 in UTC or a date)")
        })?,
        (None, None) => return Ok(None),
    };
    if end <= now {
        return Err("the pause would end before it starts".into());
    }
    if end - now > MAX_PAUSE_MS {
        return Err(
            "a pause lasts at most 30 days; pause without an end to keep it until you resume"
                .into(),
        );
    }
    Ok(Some(end))
}

/// Whether the wrapped hooks answer nothing at `now`. A pause with an end is over once the end
/// has passed, with nothing written: the hook reads this, so the hooks resume without an app.
pub fn pause_active(ledger: &Ledger, now: i64) -> bool {
    ledger.paused_since.is_some()
        && ledger
            .paused_until
            .as_deref()
            .is_none_or(|until| parse_ts(until).is_some_and(|end| now < end))
}

/// Records the end of a pause that ran out as the resume it is, at the time it ran out.
fn settle(ledger: &mut Ledger, now: i64) -> bool {
    if ledger.paused_since.is_none() || pause_active(ledger, now) {
        return false;
    }
    let at = ledger
        .paused_until
        .as_deref()
        .and_then(parse_ts)
        .unwrap_or(now);
    ledger.paused_since = None;
    ledger.paused_until = None;
    ledger.epochs.push(Epoch {
        event: "resume".into(),
        at: format_ts(at),
        provider: None,
    });
    true
}

/// The epochs of the ledger with a pause that ran out already closed.
pub fn effective_epochs(ledger: &Ledger, now: i64) -> Vec<Epoch> {
    let mut settled = ledger.clone();
    settle(&mut settled, now);
    settled.epochs
}

pub fn set_paused(
    paths: &Paths,
    paused: bool,
    until: Option<i64>,
    now: i64,
) -> Result<Value, String> {
    let mut ledger = read_ledger(paths)?;
    let settled = settle(&mut ledger, now);
    let was = pause_active(&ledger, now);
    let until = until.map(format_ts);
    let changed = if paused && !was {
        ledger.paused_since = Some(format_ts(now));
        ledger.paused_until = until;
        ledger.epochs.push(Epoch {
            event: "pause".into(),
            at: format_ts(now),
            provider: None,
        });
        true
    } else if paused {
        let moved = ledger.paused_until != until;
        ledger.paused_until = until;
        moved
    } else if was {
        ledger.paused_since = None;
        ledger.paused_until = None;
        ledger.epochs.push(Epoch {
            event: "resume".into(),
            at: format_ts(now),
            provider: None,
        });
        true
    } else {
        false
    };
    if changed || settled {
        write_ledger(paths, &ledger)?;
    }
    Ok(json!({
        "paused": paused,
        "changed": changed,
        "pausedSince": ledger.paused_since,
        "pausedUntil": ledger.paused_until,
    }))
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CounterSummary {
    pub calls: usize,
    pub answered: usize,
    pub failed: usize,
    pub median_ms: Option<u64>,
    pub p95_ms: Option<u64>,
    pub max_ms: Option<u64>,
    pub first: Option<String>,
    pub last: Option<String>,
}

pub fn summarize(paths: &Paths, provider: &str) -> CounterSummary {
    let text = std::fs::read_to_string(counters_path(paths)).unwrap_or_default();
    let mut latencies = Vec::new();
    let mut summary = CounterSummary::default();
    for line in text.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if row["provider"] != provider {
            continue;
        }
        summary.calls += 1;
        summary.answered += usize::from(row["answered"] == true);
        summary.failed += usize::from(row["ok"] == false);
        latencies.extend(row["latencyMs"].as_u64());
        let at = row["at"].as_str().map(String::from);
        summary.first = summary.first.take().or(at.clone());
        summary.last = at.or(summary.last.take());
    }
    latencies.sort_unstable();
    summary.median_ms = latencies.get(latencies.len() / 2).copied();
    summary.p95_ms = latencies
        .get(((latencies.len().max(1) - 1) as f64 * 0.95).round() as usize)
        .copied();
    summary.max_ms = latencies.last().copied();
    summary
}

/// What the ledger says about an entry next to what the file says: `missing` is a wrapped entry
/// that is gone, `edited` one that no longer reads as Toolport wrote it.
fn health(row: &Row, ledger: &Ledger) -> (&'static str, Option<String>) {
    let file = row.file.to_string_lossy();
    let recorded = ledger
        .entries
        .iter()
        .find(|e| e.file == file && e.provider == row.provider);
    let health = match (row.state, recorded) {
        (State::Foreign, _) => "foreign",
        (State::Ours, Some(e)) if row.command.as_deref() == Some(e.wrapped.as_str()) => "ok",
        (State::Ours, _) => "edited",
        (State::Absent, Some(_)) => "missing",
        (State::Absent, None) => "absent",
    };
    (health, recorded.map(|e| e.at.clone()))
}

pub fn status(store: &dyn SettingsStore, paths: &Paths) -> Result<Value, String> {
    let ledger = read_ledger(paths)?;
    let now = now_ms();
    let paused = pause_active(&ledger, now);
    let rows = rows(store)?;
    let counters: Vec<Value> = HOOK_PROVIDERS
        .iter()
        .map(|p| {
            let s = summarize(paths, p.id);
            json!({
                "provider": p.id,
                "basis": "mechanical",
                "calls": s.calls,
                "answered": s.answered,
                "failed": s.failed,
                "medianLatencyMs": s.median_ms,
                "p95LatencyMs": s.p95_ms,
                "maxLatencyMs": s.max_ms,
                "firstCall": s.first,
                "lastCall": s.last,
            })
        })
        .collect();
    Ok(json!({
        "paused": paused,
        "pausedSince": ledger.paused_since.as_ref().filter(|_| paused),
        "pausedUntil": ledger.paused_until.as_ref().filter(|_| paused),
        "entries": rows
            .iter()
            .map(|r| {
                let (health, wrapped_at) = health(r, &ledger);
                let mut entry = r.to_value(None);
                entry["health"] = json!(health);
                entry["wrappedAt"] = json!(wrapped_at);
                entry
            })
            .collect::<Vec<_>>(),
        "counters": counters,
        "wrapped": ledger.entries.len(),
        "epochs": effective_epochs(&ledger, now).len(),
        "cannotTell": CANNOT_TELL,
        "trust": ProviderName::ALL.iter().map(|p| json!({"provider": p.as_str(), "trust": trust_json(*p)})).collect::<Vec<_>>(),
    }))
}

pub struct Outcome {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs the wrapped command with the hook payload on stdin. Fails open: when the command cannot
/// be started or Toolport cannot count, the call still gets whatever the command answered.
pub fn run_wrapper(paths: &Paths, id: &str, argv: &[String], stdin: &[u8]) -> Outcome {
    let verifying = std::env::var_os(VERIFY_ENV).is_some();
    let paused = !verifying && read_ledger(paths).is_ok_and(|l| pause_active(&l, now_ms()));
    if paused || argv.is_empty() {
        return Outcome {
            code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
    }
    let started = Instant::now();
    let spawned = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let outcome = match spawned {
        Ok(mut child) => {
            if let Some(mut input) = child.stdin.take() {
                let _ = input.write_all(stdin);
            }
            match child.wait_with_output() {
                Ok(out) => Outcome {
                    code: out.status.code().unwrap_or(1),
                    stdout: out.stdout,
                    stderr: out.stderr,
                },
                Err(_) => Outcome {
                    code: 0,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                },
            }
        }
        Err(_) => Outcome {
            code: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
        },
    };
    if verifying {
        return outcome;
    }
    let row = json!({
        "at": format_ts(now_ms()),
        "provider": id,
        "bytesIn": stdin.len(),
        "bytesOut": outcome.stdout.len(),
        "latencyMs": started.elapsed().as_millis() as u64,
        "answered": !outcome.stdout.is_empty(),
        "ok": outcome.code == 0,
    });
    if let Ok(mut file) = super::sys::open_append_private(&counters_path(paths)) {
        let _ = writeln!(file, "{row}");
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolport_plus_api::MemoryHost;

    struct World {
        _scratch: crate::randutil::ScratchDir,
        paths: Paths,
        host: MemoryHost,
        file: PathBuf,
    }

    fn world(settings: Value) -> World {
        let scratch = crate::randutil::ScratchDir::new("hooks");
        let dir = scratch.path().to_path_buf();
        let file = dir.join("settings.json");
        std::fs::write(&file, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
        World {
            paths: Paths::new(&dir),
            host: MemoryHost {
                settings_paths: vec![file.clone()],
                ..Default::default()
            },
            file,
            _scratch: scratch,
        }
    }

    fn rtk_settings() -> Value {
        json!({
            "theme": "dark",
            "hooks": {"PreToolUse": [
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "rtk hook claude"}]},
                {"matcher": "Write", "hooks": [{"type": "command", "command": "echo unrelated"}]}
            ]}
        })
    }

    fn settings(w: &World) -> Value {
        serde_json::from_str(&std::fs::read_to_string(&w.file).unwrap()).unwrap()
    }

    #[test]
    fn test01_apply_wraps_a_foreign_entry_and_keeps_everything_else() {
        let w = world(rtk_settings());
        let data = apply(&w.host, &w.paths, None, false).unwrap();
        let s = settings(&w);
        let command = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(command, wrap("rtk", "rtk hook claude"));
        assert_eq!(s["theme"], "dark");
        assert_eq!(
            s["hooks"]["PreToolUse"][1],
            rtk_settings()["hooks"]["PreToolUse"][1]
        );
        let actions: Vec<_> = data["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["provider"].as_str().unwrap(),
                    r["action"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(actions, [("rtk", "wrapped"), ("read-once", "none to wrap")]);
        assert_eq!(read_ledger(&w.paths).unwrap().entries.len(), 1);
    }

    #[test]
    fn test02_apply_twice_changes_nothing_the_second_time() {
        let w = world(rtk_settings());
        apply(&w.host, &w.paths, None, false).unwrap();
        let first = std::fs::read(&w.file).unwrap();
        let data = apply(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(std::fs::read(&w.file).unwrap(), first);
        assert_eq!(data["rows"][0]["action"], "kept");
        assert_eq!(read_ledger(&w.paths).unwrap().entries.len(), 1);
    }

    #[test]
    fn test03_dry_run_writes_neither_settings_nor_ledger() {
        let w = world(rtk_settings());
        let before = std::fs::read(&w.file).unwrap();
        let data = apply(&w.host, &w.paths, None, true).unwrap();
        assert_eq!(data["rows"][0]["action"], "would wrap");
        assert_eq!(std::fs::read(&w.file).unwrap(), before);
        assert!(!w.paths.dir.join(LEDGER_FILE).exists());
    }

    #[test]
    fn test04_undo_restores_the_original_command_exactly() {
        let w = world(rtk_settings());
        apply(&w.host, &w.paths, None, false).unwrap();
        let data = undo(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(data["rows"][0]["action"], "restored");
        assert_eq!(settings(&w)["hooks"], rtk_settings()["hooks"]);
        assert!(read_ledger(&w.paths).unwrap().entries.is_empty());
    }

    #[test]
    fn test05_undo_leaves_an_entry_edited_since_alone() {
        let w = world(rtk_settings());
        apply(&w.host, &w.paths, None, false).unwrap();
        let mut s = settings(&w);
        s["hooks"]["PreToolUse"][0]["hooks"][0]["command"] =
            json!(format!("{}  # edited", wrap("rtk", "rtk hook claude")));
        std::fs::write(&w.file, s.to_string()).unwrap();
        let edited = std::fs::read(&w.file).unwrap();
        let data = undo(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(data["rows"][0]["action"], "edited since, left alone");
        assert_eq!(std::fs::read(&w.file).unwrap(), edited);
        assert_eq!(read_ledger(&w.paths).unwrap().entries.len(), 1);
    }

    #[test]
    fn test06_a_quote_in_the_original_survives_wrapping() {
        let original = "env FOO='a b' rtk hook claude";
        let w = world(json!({"hooks": {"PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": original}]}
        ]}}));
        apply(&w.host, &w.paths, None, false).unwrap();
        let wrapped = wrap("rtk", original);
        assert!(wrapped.ends_with("-c 'env FOO='\\''a b'\\'' rtk hook claude'"));
        undo(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(
            settings(&w)["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            original
        );
    }

    #[test]
    fn test07_status_reports_absent_foreign_and_ours() {
        let w = world(rtk_settings());
        let state =
            |data: &Value, i: usize| data["entries"][i]["state"].as_str().unwrap().to_string();
        let data = status(&w.host, &w.paths).unwrap();
        assert_eq!(
            (state(&data, 0), state(&data, 1)),
            ("foreign".into(), "absent".into())
        );
        apply(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(state(&status(&w.host, &w.paths).unwrap(), 0), "ours");
    }

    #[cfg(unix)]
    #[test]
    fn test08_the_wrapper_passes_the_answer_through_and_counts_the_call() {
        let w = world(rtk_settings());
        let argv: Vec<String> = ["/bin/sh", "-c", "cat; echo '{\"answer\":1}'; exit 0"]
            .map(String::from)
            .to_vec();
        let out = run_wrapper(&w.paths, "rtk", &argv, b"payload");
        assert_eq!(out.code, 0);
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "payload{\"answer\":1}\n"
        );
        let s = summarize(&w.paths, "rtk");
        assert_eq!((s.calls, s.answered, s.failed), (1, 1, 0));
        assert!(s.median_ms.is_some());
        let blocked: Vec<String> = ["/bin/sh", "-c", "exit 2"].map(String::from).to_vec();
        assert_eq!(run_wrapper(&w.paths, "rtk", &blocked, b"").code, 2);
        assert_eq!(summarize(&w.paths, "rtk").failed, 1);
    }

    #[test]
    fn test09_a_command_that_cannot_start_fails_open() {
        let w = world(rtk_settings());
        let argv = vec!["/nonexistent/provider".to_string()];
        let out = run_wrapper(&w.paths, "rtk", &argv, b"x");
        assert_eq!((out.code, out.stdout.len()), (0, 0));
    }

    #[test]
    fn test10_a_paused_wrapper_answers_nothing_and_counts_nothing() {
        let w = world(rtk_settings());
        set_paused(&w.paths, true, None, now_ms()).unwrap();
        let argv: Vec<String> = ["/bin/sh", "-c", "echo hi"].map(String::from).to_vec();
        let out = run_wrapper(&w.paths, "rtk", &argv, b"");
        assert!(out.stdout.is_empty());
        assert_eq!(summarize(&w.paths, "rtk").calls, 0);
        assert_eq!(
            set_paused(&w.paths, true, None, now_ms()).unwrap()["changed"],
            false
        );
        set_paused(&w.paths, false, None, now_ms()).unwrap();
        let events: Vec<_> = read_ledger(&w.paths)
            .unwrap()
            .epochs
            .into_iter()
            .map(|e| e.event)
            .collect();
        assert_eq!(events, ["pause", "resume"]);
    }

    fn health_of(w: &World, i: usize) -> String {
        status(&w.host, &w.paths).unwrap()["entries"][i]["health"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn test12_status_tells_ok_edited_and_missing_from_the_ledger() {
        let w = world(rtk_settings());
        assert_eq!(health_of(&w, 0), "foreign");
        assert_eq!(health_of(&w, 1), "absent");
        apply(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(health_of(&w, 0), "ok");
        let wrapped = wrap("rtk", "rtk hook claude");
        let mut s = settings(&w);
        s["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = json!(format!("{wrapped} # edited"));
        std::fs::write(&w.file, s.to_string()).unwrap();
        assert_eq!(health_of(&w, 0), "edited");
        std::fs::write(&w.file, json!({"theme": "dark"}).to_string()).unwrap();
        assert_eq!(health_of(&w, 0), "missing");
    }

    #[test]
    fn test13_apply_restores_an_edited_entry_and_puts_a_missing_one_back() {
        let w = world(rtk_settings());
        apply(&w.host, &w.paths, None, false).unwrap();
        let wrapped = wrap("rtk", "rtk hook claude");
        let mut s = settings(&w);
        s["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = json!(format!("{wrapped} # edited"));
        std::fs::write(&w.file, s.to_string()).unwrap();
        let preview = apply(&w.host, &w.paths, None, true).unwrap();
        assert_eq!(preview["rows"][0]["action"], "would restore ours");
        assert_eq!(health_of(&w, 0), "edited");
        let data = apply(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(data["rows"][0]["action"], "restored ours");
        assert_eq!(health_of(&w, 0), "ok");

        std::fs::write(&w.file, json!({"theme": "dark"}).to_string()).unwrap();
        let data = apply(&w.host, &w.paths, None, false).unwrap();
        assert_eq!(data["rows"][0]["action"], "re-applied");
        assert_eq!(health_of(&w, 0), "ok");
        assert_eq!(settings(&w)["theme"], "dark");
        assert_eq!(
            settings(&w)["hooks"]["PreToolUse"][0],
            json!({"matcher": "Bash", "hooks": [{"type": "command", "command": wrapped}]})
        );
    }

    #[test]
    fn test14_the_summary_reports_the_95th_percentile_of_the_latency() {
        let w = world(rtk_settings());
        let rows: Vec<String> = (1..=20)
            .map(|n| {
                json!({"provider": "rtk", "latencyMs": n * 10, "answered": true, "ok": true})
                    .to_string()
            })
            .collect();
        std::fs::write(w.paths.dir.join(COUNTERS_FILE), rows.join("\n")).unwrap();
        let s = summarize(&w.paths, "rtk");
        assert_eq!(
            (s.median_ms, s.p95_ms, s.max_ms),
            (Some(110), Some(190), Some(200))
        );
        assert_eq!(summarize(&w.paths, "read-once").p95_ms, None);
    }

    #[test]
    fn test11_only_parsec_needs_consent_to_send_code_away() {
        for p in ProviderName::ALL {
            assert_eq!(
                trust_of(p).needs_egress_consent(),
                p == ProviderName::Parsec,
                "{}",
                p.as_str()
            );
        }
    }
}
