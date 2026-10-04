use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::*;

const T0: i64 = 1_000_000;
const MIN: i64 = 60;

fn oauth(code: &str, description: &str) -> ProbeOutcome {
    ProbeOutcome::OauthError {
        code: code.into(),
        description: description.into(),
    }
}

fn tracked(state: AuthState, reason: &str) -> Tracked {
    Tracked {
        state,
        reason: reason.into(),
        since: T0,
        transient: None,
    }
}

fn ok() -> Tracked {
    tracked(AuthState::Ok, "ok")
}

#[test]
fn classify_uses_oauth_code_not_http_status() {
    use Classification::*;
    assert_eq!(
        classify(&oauth("invalid_grant", "Token has been expired")),
        Definitive {
            state: AuthState::NeedsReauth,
            reason: "invalid_grant"
        }
    );
    assert_eq!(
        classify(&oauth(
            "invalid_grant",
            "Token has been expired or REVOKED."
        )),
        Definitive {
            state: AuthState::Revoked,
            reason: "revoked"
        }
    );
    for (code, reason) in [
        ("invalid_client", "invalid_client"),
        ("deleted_client", "deleted_client"),
        ("unauthorized_client", "unauthorized_client"),
    ] {
        assert_eq!(
            classify(&oauth(code, "")),
            Definitive {
                state: AuthState::Misconfigured,
                reason
            }
        );
    }
    assert_eq!(classify(&oauth("temporarily_unavailable", "")), Transient);
    assert_eq!(classify(&oauth("server_error", "")), Transient);
    assert_eq!(classify(&oauth("something_new", "")), Transient);
    assert_eq!(
        classify(&oauth(" Invalid_Grant ", "")).clone(),
        Definitive {
            state: AuthState::NeedsReauth,
            reason: "invalid_grant"
        }
    );
}

#[test]
fn classify_http_and_transport() {
    assert_eq!(
        classify(&ProbeOutcome::HttpStatus { status: 200 }),
        Classification::Healthy
    );
    for status in [400, 401, 403, 429, 500, 503] {
        assert_eq!(
            classify(&ProbeOutcome::HttpStatus { status }),
            Classification::Transient,
            "{status}"
        );
    }
    assert_eq!(
        classify(&ProbeOutcome::TransportError),
        Classification::Transient
    );
    assert_eq!(classify(&ProbeOutcome::Success), Classification::Healthy);
    assert_eq!(
        classify(&ProbeOutcome::FileChanged),
        Classification::Healthy
    );
}

#[test]
fn unknown_to_ok_on_success() {
    let next = step(&Tracked::unknown(T0), &ProbeOutcome::Success, T0 + 5);
    assert_eq!(next.state, AuthState::Ok);
    assert_eq!(next.since, T0 + 5);
}

#[test]
fn ok_stays_ok_and_keeps_since() {
    let next = step(&ok(), &ProbeOutcome::Success, T0 + 100);
    assert_eq!(next, ok());
}

#[test]
fn ttl_transitions() {
    let near = step(&ok(), &ProbeOutcome::TokenTtl { secs: 3600 }, T0 + 10);
    assert_eq!(near.state, AuthState::Expiring { eta: T0 + 3610 });
    assert_eq!(near.since, T0 + 10);

    let again = step(&near, &ProbeOutcome::TokenTtl { secs: 3000 }, T0 + 700);
    assert_eq!(again.state, AuthState::Expiring { eta: T0 + 3700 });
    assert_eq!(again.since, near.since);

    let far = step(&near, &ProbeOutcome::TokenTtl { secs: 90_000 }, T0 + 20);
    assert_eq!(far.state, AuthState::Ok);

    let edge = step(
        &ok(),
        &ProbeOutcome::TokenTtl {
            secs: EXPIRING_WINDOW_SECS,
        },
        T0,
    );
    assert!(matches!(edge.state, AuthState::Expiring { .. }));
    let beyond = step(
        &ok(),
        &ProbeOutcome::TokenTtl {
            secs: EXPIRING_WINDOW_SECS + 1,
        },
        T0,
    );
    assert_eq!(beyond.state, AuthState::Ok);

    let dead = step(&near, &ProbeOutcome::TokenTtl { secs: 0 }, T0 + 99);
    assert_eq!(dead.state, AuthState::NeedsReauth);
    assert_eq!(dead.reason, "token_expired");
}

#[test]
fn expiring_becomes_needs_reauth_when_eta_passes() {
    let expiring = tracked(AuthState::Expiring { eta: T0 + 100 }, "token_expiring");
    let before = step(&expiring, &ProbeOutcome::TransportError, T0 + 99);
    assert_eq!(before.state, AuthState::Expiring { eta: T0 + 100 });
    let after = step(&expiring, &ProbeOutcome::TransportError, T0 + 100);
    assert_eq!(after.state, AuthState::NeedsReauth);
    assert_eq!(after.since, T0 + 100);
}

