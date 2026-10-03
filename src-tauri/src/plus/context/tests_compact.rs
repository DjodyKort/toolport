use super::compact;
use super::config::ProfileSpec;
use super::launch;
use super::loads::what_loads;
use super::{ContextConfig, Report, Roots};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ctx-compact-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".claude")).unwrap();
        fs::write(dir.join(".claude/settings.json"), r#"{"model":"sonnet"}"#).unwrap();
        fs::write(
            dir.join(".claude.json"),
            r#"{"mcpServers":{"alpha":{"command":"a"}}}"#,
        )
        .unwrap();
        Self(dir)
    }

    fn roots(&self) -> Roots {
        Roots::from_home(&self.0)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn spec(value: Value) -> ProfileSpec {
    serde_json::from_value(value).unwrap()
}

fn generate(roots: &Roots, s: &ProfileSpec) -> (Value, Option<String>, Report) {
    let mut report = Report::default();
    launch::generate_profile(roots, "lean", s, &mut report, false).unwrap();
    let dir = launch::profile_dir(roots, "lean");
    let settings =
        serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
    let append = fs::read_to_string(dir.join("append-system-prompt.md")).ok();
    (settings, append, report)
}

#[test]
fn settings_carry_window_per_model_and_enabled() {
    let h = Home::new("settings");
    let s = spec(json!({
        "auto_compact_window": 300000,
        "model_windows": {"model-a": 500000, "model-b": "auto"},
        "auto_compact_enabled": true
    }));
    let (settings, append, _) = generate(&h.roots(), &s);
    assert_eq!(settings["autoCompactWindow"], 300000);
    assert_eq!(settings["autoCompactEnabled"], true);
    assert_eq!(
        settings["modelSettings"]["model-a"]["autoCompactWindow"],
        500000
    );
    assert_eq!(
        settings["modelSettings"]["model-b"]["autoCompactWindow"],
        "auto"
    );
    assert_eq!(settings["model"], "sonnet");
    assert!(append.is_none());
}

#[test]
fn compact_instructions_render_into_append_layer() {
    let h = Home::new("instr");
    let roots = h.roots();
    let s = spec(json!({"rules": "none", "compact_instructions": "Keep open decisions.\n"}));
    let (_, append, _) = generate(&roots, &s);
    let append = append.unwrap();
    assert_eq!(
        append,
        format!(
            "{}\n\n# Compact instructions\n\nKeep open decisions.\n",
            launch::APPEND_HEADER
        )
    );
    let argv = launch::launch_argv(&roots, "lean", &s);
    assert!(argv.contains(&"--append-system-prompt-file".to_string()));
}

#[test]
fn unconfigured_profile_is_byte_identical_to_before() {
    let h = Home::new("identical");
    let roots = h.roots();
    let plain = spec(json!({}));
    let (settings, append, report) = generate(&roots, &plain);
    assert!(settings.get("autoCompactWindow").is_none());
    assert!(settings.get("modelSettings").is_none());
    assert!(settings.get("hooks").is_none());
    assert!(append.is_none());
    assert!(report.warnings.is_empty());
    assert_eq!(launch::launch_argv(&roots, "lean", &plain).len(), 5);
    let text = serde_json::to_string(&plain).unwrap();
    assert!(!text.contains("compact") && !text.contains("checkpoint"));
    assert_eq!(
        fs::read_to_string(launch::profile_dir(&roots, "lean").join("settings.json")).unwrap(),
        "{\n  \"model\": \"sonnet\"\n}\n"
    );
}

#[test]
fn validation_rejects_out_of_range_and_empty_keys() {
    for bad in [
        json!(99999),
        json!(1000001),
        json!("big"),
        json!(-5),
        json!(true),
    ] {
        let cfg = json!({"profiles": {"p": {"auto_compact_window": bad}}});
        assert!(ContextConfig::from_value(cfg).is_err());
    }
    assert!(
        ContextConfig::from_value(json!({"profiles": {"p": {"model_windows": {"": 200000}}}}))
            .is_err()
    );
    assert!(
        ContextConfig::from_value(json!({"profiles": {"p": {"model_windows": {"m": 50}}}}))
            .is_err()
    );
    assert!(
        ContextConfig::from_value(json!({"profiles": {"p": {"compact_instructions": "  "}}}))
            .is_err()
    );
    assert!(ContextConfig::from_value(
        json!({"profiles": {"p": {"checkpoint": {"checkpoint_at": 0}}}})
    )
    .is_err());
    for ok in [json!(100000), json!(1000000), json!("auto")] {
        assert!(
            ContextConfig::from_value(json!({"profiles": {"p": {"auto_compact_window": ok}}}))
                .is_ok()
        );
    }
}

#[test]
fn checkpoint_command_becomes_pre_compact_hook() {
    let h = Home::new("hook");
    let roots = h.roots();
    fs::write(
        h.0.join(".claude/settings.json"),
        r#"{"hooks":{"PreCompact":[{"hooks":[{"type":"command","command":"existing"}]}]}}"#,
    )
    .unwrap();
    let s = spec(json!({"checkpoint": {"command": "save-notes --quick", "checkpoint_at": 50000}}));
    let (settings, _, _) = generate(&roots, &s);
    let pre = settings["hooks"]["PreCompact"].as_array().unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[1]["hooks"][0]["command"], "save-notes --quick");
    let no_cmd = spec(json!({"checkpoint": {"checkpoint_at": 50000}}));
    let (settings, _, _) = generate(&roots, &no_cmd);
    assert_eq!(settings["hooks"]["PreCompact"].as_array().unwrap().len(), 1);
}

