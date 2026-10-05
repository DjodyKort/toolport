use std::collections::BTreeMap;

use serde_json::json;

use super::cache::{ProbeHintKind, ServerEntry};
use super::surfaces::*;
use super::*;

const NOW: i64 = 2_000_000;
const FAKE_TOKEN: &str = "FAKE-REFRESH-TOKEN-do-not-print-91c2";

fn entry(state: AuthState, reason: &str, last: Option<i64>) -> ServerEntry {
    entry_with_hint(state, reason, last, ProbeHintKind::OAuth, None)
}

fn entry_with_hint(
    state: AuthState,
    reason: &str,
    last: Option<i64>,
    hint_kind: ProbeHintKind,
    token_key: Option<&str>,
) -> ServerEntry {
    ServerEntry {
        tracked: Tracked {
            state,
            reason: reason.into(),
            since: NOW - 100,
            transient: None,
        },
        last_probe_at: last,
        next_due_at: NOW + 60,
        hint_kind,
        token_key: token_key.map(str::to_string),
    }
}

fn keys(status: &StatusFile) -> Vec<String> {
    status.servers.keys().cloned().collect()
}

fn sample() -> StatusFile {
    let mut status = StatusFile::default();
    for (name, e) in [
        ("alpha", entry(AuthState::Ok, "ok", Some(NOW - 5))),
        (
            "beta",
            entry(AuthState::NeedsReauth, "invalid_grant", Some(NOW - 10)),
        ),
        (
            "gamma",
            entry(
                AuthState::Expiring { eta: NOW + 600 },
                "token_expiring",
                Some(NOW - 20),
            ),
        ),
        ("delta", entry(AuthState::Revoked, "revoked", None)),
        (
            "epsilon",
            entry(AuthState::Misconfigured, "invalid_client", None),
        ),
        ("zeta", entry(AuthState::Unreachable, "unreachable", None)),
        ("eta", entry(AuthState::Unknown, "unknown", None)),
    ] {
        status.servers.insert(name.to_string(), e);
    }
    status
}

#[test]
fn rows_golden_sorted_by_severity_then_name() {
    let value = rows_value(&sample(), NOW);
    let names: Vec<&str> = value["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["server"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["delta", "beta", "epsilon", "gamma", "zeta", "eta", "alpha"]
    );
    assert_eq!(
        value["rows"][3],
        json!({
            "server": "gamma",
            "state": "expiring",
            "reason": "token_expiring",
            "since": NOW - 100,
            "expiresAt": NOW + 600,
            "ttlSecs": 600,
            "lastProbe": NOW - 20,
            "fix": {
                "action": "reauth",
                "server": "gamma",
                "label": "Sign in to gamma again",
                "command": "toolportctl auth login gamma",
                "ipc": null
            }
        })
    );
    assert_eq!(
        value["counts"],
        json!({"ok":1,"expiring":1,"needs_reauth":1,"revoked":1,"misconfigured":1,"unreachable":1,"unknown":1})
    );
}

#[test]
fn fix_descriptor_per_state() {
    let cases = [
        (AuthState::NeedsReauth, Some("reauth")),
        (AuthState::Expiring { eta: 1 }, Some("reauth")),
        (AuthState::Revoked, Some("reconsent")),
        (AuthState::Misconfigured, Some("fix_config")),
        (AuthState::Unreachable, Some("retry")),
        (AuthState::Ok, None),
        (AuthState::Unknown, None),
    ];
    for (state, expected) in cases {
        let fix = fix_action("alpha", &state, ProbeHintKind::OAuth, None);
        assert_eq!(fix.as_ref().map(|f| f.action), expected, "{state:?}");
    }
    let retry = fix_action("alpha", &AuthState::Unreachable, ProbeHintKind::OAuth, None).unwrap();
    assert_eq!(
        retry.ipc.unwrap(),
        json!({"command": "plus.auth.probe", "args": {"server": "alpha", "force": true}})
    );
}

