use serde_json::json;

use super::stdio::{classify, find_consent_url, stdio_registry, Capture, HINT_KIND};
use super::testkit::{server, stdio_json, write_registry, CONSENT_URL};
use super::*;
use crate::registry::Registry;

fn cap(url: Option<&str>, exit: Option<i32>, tail: &[&str]) -> Capture {
    Capture {
        url: url.map(String::from),
        exit,
        timed_out: url.is_none() && exit.is_none(),
        tail: tail.iter().map(|line| line.to_string()).collect(),
    }
}

fn settle(outcome: &ProbeOutcome) -> (&'static str, String) {
    let tracked = step(&Tracked::unknown(0), outcome, 0);
    (tracked.state.name(), tracked.reason.clone())
}

#[test]
fn consent_urls_are_found_in_the_printed_line() {
    let cases: &[(&str, Option<&str>)] = &[
        (
            "Open https://auth.example.invalid/consent?state=abc to sign in",
            Some("https://auth.example.invalid/consent?state=abc"),
        ),
        ("visit (https://a.example/x).", Some("https://a.example/x")),
        (
            "http://localhost:8080/cb?code=1,",
            Some("http://localhost:8080/cb?code=1"),
        ),
        (
            "see http://a.example and https://b.example",
            Some("http://a.example"),
        ),
        (
            "https://b.example then http://a.example",
            Some("https://b.example"),
        ),
        (
            "quoted \"https://a.example/q\"",
            Some("https://a.example/q"),
        ),
        ("no address on this line", None),
        ("https://user:pw@example.invalid/x", None),
        ("ftp://example.invalid/file", None),
        ("https://", None),
        ("", None),
    ];
    for (line, expected) in cases {
        assert_eq!(find_consent_url(line).as_deref(), *expected, "{line}");
    }
}

#[test]
fn captures_classify_into_the_existing_state_model() {
    let signed_in = cap(None, Some(0), &["already signed in"]);
    let timeout = cap(None, None, &[]);
    let cases: Vec<(&str, Capture, &str, &str)> = vec![
        (
            "consent url",
            cap(Some(CONSENT_URL), None, &["Open the url to sign in"]),
            "needs_reauth",
            "not_authed",
        ),
        (
            "consent url after expiry",
            cap(Some(CONSENT_URL), None, &["Token expired, sign in again"]),
            "needs_reauth",
            "token_expired",
        ),
        (
            "consent url after revocation",
            cap(Some(CONSENT_URL), None, &["access REVOKED by the admin"]),
            "revoked",
            "token_revoked",
        ),
        ("clean exit", signed_in, "ok", "ok"),
        (
            "invalid grant",
            cap(None, Some(1), &["error: invalid_grant"]),
            "needs_reauth",
            "invalid_grant",
        ),
        (
            "invalid grant, revoked",
            cap(None, Some(1), &["invalid_grant: token has been revoked"]),
            "revoked",
            "revoked",
        ),
        (
            "failed, revoked",
            cap(None, Some(1), &["the grant was revoked"]),
            "revoked",
            "token_revoked",
        ),
        (
            "failed, expired",
            cap(None, Some(1), &["session expired"]),
            "needs_reauth",
            "token_expired",
        ),
        (
            "failed without a reason",
            cap(None, Some(3), &[]),
            "unknown",
            "",
        ),
        ("no output before the deadline", timeout, "unknown", ""),
    ];
    for (name, capture, state, reason) in cases {
        let (got_state, got_reason) = settle(&classify(&capture));
        assert_eq!(got_state, state, "{name}");
        if !reason.is_empty() {
            assert_eq!(got_reason, reason, "{name}");
        }
    }
}

#[test]
fn an_inconclusive_capture_is_transient_not_a_verdict() {
    for capture in [cap(None, Some(3), &[]), cap(None, None, &[])] {
        assert_eq!(classify(&capture), ProbeOutcome::TransportError);
    }
}

fn registry_of(servers: serde_json::Value, enabled: &[&str]) -> Registry {
    serde_json::from_value(json!({
        "version": 1,
        "servers": servers,
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": enabled}],
        "activeProfileId": "default",
    }))
    .unwrap()
}

#[test]
fn the_stdio_probe_is_opt_in_per_server() {
    let command = std::path::Path::new("acme-mcp");
    let mut remote = stdio_json("remote", command, "consent", vec![], true);
    remote["transport"] = json!("http");
    remote["url"] = json!("https://example.invalid/mcp");
    remote["command"] = json!(null);
    let mut other = stdio_json("other-kind", command, "consent", vec![], false);
    other["plus"] = json!({"authProbe": {"kind": "none"}});
    let registry = registry_of(
        json!([
            stdio_json("hinted", command, "consent", vec![], true),
            stdio_json("plain", command, "consent", vec![], false),
            remote,
            other,
        ]),
        &[],
    );
    let reg = stdio_registry(&registry);
    let ids: Vec<&str> = reg.iter().map(|spec| spec.server.as_str()).collect();
    assert_eq!(ids, ["hinted"]);
    let spec = reg.get("hinted").unwrap();
    assert_eq!(spec.kind, ProbeKind::Stdio);
    assert_eq!(spec.min_interval_secs, 30 * 60);
    assert_eq!(HINT_KIND, "stdio");
}

