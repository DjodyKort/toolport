//! Pure presentation of auth state for every surface (B2-03): rows, status JSON,
//! statusline/hook payloads, notifications and one-click fix descriptors.
//! Token values never enter this module; it only sees `StatusFile`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::{json, Value};

use super::cache::{EdgeEvent, StatusFile, STATUS_VERSION};
use super::types::AuthState;

pub const NOTIFY_DEDUPE_SECS: i64 = 6 * 3600;
const WORST_MAX: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixAction {
    pub action: &'static str,
    pub server: String,
    pub label: String,
    pub command: Option<String>,
    pub ipc: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthRow {
    pub server: String,
    pub state: &'static str,
    pub reason: String,
    pub since: i64,
    pub expires_at: Option<i64>,
    pub ttl_secs: Option<i64>,
    pub last_probe: Option<i64>,
    pub fix: Option<FixAction>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AuthCounts {
    pub ok: u32,
    pub expiring: u32,
    pub needs_reauth: u32,
    pub revoked: u32,
    pub misconfigured: u32,
    pub unreachable: u32,
    pub unknown: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub server: String,
    pub state: &'static str,
    pub title: String,
    pub body: String,
    pub dedupe_key: String,
}

fn severity(state: &AuthState) -> u8 {
    match state {
        AuthState::Revoked => 0,
        AuthState::NeedsReauth => 1,
        AuthState::Misconfigured => 2,
        AuthState::Expiring { .. } => 3,
        AuthState::Unreachable => 4,
        AuthState::Unknown => 5,
        AuthState::Ok => 6,
    }
}

pub fn fix_action(server: &str, state: &AuthState) -> Option<FixAction> {
    let make = |action, label: String, command: Option<String>, ipc: Option<Value>| FixAction {
        action,
        server: server.to_string(),
        label,
        command,
        ipc,
    };
    match state {
        AuthState::Expiring { .. } | AuthState::NeedsReauth => Some(make(
            "reauth",
            format!("Sign in to {server} again"),
            Some(format!("toolportctl auth login {server}")),
            None,
        )),
        AuthState::Revoked => Some(make(
            "reconsent",
            format!("Re-consent access for {server}"),
            Some(format!("toolportctl auth login {server}")),
            None,
        )),
        AuthState::Misconfigured => Some(make(
            "fix_config",
            format!("Check the OAuth client configuration of {server}"),
            None,
            None,
        )),
        AuthState::Unreachable => Some(make(
            "retry",
            format!("Re-check {server}"),
            None,
            Some(json!({
                "command": "plus.auth.probe",
                "args": {"server": server, "force": true},
            })),
        )),
        AuthState::Unknown | AuthState::Ok => None,
    }
}

pub fn rows(status: &StatusFile, now: i64) -> Vec<AuthRow> {
    let mut rows: Vec<(u8, AuthRow)> = status
        .servers
        .iter()
        .map(|(server, entry)| {
            let state = entry.tracked.state;
            let expires_at = match state {
                AuthState::Expiring { eta } => Some(eta),
                _ => None,
            };
            (
                severity(&state),
                AuthRow {
                    server: server.clone(),
                    state: state.name(),
                    reason: entry.tracked.reason.clone(),
                    since: entry.tracked.since,
                    expires_at,
                    ttl_secs: expires_at.map(|eta| eta - now),
                    last_probe: entry.last_probe_at,
                    fix: fix_action(server, &state),
                },
            )
        })
        .collect();
    rows.sort_by(|a, b| (a.0, &a.1.server).cmp(&(b.0, &b.1.server)));
    rows.into_iter().map(|(_, row)| row).collect()
}

pub fn counts(rows: &[AuthRow]) -> AuthCounts {
    let mut counts = AuthCounts::default();
    for row in rows {
        match row.state {
            "ok" => counts.ok += 1,
            "expiring" => counts.expiring += 1,
            "needs_reauth" => counts.needs_reauth += 1,
            "revoked" => counts.revoked += 1,
            "misconfigured" => counts.misconfigured += 1,
            "unreachable" => counts.unreachable += 1,
            _ => counts.unknown += 1,
        }
    }
    counts
}

fn worst(rows: &[AuthRow]) -> Vec<String> {
    rows.iter()
        .filter(|r| r.fix.is_some())
        .take(WORST_MAX)
        .map(|r| r.server.clone())
        .collect()
}

pub fn rows_value(status: &StatusFile, now: i64) -> Value {
    let rows = rows(status, now);
    json!({"counts": counts(&rows), "rows": rows})
}

pub fn status_summary(status: &StatusFile, now: i64) -> Value {
    let rows = rows(status, now);
    json!({"counts": counts(&rows), "servers": rows})
}

fn summary_text(counts: &AuthCounts, worst: &[String]) -> String {
    let mut parts = Vec::new();
    for (n, label) in [
        (counts.revoked, "revoked"),
        (counts.needs_reauth, "need re-auth"),
        (counts.misconfigured, "misconfigured"),
        (counts.expiring, "expiring"),
        (counts.unreachable, "unreachable"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {label}"));
        }
    }
    if parts.is_empty() {
        return format!("auth ok ({})", counts.ok);
    }
    format!("auth: {} [{}]", parts.join(", "), worst.join(", "))
}

pub fn statusline(status: &StatusFile, now: i64) -> Value {
    let rows = rows(status, now);
    statusline_from(&counts(&rows), &worst(&rows))
}

fn statusline_from(counts: &AuthCounts, worst: &[String]) -> Value {
    json!({
        "auth": {
            "ok": counts.ok,
            "expiring": counts.expiring,
            "needs_reauth": counts.needs_reauth,
            "revoked": counts.revoked,
            "misconfigured": counts.misconfigured,
            "unreachable": counts.unreachable,
            "worst": worst,
            "text": summary_text(counts, worst),
        }
    })
}

pub fn hook(status: &StatusFile, now: i64) -> Value {
    let rows = rows(status, now);
    let counts = counts(&rows);
    let worst = worst(&rows);
    let mut value = statusline_from(&counts, &worst);
    let issues = rows.iter().any(|r| r.fix.is_some());
    if issues {
        let lines: Vec<String> = rows
            .iter()
            .filter_map(|r| {
                r.fix
                    .as_ref()
                    .map(|f| format!("{}: {} ({})", r.server, r.state, f.label))
            })
            .collect();
        value["hookSpecificOutput"] = json!({
            "hookEventName": "SessionStart",
            "additionalContext": format!(
                "{}\n{}",
                summary_text(&counts, &worst),
                lines.join("\n")
            ),
        });
    }
    value
}

pub fn notification_for(event: &EdgeEvent) -> Option<Notification> {
    let (state, title, body) = match event.to.as_str() {
        "needs_reauth" => (
            "needs_reauth",
            format!("{} needs a new login", event.server),
            format!(
                "{} can no longer authenticate ({}). Sign in again.",
                event.server, event.reason
            ),
        ),
        "expiring" => (
            "expiring",
            format!("{} login is expiring", event.server),
            format!(
                "The login for {} expires soon. Re-authenticate to avoid an interruption.",
                event.server
            ),
        ),
        _ => return None,
    };
    Some(Notification {
        server: event.server.clone(),
        state,
        title,
        body,
        dedupe_key: format!("{}:{state}", event.server),
    })
}

/// `sent` maps dedupe key to the time it was last notified.
pub fn plan_notifications(
    events: &[EdgeEvent],
    sent: &BTreeMap<String, i64>,
    now: i64,
    window_secs: i64,
) -> Vec<Notification> {
    let mut planned: Vec<Notification> = Vec::new();
    for event in events {
        let Some(note) = notification_for(event) else {
            continue;
        };
        if let Some(last) = sent.get(&note.dedupe_key) {
            if now - last < window_secs {
                continue;
            }
        }
        if planned.iter().any(|p| p.dedupe_key == note.dedupe_key) {
            continue;
        }
        planned.push(note);
    }
    planned
}

/// Lock-free and write-free: `status.json` is replaced atomically, so inspection
/// surfaces never create the auth directory or quarantine a bad file.
pub fn read_status(dir: &Path) -> StatusFile {
    crate::plus::jsonfs::read_json::<StatusFile>(&dir.join("status.json"))
        .filter(|status| status.version == STATUS_VERSION)
        .unwrap_or_default()
}

pub fn auth_dir() -> Option<std::path::PathBuf> {
    crate::registry::conduit_dir().map(|dir| dir.join("auth"))
}

pub fn rows_handler(_args: Value) -> Result<Value, String> {
    use super::probe::Clock;
    let dir = auth_dir().ok_or_else(|| "data directory unavailable".to_string())?;
    Ok(rows_value(&read_status(&dir), super::SystemClock.now()))
}