#[test]
fn definitive_errors_from_any_state() {
    let states = [
        Tracked::unknown(T0),
        ok(),
        tracked(AuthState::Expiring { eta: T0 + 5 }, "token_expiring"),
        tracked(AuthState::NeedsReauth, "invalid_grant"),
        tracked(AuthState::Unreachable, "transient_failures"),
    ];
    for prev in &states {
        let revoked = step(prev, &oauth("invalid_grant", "revoked"), T0 + 7);
        assert_eq!(revoked.state, AuthState::Revoked);
        let reauth = step(prev, &oauth("invalid_grant", "expired"), T0 + 7);
        assert_eq!(reauth.state, AuthState::NeedsReauth);
        let mis = step(prev, &oauth("deleted_client", ""), T0 + 7);
        assert_eq!(mis.state, AuthState::Misconfigured);
        assert_eq!(mis.reason, "deleted_client");
        assert!(mis.transient.is_none());
    }
}

#[test]
fn definitive_same_state_keeps_since_but_new_reason_resets() {
    let prev = tracked(AuthState::Misconfigured, "invalid_client");
    let same = step(&prev, &oauth("invalid_client", ""), T0 + 50);
    assert_eq!(same.since, T0);
    let other = step(&prev, &oauth("deleted_client", ""), T0 + 50);
    assert_eq!(other.since, T0 + 50);
}

fn fail_at(prev: &Tracked, offsets_min: &[i64]) -> Tracked {
    let mut current = prev.clone();
    for m in offsets_min {
        current = step(&current, &ProbeOutcome::TransportError, T0 + m * MIN);
    }
    current
}

#[test]
fn unreachable_needs_three_failures_over_thirty_minutes() {
    let after_two = fail_at(&ok(), &[0, 60]);
    assert_eq!(after_two.state, AuthState::Ok);
    assert_eq!(after_two.transient.unwrap().count, 2);

    let exactly_three_late = fail_at(&ok(), &[0, 15, 30]);
    assert_eq!(exactly_three_late.state, AuthState::Unreachable);
    assert_eq!(exactly_three_late.reason, "transient_failures");
    assert_eq!(exactly_three_late.since, T0 + 30 * MIN);

    let just_short = step(
        &fail_at(&ok(), &[0, 15]),
        &ProbeOutcome::TransportError,
        T0 + 30 * MIN - 1,
    );
    assert_eq!(just_short.state, AuthState::Ok);

    let twenty_nine = fail_at(&ok(), &[0, 10, 29]);
    assert_eq!(twenty_nine.state, AuthState::Ok);
    assert_eq!(twenty_nine.transient.unwrap().count, 3);

    let fourth = step(&twenty_nine, &ProbeOutcome::TransportError, T0 + 30 * MIN);
    assert_eq!(fourth.state, AuthState::Unreachable);
}

#[test]
fn unreachable_from_any_state_and_stays() {
    for prev in [
        Tracked::unknown(T0),
        ok(),
        tracked(AuthState::NeedsReauth, "invalid_grant"),
        tracked(AuthState::Misconfigured, "invalid_client"),
    ] {
        let next = fail_at(&prev, &[0, 20, 40]);
        assert_eq!(next.state, AuthState::Unreachable);
        let more = step(
            &next,
            &ProbeOutcome::HttpStatus { status: 503 },
            T0 + 50 * MIN,
        );
        assert_eq!(more.state, AuthState::Unreachable);
        assert_eq!(more.since, next.since);
    }
}

#[test]
fn transient_failures_do_not_change_non_expiring_state() {
    let prev = tracked(AuthState::NeedsReauth, "invalid_grant");
    let next = step(&prev, &ProbeOutcome::TransportError, T0 + 1);
    assert_eq!(next.state, AuthState::NeedsReauth);
    assert_eq!(
        next.transient.unwrap(),
        TransientRun {
            count: 1,
            first_at: T0 + 1
        }
    );
}

