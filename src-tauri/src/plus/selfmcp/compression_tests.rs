use super::state_tests::{apply, call, kind, strings, without_confirm};
use super::tests::Fixture;
use crate::plus::testutil::tree_snapshot;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

fn status() -> Value {
    call("compression_status", json!({})).unwrap()
}

fn config_path() -> PathBuf {
    PathBuf::from(status()["configPath"].as_str().unwrap())
}

fn server_names() -> Vec<String> {
    call("servers_list", json!({})).unwrap()["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect()
}

struct FakeProxy {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Drop for FakeProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn fake_proxy(health: &Value) -> FakeProxy {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let body = health.to_string();
    std::thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut seen = Vec::new();
                    let mut chunk = [0u8; 512];
                    while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => seen.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    });
    FakeProxy { port, stop }
}

fn closed_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn compression_status_reads_the_policy_without_writing() {
    let fixture = Fixture::new("cmp-status");
    let before = tree_snapshot(&fixture.dir);
    let status = status();
    assert_eq!(status["configExists"], false);
    assert_eq!(status["provider"], "none");
    assert_eq!(status["preset"]["name"], "interactive");
    assert_eq!(tree_snapshot(&fixture.dir), before, "a read must not write");
}

#[test]
fn compression_enable_previews_by_default_and_applies_only_with_confirm() {
    let fixture = Fixture::new("cmp-enable");
    let args = json!({
        "provider": "rtk-only", "port": 9311, "preset": "agent",
        "mode": "cache", "telemetry": "on"
    });
    let before = tree_snapshot(&fixture.dir);

    let planned = call("compression_enable", args.clone()).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(planned["provider"], "rtk-only");
    assert!(strings(&planned["actions"])
        .iter()
        .any(|a| a == "would save config (provider=rtk-only)"));
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "the default must not write"
    );
    assert_eq!(status()["configExists"], false);
    assert_eq!(
        kind(without_confirm("compression_enable", args.clone())),
        "refused"
    );
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "a refused call must not write"
    );

    let done = apply("compression_enable", args).unwrap();
    assert_eq!(done["dryRun"], false);
    let status = status();
    assert_eq!(status["configExists"], true);
    assert_eq!(status["provider"], "rtk-only");
    assert_eq!(status["preset"]["name"], "agent");
    assert_eq!(status["preset"]["port"], 9311);
    assert_eq!(status["preset"]["mode"], "cache");
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(config_path()).unwrap()).unwrap();
    assert_eq!(saved["options"]["telemetry"], "on");
}

#[test]
fn compression_enable_refuses_bad_values_without_writing() {
    let fixture = Fixture::new("cmp-enable-bad");
    let before = tree_snapshot(&fixture.dir);
    for bad in [
        json!({"provider": "bogus"}),
        json!({"mode": "turbo"}),
        json!({"telemetry": "maybe"}),
        json!({"preset": "ghost"}),
        json!({"port": 0}),
        json!({"port": 70000}),
        json!({"port": -1}),
        json!({"port": "8787"}),
        json!({"unknown": true}),
    ] {
        assert_eq!(
            kind(apply("compression_enable", bad.clone())),
            "invalid_arguments",
            "{bad}"
        );
    }
    assert_eq!(tree_snapshot(&fixture.dir), before);
}

#[test]
fn enabling_headroom_registers_its_server_and_disable_removes_it_again() {
    let fixture = Fixture::new("cmp-headroom");
    let before = tree_snapshot(&fixture.dir);
    let planned = call("compression_enable", json!({"provider": "headroom"})).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert!(!planned["written"].as_array().unwrap().is_empty());
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "the default must not write"
    );
    assert!(!server_names().contains(&"headroom".to_string()));

    apply("compression_enable", json!({"provider": "headroom"})).unwrap();
    assert!(server_names().contains(&"headroom".to_string()));
    assert_eq!(status()["provider"], "headroom");
    let snippet = fixture.dir.join("compression-env.sh");
    assert!(snippet.is_file());

    let before = tree_snapshot(&fixture.dir);
    let planned = call("compression_disable", json!({})).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(planned["teardown"], false);
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "the default must not write"
    );
    assert_eq!(
        kind(without_confirm("compression_disable", json!({}))),
        "refused"
    );
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "a refused call must not write"
    );

    let done = apply("compression_disable", json!({})).unwrap();
    assert_eq!(done["dryRun"], false);
    assert_eq!(done["provider"], "none");
    assert_eq!(status()["provider"], "none");
    assert!(!server_names().contains(&"headroom".to_string()));
    assert_eq!(
        strings(&done["removed"]),
        [snippet.to_string_lossy().into_owned()]
    );
    assert!(!snippet.exists());
}

