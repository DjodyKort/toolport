use super::otel_setup::{disable_at, enable_at, load_config, status_at, OtelConfig};
use crate::plus::testutil::tree_snapshot;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct World {
    base: PathBuf,
    data: PathBuf,
    home: PathBuf,
}

impl World {
    fn new(label: &str) -> World {
        let base = std::env::temp_dir().join(format!("otel-setup-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (data, home) = (base.join("data/obs"), base.join("home"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        World { base, data, home }
    }

    fn settings(&self) -> PathBuf {
        self.home.join(".claude/settings.json")
    }

    fn write_settings(&self, value: &Value) {
        std::fs::write(self.settings(), serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    fn read_settings(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.settings()).unwrap()).unwrap()
    }

    fn args(&self, extra: Value) -> Value {
        let mut args = json!({"home": self.home.to_string_lossy()});
        for (k, v) in extra.as_object().into_iter().flatten() {
            args[k] = v.clone();
        }
        args
    }

    fn enable(&self, extra: Value) -> Value {
        enable_at(&self.data, &self.args(extra)).unwrap()
    }

    fn disable(&self, extra: Value) -> Value {
        disable_at(&self.data, &self.args(extra)).unwrap()
    }

    fn backups(&self) -> Vec<String> {
        let root = self.home.join(".cache/mcpm/context/backups");
        let mut names = Vec::new();
        for stamp in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            for file in std::fs::read_dir(stamp.path()).into_iter().flatten().flatten() {
                names.push(file.file_name().to_string_lossy().into_owned());
            }
        }
        names
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

const KEYS: [&str; 5] = [
    "CLAUDE_CODE_ENABLE_TELEMETRY",
    "OTEL_METRICS_EXPORTER",
    "OTEL_LOGS_EXPORTER",
    "OTEL_EXPORTER_OTLP_PROTOCOL",
    "OTEL_EXPORTER_OTLP_ENDPOINT",
];

fn user_settings() -> Value {
    json!({
        "theme": "dark",
        "permissions": {"allow": ["Bash(ls:*)"], "deny": []},
        "env": {"FOO": "bar", "ANTHROPIC_MODEL": "claude-x"},
        "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "echo hi"}]}]},
        "note": "caf\u{e9}"
    })
}

fn tree(root: &Path) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
    tree_snapshot(root)
}

#[test]
fn enable_and_disable_round_trip_keeps_every_user_key() {
    let w = World::new("roundtrip");
    w.write_settings(&user_settings());

    let on = w.enable(json!({}));
    assert_eq!(on["enabled"], true);
    assert_eq!(on["changed"], true);
    assert_eq!(on["port"], 4318);
    assert_eq!(on["endpoint"], "http://127.0.0.1:4318");
    assert_eq!(on["conflicts"], json!([]));
    let after = w.read_settings();
    let env = &after["env"];
    assert_eq!(env["CLAUDE_CODE_ENABLE_TELEMETRY"], "1");
    assert_eq!(env["OTEL_METRICS_EXPORTER"], "otlp");
    assert_eq!(env["OTEL_LOGS_EXPORTER"], "otlp");
    assert_eq!(env["OTEL_EXPORTER_OTLP_PROTOCOL"], "http/json");
    assert_eq!(env["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://127.0.0.1:4318");
    assert_eq!(env["FOO"], "bar");
    assert_eq!(env["ANTHROPIC_MODEL"], "claude-x");
    for key in ["OTEL_LOG_USER_PROMPTS", "OTEL_LOG_TOOL_DETAILS", "OTEL_LOG_TOOL_CONTENT"] {
        assert!(env.get(key).is_none(), "{key} must never be written");
    }
    for key in ["theme", "permissions", "hooks", "note"] {
        assert_eq!(after[key], user_settings()[key], "{key}");
    }
    assert_eq!(w.backups(), ["settings.json"], "the original is backed up once");
    let config = load_config(&w.data);
    assert!(config.enabled);
    assert_eq!(config.managed.keys.len(), 5);

    let off = w.disable(json!({}));
    assert_eq!(off["enabled"], false);
    assert_eq!(off["changed"], true);
    assert_eq!(off["kept"], json!([]));
    assert_eq!(w.read_settings(), user_settings());
    assert_eq!(
        String::from_utf8(std::fs::read(w.settings()).unwrap()).unwrap().matches("FOO").count(),
        1
    );
    let config = load_config(&w.data);
    assert!(!config.enabled);
    assert!(config.managed.keys.is_empty());
}

#[test]
fn a_settings_file_that_did_not_exist_is_removed_again_by_disable() {
    let w = World::new("fresh");
    assert!(!w.settings().exists());
    let on = w.enable(json!({"port": 4400}));
    assert_eq!(on["endpoint"], "http://127.0.0.1:4400");
    let written = w.read_settings();
    assert_eq!(written["env"].as_object().unwrap().len(), 5);
    assert_eq!(written.as_object().unwrap().len(), 1);
    w.disable(json!({}));
    assert!(!w.settings().exists(), "the file Toolport created goes away");
    assert!(w.backups().is_empty());
}

#[test]
fn an_env_block_that_did_not_exist_is_removed_again_by_disable() {
    let w = World::new("noenv");
    w.write_settings(&json!({"theme": "dark"}));
    w.enable(json!({}));
    assert!(w.read_settings().get("env").is_some());
    w.disable(json!({}));
    assert_eq!(w.read_settings(), json!({"theme": "dark"}));
}

#[test]
fn enable_is_idempotent_and_the_second_run_writes_nothing() {
    let w = World::new("idempotent");
    w.write_settings(&user_settings());
    w.enable(json!({}));
    let files = tree(&w.base);
    let again = w.enable(json!({}));
    assert_eq!(again["changed"], false);
    assert!(again["actions"].as_array().unwrap().iter().all(|a| a.as_str().unwrap().ends_with("already set")));
    assert_eq!(tree(&w.base), files, "no write, no new backup");
    let third = w.enable(json!({"port": 4318}));
    assert_eq!(third["changed"], false);
}

#[test]
fn dry_run_writes_nothing_for_enable_and_disable() {
    let w = World::new("dry");
    w.write_settings(&user_settings());
    let before = tree(&w.base);
    let plan = w.enable(json!({"dryRun": true, "port": 4999}));
    assert_eq!(plan["dryRun"], true);
    assert_eq!(plan["changed"], true);
    assert_eq!(plan["endpoint"], "http://127.0.0.1:4999");
    assert_eq!(plan["actions"].as_array().unwrap().len(), 5);
    assert_eq!(tree(&w.base), before);
    assert!(!w.data.join("otel.json").exists());

    w.enable(json!({}));
    let enabled = tree(&w.base);
    let plan = w.disable(json!({"dryRun": true}));
    assert_eq!(plan["dryRun"], true);
    assert_eq!(plan["changed"], true);
    assert_eq!(tree(&w.base), enabled);
    assert!(load_config(&w.data).enabled);
}

#[test]
fn a_user_key_with_another_value_is_never_overwritten() {
    let w = World::new("conflict");
    let mut settings = user_settings();
    settings["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"] = json!("http://collector.internal:4318");
    settings["env"]["OTEL_METRICS_EXPORTER"] = json!("prometheus");
    w.write_settings(&settings);
    let before = tree(&w.base);
    let out = w.enable(json!({}));
    assert_eq!(out["enabled"], false);
    assert_eq!(out["changed"], false);
    assert_eq!(out["conflicts"], json!(["OTEL_METRICS_EXPORTER", "OTEL_EXPORTER_OTLP_ENDPOINT"]));
    assert!(!out.to_string().contains("collector.internal"), "values are not echoed");
    assert_eq!(tree(&w.base), before, "a conflict writes nothing at all");
}

#[test]
fn keys_the_user_already_set_to_the_same_value_stay_theirs_after_disable() {
    let w = World::new("shared");
    let mut settings = user_settings();
    settings["env"]["CLAUDE_CODE_ENABLE_TELEMETRY"] = json!("1");
    w.write_settings(&settings);
    let on = w.enable(json!({}));
    let actions = on["actions"].as_array().unwrap();
    assert_eq!(actions[0], "env.CLAUDE_CODE_ENABLE_TELEMETRY: already set");
    assert_eq!(load_config(&w.data).managed.keys.len(), 4);
    w.disable(json!({}));
    assert_eq!(w.read_settings(), settings);
}

#[test]
fn disable_keeps_a_key_the_user_changed_after_enable() {
    let w = World::new("changed");
    w.write_settings(&user_settings());
    w.enable(json!({}));
    let mut edited = w.read_settings();
    edited["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"] = json!("http://elsewhere:9");
    w.write_settings(&edited);
    let off = w.disable(json!({}));
    assert_eq!(off["kept"], json!(["OTEL_EXPORTER_OTLP_ENDPOINT"]));
    let left = w.read_settings();
    assert_eq!(left["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://elsewhere:9");
    for key in &KEYS[..4] {
        assert!(left["env"].get(*key).is_none(), "{key}");
    }
    assert_eq!(left["env"]["FOO"], "bar");
}

#[test]
fn changing_the_port_updates_only_the_endpoint_that_toolport_wrote() {
    let w = World::new("port");
    w.write_settings(&user_settings());
    w.enable(json!({}));
    let out = w.enable(json!({"port": 4400}));
    assert_eq!(out["changed"], true);
    let actions: Vec<&str> = out["actions"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert_eq!(actions[4], "env.OTEL_EXPORTER_OTLP_ENDPOINT: updated");
    assert!(actions[..4].iter().all(|a| a.ends_with("already set")));
    assert_eq!(w.read_settings()["env"]["OTEL_EXPORTER_OTLP_ENDPOINT"], "http://127.0.0.1:4400");
    assert_eq!(load_config(&w.data).port, 4400);
    let kept = w.enable(json!({}));
    assert_eq!(kept["port"], 4400, "the configured port is the default for a later enable");
    assert_eq!(kept["changed"], false);
}

#[test]
fn prompt_logging_the_user_turned_on_is_left_alone_and_reported() {
    let w = World::new("prompts");
    let mut settings = user_settings();
    settings["env"]["OTEL_LOG_USER_PROMPTS"] = json!("1");
    w.write_settings(&settings);
    let out = w.enable(json!({}));
    let warnings = out["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].as_str().unwrap().contains("OTEL_LOG_USER_PROMPTS"));
    assert_eq!(w.read_settings()["env"]["OTEL_LOG_USER_PROMPTS"], "1");
    let status = status_at(&w.data, &w.args(json!({}))).unwrap();
    assert_eq!(status["settings"]["warnings"].as_array().unwrap().len(), 1);
    w.disable(json!({}));
    assert_eq!(w.read_settings(), settings);
}

#[test]
fn unreadable_settings_and_bad_arguments_are_errors_that_write_nothing() {
    let w = World::new("errors");
    std::fs::write(w.settings(), "{ not json").unwrap();
    let before = tree(&w.base);
    let err = enable_at(&w.data, &w.args(json!({}))).unwrap_err();
    assert!(err.contains("not valid JSON"), "{err}");
    assert_eq!(tree(&w.base), before);

    w.write_settings(&json!(["array"]));
    assert!(enable_at(&w.data, &w.args(json!({}))).unwrap_err().contains("not a JSON object"));
    w.write_settings(&json!({"env": "text"}));
    assert!(enable_at(&w.data, &w.args(json!({}))).unwrap_err().contains("`env` is not an object"));

    w.write_settings(&json!({}));
    for port in [json!(0), json!(65536), json!("4318"), json!(-1), json!(1.5)] {
        let err = enable_at(&w.data, &w.args(json!({"port": port}))).unwrap_err();
        assert!(err.contains("between 1 and 65535"), "{err}");
    }
    assert!(!w.data.join("otel.json").exists());
}

#[test]
fn enabling_from_a_different_claude_home_asks_for_disable_first() {
    let w = World::new("twohomes");
    w.enable(json!({}));
    let other = w.base.join("other-home");
    std::fs::create_dir_all(&other).unwrap();
    let err = enable_at(&w.data, &json!({"home": other.to_string_lossy()})).unwrap_err();
    assert!(err.contains("already enabled through"), "{err}");
    assert!(err.contains("obs otel disable"));
    assert!(!other.join(".claude/settings.json").exists());
}

#[test]
fn disable_without_an_enable_changes_nothing_and_never_reads_settings() {
    let w = World::new("never");
    std::fs::write(w.settings(), "{ not json").unwrap();
    let before = tree(&w.base);
    let out = w.disable(json!({}));
    assert_eq!(out["changed"], false);
    assert_eq!(tree(&w.base), before);
}

#[test]
fn status_reports_settings_state_events_and_a_stopped_receiver() {
    let w = World::new("status");
    let off = status_at(&w.data, &w.args(json!({}))).unwrap();
    assert_eq!(off["enabled"], false);
    assert_eq!(off["receiver"]["state"], "disabled");
    assert_eq!(off["settings"]["state"], "missing");
    assert_eq!(off["settings"]["exists"], false);
    assert_eq!(off["events"], json!({"count": 0, "latest": null}));
    assert!(!w.data.join("obs.lock").exists(), "status takes no lock and creates nothing");
    assert!(!w.data.join("events.jsonl").exists());

    w.write_settings(&json!({"env": {"CLAUDE_CODE_ENABLE_TELEMETRY": "1", "OTEL_LOGS_EXPORTER": "none"}}));
    let partial = status_at(&w.data, &w.args(json!({}))).unwrap();
    assert_eq!(partial["settings"]["state"], "partial");
    assert_eq!(partial["settings"]["keys"]["CLAUDE_CODE_ENABLE_TELEMETRY"], "ok");
    assert_eq!(partial["settings"]["keys"]["OTEL_LOGS_EXPORTER"], "differs");
    assert_eq!(partial["settings"]["keys"]["OTEL_METRICS_EXPORTER"], "missing");

    w.write_settings(&json!({}));
    w.enable(json!({}));
    let event = r#"{"kind":"cost","tsMs":1759492800000,"day":"2025-10-03","value":0.25,"attrs":{}}"#;
    std::fs::write(w.data.join("events.jsonl"), format!("{event}\n{{\"torn")).unwrap();
    let on = status_at(&w.data, &w.args(json!({}))).unwrap();
    assert_eq!(on["enabled"], true);
    assert_eq!(on["settings"]["state"], "configured");
    assert_eq!(on["events"], json!({"count": 1, "latest": "2025-10-03T12:00:00Z"}));
    assert!(matches!(on["receiver"]["state"].as_str(), Some("stopped" | "port-in-use")));
    let config: OtelConfig = load_config(&w.data);
    assert!(config.enabled);
}