#[test]
fn recovery_on_success_file_change_and_healthy_http() {
    let down = fail_at(&ok(), &[0, 20, 40]);
    assert_eq!(down.state, AuthState::Unreachable);
    for outcome in [
        ProbeOutcome::Success,
        ProbeOutcome::FileChanged,
        ProbeOutcome::HttpStatus { status: 204 },
    ] {
        let up = step(&down, &outcome, T0 + 41 * MIN);
        assert_eq!(up.state, AuthState::Ok);
        assert!(up.transient.is_none());
        assert_eq!(up.since, T0 + 41 * MIN);
    }
    for stuck in [
        tracked(AuthState::NeedsReauth, "invalid_grant"),
        tracked(AuthState::Revoked, "revoked"),
        tracked(AuthState::Misconfigured, "invalid_client"),
    ] {
        assert_eq!(
            step(&stuck, &ProbeOutcome::FileChanged, T0 + 9).state,
            AuthState::Ok
        );
        assert_eq!(
            step(&stuck, &ProbeOutcome::Success, T0 + 9).state,
            AuthState::Ok
        );
    }
}

#[test]
fn success_resets_the_failure_window() {
    let two = fail_at(&ok(), &[0, 10]);
    let healed = step(&two, &ProbeOutcome::Success, T0 + 11 * MIN);
    assert!(healed.transient.is_none());
    let fresh = step(&healed, &ProbeOutcome::TransportError, T0 + 60 * MIN);
    assert_eq!(fresh.transient.unwrap().first_at, T0 + 60 * MIN);
    assert_eq!(fresh.state, AuthState::Ok);
}

#[test]
fn definitive_error_resets_the_failure_window() {
    let two = fail_at(&ok(), &[0, 10]);
    let reauth = step(&two, &oauth("invalid_grant", ""), T0 + 11 * MIN);
    assert!(reauth.transient.is_none());
}

#[test]
fn issues_are_sorted_with_stable_ids() {
    let mut map = BTreeMap::new();
    map.insert("slack".to_string(), ok());
    map.insert("odoo".to_string(), Tracked::unknown(T0));
    map.insert(
        "google-b".to_string(),
        tracked(AuthState::NeedsReauth, "invalid_grant"),
    );
    map.insert(
        "google-a".to_string(),
        tracked(AuthState::Revoked, "revoked"),
    );
    map.insert(
        "miro".to_string(),
        tracked(AuthState::Expiring { eta: T0 + 9 }, "token_expiring"),
    );
    map.insert(
        "stitch".to_string(),
        tracked(AuthState::Unreachable, "transient_failures"),
    );
    map.insert(
        "fig".to_string(),
        tracked(AuthState::Misconfigured, "invalid_client"),
    );
    let issues = compute_issues(&map);
    let ids: Vec<&str> = issues.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "fig:misconfigured:invalid_client",
            "google-a:revoked:revoked",
            "google-b:needs_reauth:invalid_grant",
            "miro:expiring:token_expiring",
            "stitch:unreachable:transient_failures",
        ]
    );
    assert!(issues.iter().all(|i| !i.fix_hint.is_empty()));
    assert_eq!(compute_issues(&map), issues);
    assert!(compute_issues(&BTreeMap::new()).is_empty());
}

#[test]
fn issue_id_is_stable_while_state_persists() {
    let mut map = BTreeMap::new();
    let mut t = step(&Tracked::unknown(T0), &oauth("invalid_grant", ""), T0);
    map.insert("g".to_string(), t.clone());
    let first = compute_issues(&map);
    t = step(&t, &oauth("invalid_grant", ""), T0 + 999);
    map.insert("g".to_string(), t);
    assert_eq!(compute_issues(&map), first);
}

#[test]
fn auth_state_serde_goldens() {
    let cases = [
        (AuthState::Unknown, json!({"state": "unknown"})),
        (AuthState::Ok, json!({"state": "ok"})),
        (
            AuthState::Expiring { eta: 42 },
            json!({"state": "expiring", "eta": 42}),
        ),
        (AuthState::NeedsReauth, json!({"state": "needs_reauth"})),
        (AuthState::Revoked, json!({"state": "revoked"})),
        (AuthState::Misconfigured, json!({"state": "misconfigured"})),
        (AuthState::Unreachable, json!({"state": "unreachable"})),
    ];
    for (state, golden) in cases {
        assert_eq!(serde_json::to_value(state).unwrap(), golden);
        let back: AuthState = serde_json::from_value(golden).unwrap();
        assert_eq!(back, state);
        assert!(!state.name().is_empty());
    }
}

