use regex::Regex;
use serde_json::{json, Value};

use super::*;
use crate::plus::auth::login::{plan, Plan, RemoteFacts};
use crate::plus::auth::surfaces::{self, fix_action};
use crate::plus::auth::testkit::write_registry;
use crate::plus::auth::{combined_registry, AuthState, StatusFile, Tracked};
use crate::plus::testutil::tree_snapshot;
use crate::secrets::tests::with_isolated_vault;

const STATIC_TOKEN: &str = "FAKE-static-token-4d1e";

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn run_cli(list: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args(list), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn json_of(list: &[&str]) -> (i32, Value) {
    let (code, out, _) = run_cli(list);
    (code, serde_json::from_str(out.trim()).expect(&out))
}

fn remote(id: &str, name: &str) -> Value {
    json!({"id": id, "name": name, "transport": "http", "url": "https://mcp.example.test/mcp"})
}

fn facts(oauth_state: bool, static_token: bool, detected: &str) -> RemoteFacts {
    RemoteFacts {
        oauth_state,
        static_token,
        detected: detected.into(),
    }
}

fn data_dir() -> std::path::PathBuf {
    crate::registry::conduit_dir().unwrap()
}

fn two_remotes() {
    write_registry(
        vec![remote("srv-figma", "figma"), remote("srv-slack", "slack")],
        &[],
    );
    crate::secrets::set_secret("srv-slack", crate::secrets::HTTP_AUTH_KEY, STATIC_TOKEN).unwrap();
}

fn row_of<'a>(out: &'a str, server: &str) -> &'a str {
    out.lines()
        .find(|line| line.contains(server))
        .unwrap_or_else(|| panic!("no row for {server} in\n{out}"))
}

#[test]
fn auth_probe_prints_state_rows_and_writes_the_cache() {
    with_isolated_vault(|| {
        two_remotes();
        let (code, out, err) = run_cli(&["auth", "probe"]);
        assert_eq!((code, err.as_str()), (0, ""), "{out}");
        assert!(
            out.starts_with("auth probe: 2 probed, 0 skipped, 0 failed\n"),
            "{out}"
        );
        for header in ["server", "state", "reason", "probe", "fix"] {
            assert!(out.contains(header), "{out}");
        }
        let figma = row_of(&out, "srv-figma");
        for cell in [
            "needs_reauth",
            "no_token",
            "ran",
            "toolportctl auth login srv-figma",
        ] {
            assert!(figma.contains(cell), "{figma}");
        }
        let slack = row_of(&out, "srv-slack");
        assert!(slack.contains("ok") && slack.contains("ran"), "{slack}");
        assert!(!slack.contains("toolportctl"), "{slack}");
        assert!(!out.contains(STATIC_TOKEN));

        let status = data_dir().join("auth/status.json");
        assert!(status.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&status).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let (code, out, _) = run_cli(&["auth", "probe"]);
        assert_eq!(code, 0);
        assert_eq!(out.trim(), "auth probe: nothing due (2 registered)");

        let (_, line, _) = run_cli(&["auth", "statusline"]);
        let line: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(line["auth"]["needs_reauth"], 1);
        assert_eq!(line["auth"]["worst"], json!(["srv-figma"]));
    });
}

#[test]
fn auth_probe_server_and_force_follow_the_cache() {
    with_isolated_vault(|| {
        two_remotes();
        run_cli(&["auth", "probe"]);

        let (code, out, _) = run_cli(&["auth", "probe", "--server", "srv-figma"]);
        assert_eq!(code, 0);
        assert!(
            out.starts_with("auth probe: 0 probed, 1 skipped, 0 failed"),
            "{out}"
        );
        assert!(
            row_of(&out, "srv-figma").contains("skipped (not_due)"),
            "{out}"
        );

        let (code, out, _) = run_cli(&["auth", "probe", "--server", "figma", "--force"]);
        assert_eq!(code, 0);
        assert!(
            out.starts_with("auth probe: 1 probed, 0 skipped, 0 failed"),
            "{out}"
        );
        assert!(row_of(&out, "srv-figma").contains("ran"), "{out}");
        assert!(!out.contains("srv-slack"), "{out}");

        let (code, out, _) = run_cli(&["auth", "probe", "--force"]);
        assert_eq!(code, 0);
        assert!(
            out.starts_with("auth probe: 2 probed, 0 skipped, 0 failed"),
            "{out}"
        );
    });
}