#[test]
fn compression_set_provider_and_use_preview_by_default() {
    let fixture = Fixture::new("cmp-provider");
    let before = tree_snapshot(&fixture.dir);

    let planned = call("compression_set_provider", json!({"provider": "rtk-only"})).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(tree_snapshot(&fixture.dir), before);
    assert_eq!(
        kind(without_confirm(
            "compression_set_provider",
            json!({"provider": "rtk-only"})
        )),
        "refused"
    );
    for bad in [
        json!({"provider": "bogus"}),
        json!({"provider": ""}),
        json!({}),
    ] {
        assert_eq!(
            kind(apply("compression_set_provider", bad)),
            "invalid_arguments"
        );
    }
    assert_eq!(tree_snapshot(&fixture.dir), before);
    apply("compression_set_provider", json!({"provider": "rtk-only"})).unwrap();
    assert_eq!(status()["provider"], "rtk-only");

    let before = tree_snapshot(&fixture.dir);
    let planned = call("compression_use", json!({"preset": "agent"})).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(planned["preset"]["name"], "agent");
    assert_eq!(status()["preset"]["name"], "interactive");
    assert_eq!(tree_snapshot(&fixture.dir), before);
    assert_eq!(
        kind(without_confirm(
            "compression_use",
            json!({"preset": "agent"})
        )),
        "refused"
    );
    for bad in [json!({"preset": "ghost"}), json!({})] {
        assert_eq!(kind(apply("compression_use", bad)), "invalid_arguments");
    }
    assert_eq!(tree_snapshot(&fixture.dir), before);
    apply("compression_use", json!({"preset": "agent"})).unwrap();
    assert_eq!(status()["preset"]["name"], "agent");
}

const LEGACY_POLICY: &str = r#"{
  "provider": "rtk-only",
  "runtime": "hook",
  "presets": {"agent": {"mode": "token", "savings_profile": "agent-90",
    "env": {"HEADROOM_MODE": "token"}, "code_aware": false, "port": 8788}},
  "active_preset": "agent"
}"#;

#[test]
fn compression_sync_adopts_a_legacy_policy_only_when_applied() {
    let fixture = Fixture::new("cmp-sync");
    let legacy = fixture.home.join("compression.json");
    std::fs::write(&legacy, LEGACY_POLICY).unwrap();
    let before = tree_snapshot(&fixture.dir);

    let planned = call("compression_sync", json!({})).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert!(planned["adopted"]["from"]
        .as_str()
        .unwrap()
        .ends_with("compression.json"));
    assert!(strings(&planned["actions"])[0].starts_with("would adopt legacy policy from "));
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "the default must not write"
    );
    assert_eq!(status()["configExists"], false);
    assert_eq!(
        kind(without_confirm("compression_sync", json!({}))),
        "refused"
    );
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "a refused call must not write"
    );

    let done = apply("compression_sync", json!({})).unwrap();
    assert_eq!(done["dryRun"], false);
    let status = status();
    assert_eq!(status["configExists"], true);
    assert_eq!(status["provider"], "rtk-only");
    assert_eq!(status["preset"]["name"], "agent");
    assert_eq!(status["preset"]["port"], 8788);
    assert_eq!(std::fs::read_to_string(&legacy).unwrap(), LEGACY_POLICY);
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

#[test]
fn compression_sync_mcpm_root_refuses_anything_but_an_absolute_existing_directory() {
    let fixture = Fixture::new("cmp-root-bad");
    let file = fixture.dir.join("not-a-directory");
    std::fs::write(&file, LEGACY_POLICY).unwrap();
    let before = tree_snapshot(&fixture.dir);
    for bad in [
        json!("relative/legacy"),
        json!("./legacy"),
        json!(""),
        json!(fixture.dir.join("missing").to_string_lossy()),
        json!(file.to_string_lossy()),
        json!(5),
    ] {
        let args = json!({"mcpm_root": bad});
        assert_eq!(kind(call("compression_sync", args.clone())), "invalid_arguments", "{bad}");
        assert_eq!(kind(apply("compression_sync", args)), "invalid_arguments", "{bad}");
    }
    assert_eq!(tree_snapshot(&fixture.dir), before);
}

