use super::*;
use crate::plus::compression::model::{CompressionConfig, ProviderName};
use crate::plus::compression::store::{self, Paths};
use serde_json::{json, Value};
use std::path::PathBuf;

struct Fx {
    dir: PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
    _override: crate::registry::DataDirOverride,
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
            _lock: lock,
            _override: guard,
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