#[test]
fn auth_probe_json_reports_the_run_and_the_counts() {
    with_isolated_vault(|| {
        two_remotes();
        let (code, value) = json_of(&["--json", "auth", "probe", "--force"]);
        assert_eq!(code, 0);
        assert_eq!(value["ok"], true);
        assert_eq!(value["command"], "auth probe");
        let data = &value["data"];
        assert_eq!(data["mode"], "all");
        assert_eq!(data["probes"].as_array().unwrap().len(), 2);
        assert_eq!(data["failures"], json!([]));
        assert_eq!(data["counts"]["ok"], 1);
        assert_eq!(data["counts"]["needs_reauth"], 1);
        assert_eq!(data["servers"][0]["server"], "srv-figma");
        assert_eq!(data["servers"][0]["state"], "needs_reauth");
        assert_eq!(
            data["servers"][0]["fix"]["command"],
            "toolportctl auth login srv-figma"
        );
        assert!(!value.to_string().contains(STATIC_TOKEN));

        let (_, value) = json_of(&[
            "--json",
            "auth",
            "probe",
            "--server",
            "srv-slack",
            "--force",
        ]);
        assert_eq!(value["data"]["mode"], "server");
        assert_eq!(value["data"]["servers"].as_array().unwrap().len(), 1);
        assert_eq!(value["data"]["servers"][0]["state"], "ok");

        let (_, value) = json_of(&["--json", "auth", "probe"]);
        assert_eq!(value["data"]["mode"], "due");
        assert_eq!(value["data"]["probes"], json!([]));
        assert_eq!(value["data"]["servers"], json!([]));
    });
}

#[test]
fn auth_probe_and_login_no_ops_write_nothing() {
    with_isolated_vault(|| {
        let plain = json!({
            "id": "srv-plain", "name": "plain", "transport": "stdio", "command": "acme-mcp"
        });
        write_registry(vec![plain, remote("srv-slack", "slack")], &[]);
        crate::secrets::set_secret("srv-slack", crate::secrets::HTTP_AUTH_KEY, STATIC_TOKEN)
            .unwrap();
        let before = tree_snapshot(&data_dir());

        let (code, out, _) = run_cli(&["auth", "probe", "--server", "srv-plain"]);
        assert_eq!(code, 1);
        assert!(out.is_empty());
        let (code, _, err) = run_cli(&["auth", "probe", "--server", "nope"]);
        assert_eq!(code, 1);
        assert!(
            err.contains("no auth probe is registered for nope"),
            "{err}"
        );
        assert!(err.contains("`toolportctl auth login nope`"), "{err}");

        let (code, value) = json_of(&["--json", "auth", "probe", "--server", "nope"]);
        assert_eq!((code, value["ok"].clone()), (1, json!(false)));
        assert_eq!(value["error"]["code"], "not_found");

        let (code, _, err) = run_cli(&["auth", "login", "nope"]);
        assert_eq!(code, 1);
        assert!(err.contains("unknown server: nope"), "{err}");

        let (code, _, err) = run_cli(&["auth", "login", "srv-slack"]);
        assert_eq!(code, 1);
        assert!(err.contains("signs in with a static token"), "{err}");
        assert!(err.contains("Next: enter the new value"), "{err}");
        assert!(
            err.contains("toolportctl auth probe --server srv-slack --force"),
            "{err}"
        );
        let (_, value) = json_of(&["--json", "auth", "login", "srv-slack"]);
        assert_eq!(value["error"]["code"], "unsupported");

        let (code, _, err) = run_cli(&["auth", "login", "plain", "--no-open"]);
        assert_eq!(code, 1, "{err}");
        assert!(!err.contains("usage"), "{err}");

        assert_eq!(tree_snapshot(&data_dir()), before);
        assert!(!data_dir().join("auth").exists());
    });
}

#[test]
fn auth_probe_and_login_reject_bad_arguments_before_doing_anything() {
    with_isolated_vault(|| {
        two_remotes();
        let before = tree_snapshot(&data_dir());
        let cases: &[(&[&str], &str)] = &[
            (
                &["auth", "probe", "extra"],
                "usage: auth probe [--server <id>] [--force]",
            ),
            (
                &["auth", "probe", "--bogus"],
                "usage: auth probe [--server <id>] [--force]",
            ),
            (
                &["auth", "probe", "--server"],
                "--server requires a server id",
            ),
            (&["auth", "probe", "--server="], "--server"),
            (&["auth", "login"], "usage: auth login <server> [--no-open]"),
            (
                &["auth", "login", "a", "b"],
                "usage: auth login <server> [--no-open]",
            ),
            (
                &["auth", "login", "--bogus"],
                "usage: auth login <server> [--no-open]",
            ),
            (
                &["auth", "login", "--no-open"],
                "usage: auth login <server> [--no-open]",
            ),
        ];
        for (command, message) in cases {
            let (code, _, err) = run_cli(command);
            assert_eq!(code, 2, "{command:?}: {err}");
            assert!(err.contains(message), "{command:?}: {err}");
        }
        let (code, _, err) = run_cli(&["auth"]);
        assert_eq!(code, 2);
        assert!(err.contains("auth statusline|hook|probe|login"), "{err}");
        assert_eq!(tree_snapshot(&data_dir()), before);
    });
}