#[test]
fn auth_kind_names_match_the_state_wire_names_and_the_typescript_union() {
    let states = [
        AuthState::Unknown,
        AuthState::Ok,
        AuthState::Expiring { eta: 1 },
        AuthState::NeedsReauth,
        AuthState::Revoked,
        AuthState::Misconfigured,
        AuthState::Unreachable,
    ];
    let mut wire = Vec::new();
    for state in states {
        let kind = state.kind();
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        assert_eq!(serde_json::to_value(state).unwrap()["state"], kind.as_str());
        assert_eq!(AuthKind::parse(kind.as_str()), Some(kind));
        assert_eq!(kind.to_string(), state.name());
        wire.push(kind.as_str());
    }
    assert_eq!(AuthKind::parse("bogus"), None);
    let api = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/plus/api.ts"))
        .unwrap();
    let union = api
        .split("export type AuthStateName =")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .unwrap();
    let mut declared: Vec<&str> = union
        .split('|')
        .map(|n| n.trim().trim_matches('"'))
        .filter(|n| !n.is_empty())
        .collect();
    declared.sort_unstable();
    wire.sort_unstable();
    assert_eq!(declared, wire);
}

#[test]
fn probe_outcome_serde_goldens() {
    let cases = [
        (ProbeOutcome::Success, json!({"kind": "success"})),
        (
            oauth("invalid_grant", "revoked"),
            json!({"kind": "oauth_error", "code": "invalid_grant", "description": "revoked"}),
        ),
        (
            ProbeOutcome::HttpStatus { status: 503 },
            json!({"kind": "http_status", "status": 503}),
        ),
        (
            ProbeOutcome::TransportError,
            json!({"kind": "transport_error"}),
        ),
        (ProbeOutcome::FileChanged, json!({"kind": "file_changed"})),
        (
            ProbeOutcome::TokenTtl { secs: 60 },
            json!({"kind": "token_ttl", "secs": 60}),
        ),
    ];
    for (outcome, golden) in cases {
        assert_eq!(serde_json::to_value(&outcome).unwrap(), golden);
        let back: ProbeOutcome = serde_json::from_value(golden).unwrap();
        assert_eq!(back, outcome);
    }
}

#[test]
fn tracked_and_issue_serde_goldens() {
    let t = Tracked {
        state: AuthState::Expiring { eta: 7 },
        reason: "token_expiring".into(),
        since: 3,
        transient: Some(TransientRun {
            count: 2,
            first_at: 1,
        }),
    };
    let golden = json!({
        "state": "expiring", "eta": 7, "reason": "token_expiring", "since": 3,
        "transient": {"count": 2, "first_at": 1}
    });
    assert_eq!(serde_json::to_value(&t).unwrap(), golden);
    assert_eq!(serde_json::from_value::<Tracked>(golden).unwrap(), t);

    let mut map = BTreeMap::new();
    map.insert(
        "google".to_string(),
        tracked(AuthState::NeedsReauth, "invalid_grant"),
    );
    let issues = compute_issues(&map);
    let golden = json!([{
        "id": "google:needs_reauth:invalid_grant",
        "server": "google",
        "state": "needs_reauth",
        "reason": "invalid_grant",
        "since": T0,
        "fix_hint": "Sign in to google again."
    }]);
    assert_eq!(serde_json::to_value(&issues).unwrap(), golden);
    assert_eq!(
        serde_json::from_value::<Vec<AuthIssue>>(golden).unwrap(),
        issues
    );
}

fn assert_no_secret_keys(value: &Value) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let lower = key.to_ascii_lowercase();
                assert!(
                    !lower.contains("token") && !lower.contains("secret"),
                    "field {key}"
                );
                assert_no_secret_keys(inner);
            }
        }
        Value::Array(items) => items.iter().for_each(assert_no_secret_keys),
        _ => {}
    }
}

#[test]
fn no_type_carries_tokens_or_secrets() {
    let tracked_all = [
        tracked(AuthState::Expiring { eta: 1 }, "token_expiring"),
        tracked(AuthState::Revoked, "revoked"),
    ];
    let outcomes = [
        ProbeOutcome::Success,
        oauth("invalid_grant", "d"),
        ProbeOutcome::HttpStatus { status: 500 },
        ProbeOutcome::TransportError,
        ProbeOutcome::FileChanged,
        ProbeOutcome::TokenTtl { secs: 1 },
    ];
    let map: BTreeMap<String, Tracked> = tracked_all
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, t)| (format!("s{i}"), t))
        .collect();
    let issues = compute_issues(&map);

    let mut dumps = vec![
        serde_json::to_value(&tracked_all).unwrap(),
        serde_json::to_value(&outcomes).unwrap(),
        serde_json::to_value(&issues).unwrap(),
    ];
    dumps.push(serde_json::to_value(AuthState::Unknown).unwrap());
    dumps.iter().for_each(assert_no_secret_keys);

    let debug = format!("{tracked_all:?}{outcomes:?}{issues:?}").to_ascii_lowercase();
    for needle in [
        "token:",
        "secret:",
        "token =",
        "secret =",
        "access_token",
        "refresh_token",
    ] {
        assert!(!debug.contains(needle), "{needle}");
    }
}
