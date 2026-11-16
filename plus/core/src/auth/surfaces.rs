//! Pure presentation of login state for every surface: rows, the status summary, the statusline
//! and hook payloads and the one-step fix of a row. Token values never enter this module; it only
//! sees the status file.

use serde::Serialize;
use serde_json::{json, Value};

use super::store::StatusFile;
use super::types::{AuthKind, AuthState};

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
    pub state: AuthKind,
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
            Some(format!("toolportctl auth probe --server {server} --force")),
            None,
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
                    state: state.kind(),
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

/// The rows of the servers that still have a probe: a cached state of a server whose probe is
/// gone (removed, switched off, no longer opted in) is not shown.
pub fn rows_for(status: &StatusFile, now: i64, servers: &[String]) -> Vec<AuthRow> {
    let mut rows = rows(status, now);
    rows.retain(|row| servers.contains(&row.server));
    rows
}

pub fn counts(rows: &[AuthRow]) -> AuthCounts {
    let mut counts = AuthCounts::default();
    for row in rows {
        match row.state {
            AuthKind::Ok => counts.ok += 1,
            AuthKind::Expiring => counts.expiring += 1,
            AuthKind::NeedsReauth => counts.needs_reauth += 1,
            AuthKind::Revoked => counts.revoked += 1,
            AuthKind::Misconfigured => counts.misconfigured += 1,
            AuthKind::Unreachable => counts.unreachable += 1,
            AuthKind::Unknown => counts.unknown += 1,
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

pub fn status_summary(status: &StatusFile, now: i64, servers: &[String]) -> Value {
    let rows = rows_for(status, now, servers);
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

pub fn statusline(status: &StatusFile, now: i64, servers: &[String]) -> Value {
    let rows = rows_for(status, now, servers);
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

pub fn hook(status: &StatusFile, now: i64, servers: &[String]) -> Value {
    let rows = rows_for(status, now, servers);
    let counts = counts(&rows);
    let worst = worst(&rows);
    let mut value = statusline_from(&counts, &worst);
    if rows.iter().any(|r| r.fix.is_some()) {
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
