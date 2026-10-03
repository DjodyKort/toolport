use serde_json::json;

use super::http_probes::PARAM_TOKEN_KEY;
use super::login::{plan, review_gate, Plan, RemoteFacts};
use super::testkit::{server, stdio_json};
use super::*;
use crate::registry::{Registry, ServerEntry};

fn remote(id: &str, extra: serde_json::Value) -> ServerEntry {
    let mut value = json!({
        "id": id,
        "name": id,
        "transport": "http",
        "url": "https://mcp.example.test/mcp",
    });
    if let (Some(target), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
        target.extend(extra.clone());
    }
    server(value)
}

fn facts(oauth_state: bool, static_token: bool, detected: &str) -> RemoteFacts {
    RemoteFacts {
        oauth_state,
        static_token,
        detected: detected.into(),
    }
}

fn unsupported(plan: Plan) -> (String, String) {
    match plan {
        Plan::Unsupported { reason, next } => (reason, next),
        other => panic!("expected an unsupported plan, got {other:?}"),
    }
}

#[test]
fn the_plan_picks_the_flow_each_server_can_use() {
    let command = std::path::Path::new("acme-mcp");
    let stdio = server(stdio_json("local", command, "consent", vec![], false));
    let mut no_command = stdio.clone();
    no_command.command = None;
    let oauth_remote = remote("figma", json!({}));
    let credentials = remote(
        "headless",
        json!({"clientCredentials": {"clientId": "acme-client"}}),
    );
    let never = || -> RemoteFacts { panic!("only remote servers are inspected") };

    assert_eq!(plan(&stdio, None, never), Plan::Stdio);
    assert_eq!(
        plan(&oauth_remote, None, || facts(true, false, "")),
        Plan::Browser
    );
    assert_eq!(
        plan(&oauth_remote, None, || facts(false, false, "oauth")),
        Plan::Browser
    );
    assert_eq!(
        plan(&oauth_remote, None, || facts(false, false, "")),
        Plan::Browser
    );

    let (reason, next) = unsupported(plan(&oauth_remote, None, || facts(false, true, "")));
    assert!(reason.contains("static token"), "{reason}");
    assert!(
        next.contains("then run toolportctl auth probe --server figma --force"),
        "{next}"
    );

    let (reason, _) = unsupported(plan(&oauth_remote, None, || facts(false, false, "token")));
    assert!(reason.contains("API token"), "{reason}");

    let (reason, next) = unsupported(plan(&oauth_remote, None, || facts(false, false, "none")));
    assert!(reason.contains("does not ask for a sign-in"), "{reason}");
    assert!(next.starts_with("toolportctl server info figma"), "{next}");

    let (reason, _) = unsupported(plan(&credentials, None, never));
    assert!(reason.contains("client credentials"), "{reason}");

    let (reason, next) = unsupported(plan(&no_command, None, never));
    assert!(reason.contains("has no sign-in flow"), "{reason}");
    assert_eq!(next, "toolportctl server info local");
}

#[test]
fn an_api_token_server_is_pointed_at_the_secret_command() {
    let slack = remote("slack", json!({}));
    let spec =
        ProbeSpec::new("slack", ProbeKind::Http).with_param(PARAM_TOKEN_KEY, "SLACK_BOT_TOKEN");
    let never = || -> RemoteFacts { panic!("the probe kind already decides") };
    let (reason, next) = unsupported(plan(&slack, Some(&spec), never));
    assert!(reason.contains("API token"), "{reason}");
    assert!(
        next.starts_with("toolportctl secret set slack SLACK_BOT_TOKEN "),
        "{next}"
    );
    assert!(
        next.ends_with("toolportctl auth probe --server slack --force"),
        "{next}"
    );
}

#[test]
fn a_team_server_must_be_reviewed_before_it_is_signed_in_to() {
    let command = std::path::Path::new("acme-mcp");
    let mut value = stdio_json("shared", command, "consent", vec![], true);
    value["source"] = json!("team:acme");
    let registry = |enabled: &[&str]| -> Registry {
        serde_json::from_value(json!({
            "version": 1,
            "servers": [value],
            "profiles": [{"id": "default", "name": "Default", "enabledServerIds": enabled}],
            "activeProfileId": "default",
        }))
        .unwrap()
    };
    let pending = registry(&[]);
    let (reason, next) = unsupported(review_gate(&pending, &pending.servers[0]).unwrap());
    assert!(reason.contains("needs consent"), "{reason}");
    assert!(next.ends_with("toolportctl auth login shared"), "{next}");
    let reviewed = registry(&["shared"]);
    assert!(review_gate(&reviewed, &reviewed.servers[0]).is_none());
    let own = server(stdio_json("own", command, "consent", vec![], true));
    assert!(review_gate(&pending, &own).is_none());
}

#[cfg(unix)]
mod flows {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::plus::auth::login::{login, LoginError, LoginOptions, UrlSink};
    use crate::plus::auth::testkit::{
        alive, env_var, mock, with_world, write_registry, World, CONSENT_URL, LEAKY,
    };

    const NO_BROWSER: LoginOptions = LoginOptions {
        open_browser: false,
    };

    type Seen = Arc<Mutex<Vec<String>>>;

    fn sink_for(seen: &Seen, finish: Option<std::path::PathBuf>) -> UrlSink {
        let seen = Arc::clone(seen);
        Arc::new(move |url| {
            seen.lock().unwrap().push(url.to_string());
            if let Some(done) = &finish {
                std::fs::write(done, b"").unwrap();
            }
        })
    }

    fn urls(seen: &Seen) -> Vec<String> {
        seen.lock().unwrap().clone()
    }

