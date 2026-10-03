use super::*;
use crate::plus::compression::model::{CompressionConfig, ProviderName};
use crate::plus::compression::store::{self, Paths};
use serde_json::{json, Value};
use std::path::PathBuf;

struct Fx {
    dir: PathBuf,
    // fields drop in order: the override must go before the lock is released
    _override: crate::registry::DataDirOverride,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Fx {
    fn new(tag: &str, config: Option<CompressionConfig>) -> Self {
        let lock = crate::registry::data_dir_test_lock();
        let dir =
            std::env::temp_dir().join(format!("toolportctl-cmp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if let Some(config) = config {
            store::save(&Paths::new(&dir), &config).unwrap();
        }
        let guard = crate::registry::DataDirOverride::set(&dir);
        Self {
            dir,
            _override: guard,
            _lock: lock,
        }
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn run_cli(list: &[&str]) -> (i32, String, String) {
    let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&list, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn json_of(list: &[&str]) -> (i32, Value) {
    let (code, out, _) = run_cli(list);
    (
        code,
        serde_json::from_str(out.trim()).expect("json envelope"),
    )
}

fn config_with(provider: ProviderName) -> CompressionConfig {
    CompressionConfig::with_provider(provider)
}

#[test]
fn status_defaults_without_creating_anything() {
    let fx = Fx::new("status-empty", None);
    let (code, value) = json_of(&["--json", "compression", "status"]);
    assert_eq!(code, 0);
    assert_eq!(value["command"], "compression status");
    let data = &value["data"];
    assert_eq!(data["configExists"], false);
    assert_eq!(data["provider"], "none");
    assert_eq!(
        data["pin"]["requirement"],
        "headroom-ai[proxy,code,ml]==0.29.0"
    );
    assert_eq!(data["pin"]["drift"], Value::Null);
    assert_eq!(data["preset"]["name"], "interactive");
    assert_eq!(std::fs::read_dir(&fx.dir).unwrap().count(), 0);
}

#[test]
fn status_reports_a_stored_policy_and_never_rewrites_it() {
    let mut cfg = config_with(ProviderName::RtkOnly);
    cfg.active_preset = "agent".into();
    let fx = Fx::new("status-stored", Some(cfg));
    let before = std::fs::read(fx.dir.join("compression.json")).unwrap();
    let (code, value) = json_of(&["--json", "compression", "status"]);
    assert_eq!(code, 0);
    assert_eq!(value["data"]["provider"], "rtk-only");
    assert_eq!(value["data"]["preset"]["port"], 8788);
    assert_eq!(value["data"]["configExists"], true);
    assert_eq!(
        std::fs::read(fx.dir.join("compression.json")).unwrap(),
        before
    );
    let (code, out, _) = run_cli(&["compression", "status"]);
    assert_eq!(code, 0);
    assert!(out.contains("rtk-only") && out.contains("agent"));
}

#[test]
fn corrupt_policy_is_a_command_error_and_is_left_alone() {
    let fx = Fx::new("corrupt", None);
    std::fs::write(fx.dir.join("compression.json"), "{oops").unwrap();
    let (code, value) = json_of(&["--json", "compression", "status"]);
    assert_eq!(code, 1);
    assert_eq!(value["error"]["code"], "config_invalid");
    assert_eq!(
        std::fs::read_to_string(fx.dir.join("compression.json")).unwrap(),
        "{oops"
    );
}

#[test]
fn presets_lists_the_defaults_with_the_active_one_marked() {
    let _fx = Fx::new("presets", None);
    let (code, value) = json_of(&["--json", "compression", "presets"]);
    assert_eq!(code, 0);
    let rows = value["data"]["presets"].as_array().unwrap();
    let names: Vec<_> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["interactive", "agent", "balanced"]);
    assert_eq!(rows[0]["active"], true);
    assert_eq!(rows[1]["active"], false);
    assert_eq!(rows[1]["savingsProfile"], "agent-90");
    let (_, out, _) = run_cli(&["compression", "presets"]);
    assert!(out.contains("* interactive"));
}

#[test]
fn run_plan_prints_the_plan_and_passes_help_through() {
    let _fx = Fx::new("plan", Some(config_with(ProviderName::None)));
    let (code, value) = json_of(&[
        "--json",
        "compression",
        "run",
        "--plan",
        "--cwd",
        "/work/x",
        "--",
        "-h",
        "--json",
        "two words",
    ]);
    assert_eq!(code, 0);
    let data = &value["data"];
    assert_eq!(data["cwd"], "/work/x");
    assert_eq!(data["routed"], false);
    assert_eq!(data["program"], "claude");
    assert_eq!(data["argv"], json!(["-h", "--json", "two words"]));
    assert_eq!(data["env"]["unset"], json!(["ANTHROPIC_BASE_URL"]));
    assert_eq!(data["proxy"], Value::Null);
}

#[test]
fn run_plan_without_separator_treats_the_first_unknown_token_as_claude_args() {
    let _fx = Fx::new("plan-bare", Some(config_with(ProviderName::None)));
    let (_, value) = json_of(&[
        "--json",
        "compression",
        "run",
        "--plan",
        "-p",
        "hello",
        "--model",
        "x",
    ]);
    assert_eq!(
        value["data"]["argv"],
        json!(["-p", "hello", "--model", "x"])
    );
    let (_, value) = json_of(&["--json", "compression", "run", "--plan", "--bogus"]);
    assert_eq!(value["data"]["argv"], json!(["--bogus"]));
}

#[cfg(unix)]
#[test]
fn run_plan_routes_headroom_only_when_the_installed_build_matches_the_pin() {
    use std::os::unix::fs::PermissionsExt;
    let mut cfg = config_with(ProviderName::Headroom);
    cfg.provider_version.pin = "0.29.0".into();
    let fx = Fx::new("plan-headroom", Some(cfg));
    let bin = fx.dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("headroom");
    std::fs::write(&exe, "#!/bin/sh\necho 'headroom 0.31.0'\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let original = std::env::var_os("PATH").unwrap_or_default();
    let mut parts = vec![bin.clone()];
    parts.extend(std::env::split_paths(&original));
    std::env::set_var("PATH", std::env::join_paths(parts).unwrap());

    let (_, drifted) = json_of(&["--json", "compression", "run", "--plan", "--cwd", "/w"]);
    std::fs::write(&exe, "#!/bin/sh\necho 'headroom 0.29.0'\n").unwrap();
    let (_, matched) = json_of(&["--json", "compression", "run", "--plan", "--cwd", "/w"]);
    let (_, forced) = {
        std::fs::write(&exe, "#!/bin/sh\necho 'headroom 0.31.0'\n").unwrap();
        json_of(&[
            "--json",
            "compression",
            "run",
            "--plan",
            "--force",
            "--cwd",
            "/w",
        ])
    };
    let (_, status) = json_of(&["--json", "compression", "status"]);
    std::env::set_var("PATH", original);

    assert_eq!(drifted["data"]["routed"], false);
    assert!(drifted["data"]["warnings"][0]
        .as_str()
        .unwrap()
        .contains("0.31.0"));
    assert_eq!(matched["data"]["routed"], true);
    assert_eq!(matched["data"]["proxy"]["port"], 8787);
    assert_eq!(
        matched["data"]["env"]["set"]["ANTHROPIC_BASE_URL"],
        "http://127.0.0.1:8787"
    );
    assert_eq!(forced["data"]["routed"], true);
    assert_eq!(status["data"]["pin"]["installed"], "0.31.0");
    assert_eq!(status["data"]["pin"]["drift"], true);
}

#[test]
fn compression_usage_errors_exit_2() {
    let _fx = Fx::new("usage", None);
    for list in [
        &["compression", "status", "extra"][..],
        &["compression", "presets", "extra"][..],
        &["compression", "run", "--cwd"][..],
    ] {
        assert_eq!(run_cli(list).0, 2, "{list:?}");
    }
    assert_eq!(run_cli(&["--bogus", "status"]).0, 2);
    assert_eq!(run_cli(&["status", "--bogus"]).0, 2);
}

#[test]
fn parse_keeps_the_separator_and_unknown_flags_for_compression_only() {
    let parsed = parse(&[
        "compression".to_string(),
        "run".into(),
        "--plan".into(),
        "--".into(),
        "-h".into(),
    ])
    .unwrap();
    assert_eq!(
        parsed.positional,
        ["compression", "run", "--plan", "--", "-h"]
    );
    assert!(!parsed.help);
    let parsed = parse(&["status".to_string(), "--".into(), "x".into()]).unwrap();
    assert_eq!(parsed.positional, ["status", "x"]);
}

mod engine_cli {
    use super::super::compression::{proxy_with, update_with, verify_with};
    use super::*;
    use crate::plus::compression::engine::EngineOps;
    use crate::plus::compression::ledger;
    use crate::plus::compression::verify::HealthProbe;
    use std::collections::BTreeMap;

    struct Quiet;

    impl HealthProbe for Quiet {
        fn binary(&self, _name: &str) -> Option<String> {
            None
        }
        fn installed_version(&self) -> Option<String> {
            None
        }
        fn proxy_health(&self, _port: u16) -> Option<Value> {
            None
        }
    }

    #[derive(Default)]
    struct Engine {
        up: bool,
        pid: Option<u64>,
        killed: Vec<u32>,
        spawned: usize,
        installs: Vec<String>,
        latest: Option<String>,
    }

    impl EngineOps for Engine {
        fn proxy_health(&self, _port: u16) -> Option<Value> {
            self.up
                .then(|| json!({"ready": true, "config": {"pid": self.pid}}))
        }
        fn spawn_proxy(
            &mut self,
            _port: u16,
            _mode: &str,
            _env: &BTreeMap<String, String>,
        ) -> Result<(), String> {
            self.spawned += 1;
            self.up = true;
            Ok(())
        }
        fn listening_pids(&self, _port: u16) -> Vec<u32> {
            Vec::new()
        }
        fn terminate(&mut self, pid: u32) -> Result<(), String> {
            self.killed.push(pid);
            self.up = false;
            Ok(())
        }
        fn sleep(&mut self, _seconds: u64) {}
        fn installed_version(&self) -> Option<String> {
            Some("0.30.0".into())
        }
        fn latest_version(&self, _package: &str) -> Option<String> {
            self.latest.clone()
        }
        fn install(
            &mut self,
            requirement: &str,
        ) -> Result<(Option<String>, Option<String>), String> {
            self.installs.push(requirement.into());
            Ok((Some("0.29.0".into()), Some("0.30.0".into())))
        }
        fn agent_savings(&self, profile: &str) -> Result<Vec<(String, String)>, String> {
            Err(format!("no profile {profile}"))
        }
    }

    fn write_transcript(root: &std::path::Path, cwd: &str, start: i64, read: u64, create: u64) {
        let dir = root.join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = format!(
            "{}\n",
            json!({"timestamp": ledger::format_ts(start), "cwd": cwd})
        );
        for _ in 0..6 {
            text.push_str(&format!(
                "{}\n",
                json!({"message": {"usage": {
                    "cache_read_input_tokens": read,
                    "cache_creation_input_tokens": create,
                }}})
            ));
        }
        std::fs::write(dir.join("s.jsonl"), text).unwrap();
    }

    #[test]
    fn ledger_records_savings_and_summarizes_per_provider() {
        let fx = Fx::new("ledger-cli", None);
        let rec = |provider: &str, before: &str, after: &str| {
            json_of(&[
                "--json",
                "compression",
                "ledger",
                "record",
                "--provider",
                provider,
                "--before",
                before,
                "--after",
                after,
                "--source",
                "synthetic",
            ])
        };
        let (code, value) = rec("rtk-only", "1000", "400");
        assert_eq!(code, 0);
        assert_eq!(value["data"]["tokensSaved"], 600);
        assert_eq!(rec("rtk-only", "500", "500").0, 0);
        assert_eq!(rec("headroom", "200", "300").0, 0);

        let (code, value) = json_of(&["--json", "compression", "ledger"]);
        assert_eq!(code, 0);
        let rows = value["data"]["providers"].as_array().unwrap();
        assert_eq!(rows[0]["provider"], "headroom");
        assert_eq!(rows[0]["tokensSaved"], -100);
        assert_eq!(rows[1]["provider"], "rtk-only");
        assert_eq!(rows[1]["tokensSaved"], 600);
        assert_eq!(rows[1]["savedPercent"], 40.0);
        assert_eq!(value["data"]["tokensSaved"], 500);

        let (_, value) = json_of(&[
            "--json",
            "compression",
            "ledger",
            "summary",
            "--provider",
            "rtk-only",
        ]);
        assert_eq!(value["data"]["providers"].as_array().unwrap().len(), 1);
        let (_, out, _) = run_cli(&["compression", "ledger"]);
        assert!(out.contains("rtk-only") && out.contains("600"));
        assert!(fx.dir.join("compression-savings.jsonl").exists());
    }

    #[test]
    fn ledger_rejects_bad_input_with_usage_errors() {
        let _fx = Fx::new("ledger-usage", None);
        for list in [
            &[
                "compression",
                "ledger",
                "record",
                "--provider",
                "rtk-only",
                "--before",
                "1",
            ][..],
            &[
                "compression",
                "ledger",
                "record",
                "--provider",
                "nope",
                "--before",
                "1",
                "--after",
                "1",
            ][..],
            &[
                "compression",
                "ledger",
                "record",
                "--provider",
                "none",
                "--before",
                "x",
                "--after",
                "1",
            ][..],
            &["compression", "ledger", "summary", "--since", "yesterday"][..],
            &["compression", "ledger", "summary", "--bogus"][..],
        ] {
            assert_eq!(run_cli(list).0, 2, "{list:?}");
        }
        let (code, value) = json_of(&["--json", "compression", "ledger"]);
        assert_eq!(code, 0);
        assert_eq!(value["data"]["providers"], json!([]));
    }

    #[test]
    fn verify_reports_checks_buckets_and_the_verdict() {
        let fx = Fx::new("verify-cli", Some(config_with(ProviderName::None)));
        let root = fx.dir.join("transcripts");
        let now = ledger::now_ms();
        write_transcript(&root, "/work/a", now + 1_000, 960, 20);
        ledger::append_launch(
            &Paths::new(&fx.dir),
            &crate::plus::compression::launch::LedgerEntry {
                cwd: "/work/a".into(),
                provider: ProviderName::Headroom,
                preset: "interactive".into(),
                routed: true,
                port: Some(8787),
                pin: "0.29.0".into(),
            },
            now,
        )
        .unwrap();
        let root_arg = root.to_str().unwrap();
        let (code, value) = json_of(&[
            "--json",
            "compression",
            "verify",
            "--transcripts",
            root_arg,
            "--by-pin",
            "--min-turns",
            "3",
        ]);
        assert_eq!(code, 0, "{value}");
        let data = &value["data"];
        assert_eq!(data["buckets"]["proxied"]["sessions"], 1);
        assert_eq!(data["buckets"]["plain"]["sessions"], 0);
        assert_eq!(data["verdict"]["pass"], true);
        assert_eq!(data["byPin"][0]["pin"], "0.29.0");
        assert!(data["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["ok"] == true));

        write_transcript(&root, "/work/a", now + 1_000, 700, 300);
        let (code, value) =
            json_of(&["--json", "compression", "verify", "--transcripts", root_arg]);
        assert_eq!(code, 1);
        assert_eq!(value["error"]["code"], "unhealthy");
        assert_eq!(value["data"]["verdict"]["pass"], false);
    }

    #[test]
    fn verify_fails_loudly_without_transcripts_or_cache_fields() {
        let fx = Fx::new("verify-empty", Some(config_with(ProviderName::None)));
        let missing = fx.dir.join("nothing");
        let (code, value) = json_of(&[
            "--json",
            "compression",
            "verify",
            "--transcripts",
            missing.to_str().unwrap(),
        ]);
        assert_eq!(code, 1);
        assert_eq!(value["data"]["transcripts"]["count"], 0);
        let (_, out, _) = run_cli(&[
            "compression",
            "verify",
            "--transcripts",
            missing.to_str().unwrap(),
        ]);
        assert!(out.contains("no Claude Code transcripts found"));

        let dir = fx.dir.join("t").join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("x.jsonl"),
            "{\"message\":{\"usage\":{\"input_tokens\":1}}}\n",
        )
        .unwrap();
        let (code, _, _) = run_cli(&[
            "compression",
            "verify",
            "--transcripts",
            fx.dir.join("t").to_str().unwrap(),
        ]);
        assert_eq!(code, 1);
    }

    #[test]
    fn verify_with_a_failing_check_exits_nonzero_even_with_good_sessions() {
        let fx = Fx::new(
            "verify-fail-check",
            Some(config_with(ProviderName::RtkOnly)),
        );
        let loaded = store::read(&Paths::new(&fx.dir)).unwrap();
        let root = fx.dir.join("t");
        write_transcript(&root, "/work/a", ledger::now_ms(), 960, 20);
        let out = verify_with(
            &Paths::new(&fx.dir),
            &loaded.config,
            &Quiet,
            &root,
            None,
            3,
            false,
        )
        .unwrap();
        assert!(out.failed);
        assert!(out.human.contains("FAIL engine"));
        assert_eq!(out.data["verdict"], Value::Null);
    }

    #[test]
    fn proxy_actions_drive_the_engine_and_report_failures() {
        let cfg = config_with(ProviderName::Headroom);
        let mut engine = Engine::default();
        let out = proxy_with(&cfg, &mut engine, "up").unwrap();
        assert_eq!(out.data["port"], 8787);
        assert_eq!(engine.spawned, 1);
        assert!(proxy_with(&cfg, &mut engine, "up")
            .unwrap()
            .human
            .starts_with("reusing"));
        assert_eq!(engine.spawned, 1);

        engine.pid = Some(99);
        let out = proxy_with(&cfg, &mut engine, "restart").unwrap();
        assert_eq!(engine.killed, [99]);
        assert_eq!(engine.spawned, 2);
        assert_eq!(out.data["steps"].as_array().unwrap().len(), 2);

        proxy_with(&cfg, &mut engine, "down").unwrap();
        assert_eq!(
            proxy_with(&cfg, &mut engine, "down").err().unwrap().code,
            "proxy_down"
        );
        let restart_cold = proxy_with(&cfg, &mut engine, "restart").unwrap();
        assert_eq!(restart_cold.data["steps"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn proxy_cli_validates_the_action() {
        let _fx = Fx::new("proxy-usage", Some(config_with(ProviderName::Headroom)));
        assert_eq!(run_cli(&["compression", "proxy"]).0, 2);
        assert_eq!(run_cli(&["compression", "proxy", "sideways"]).0, 2);
    }

    #[test]
    fn update_previews_without_changes_and_accept_moves_pin_and_installs() {
        let fx = Fx::new("update", Some(config_with(ProviderName::Headroom)));
        let paths = Paths::new(&fx.dir);
        let cfg = store::read(&paths).unwrap().config;
        let mut engine = Engine {
            latest: Some("0.30.0".into()),
            ..Engine::default()
        };
        let before = std::fs::read(paths.config()).unwrap();

        let out = update_with(&paths, cfg.clone(), &mut engine, None, true, false).unwrap();
        assert_eq!(out.data["target"], "0.30.0");
        assert_eq!(out.data["accepted"], false);
        assert!(out.human.contains("preview only"));
        assert!(engine.installs.is_empty());
        assert_eq!(std::fs::read(paths.config()).unwrap(), before);

        let out = update_with(&paths, cfg, &mut engine, None, true, true).unwrap();
        assert_eq!(out.data["installed"], "0.29.0 -> 0.30.0");
        assert_eq!(out.data["versionChanged"], true);
        assert_eq!(engine.installs, ["headroom-ai[proxy,code,ml]==0.30.0"]);
        let saved = store::read(&paths).unwrap().config;
        assert_eq!(saved.provider_version.pin, "0.30.0");
        assert!(out.human.contains("proxy restart"));
    }

    #[test]
    fn update_errors_map_to_codes() {
        let fx = Fx::new("update-err", Some(config_with(ProviderName::Headroom)));
        let paths = Paths::new(&fx.dir);
        let cfg = store::read(&paths).unwrap().config;
        let mut offline = Engine::default();
        let err = update_with(&paths, cfg.clone(), &mut offline, None, true, true)
            .err()
            .unwrap();
        assert_eq!(err.code, "update_unresolved");
        let err = update_with(&paths, cfg, &mut offline, Some("0.30.0"), true, false)
            .err()
            .unwrap();
        assert_eq!(err.code, "usage");
        assert_eq!(run_cli(&["compression", "update", "--to"]).0, 2);
    }

    #[test]
    fn registered_commands_are_listed_in_help() {
        let (_, out, _) = run_cli(&["--help"]);
        for name in [
            "compression verify",
            "compression ledger",
            "compression proxy",
            "compression update",
        ] {
            assert!(out.contains(name), "{name}");
        }
    }
}