#[test]
fn the_new_commands_are_registered_and_keep_their_option_rules() {
    for path in [
        &["auth"][..],
        &["auth", "statusline"],
        &["auth", "hook"],
        &["auth", "probe"],
        &["auth", "login"],
    ] {
        let words = args(path);
        let (command, rest) = find_command(&words).unwrap();
        assert_eq!(command.path, path);
        assert!(rest.is_empty());
        assert!(!command.planned(), "{path:?}");
    }
    let parsed = parse(&args(&["auth", "probe", "--server", "acme", "--force"])).unwrap();
    assert_eq!(
        parsed.positional,
        ["auth", "probe", "--server", "acme", "--force"]
    );
    let parsed = parse(&args(&["auth", "login", "--no-open", "acme"])).unwrap();
    assert_eq!(parsed.positional, ["auth", "login", "--no-open", "acme"]);
    assert_eq!(
        parse(&args(&["auth", "--bogus"])).err().as_deref(),
        Some("unknown option: --bogus")
    );
    assert_eq!(
        parse(&args(&["auth", "--force"])).err().as_deref(),
        Some("unknown option: --force")
    );
    assert_eq!(
        parse(&args(&["auth", "statusline", "--bogus"]))
            .err()
            .as_deref(),
        Some("unknown option: --bogus")
    );
}

fn every_state() -> StatusFile {
    let states = [
        ("srv-ok", AuthState::Ok),
        ("srv-expiring", AuthState::Expiring { eta: 5_000 }),
        ("srv-reauth", AuthState::NeedsReauth),
        ("srv-revoked", AuthState::Revoked),
        ("srv-config", AuthState::Misconfigured),
        ("srv-down", AuthState::Unreachable),
        ("srv-new", AuthState::Unknown),
    ];
    let mut status = StatusFile::default();
    for (server, state) in states {
        let mut tracked = Tracked::unknown(1_000);
        tracked.state = state;
        tracked.reason = "probe".into();
        status.servers.insert(
            server.to_string(),
            crate::plus::auth::ServerEntry {
                tracked,
                last_probe_at: Some(1_000),
                next_due_at: 2_000,
                hint_kind: crate::plus::auth::ProbeHintKind::default(),
                token_key: None,
            },
        );
    }
    status
}

#[test]
fn every_toolportctl_command_the_auth_surfaces_print_is_a_real_command() {
    let status = every_state();
    let mut printed = vec![
        surfaces::statusline(&status, 1_500).to_string(),
        surfaces::hook(&status, 1_500).to_string(),
        serde_json::to_string(&surfaces::rows(&status, 1_500)).unwrap(),
    ];
    for row in surfaces::rows(&status, 1_500) {
        if let Some(fix) = &row.fix {
            printed.push(format!("{} {:?} {:?}", fix.label, fix.command, fix.ipc));
        }
    }
    let unknown = crate::plus::auth::testkit::server(remote("srv-x", "x"));
    let mut slack = remote("srv-x", "x");
    slack["env"] = json!([{"key": "SLACK_BOT_TOKEN", "secret": true}]);
    let slack_registry: crate::registry::Registry =
        serde_json::from_value(json!({"version": 1, "servers": [slack], "profiles": []})).unwrap();
    let spec = combined_registry(&slack_registry)
        .get("srv-x")
        .unwrap()
        .clone();
    let inputs = [
        (None, facts(true, false, "")),
        (None, facts(false, true, "")),
        (None, facts(false, false, "token")),
        (None, facts(false, false, "none")),
        (Some(&spec), facts(false, false, "")),
    ];
    for (spec, facts) in inputs {
        if let Plan::Unsupported { reason, next } = plan(&unknown, spec, || facts) {
            printed.push(format!("{reason}. Next: {next}"));
        }
    }
    let mut stdioless = unknown.clone();
    stdioless.transport = "stdio".into();
    stdioless.url = None;
    if let Plan::Unsupported { reason, next } = plan(&stdioless, None, RemoteFacts::default) {
        printed.push(format!("{reason}. Next: {next}"));
    }
    let command = Regex::new(r"toolportctl((?: [a-z][a-z-]*)+)").unwrap();
    with_isolated_vault(|| {
        two_remotes();
        for list in [
            &["auth", "probe", "--server", "nope"][..],
            &["auth", "login", "srv-slack"],
            &["auth", "login", "plain"],
            &["auth", "probe"],
        ] {
            let (_, out, err) = run_cli(list);
            printed.push(format!("{out}{err}"));
        }
    });
    let mut resolved_login = 0;
    let mut seen = Vec::new();
    for text in &printed {
        for found in command.captures_iter(text) {
            let words: Vec<String> = found[1].split_whitespace().map(String::from).collect();
            let (resolved, rest) = find_command(&words)
                .unwrap_or_else(|| panic!("no ctl command for `toolportctl{}`", &found[1]));
            assert!(
                !resolved.planned(),
                "`toolportctl{}` is only planned",
                &found[1]
            );
            if found[1].starts_with(" auth login") {
                assert_eq!(resolved.path, ["auth", "login"], "{}", &found[1]);
                assert_eq!(rest.len(), 1, "{}", &found[1]);
                resolved_login += 1;
            }
            seen.push(resolved.path.join(" "));
        }
    }
    assert!(resolved_login >= 3, "{resolved_login} fix hints resolved");
    for expected in ["auth login", "auth probe", "server info", "secret set"] {
        assert!(
            seen.iter().any(|s| s == expected),
            "{expected} not printed: {seen:?}"
        );
    }
    for state in [
        AuthState::NeedsReauth,
        AuthState::Revoked,
        AuthState::Expiring { eta: 1 },
    ] {
        let fix = fix_action("acme", &state, crate::plus::auth::ProbeHintKind::OAuth, None).unwrap();
        assert_eq!(fix.command.as_deref(), Some("toolportctl auth login acme"));
    }
}