#[test]
fn autocompact_flag_only_when_configured() {
    let h = Home::new("flag");
    let roots = h.roots();
    let off = spec(json!({"auto_compact_window": 300000}));
    assert_eq!(launch::launch_argv(&roots, "lean", &off).len(), 5);
    let on = spec(json!({"auto_compact_window": 300000, "autocompact_flag": true}));
    let argv = launch::launch_argv(&roots, "lean", &on);
    assert_eq!(
        &argv[5..],
        ["--autocompact".to_string(), "300000".to_string()]
    );
}

#[test]
fn warns_on_env_override_and_managed_settings() {
    let h = Home::new("warn");
    let mut roots = h.roots();
    roots.env_auto_compact_window = Some("250000".into());
    let managed = h.0.join("managed.json");
    fs::write(&managed, r#"{"autoCompactWindow": 200000}"#).unwrap();
    roots.managed_settings = Some(managed);
    let s = spec(json!({"auto_compact_window": 300000}));
    let (_, _, report) = generate(&roots, &s);
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("CLAUDE_CODE_AUTO_COMPACT_WINDOW=250000")));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("managed settings")));
    let (_, _, quiet) = generate(&roots, &spec(json!({})));
    assert!(quiet.warnings.is_empty());
}

#[test]
fn what_loads_reports_window_and_checkpoint() {
    let h = Home::new("loads");
    let mut roots = h.roots();
    let mut cfg = ContextConfig::default();
    cfg.profiles.insert(
        "lean".into(),
        spec(json!({
            "auto_compact_window": 300000,
            "checkpoint": {"checkpoint_at": 40000},
            "compact_instructions": "Keep open decisions."
        })),
    );
    cfg.profiles.insert("plain".into(), spec(json!({})));
    let cwd = Path::new("/nonexistent-cwd");
    let r = what_loads(&roots, &cfg, Some("lean"), cwd).unwrap();
    let c = r.compact.unwrap();
    assert_eq!(c.window, json!(300000));
    assert_eq!(c.window_source, "profile");
    assert_eq!(c.checkpoint_point, Some(260000));
    assert!(c.instructions_tokens > 0);
    assert!(what_loads(&roots, &cfg, Some("plain"), cwd)
        .unwrap()
        .compact
        .is_none());
    roots.env_auto_compact_window = Some("200000".into());
    let r = what_loads(&roots, &cfg, Some("lean"), cwd).unwrap();
    let c = r.compact.unwrap();
    assert_eq!((c.window_source, c.checkpoint_point), ("env", Some(160000)));
    assert!(r.notes.iter().any(|n| n.contains(compact::ENV_WINDOW)));
}

#[test]
fn checkpoint_status_reports_used_against_offset_window() {
    let h = Home::new("status");
    let roots = h.roots();
    let s = spec(json!({"auto_compact_window": 300000, "checkpoint": {"checkpoint_at": 50000}}));
    let line = json!({
        "model": {"id": "model-a"},
        "context_window": {
            "context_window_size": 1000000,
            "current_usage": {"input_tokens": 1000, "cache_creation_input_tokens": 4000, "cache_read_input_tokens": 245000, "output_tokens": 9}
        }
    });
    let out = compact::checkpoint_status(&roots, &line, Some(&s), None, None).unwrap();
    assert_eq!(out["used_tokens"], 250000);
    assert_eq!(out["window"], 300000);
    assert_eq!(out["checkpoint_point"], 250000);
    assert_eq!(out["at_checkpoint"], true);
    assert_eq!(out["remaining_to_checkpoint"], 0);
    let pct = json!({"context_window": {"context_window_size": 200000, "used_percentage": 50}});
    let out = compact::checkpoint_status(&roots, &pct, None, None, Some(20000)).unwrap();
    assert_eq!(out["used_tokens"], 100000);
    assert_eq!(out["checkpoint_point"], 180000);
    assert_eq!(out["at_checkpoint"], false);
    assert!(compact::checkpoint_status(&roots, &json!({}), None, None, None).is_err());
}

#[test]
fn ctl_checkpoint_status_reads_stdin() {
    let h = Home::new("ctl");
    let roots = h.roots();
    let mut input: &[u8] =
        br#"{"context_window":{"context_window_size":200000,"used_percentage":10}}"#;
    let out = crate::plus::ctl::context::checkpoint_status_from(
        &["--checkpoint-at=30000".to_string()],
        &mut input,
        &roots,
    )
    .unwrap();
    assert_eq!(out.data["used_tokens"], 20000);
    assert_eq!(out.data["checkpoint_point"], 170000);
    let mut bad: &[u8] = b"not json";
    assert!(crate::plus::ctl::context::checkpoint_status_from(&[], &mut bad, &roots).is_err());
}