#[test]
fn a_team_command_is_not_probed_before_it_is_reviewed() {
    let command = std::path::Path::new("acme-mcp");
    let mut team = stdio_json("shared", command, "consent", vec![], true);
    team["source"] = json!("team:acme");
    let servers = json!([team]);
    assert!(server(servers[0].clone()).needs_team_enable_review());
    assert_eq!(
        stdio_registry(&registry_of(servers.clone(), &[]))
            .iter()
            .count(),
        0
    );
    let enabled = registry_of(servers, &["shared"]);
    assert_eq!(stdio_registry(&enabled).iter().count(), 1);
}

#[test]
fn the_stdio_hint_wins_over_token_inference() {
    let command = std::path::Path::new("acme-mcp");
    let slack_env = vec![super::testkit::env_var("SLACK_BOT_TOKEN", "", true)];
    let inferred = stdio_json("chat", command, "consent", slack_env.clone(), false);
    let hinted = stdio_json("chat", command, "consent", slack_env, true);
    let plain = combined_registry(&registry_of(json!([inferred]), &[]));
    assert_eq!(plain.get("chat").unwrap().kind, ProbeKind::Http);
    let chosen = combined_registry(&registry_of(json!([hinted]), &[]));
    assert_eq!(chosen.get("chat").unwrap().kind, ProbeKind::Stdio);
}

#[cfg(unix)]
mod launch {
    use std::path::Path;
    use std::time::Duration;

    use super::*;
    use crate::plus::auth::stdio::{start, LaunchFault, StdioProbe};
    use crate::plus::auth::testkit::{env_var, mock, wait_gone, with_world, LEAKY};

    fn probe_outcome(id: &str, wait: Duration) -> ProbeOutcome {
        StdioProbe::default()
            .with_wait(wait)
            .run(&ProbeSpec::new(id, ProbeKind::Stdio))
    }

    #[test]
    fn a_mock_server_printing_a_consent_url_needs_auth_with_that_url() {
        with_world("stdio-consent", |world| {
            let entry = server(mock(world, "acme", "consent"));
            let mut session = start(&entry).unwrap();
            let capture = session.wait_url(Duration::from_secs(10));
            assert_eq!(capture.url.as_deref(), Some(CONSENT_URL));
            assert_eq!(capture.exit, None);
            assert!(!capture.timed_out);
            assert_eq!(
                settle(&classify(&capture)),
                ("needs_reauth", "not_authed".to_string())
            );
            session.kill();
            assert!(wait_gone(&world.path("pid")));
        });
    }

    #[test]
    fn the_probe_kills_the_whole_process_group_after_the_url() {
        with_world("group", |world| {
            write_registry(vec![mock(world, "acme", "expired")], &[]);
            let outcome = probe_outcome("acme", Duration::from_secs(10));
            assert_eq!(
                settle(&outcome),
                ("needs_reauth", "token_expired".to_string())
            );
            assert!(wait_gone(&world.path("pid")), "the server is still running");
            assert!(
                wait_gone(&world.path("child")),
                "its child is still running"
            );
        });
    }

    #[test]
    fn exit_codes_and_silence_map_onto_states() {
        let cases = [
            ("signed_in", "ok"),
            ("revoked", "revoked"),
            ("crash", "unknown"),
        ];
        for (mode, state) in cases {
            with_world(mode, |world| {
                write_registry(vec![mock(world, "acme", mode)], &[]);
                let outcome = probe_outcome("acme", Duration::from_secs(10));
                assert_eq!(settle(&outcome).0, state, "{mode}");
            });
        }
        with_world("silent", |world| {
            write_registry(vec![mock(world, "acme", "silent")], &[]);
            let outcome = probe_outcome("acme", Duration::from_millis(300));
            assert_eq!(outcome, ProbeOutcome::TransportError);
            assert!(wait_gone(&world.path("pid")));
        });
    }