#[cfg(unix)]
mod login {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::plus::auth::testkit::{env_var, mock, with_world, World, CONSENT_URL, LEAKY};

    fn finish_when_printed(world: &World) -> std::thread::JoinHandle<()> {
        let (printed, done) = (world.printed(), world.done());
        let _ = std::fs::remove_file(&printed);
        let _ = std::fs::remove_file(&done);
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !printed.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::fs::write(done, b"").unwrap();
        })
    }

    #[test]
    fn auth_login_signs_in_a_stdio_server_and_the_probe_turns_ok() {
        with_world("ctl-login", |world| {
            write_registry(vec![mock(world, "acme", "consent")], &["acme"]);

            let (code, out, _) = run_cli(&["auth", "probe", "--server", "acme", "--force"]);
            assert_eq!(code, 0);
            let acme = row_of(&out, "acme");
            for cell in ["needs_reauth", "not_authed", "toolportctl auth login acme"] {
                assert!(acme.contains(cell), "{acme}");
            }

            let helper = finish_when_printed(world);
            let (code, out, err) = run_cli(&["auth", "login", "acme", "--no-open"]);
            helper.join().unwrap();
            assert_eq!((code, err.as_str()), (0, ""), "{out}");
            assert!(out.starts_with("Signed in to acme.\n"), "{out}");
            assert!(
                out.contains(&format!("Consent URL: {CONSENT_URL}")),
                "{out}"
            );
            let acme = row_of(&out, "│ acme");
            assert!(acme.contains("ok") && acme.contains("ran"), "{acme}");

            let (_, line, _) = run_cli(&["auth", "statusline"]);
            let line: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(line["auth"]["needs_reauth"], 0);

            let helper = finish_when_printed(world);
            let (code, value) = json_of(&["--json", "auth", "login", "acme", "--no-open"]);
            helper.join().unwrap();
            assert_eq!(code, 0);
            assert_eq!(value["command"], "auth login");
            let data = &value["data"];
            assert_eq!(data["server"], "acme");
            assert_eq!(data["flow"], "stdio");
            assert_eq!(data["consentUrl"], CONSENT_URL);
            assert_eq!(data["signedIn"], true);
            assert_eq!(data["probe"]["tracked"]["state"], "ok");
            assert_eq!(data["servers"][0]["state"], "ok");
        });
    }

    #[test]
    fn auth_login_and_probe_never_print_launch_secrets() {
        with_world("ctl-leak", |world| {
            let mut value = mock(world, "acme", "leak");
            value["env"]
                .as_array_mut()
                .unwrap()
                .push(env_var("MOCK_SECRET", "", true));
            crate::secrets::set_secret("acme", "MOCK_SECRET", LEAKY).unwrap();
            write_registry(vec![value], &["acme"]);

            let (_, probed, probe_err) =
                run_cli(&["--json", "auth", "probe", "--server", "acme", "--force"]);
            let helper = finish_when_printed(world);
            let (code, out, err) = run_cli(&["--json", "auth", "login", "acme", "--no-open"]);
            helper.join().unwrap();
            assert_eq!(code, 0, "{out}{err}");
            for text in [&probed, &probe_err, &out, &err] {
                assert!(!text.contains(LEAKY), "{text}");
                assert!(!text.contains("publisher-synthetic-vault"), "{text}");
            }
            let mut on_disk = String::new();
            for entry in std::fs::read_dir(data_dir().join("auth"))
                .unwrap()
                .flatten()
            {
                on_disk.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
            assert!(!on_disk.contains(LEAKY));
        });
    }
}