#[test]
fn compression_sync_mcpm_root_reads_only_the_named_directory() {
    let fixture = Fixture::new("cmp-root");
    let decoy = fixture.home.join("compression.json");
    std::fs::write(
        &decoy,
        LEGACY_POLICY
            .replace("8788", "8799")
            .replace("rtk-only", "none"),
    )
    .unwrap();
    let legacy_dir = fixture.dir.join("legacy-mcpm");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(legacy_dir.join("compression.json"), LEGACY_POLICY).unwrap();
    let before = tree_snapshot(&fixture.dir);
    let legacy_before = tree_snapshot(&legacy_dir);
    let canonical_dir = canonical(&legacy_dir);
    let expected_from = canonical_dir.join("compression.json");

    let planned = call(
        "compression_sync",
        json!({"mcpm_root": legacy_dir.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(planned["mcpmRoot"], json!(canonical_dir.to_string_lossy()));
    assert_eq!(
        planned["adopted"]["from"],
        json!(expected_from.to_string_lossy())
    );
    assert_eq!(tree_snapshot(&fixture.dir), before, "the preview must not write");
    assert_eq!(status()["configExists"], false);
    assert_eq!(
        kind(without_confirm(
            "compression_sync",
            json!({"mcpm_root": legacy_dir.to_string_lossy()})
        )),
        "refused"
    );
    assert_eq!(tree_snapshot(&fixture.dir), before);

    #[cfg(unix)]
    let link = {
        let link = fixture.dir.join("legacy-link");
        std::os::unix::fs::symlink(&legacy_dir, &link).unwrap();
        link
    };
    #[cfg(unix)]
    {
        let through_link = call(
            "compression_sync",
            json!({"mcpm_root": link.to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(through_link["mcpmRoot"], json!(canonical_dir.to_string_lossy()));
    }

    let done = apply(
        "compression_sync",
        json!({"mcpm_root": legacy_dir.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(done["dryRun"], false);
    assert_eq!(done["adopted"]["from"], json!(expected_from.to_string_lossy()));
    let status = status();
    assert_eq!(status["provider"], "rtk-only");
    assert_eq!(status["preset"]["port"], 8788, "the default directory was read");
    assert_eq!(
        tree_snapshot(&legacy_dir),
        legacy_before,
        "the named directory is only read"
    );
}

#[cfg(unix)]
struct Sentinel {
    _path: crate::plus::compression::manage_tests::EnvGuard,
    log: PathBuf,
}

#[cfg(unix)]
impl Sentinel {
    fn new(fixture: &Fixture) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let bin = fixture.dir.join("sentinel-bin");
        std::fs::create_dir_all(&bin).unwrap();
        let log = fixture.dir.join("headroom-calls.log");
        let script = bin.join("headroom");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"$*\" >> '{}'\necho done\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut dirs = vec![bin];
        dirs.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        let joined = std::env::join_paths(dirs).unwrap();
        Self {
            _path: crate::plus::compression::manage_tests::EnvGuard::set(&[(
                "PATH",
                Path::new(&joined),
            )]),
            log,
        }
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .map(|text| text.lines().map(String::from).collect())
            .unwrap_or_default()
    }
}

#[cfg(unix)]
#[test]
fn compression_disable_teardown_names_its_commands_and_runs_them_only_when_applied() {
    let fixture = Fixture::new("cmp-teardown");
    let sentinel = Sentinel::new(&fixture);
    apply("compression_enable", json!({"provider": "rtk-only"})).unwrap();
    assert!(sentinel.calls().is_empty(), "enabling must not start the engine");
    let before = tree_snapshot(&fixture.dir);
    let named = |data: &Value| {
        let actions = strings(&data["actions"]);
        (
            actions.iter().any(|a| a == "would run `headroom mcp uninstall`"),
            actions.iter().any(|a| a == "would run `headroom unwrap claude`"),
        )
    };

    let plain = call("compression_disable", json!({})).unwrap();
    assert_eq!(plain["teardown"], false);
    assert_eq!(named(&plain), (false, false));

    for args in [
        json!({"teardown": true}),
        json!({"teardown": true, "dry_run": true}),
        json!({"teardown": true, "confirm": true}),
    ] {
        let planned = call("compression_disable", args.clone()).unwrap();
        assert_eq!(planned["dryRun"], true, "{args}");
        assert_eq!(planned["teardown"], true, "{args}");
        assert_eq!(named(&planned), (true, true), "{args}");
        assert!(sentinel.calls().is_empty(), "{args} started headroom");
        assert_eq!(tree_snapshot(&fixture.dir), before, "{args} wrote");
    }
    assert_eq!(
        kind(without_confirm("compression_disable", json!({"teardown": true}))),
        "refused"
    );
    assert!(sentinel.calls().is_empty());
    assert_eq!(
        kind(call("compression_disable", json!({"teardown": "yes"}))),
        "invalid_arguments"
    );

    let done = apply("compression_disable", json!({"teardown": true})).unwrap();
    assert_eq!(done["dryRun"], false);
    assert_eq!(done["teardown"], true);
    assert_eq!(sentinel.calls(), ["mcp uninstall", "unwrap claude"]);
    let actions = strings(&done["actions"]);
    assert!(actions.iter().any(|a| a.starts_with("headroom mcp uninstall")), "{actions:?}");
    assert!(actions.iter().any(|a| a.starts_with("headroom unwrap claude")), "{actions:?}");
    assert_eq!(status()["provider"], "none");

    apply("compression_disable", json!({})).unwrap();
    assert_eq!(sentinel.calls().len(), 2, "a disable without teardown runs nothing");
}

#[test]
fn compression_seal_records_the_live_posture_only_when_applied() {
    let fixture = Fixture::new("cmp-seal");
    let proxy = fake_proxy(&json!({
        "ready": true,
        "config": {"max_items_after_crush": 50, "accuracy_guard": true, "protect_recent": null}
    }));
    apply(
        "compression_enable",
        json!({"provider": "none", "port": proxy.port}),
    )
    .unwrap();
    let before = tree_snapshot(&fixture.dir);

    let planned = call("compression_seal", json!({})).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(planned["port"], proxy.port);
    assert_eq!(
        planned["declarable"],
        json!([
            {"knob": "HEADROOM_MAX_ITEMS", "value": "50"},
            {"knob": "HEADROOM_ACCURACY_GUARD", "value": "1"}
        ])
    );
    assert_eq!(planned["unset"], json!(["HEADROOM_PROTECT_RECENT"]));
    assert_eq!(planned["sealed"], 0);
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "the default must not write"
    );
    assert_eq!(
        kind(without_confirm("compression_seal", json!({}))),
        "refused"
    );
    assert_eq!(
        tree_snapshot(&fixture.dir),
        before,
        "a refused call must not write"
    );

    let done = apply("compression_seal", json!({})).unwrap();
    assert_eq!(done["dryRun"], false);
    assert_eq!(done["sealed"], 2);
    let saved = std::fs::read_to_string(config_path()).unwrap();
    assert!(saved.contains("HEADROOM_MAX_ITEMS"), "{saved}");
    assert_eq!(apply("compression_seal", json!({})).unwrap()["sealed"], 0);
}

#[test]
fn compression_seal_refuses_an_unknown_preset_and_reports_a_missing_proxy() {
    let fixture = Fixture::new("cmp-seal-bad");
    apply(
        "compression_enable",
        json!({"provider": "none", "port": closed_port()}),
    )
    .unwrap();
    let before = tree_snapshot(&fixture.dir);
    assert_eq!(
        kind(apply("compression_seal", json!({"preset": "ghost"}))),
        "invalid_arguments"
    );
    let missing = apply("compression_seal", json!({})).unwrap_err();
    assert_eq!(missing.kind, "backend_error");
    assert!(missing.message.starts_with("no proxy on :"), "{missing:?}");
    assert_eq!(tree_snapshot(&fixture.dir), before);
}

#[test]
fn the_compression_results_never_carry_a_secret() {
    let fixture = Fixture::with_vault("cmp-secrets");
    let canary = "CANARY-synthetic-token-1f64";
    let mut legacy: Value = serde_json::from_str(LEGACY_POLICY).unwrap();
    legacy["api_token"] = json!(canary);
    legacy["options"] = json!({"api_key": canary, "telemetry": "off"});
    std::fs::write(fixture.home.join("compression.json"), legacy.to_string()).unwrap();
    let mut seen = String::new();
    for (tool, args) in [
        ("compression_status", json!({})),
        ("compression_sync", json!({})),
        ("compression_enable", json!({"provider": "rtk-only"})),
        ("compression_set_provider", json!({"provider": "none"})),
        ("compression_use", json!({"preset": "agent"})),
        ("compression_disable", json!({})),
    ] {
        let result = call(tool, args).unwrap_or_else(|e| panic!("{tool}: {e:?}"));
        seen.push_str(&result.to_string());
    }
    let result = apply("compression_sync", json!({})).unwrap();
    seen.push_str(&result.to_string());
    seen.push_str(&status().to_string());
    assert!(seen.contains("rtk-only"), "{seen}");
    assert!(!seen.contains(canary), "{seen}");
    let saved = std::fs::read_to_string(config_path()).unwrap();
    assert!(
        saved.contains(canary),
        "the canary must reach the stored policy"
    );
    assert!(!seen.contains(&"cd".repeat(32)), "{seen}");
}