    #[test]
    fn a_server_that_is_gone_or_refused_is_classified_without_spawning() {
        with_world("refused", |world| {
            let mut sudo = mock(world, "wrapped", "consent");
            sudo["command"] = json!("sudo");
            let mut missing = mock(world, "missing", "consent");
            missing["command"] = json!(world.path("not-installed").to_string_lossy());
            let mut remote = mock(world, "remote", "consent");
            remote["transport"] = json!("http");
            remote["url"] = json!("https://example.invalid/mcp");
            remote["command"] = json!(null);
            write_registry(vec![sudo.clone(), missing.clone(), remote.clone()], &[]);

            let refused = start(&server(sudo)).err().unwrap();
            assert_eq!(refused.fault, LaunchFault::Refused);
            let spawn = start(&server(missing)).err().unwrap();
            assert_eq!(spawn.fault, LaunchFault::Spawn);
            let unsupported = start(&server(remote)).err().unwrap();
            assert_eq!(unsupported.fault, LaunchFault::Unsupported);

            let wait = Duration::from_secs(5);
            assert_eq!(settle(&probe_outcome("wrapped", wait)).0, "misconfigured");
            assert_eq!(settle(&probe_outcome("remote", wait)).0, "misconfigured");
            assert_eq!(probe_outcome("missing", wait), ProbeOutcome::TransportError);
            assert_eq!(settle(&probe_outcome("absent", wait)).0, "misconfigured");
        });
    }

    #[test]
    fn launch_secrets_never_reach_the_capture_or_the_status_cache() {
        with_world("leak", |world| {
            let mut value = mock(world, "acme", "leak");
            value["env"]
                .as_array_mut()
                .unwrap()
                .push(env_var("MOCK_SECRET", "", true));
            crate::secrets::set_secret("acme", "MOCK_SECRET", LEAKY).unwrap();
            write_registry(vec![value.clone()], &[]);

            let mut session = start(&server(value)).unwrap();
            let capture = session.wait_url(Duration::from_secs(10));
            session.kill();
            assert_eq!(capture.url.as_deref(), Some(CONSENT_URL));
            let tail = capture.tail.join("\n");
            assert!(tail.contains("using key <redacted>"), "{tail}");
            assert!(!tail.contains(LEAKY), "{tail}");
            assert!(tail.contains("vault=unset"), "{tail}");
            assert!(!tail.contains("publisher-synthetic-vault"), "{tail}");

            let prober = crate::plus::auth::scan::default_prober().unwrap();
            let run = crate::plus::auth::scan::run(
                &prober,
                &crate::plus::auth::scan::Selector::One("acme".into()),
                true,
                1,
            )
            .unwrap();
            assert_eq!(run.reports[0].tracked.state.name(), "needs_reauth");
            let mut on_disk = String::new();
            let data = crate::registry::conduit_dir().unwrap();
            gather(&data.join("auth"), &mut on_disk);
            assert!(!on_disk.is_empty());
            assert!(!on_disk.contains(LEAKY), "{on_disk}");
            assert!(!format!("{run:?}").contains(LEAKY));
        });
    }

    fn gather(dir: &Path, out: &mut String) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                gather(&path, out);
            } else if let Ok(bytes) = std::fs::read(&path) {
                out.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }

    #[test]
    fn a_launcher_server_keeps_its_bound_secrets_out_of_the_capture() {
        with_world("launcher", |world| {
            let mut value = mock(world, "acme", "leak");
            value["args"] = json!(["--token", "<launch-input>"]);
            value["launch"] = json!({
                "inputs": [{"key": "API_TOKEN", "label": "API token", "secret": true}],
                "bindings": [{"index": 1, "parts": [{"kind": "input", "key": "API_TOKEN"}]}],
            });
            crate::secrets::set_secret("acme", "API_TOKEN", LEAKY).unwrap();

            let mut session = start(&server(value)).unwrap();
            let capture = session.wait_url(Duration::from_secs(10));
            session.kill();
            assert_eq!(capture.url.as_deref(), Some(CONSENT_URL));
            assert!(capture.tail.is_empty(), "{:?}", capture.tail);
            assert!(!format!("{capture:?}").contains(LEAKY));
        });
    }

    #[test]
    fn a_url_that_echoes_a_bound_secret_is_held_back() {
        with_world("bound-url", |world| {
            let mut value = mock(world, "acme", "bound_url");
            value["args"] = json!(["--token", "<launch-input>"]);
            value["launch"] = json!({
                "inputs": [{"key": "API_TOKEN", "label": "API token", "secret": true}],
                "bindings": [{"index": 1, "parts": [{"kind": "input", "key": "API_TOKEN"}]}],
            });
            crate::secrets::set_secret("acme", "API_TOKEN", LEAKY).unwrap();

            let mut session = start(&server(value)).unwrap();
            let capture = session.wait_url(Duration::from_millis(500));
            session.kill();
            assert_eq!(capture.url, None);
            assert!(capture.tail.is_empty());
            assert_eq!(classify(&capture), ProbeOutcome::TransportError);
            assert!(!format!("{capture:?}").contains(LEAKY));
        });
    }
}