#[test]
fn fix_descriptor_api_token_kind_points_at_secret_set_not_login() {
    for state in [
        AuthState::NeedsReauth,
        AuthState::Expiring { eta: 1 },
        AuthState::Revoked,
    ] {
        let fix = fix_action(
            "stitch",
            &state,
            ProbeHintKind::ApiToken,
            Some("STITCH_API_KEY"),
        )
        .unwrap();
        assert_eq!(fix.action, "fix_config", "{state:?}");
        assert_eq!(
            fix.command.as_deref(),
            Some(
                "toolportctl secret set stitch STITCH_API_KEY (value on stdin or --value-env <VAR>), then toolportctl auth probe --server stitch --force"
            ),
            "{state:?}"
        );
        assert!(!fix.label.contains("Sign in"), "{state:?}: {}", fix.label);
    }
    let no_key = fix_action("stitch", &AuthState::Revoked, ProbeHintKind::ApiToken, None).unwrap();
    assert!(no_key.command.unwrap().contains("secret set stitch <KEY>"));
    let oauth_unaffected =
        fix_action("stitch", &AuthState::NeedsReauth, ProbeHintKind::OAuth, None).unwrap();
    assert_eq!(
        oauth_unaffected.command.as_deref(),
        Some("toolportctl auth login stitch")
    );
}

#[test]
fn rows_value_api_token_row_shows_secret_set_not_login() {
    let mut status = StatusFile::default();
    status.servers.insert(
        "stitch".to_string(),
        entry_with_hint(
            AuthState::NeedsReauth,
            "invalid_grant",
            Some(NOW - 10),
            ProbeHintKind::ApiToken,
            Some("STITCH_API_KEY"),
        ),
    );
    let value = rows_value(&status, NOW);
    assert_eq!(
        value["rows"][0]["fix"],
        json!({
            "action": "fix_config",
            "server": "stitch",
            "label": "Set the API token for stitch",
            "command": "toolportctl secret set stitch STITCH_API_KEY (value on stdin or --value-env <VAR>), then toolportctl auth probe --server stitch --force",
            "ipc": null
        })
    );
}