    #[test]
    fn a_stdio_server_signs_in_through_its_own_auth_command() {
        with_world("login-ok", |world| {
            write_registry(vec![mock(world, "acme", "consent")], &["acme"]);
            let seen = Seen::default();
            let report = login("acme", NO_BROWSER, sink_for(&seen, Some(world.done()))).unwrap();
            assert_eq!(report.flow, "stdio");
            assert_eq!(report.consent_url.as_deref(), Some(CONSENT_URL));
            assert!(report.signed_in);
            assert_eq!(report.message, "Signed in to acme.");
            assert_eq!(urls(&seen), [CONSENT_URL]);
        });
    }

    #[test]
    fn the_server_can_be_named_by_its_display_name() {
        with_world("login-name", |world| {
            let mut value = mock(world, "srv-1", "consent");
            value["name"] = json!("Acme Notes");
            write_registry(vec![value], &["srv-1"]);
            let seen = Seen::default();
            let report = login(
                "acme notes",
                NO_BROWSER,
                sink_for(&seen, Some(world.done())),
            )
            .unwrap();
            assert_eq!(report.server, "srv-1");
            assert_eq!(report.name, "Acme Notes");
        });
    }

    #[test]
    fn an_already_signed_in_server_prints_no_url() {
        with_world("login-already", |world| {
            write_registry(vec![mock(world, "acme", "signed_in")], &["acme"]);
            let seen = Seen::default();
            let report = login("acme", NO_BROWSER, sink_for(&seen, None)).unwrap();
            assert_eq!(report.consent_url, None);
            assert_eq!(report.message, "acme reports it is already signed in.");
            assert!(urls(&seen).is_empty());
        });
    }

    #[test]
    fn a_failing_auth_command_is_an_error_with_its_exit_code() {
        let cases = [
            ("crash", "exited with 3 without printing a consent URL"),
            ("revoked", "exited with 1 without printing a consent URL"),
        ];
        for (mode, expected) in cases {
            with_world(mode, |world| {
                write_registry(vec![mock(world, "acme", mode)], &["acme"]);
                let seen = Seen::default();
                let error = login("acme", NO_BROWSER, sink_for(&seen, None)).unwrap_err();
                match error {
                    LoginError::Failed(message) => assert!(message.contains(expected), "{message}"),
                    other => panic!("{other:?}"),
                }
                assert!(urls(&seen).is_empty());
            });
        }
        with_world("denied", |world| {
            let mut value = mock(world, "acme", "consent");
            value["env"]
                .as_array_mut()
                .unwrap()
                .push(env_var("MOCK_EXIT", "2", false));
            write_registry(vec![value], &["acme"]);
            let seen = Seen::default();
            let error = login("acme", NO_BROWSER, sink_for(&seen, Some(world.done()))).unwrap_err();
            let LoginError::Failed(message) = error else {
                panic!("{error:?}")
            };
            assert!(
                message.contains("exited with 2 before the sign-in finished"),
                "{message}"
            );
            assert_eq!(urls(&seen), [CONSENT_URL]);
        });
    }

    #[test]
    fn unknown_and_unsupported_servers_start_nothing() {
        with_world("login-refuse", |world: &World| {
            let mut team = mock(world, "shared", "consent");
            team["source"] = json!("team:acme");
            let mut sudo = mock(world, "wrapped", "consent");
            sudo["command"] = json!("sudo");
            write_registry(vec![team, sudo], &[]);
            let seen = Seen::default();
            let sink = sink_for(&seen, None);

            let missing = login("nope", NO_BROWSER, sink.clone()).unwrap_err();
            assert_eq!(missing, LoginError::NotFound("unknown server: nope".into()));

            let gated = login("shared", NO_BROWSER, sink.clone()).unwrap_err();
            assert!(matches!(gated, LoginError::Unsupported { .. }), "{gated:?}");

            let refused = login("wrapped", NO_BROWSER, sink).unwrap_err();
            assert!(matches!(refused, LoginError::Failed(_)), "{refused:?}");

            assert!(urls(&seen).is_empty());
            assert!(!world.path("pid").exists(), "the mock server was started");
        });
    }

    #[test]
    fn a_sign_in_never_surfaces_launch_secrets() {
        with_world("login-leak", |world| {
            let mut value = mock(world, "acme", "leak");
            value["env"]
                .as_array_mut()
                .unwrap()
                .push(env_var("MOCK_SECRET", "", true));
            crate::secrets::set_secret("acme", "MOCK_SECRET", LEAKY).unwrap();
            write_registry(vec![value], &["acme"]);
            let seen = Seen::default();
            let report = login("acme", NO_BROWSER, sink_for(&seen, Some(world.done()))).unwrap();
            let everything = format!("{report:?}{:?}{}", urls(&seen), json!(report));
            assert!(!everything.contains(LEAKY), "{everything}");
            assert!(
                !everything.contains("publisher-synthetic-vault"),
                "{everything}"
            );
        });
    }

    #[test]
    fn dropping_a_session_stops_a_running_sign_in() {
        with_world("login-kill", |world| {
            write_registry(vec![mock(world, "acme", "consent")], &["acme"]);
            let entry = crate::registry::load().unwrap().servers.remove(0);
            let mut session = crate::plus::auth::stdio::start(&entry).unwrap();
            let capture = session.wait_url(std::time::Duration::from_secs(10));
            assert_eq!(capture.url.as_deref(), Some(CONSENT_URL));
            let pid = std::fs::read_to_string(world.path("pid")).unwrap();
            assert!(alive(&pid));
            assert_eq!(
                session.wait_exit(std::time::Duration::from_millis(100)),
                None
            );
            drop(session);
            assert!(crate::plus::auth::testkit::wait_gone(&world.path("pid")));
        });
    }
}