#[test]
fn hook_context_uses_secret_set_label_for_api_token_kind() {
    let mut status = StatusFile::default();
    status.servers.insert(
        "stitch".to_string(),
        entry_with_hint(
            AuthState::Revoked,
            "invalid_grant",
            Some(NOW - 10),
            ProbeHintKind::ApiToken,
            Some("STITCH_API_KEY"),
        ),
    );
    let loud = hook(&status, NOW, &keys(&status));
    let context = loud["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("stitch: revoked (Set the API token for stitch)"));
    assert!(!context.contains("Sign in"));
    assert!(!context.contains("auth login"));
}

#[test]
fn statusline_is_identical_regardless_of_hint_kind() {
    let mut oauth_status = StatusFile::default();
    oauth_status.servers.insert(
        "stitch".to_string(),
        entry_with_hint(
            AuthState::NeedsReauth,
            "invalid_grant",
            Some(NOW - 10),
            ProbeHintKind::OAuth,
            None,
        ),
    );
    let mut api_token_status = StatusFile::default();
    api_token_status.servers.insert(
        "stitch".to_string(),
        entry_with_hint(
            AuthState::NeedsReauth,
            "invalid_grant",
            Some(NOW - 10),
            ProbeHintKind::ApiToken,
            Some("STITCH_API_KEY"),
        ),
    );
    assert_eq!(
        statusline(&oauth_status, NOW, &keys(&oauth_status)),
        statusline(&api_token_status, NOW, &keys(&api_token_status))
    );
}

#[test]
fn statusline_golden() {
    assert_eq!(
        statusline(&sample(), NOW, &keys(&sample())),
        json!({"auth": {
            "ok": 1, "expiring": 1, "needs_reauth": 1, "revoked": 1,
            "misconfigured": 1, "unreachable": 1,
            "worst": ["delta", "beta", "epsilon"],
            "text": "auth: 1 revoked, 1 need re-auth, 1 misconfigured, 1 expiring, 1 unreachable [delta, beta, epsilon]"
        }})
    );
}

#[test]
fn hook_is_quiet_when_healthy_and_adds_context_otherwise() {
    let mut healthy = StatusFile::default();
    healthy
        .servers
        .insert("alpha".into(), entry(AuthState::Ok, "ok", None));
    let quiet = hook(&healthy, NOW, &keys(&healthy));
    assert!(quiet.get("hookSpecificOutput").is_none());
    assert_eq!(quiet["auth"]["text"], "auth ok (1)");

    let loud = hook(&sample(), NOW, &keys(&sample()));
    assert_eq!(loud["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let context = loud["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("beta: needs_reauth (Sign in to beta again)"));
    assert!(!context.contains("alpha:"));
}

#[test]
fn hook_extends_the_statusline_without_changing_it() {
    let loud = hook(&sample(), NOW, &keys(&sample()));
    let mut stripped = loud.clone();
    stripped.as_object_mut().unwrap().remove("hookSpecificOutput");
    assert_eq!(stripped, statusline(&sample(), NOW, &keys(&sample())));
    let context = loud["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    assert_eq!(context.lines().next(), loud["auth"]["text"].as_str());

    let mut healthy = StatusFile::default();
    healthy
        .servers
        .insert("alpha".into(), entry(AuthState::Ok, "ok", None));
    let healthy_keys = keys(&healthy);
    assert_eq!(
        hook(&healthy, NOW, &healthy_keys),
        statusline(&healthy, NOW, &healthy_keys)
    );
    let empty = StatusFile::default();
    assert_eq!(hook(&empty, NOW, &[]), statusline(&empty, NOW, &[]));
}

#[test]
fn statusline_and_hook_count_exactly_the_registered_rows() {
    let status = sample();
    let registered: Vec<String> = ["alpha", "beta", "gamma", "ghost"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let shown = rows_for(&status, NOW, &registered);
    assert_eq!(shown.len(), 3, "a registered server without a cache entry has no row");

    let line = statusline(&status, NOW, &registered);
    let wanted = counts(&shown);
    assert_eq!(line["auth"]["ok"], wanted.ok);
    assert_eq!(line["auth"]["expiring"], wanted.expiring);
    assert_eq!(line["auth"]["needs_reauth"], wanted.needs_reauth);
    assert_eq!(line["auth"]["revoked"], 0);
    assert_eq!(line["auth"]["misconfigured"], 0);
    assert_eq!(line["auth"]["unreachable"], 0);
    assert_eq!(line["auth"]["worst"], json!(["beta", "gamma"]));
    assert_eq!(
        line["auth"]["text"],
        "auth: 1 need re-auth, 1 expiring [beta, gamma]"
    );

    let loud = hook(&status, NOW, &registered);
    let context = loud["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    for removed in ["delta", "epsilon", "zeta", "eta"] {
        let line = format!("{removed}:");
        assert!(
            !context.lines().any(|l| l.starts_with(&line)),
            "{removed} leaked into {context}"
        );
    }
    assert!(context.contains("beta: needs_reauth"));
}

#[test]
fn a_cached_failure_of_a_removed_server_is_neither_counted_nor_nagged() {
    let mut status = StatusFile::default();
    status
        .servers
        .insert("slack".into(), entry(AuthState::Ok, "ok", Some(NOW - 5)));
    status.servers.insert(
        "figma".into(),
        entry(AuthState::NeedsReauth, "no_token", Some(NOW - 5)),
    );
    let both = ["figma".to_string(), "slack".to_string()];
    assert_eq!(statusline(&status, NOW, &both)["auth"]["needs_reauth"], 1);

    let only_slack = ["slack".to_string()];
    let line = statusline(&status, NOW, &only_slack);
    assert_eq!(line["auth"]["needs_reauth"], 0);
    assert_eq!(line["auth"]["worst"], json!([]));
    assert_eq!(line["auth"]["text"], "auth ok (1)");
    assert_eq!(hook(&status, NOW, &only_slack), line);
}

fn edge(server: &str, to: &str) -> EdgeEvent {
    EdgeEvent {
        ts: NOW,
        server: server.into(),
        from: "ok".into(),
        to: to.into(),
        reason: "invalid_grant".into(),
    }
}

#[test]
fn notifications_only_for_reauth_and_expiring() {
    let events = [
        edge("beta", "needs_reauth"),
        edge("gamma", "expiring"),
        edge("delta", "revoked"),
        edge("alpha", "ok"),
    ];
    let planned = plan_notifications(&events, &BTreeMap::new(), NOW, NOTIFY_DEDUPE_SECS);
    assert_eq!(planned.len(), 2);
    assert_eq!(planned[0].dedupe_key, "beta:needs_reauth");
    assert_eq!(planned[0].title, "beta needs a new login");
    assert_eq!(planned[1].dedupe_key, "gamma:expiring");
}

#[test]
fn notification_dedupe_window_and_batch() {
    let events = [edge("beta", "needs_reauth"), edge("beta", "needs_reauth")];
    let planned = plan_notifications(&events, &BTreeMap::new(), NOW, NOTIFY_DEDUPE_SECS);
    assert_eq!(planned.len(), 1);

    let mut sent = BTreeMap::new();
    sent.insert("beta:needs_reauth".to_string(), NOW - 3600);
    assert!(plan_notifications(&events, &sent, NOW, NOTIFY_DEDUPE_SECS).is_empty());
    assert_eq!(
        plan_notifications(&events, &sent, NOW + NOTIFY_DEDUPE_SECS, NOTIFY_DEDUPE_SECS).len(),
        1
    );
}

#[test]
fn read_status_is_write_free_and_tolerates_garbage() {
    let dir = std::env::temp_dir().join(format!("auth-surfaces-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(read_status(&dir), StatusFile::default());
    assert!(!dir.exists());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("status.json"), "{nope").unwrap();
    assert_eq!(read_status(&dir), StatusFile::default());
    assert!(dir.join("status.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn surfaces_never_contain_token_values() {
    let mut status = sample();
    status.servers.get_mut("beta").unwrap().tracked.reason = "invalid_grant".into();
    let events = [edge("beta", "needs_reauth")];
    let mut all = vec![
        rows_value(&status, NOW).to_string(),
        status_summary(&status, NOW).to_string(),
        statusline(&status, NOW, &keys(&status)).to_string(),
        hook(&status, NOW, &keys(&status)).to_string(),
        serde_json::to_string(&plan_notifications(&events, &BTreeMap::new(), NOW, 1)).unwrap(),
    ];
    all.push(FAKE_TOKEN.len().to_string());
    for text in all {
        assert!(!text.contains("FAKE-"), "{text}");
    }
}

#[test]
fn rows_handler_reads_the_data_dir_override() {
    let _lock = crate::registry::data_dir_test_lock();
    let dir = std::env::temp_dir().join(format!("auth-rows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("auth")).unwrap();
    std::fs::write(
        dir.join("auth/status.json"),
        serde_json::to_string(&sample()).unwrap(),
    )
    .unwrap();
    let _guard = crate::registry::DataDirOverride::set(&dir);
    let value = crate::plus::dispatch("plus.auth.rows", json!({})).unwrap();
    assert_eq!(value["rows"].as_array().unwrap().len(), 7);
    assert_eq!(value["counts"]["revoked"], 1);
    drop(_guard);
    let _ = std::fs::remove_dir_all(&dir);
}
