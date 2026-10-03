//! Mirrors mcpm-compression's tests (test_presets, test_pin, test_migrate, test_capability,
//! test_providers, test_run, parts of test_apply/test_update) on the pure Rust core. Tests
//! that need a live headroom, PyPI, the mcpm core or the ledger/verify code are not ported.

use super::capability::*;
use super::launch::*;
use super::model::*;
use super::ops::*;
use super::provider::*;
use super::shims::*;
use super::store::{self, Paths};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

static SEQ: AtomicUsize = AtomicUsize::new(0);
static SPAWN_LOCK: Mutex<()> = Mutex::new(());

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "tp-compression-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn paths(&self) -> Paths {
        Paths::new(&self.0)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct FakeProbe(Option<&'static str>);

impl Probe for FakeProbe {
    fn headroom_version(&self) -> Option<String> {
        self.0.map(str::to_string)
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn headroom_config() -> CompressionConfig {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg.runtime = RuntimeKind::Proxy;
    cfg.contexts
        .push(ContextRule::new("*/agent-batch/*", None, Some("agent")));
    let agent = cfg.presets.get_mut("agent").unwrap();
    agent
        .knobs
        .insert("HEADROOM_SAVINGS_PROFILE", KnobSpec::new("agent-90"));
    agent.knobs.insert("HEADROOM_MODE", KnobSpec::new("token"));
    cfg
}

fn env_of(plan: &LaunchPlan) -> BTreeMap<String, String> {
    plan.child_env(&BTreeMap::new())
}

// ---- test_presets ----

#[test]
fn presets_01_default_presets_present() {
    let cfg = CompressionConfig::default();
    for name in ["interactive", "agent", "balanced"] {
        assert!(cfg.presets.contains_key(name), "{name}");
    }
    assert_eq!(cfg.active_preset, "interactive");
    assert_eq!(
        cfg.presets.get("interactive").unwrap().mode,
        CompressionMode::Cache
    );
    let agent = cfg.presets.get("agent").unwrap();
    assert_eq!(agent.mode, CompressionMode::Token);
    assert_eq!(agent.savings_profile.as_deref(), Some("agent-90"));
    assert_eq!(agent.port, AGENT_PORT);
}

#[test]
fn presets_02_resolve_returns_provider_and_preset() {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg.contexts = vec![
        ContextRule::new("*/clients/*", Some(ProviderName::None), None),
        ContextRule::new("*/agent-batch/*", None, Some("agent")),
    ];
    assert_eq!(
        cfg.resolve("/Users/x/clients/acme"),
        (ProviderName::None, "interactive".into())
    );
    assert_eq!(
        cfg.resolve("/Users/x/agent-batch/run"),
        (ProviderName::Headroom, "agent".into())
    );
    assert_eq!(
        cfg.resolve("/Users/x/personal"),
        (ProviderName::Headroom, "interactive".into())
    );
    assert_eq!(
        cfg.resolved_provider("/Users/x/clients/acme"),
        ProviderName::None
    );
}

#[test]
fn presets_03_env_layers_mode_and_base_url() {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg.options.insert("telemetry".into(), json!("off"));
    let mut agent = CompressionPreset::new(CompressionMode::Token, Some("agent-90"), AGENT_PORT);
    agent
        .knobs
        .insert("HEADROOM_SAVINGS_PROFILE", KnobSpec::new("agent-90"));
    agent.knobs.insert("HEADROOM_MODE", KnobSpec::new("token"));
    let env = env_for_preset(&cfg, &agent);
    assert_eq!(
        env.get("ANTHROPIC_BASE_URL").unwrap(),
        &format!("http://127.0.0.1:{AGENT_PORT}")
    );
    assert_eq!(env.get("ENABLE_TOOL_SEARCH").unwrap(), "true");
    assert_eq!(env.get("HEADROOM_MODE").unwrap(), "token");
    assert_eq!(env.get("HEADROOM_SAVINGS_PROFILE").unwrap(), "agent-90");
    assert_eq!(env.get("HEADROOM_TELEMETRY").unwrap(), "off");
}

#[test]
fn presets_04_preset_mode_overrides_snapshot_mode() {
    let cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    let mut p = CompressionPreset::default();
    p.knobs.insert("HEADROOM_MODE", KnobSpec::new("token"));
    assert_eq!(
        env_for_preset(&cfg, &p).get("HEADROOM_MODE").unwrap(),
        "cache"
    );
}

#[test]
fn presets_05_declared_off_knob_is_emitted_explicitly() {
    let cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    let mut p = CompressionPreset::default();
    p.knobs
        .insert("HEADROOM_CODE_AWARE_ENABLED", KnobSpec::new("0"));
    assert_eq!(
        env_for_preset(&cfg, &p)
            .get("HEADROOM_CODE_AWARE_ENABLED")
            .unwrap(),
        "0"
    );
}

#[test]
fn presets_06_knobs_are_gated_by_the_pin() {
    let mut p = CompressionPreset::default();
    p.knobs.insert(
        "HEADROOM_PROTECT_READS",
        KnobSpec::new("1").applies_to(">=0.31.0"),
    );
    p.knobs.insert("HEADROOM_MODE", KnobSpec::new("cache"));
    let on_29 = env_for_preset(
        &CompressionConfig::with_provider(ProviderName::Headroom),
        &p,
    );
    assert!(!on_29.contains_key("HEADROOM_PROTECT_READS"));
    let mut cfg31 = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg31.provider_version.pin = "0.31.0".into();
    assert_eq!(
        env_for_preset(&cfg31, &p)
            .get("HEADROOM_PROTECT_READS")
            .unwrap(),
        "1"
    );
}

#[test]
fn presets_07_interactive_preset_has_no_savings_env() {
    let cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    let env = env_for_preset(&cfg, &cfg.preset_for(Some("interactive")));
    assert_eq!(env.get("HEADROOM_MODE").unwrap(), "cache");
    assert_eq!(
        env.get("ANTHROPIC_BASE_URL").unwrap(),
        &format!("http://127.0.0.1:{DEFAULT_PORT}")
    );
    assert!(!env.contains_key("HEADROOM_SAVINGS_PROFILE"));
}

#[test]
fn presets_08_pin_is_exact_never_a_floor() {
    let pv = CompressionConfig::default().provider_version;
    assert_eq!(pv.pin, DEFAULT_PIN);
    let req = pv.requirement();
    assert!(req.contains("==") && !req.contains(">="));
    assert!(req.starts_with("headroom-ai["));
}

#[test]
fn preset_for_never_fails() {
    let mut cfg = CompressionConfig::default();
    cfg.active_preset = "ghost".into();
    assert_eq!(cfg.preset_for(None).port, DEFAULT_PORT);
    cfg.presets = OrderedMap::new();
    assert_eq!(cfg.preset_for(Some("x")), CompressionPreset::default());
}

// ---- test_pin ----

#[test]
fn pin_01_requirement_is_exact_with_extras() {
    let mut cfg = CompressionConfig::default();
    cfg.provider_version.pin = "0.29.0".into();
    assert_eq!(
        cfg.provider_version.requirement(),
        "headroom-ai[proxy,code,ml]==0.29.0"
    );
    cfg.provider_version.extras.clear();
    assert_eq!(cfg.provider_version.requirement(), "headroom-ai==0.29.0");
}

#[test]
fn pin_04_set_pin_persists_and_reads_as_drift() {
    let tmp = Tmp::new("pin");
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    set_pin(&mut cfg, "0.30.0");
    store::save(&tmp.paths(), &cfg).unwrap();
    let back = store::read(&tmp.paths()).unwrap().config;
    assert_eq!(back.provider_version.pin, "0.30.0");
    assert!(
        matches!(pin_guard(&back.provider_version.pin, Some("0.31.0"), false), PinGuard::Refused(m) if m.contains("0.31.0"))
    );
}

#[test]
fn pin_05_match_needs_no_install_and_any_mismatch_is_drift() {
    assert_eq!(pin_guard("0.29.0", Some("0.29.0"), false), PinGuard::Match);
    assert_eq!(pin_guard("0.29.0", Some("0.29.0"), true), PinGuard::Match);
    // newer is unverified, not fine
    assert!(matches!(
        pin_guard("0.29.0", Some("0.31.0"), false),
        PinGuard::Refused(_)
    ));
    assert!(
        matches!(pin_guard("0.29.0", None, false), PinGuard::Refused(m) if m.contains("not on PATH"))
    );
    assert!(matches!(
        pin_guard("0.29.0", Some("0.31.0"), true),
        PinGuard::Forced(_)
    ));
}

#[test]
fn pin_05b_refresh_preserves_policy_knobs_and_replaces_profile_knobs() {
    let mut preset = CompressionPreset::new(CompressionMode::Cache, Some("agent-90"), 8788);
    preset
        .knobs
        .insert("HEADROOM_CODE_AWARE_ENABLED", KnobSpec::new("0"));
    preset.knobs.insert(
        "HEADROOM_MAX_ITEMS",
        KnobSpec::with_source("99", KnobSource::Profile),
    );
    preset.knobs.insert(
        "HEADROOM_GONE",
        KnobSpec::with_source("1", KnobSource::Profile),
    );
    let profile = vec![
        ("HEADROOM_MAX_ITEMS".to_string(), "8".to_string()),
        ("HEADROOM_CODE_AWARE_ENABLED".to_string(), "1".to_string()),
    ];
    let diff = apply_snapshot(&mut preset, &profile, Some("0.29.0"));
    let code = preset.knobs.get("HEADROOM_CODE_AWARE_ENABLED").unwrap();
    assert_eq!(
        (code.value.as_str(), code.source),
        ("0", KnobSource::Policy)
    );
    assert_eq!(preset.knobs.get("HEADROOM_MAX_ITEMS").unwrap().value, "8");
    assert!(!preset.knobs.contains_key("HEADROOM_GONE"));
    assert_eq!(preset.snapshot_version.as_deref(), Some("0.29.0"));
    assert_eq!(
        diff.removed,
        vec![("HEADROOM_GONE".to_string(), "1".to_string())]
    );
    assert_eq!(
        diff.moved,
        vec![(
            "HEADROOM_MAX_ITEMS".to_string(),
            "99".to_string(),
            "8".to_string()
        )]
    );
    assert_eq!(diff.kept, vec!["HEADROOM_CODE_AWARE_ENABLED".to_string()]);
}

// ---- test_migrate ----

fn old_config() -> Value {
    json!({
        "provider": "headroom",
        "runtime": "proxy",
        "presets": {"agent": {
            "mode": "token",
            "savings_profile": "agent-90",
            "env": {"HEADROOM_MODE": "token", "HEADROOM_CODE_AWARE_ENABLED": "1"},
            "intercept_tool_results": false,
            "code_aware": false,
            "port": 8788
        }},
        "active_preset": "agent"
    })
}

#[test]
fn migrate_01_flat_env_becomes_profile_sourced_knobs() {
    let (migrated, _) = store::migrate_raw(old_config());
    let agent = &migrated["presets"]["agent"];
    assert_eq!(
        agent["knobs"]["HEADROOM_MODE"],
        json!({"value": "token", "applies": "*", "source": "profile"})
    );
    assert!(agent.get("env").is_none());
}

#[test]
fn migrate_02_declared_false_flag_becomes_explicit_zero() {
    let (migrated, notes) = store::migrate_raw(old_config());
    let knob = &migrated["presets"]["agent"]["knobs"]["HEADROOM_CODE_AWARE_ENABLED"];
    assert_eq!(knob["value"], "0");
    assert_eq!(knob["source"], "policy");
    assert!(notes.iter().any(|n| n.contains("was being ignored")));
}

#[test]
fn migrate_03_migration_is_idempotent() {
    let (once, _) = store::migrate_raw(old_config());
    let (twice, notes) = store::migrate_raw(once.clone());
    assert_eq!(once, twice);
    assert!(notes.is_empty());
}

#[test]
fn migrate_04_migrated_config_validates_and_pins() {
    let (cfg, _) = store::parse(&old_config().to_string()).unwrap();
    assert!(cfg.provider_version.requirement().contains("=="));
    assert_eq!(cfg.presets.get("agent").unwrap().snapshot_version, None);
}

#[test]
fn migrate_legacy_files_keep_python_key_order() {
    let text = r#"{"presets": {"zz": {"mode": "cache"}, "agent": {"mode": "token",
        "env": {"HEADROOM_MODE": "token", "HEADROOM_CODE_AWARE_ENABLED": "1"},
        "intercept_tool_results": false, "code_aware": false, "port": 8788}}}"#;
    let (cfg, _) = store::parse(text).unwrap();
    assert_eq!(
        cfg.presets.keys().cloned().collect::<Vec<_>>(),
        ["zz", "agent"]
    );
    let knobs: Vec<_> = cfg
        .presets
        .get("agent")
        .unwrap()
        .knobs
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        knobs,
        [
            "HEADROOM_MODE",
            "HEADROOM_CODE_AWARE_ENABLED",
            "HEADROOM_INTERCEPT_ENABLED"
        ]
    );
}

#[test]
fn migrate_05_empty_and_odd_configs_survive() {
    assert_eq!(
        store::migrate_raw(json!({})).0,
        json!({"provider_version": {}})
    );
    assert_eq!(
        store::migrate_raw(json!({"presets": {"x": "not-a-dict"}})).0["presets"]["x"],
        "not-a-dict"
    );
    assert_eq!(store::migrate_raw(json!([1])).0, json!([1]));
}

#[test]
fn migrate_06_version_ranges() {
    assert!(spec_matches(Some("0.31.0"), ">=0.30.0,<0.32.0"));
    assert!(!spec_matches(Some("0.29.0"), ">=0.30.0,<0.32.0"));
    assert!(!spec_matches(Some("0.32.0"), ">=0.30.0,<0.32.0"));
    assert!(spec_matches(Some("0.29.0"), "*"));
    assert!(spec_matches(Some("0.29.0"), "==0.29.0"));
    assert!(spec_matches(Some("0.29.0"), "0.29.0"));
    assert!(spec_matches(Some("0.29.0"), ""));
    assert!(spec_matches(None, "*"));
    assert!(!spec_matches(None, ">=0.30.0"));
    assert!(!spec_matches(Some("0.29.0rc1"), ">=0.1.0"));
}

// ---- test_capability ----

#[test]
fn capability_01_scan_counts_only_genuine_env_reads() {
    let tmp = Tmp::new("scan");
    std::fs::write(
        tmp.path().join("m.py"),
        "HELP = \"Set HEADROOM_MENTIONED_ONLY=1 to enable.\"\n# HEADROOM_IN_A_COMMENT\n\
         a = os.environ.get(\"HEADROOM_READ_A\", \"\")\nb = _get_env_bool(\"HEADROOM_READ_B\", True)\n\
         c = os.environ[\"HEADROOM_READ_C\"]\n",
    )
    .unwrap();
    let got: Vec<String> = read_knobs(tmp.path()).into_iter().collect();
    assert_eq!(
        got,
        ["HEADROOM_READ_A", "HEADROOM_READ_B", "HEADROOM_READ_C"]
    );
}

#[test]
fn capability_02_inert_maps_the_vendors_own_broken_knob() {
    let tmp = Tmp::new("inert");
    std::fs::create_dir_all(tmp.path().join("pkg")).unwrap();
    std::fs::write(
        tmp.path().join("pkg/m.py"),
        "x = os.environ.get(\"HEADROOM_FORCE_KOMPRESS_ALL\", \"\")\n",
    )
    .unwrap();
    let inert = inert_for(&["HEADROOM_FORCE_KOMPRESS".to_string()], tmp.path());
    assert_eq!(
        inert,
        BTreeMap::from([(
            "HEADROOM_FORCE_KOMPRESS".to_string(),
            Some("HEADROOM_FORCE_KOMPRESS_ALL".to_string())
        )])
    );
}

#[test]
fn capability_03_unlocatable_build_reports_nothing_inert() {
    let tmp = Tmp::new("nobuild");
    assert!(inert_for(&["HEADROOM_ANYTHING".to_string()], &tmp.path().join("nope")).is_empty());
}

#[test]
fn capability_04_env_rendering_refuses_to_invent_a_value_for_unset() {
    assert_eq!(as_env_value(&Value::Null), None);
    assert_eq!(as_env_value(&json!(false)).as_deref(), Some("0"));
    assert_eq!(as_env_value(&json!(true)).as_deref(), Some("1"));
    assert_eq!(as_env_value(&json!(50)).as_deref(), Some("50"));
    assert_eq!(as_env_value(&json!("strict")).as_deref(), Some("strict"));
}

fn live(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[test]
fn capability_05_unsealed_splits_declarable_from_unsealable() {
    let effective = live(json!({
        "compress_user_messages": false,
        "max_items_after_crush": 50,
        "accuracy_guard": null,
        "protect_recent": null
    }));
    let unsealed = unsealed_for(&OrderedMap::new(), &effective);
    let (declarable, unset) = split_unsealed(&unsealed);
    assert_eq!(
        declarable,
        vec![
            (
                "HEADROOM_COMPRESS_USER_MESSAGES".to_string(),
                "0".to_string()
            ),
            ("HEADROOM_MAX_ITEMS".to_string(), "50".to_string())
        ]
    );
    let mut unset = unset;
    unset.sort();
    assert_eq!(
        unset,
        ["HEADROOM_ACCURACY_GUARD", "HEADROOM_PROTECT_RECENT"]
    );
}

#[test]
fn capability_06_declared_knobs_are_not_reported_unsealed() {
    let effective = live(json!({"compress_user_messages": false, "max_items_after_crush": 50}));
    let mut declared = OrderedMap::new();
    declared.insert("HEADROOM_COMPRESS_USER_MESSAGES", KnobSpec::new("0"));
    let names: Vec<String> = unsealed_for(&declared, &effective)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert!(!names.contains(&"HEADROOM_COMPRESS_USER_MESSAGES".to_string()));
    assert!(names.contains(&"HEADROOM_MAX_ITEMS".to_string()));
}

#[test]
fn capability_07_no_proxy_means_no_claim_about_the_posture() {
    assert!(unsealed_for(&OrderedMap::new(), &Map::new()).is_empty());
}

#[test]
fn seal_declares_the_live_posture_as_policy() {
    let mut preset = CompressionPreset::default();
    let n = seal(&mut preset, &[("HEADROOM_MAX_ITEMS".into(), "50".into())]);
    assert_eq!(n, 1);
    let knob = preset.knobs.get("HEADROOM_MAX_ITEMS").unwrap();
    assert_eq!(
        (knob.value.as_str(), knob.source),
        ("50", KnobSource::Policy)
    );
}

// ---- test_providers ----

#[test]
fn providers_01_four_providers() {
    let names: Vec<&str> = ProviderName::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(names, ["headroom", "rtk-only", "parsec", "none"]);
    assert_eq!(ProviderName::parse("rtk-only"), Some(ProviderName::RtkOnly));
    assert_eq!(ProviderName::parse("nope"), None);
}

#[test]
fn providers_02_headroom_mcp_and_runtime() {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg.presets.get_mut("interactive").unwrap().port = 9000;
    let mcp = mcp_server_config(ProviderName::Headroom).unwrap();
    assert_eq!(mcp["command"], "headroom");
    assert_eq!(mcp["args"], json!(["mcp", "serve"]));
    assert_eq!(mcp["proxy_mode"], "direct");
    let rt = runtime_spec(ProviderName::Headroom, &cfg);
    assert_eq!((rt.kind, rt.port), (RuntimeKind::Proxy, 9000));
    assert_eq!(
        rt.env.get("ANTHROPIC_BASE_URL").unwrap(),
        "http://127.0.0.1:9000"
    );
    assert_eq!(rt.env.get("ENABLE_TOOL_SEARCH").unwrap(), "true");
    assert_eq!(rt.env.get("HEADROOM_TELEMETRY").unwrap(), "off");
}

#[test]
fn providers_03_headroom_generates_env_snippet_and_shim() {
    let tmp = Tmp::new("art");
    let arts = activation_artifacts(
        ProviderName::Headroom,
        &CompressionConfig::with_provider(ProviderName::Headroom),
        &tmp.paths(),
    );
    let names: Vec<_> = arts
        .iter()
        .map(|a| a.path.file_name().unwrap().to_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["compression-env.sh", "compression-shims.zsh"]);
    assert_eq!(arts[0].mode, 0o600);
    assert!(arts[0]
        .content
        .contains("ANTHROPIC_BASE_URL=\"http://127.0.0.1:8787\""));
    assert!(arts[0].content.contains("HEADROOM_MODE=\"cache\""));
    assert_eq!(arts[1].mode, 0o644);
    assert!(arts[1]
        .content
        .contains("hrclaude() { toolportctl compression run -- \"$@\"; }"));
    assert!(arts[1].content.contains("hrup()") && arts[1].content.contains("hrdown()"));
}

#[test]
fn providers_04_rtk_only_has_no_mcp_no_artifacts() {
    let cfg = CompressionConfig::with_provider(ProviderName::RtkOnly);
    let tmp = Tmp::new("rtk");
    assert!(mcp_server_config(ProviderName::RtkOnly).is_none());
    assert!(activation_artifacts(ProviderName::RtkOnly, &cfg, &tmp.paths()).is_empty());
    assert_eq!(
        runtime_spec(ProviderName::RtkOnly, &cfg).kind,
        RuntimeKind::Hook
    );
}

#[test]
fn providers_05_none_and_parsec_are_inert_to_a_launch() {
    let tmp = Tmp::new("none");
    for p in [ProviderName::None, ProviderName::Parsec] {
        let cfg = CompressionConfig::with_provider(p);
        assert!(mcp_server_config(p).is_none());
        assert!(activation_artifacts(p, &cfg, &tmp.paths()).is_empty());
        assert!(runtime_spec(p, &cfg).env.is_empty());
    }
    assert_eq!(
        runtime_spec(ProviderName::None, &CompressionConfig::default()).kind,
        RuntimeKind::None
    );
    assert_eq!(
        runtime_spec(ProviderName::Parsec, &CompressionConfig::default()).kind,
        RuntimeKind::Plugin
    );
}

#[test]
fn providers_06_resolved_provider_matches_context_then_default() {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg.contexts = vec![
        ContextRule::new("*/clients/*", Some(ProviderName::None), None),
        ContextRule::new("*/oss/*", Some(ProviderName::RtkOnly), None),
    ];
    assert_eq!(
        cfg.resolved_provider("/Users/x/work/clients/acme"),
        ProviderName::None
    );
    assert_eq!(
        cfg.resolved_provider("/Users/x/oss/foo"),
        ProviderName::RtkOnly
    );
    assert_eq!(
        cfg.resolved_provider("/Users/x/personal"),
        ProviderName::Headroom
    );
}

// ---- test_apply (06) + test_update ----

#[test]
fn apply_06_enable_preserves_existing_contexts_and_options() {
    let mut cfg = CompressionConfig::with_provider(ProviderName::None);
    cfg.clients = vec!["claude-code".into(), "cursor".into()];
    cfg.contexts = vec![ContextRule::new(
        "*/clients/*",
        Some(ProviderName::None),
        None,
    )];
    cfg.options.insert("port".into(), json!(9001));
    cfg.options.insert("telemetry".into(), json!("off"));
    enable(
        &mut cfg,
        &EnableOpts {
            provider: Some(ProviderName::Headroom),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(cfg.provider, ProviderName::Headroom);
    assert_eq!(cfg.runtime, RuntimeKind::Proxy);
    assert_eq!(
        cfg.contexts
            .iter()
            .map(|c| c.pattern.as_str())
            .collect::<Vec<_>>(),
        ["*/clients/*"]
    );
    assert_eq!(cfg.clients, ["claude-code", "cursor"]);
    assert_eq!(cfg.options["port"], 9001);
    assert_eq!(cfg.legacy_port(), 9001);
}

#[test]
fn enable_applies_preset_mode_port_telemetry_and_rejects_unknown_preset() {
    let mut cfg = CompressionConfig::default();
    enable(
        &mut cfg,
        &EnableOpts {
            preset: Some("agent".into()),
            mode: Some(CompressionMode::Cache),
            port: Some(9100),
            telemetry: Some("on".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let agent = cfg.presets.get("agent").unwrap();
    assert_eq!(
        (cfg.active_preset.as_str(), agent.mode, agent.port),
        ("agent", CompressionMode::Cache, 9100)
    );
    assert_eq!(cfg.telemetry(), "on");
    let err = enable(
        &mut cfg,
        &EnableOpts {
            preset: Some("ghost".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.contains("ghost") && err.contains("interactive"));
}

#[test]
fn use_set_provider_and_disable() {
    let mut cfg = CompressionConfig::default();
    use_preset(&mut cfg, "balanced").unwrap();
    assert_eq!(cfg.active_preset, "balanced");
    assert!(use_preset(&mut cfg, "ghost").is_err());
    assert_eq!(cfg.active_preset, "balanced");
    set_provider(&mut cfg, ProviderName::RtkOnly);
    assert_eq!(cfg.runtime, RuntimeKind::Hook);
    disable(&mut cfg);
    assert_eq!(
        (cfg.provider, cfg.runtime),
        (ProviderName::None, RuntimeKind::None)
    );
    assert!(cfg.presets.contains_key("agent"));
}

#[test]
fn update_03_preview_does_not_move_the_pin() {
    let cfg = CompressionConfig::default();
    let t = resolve_update(&cfg, None, false, || Some("0.31.0".into())).unwrap();
    assert_eq!(
        (t.current.as_str(), t.target.as_str(), t.same),
        ("0.29.0", "0.31.0", false)
    );
    assert_eq!(cfg.provider_version.pin, "0.29.0");
}

#[test]
fn update_04_accept_sets_an_exact_pin_with_extras() {
    let mut cfg = CompressionConfig::default();
    let t = resolve_update(&cfg, None, true, || Some("0.31.0".into())).unwrap();
    set_pin(&mut cfg, &t.target);
    assert_eq!(
        cfg.provider_version.requirement(),
        "headroom-ai[proxy,code,ml]==0.31.0"
    );
}

#[test]
fn update_05_to_names_an_exact_version_and_skips_the_lookup() {
    let cfg = CompressionConfig::default();
    let t = resolve_update(&cfg, Some("0.30.1"), false, || {
        panic!("lookup must not run")
    })
    .unwrap();
    assert_eq!(t.target, "0.30.1");
    assert!(
        resolve_update(&cfg, Some("0.29.0"), false, || None)
            .unwrap()
            .same
    );
}

#[test]
fn update_06_to_and_latest_together_is_rejected() {
    let cfg = CompressionConfig::default();
    assert_eq!(
        resolve_update(&cfg, Some("0.30.0"), true, || None),
        Err(UpdateError::Conflict)
    );
}

#[test]
fn update_07_unresolvable_latest_fails_loud() {
    let cfg = CompressionConfig::default();
    assert_eq!(
        resolve_update(&cfg, None, true, || None),
        Err(UpdateError::Unresolvable)
    );
}

#[test]
fn update_08_unparseable_target_is_rejected() {
    let cfg = CompressionConfig::default();
    let err = resolve_update(&cfg, Some("latest"), false, || None).unwrap_err();
    assert_eq!(err, UpdateError::Unparseable("latest".into()));
    assert!(err.message().contains("X.Y.Z"));
}

// ---- model and store ----

const DEFAULT_GOLDEN: &str = r#"{
  "provider": "none",
  "runtime": "none",
  "provider_version": {
    "package": "headroom-ai",
    "pin": "0.29.0",
    "extras": [
      "proxy",
      "code",
      "ml"
    ]
  },
  "scope": [
    "default"
  ],
  "clients": [
    "claude-code"
  ],
  "contexts": [],
  "presets": {
    "interactive": {
      "mode": "cache",
      "savings_profile": null,
      "knobs": {},
      "snapshot_version": null,
      "port": 8787
    },
    "agent": {
      "mode": "token",
      "savings_profile": "agent-90",
      "knobs": {},
      "snapshot_version": null,
      "port": 8788
    },
    "balanced": {
      "mode": "token",
      "savings_profile": "balanced",
      "knobs": {},
      "snapshot_version": null,
      "port": 8787
    }
  },
  "active_preset": "interactive",
  "options": {}
}
"#;

#[test]
fn default_config_serializes_exactly_like_the_pydantic_dump() {
    assert_eq!(
        store::render(&CompressionConfig::default()).unwrap(),
        DEFAULT_GOLDEN
    );
    let (back, notes) = store::parse(DEFAULT_GOLDEN).unwrap();
    assert!(notes.is_empty());
    assert_eq!(back, CompressionConfig::default());
}

#[test]
fn round_trip_keeps_order_knobs_non_ascii_and_unknown_fields() {
    let text = r#"{
  "provider": "headroom",
  "runtime": "proxy",
  "provider_version": {"package": "headroom-ai", "pin": "0.29.0", "extras": [], "future_pv": 1},
  "scope": ["default"],
  "clients": ["claude-code"],
  "contexts": [{"match": "*/café/*", "provider": "none", "preset": null, "note": "x"}],
  "presets": {
    "zeta": {"mode": "cache", "savings_profile": null,
             "knobs": {"HEADROOM_Z": {"value": "1", "applies": ">=0.29.0", "source": "profile", "why": "y"},
                       "HEADROOM_A": {"value": "0"}},
             "snapshot_version": "0.29.0", "port": 9000, "future_preset": [1, 2]},
    "alpha": {"mode": "token"}
  },
  "active_preset": "zeta",
  "options": {"telemetry": "off"},
  "futureTop": {"keep": true}
}"#;
    let (cfg, _) = store::parse(text).unwrap();
    let names: Vec<_> = cfg.presets.keys().cloned().collect();
    assert_eq!(names, ["zeta", "alpha"]);
    let knobs: Vec<_> = cfg
        .presets
        .get("zeta")
        .unwrap()
        .knobs
        .keys()
        .cloned()
        .collect();
    assert_eq!(knobs, ["HEADROOM_Z", "HEADROOM_A"]);
    assert_eq!(
        cfg.presets
            .get("zeta")
            .unwrap()
            .knobs
            .get("HEADROOM_A")
            .unwrap()
            .source,
        KnobSource::Policy
    );
    let out = store::render(&cfg).unwrap();
    assert!(out.is_ascii());
    assert!(out.contains("\"*/caf\\u00e9/*\""));
    assert!(
        out.contains("\"futureTop\"")
            && out.contains("\"future_pv\"")
            && out.contains("\"future_preset\"")
    );
    assert!(out.contains("\"why\": \"y\"") && out.contains("\"note\": \"x\""));
    assert!(out.find("HEADROOM_Z").unwrap() < out.find("HEADROOM_A").unwrap());
    assert!(out.find("\"zeta\"").unwrap() < out.find("\"alpha\"").unwrap());
    assert_eq!(store::parse(&out).unwrap().0, cfg);
    assert_eq!(store::render(&store::parse(&out).unwrap().0).unwrap(), out);
}

#[test]
fn astral_characters_escape_as_surrogate_pairs() {
    let mut cfg = CompressionConfig::default();
    cfg.options.insert("note".into(), json!("a\u{1F600}b"));
    let out = store::render(&cfg).unwrap();
    assert!(out.contains("a\\ud83d\\ude00b"), "{out}");
    assert_eq!(store::parse(&out).unwrap().0.options["note"], "a\u{1F600}b");
}

#[test]
fn load_save_is_atomic_private_and_missing_means_default() {
    let tmp = Tmp::new("store");
    let paths = Paths::new(tmp.path().join("nested"));
    let first = store::load(&paths).unwrap();
    assert!(!first.existed && first.config == CompressionConfig::default());
    assert!(!paths.config().exists());
    store::save(&paths, &headroom_config()).unwrap();
    assert_eq!(store::load(&paths).unwrap().config, headroom_config());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(paths.config())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let leftovers: Vec<_> = std::fs::read_dir(&paths.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["compression.json"]);
}

#[test]
fn corrupt_file_is_an_error_not_a_silent_default_and_is_left_alone() {
    let tmp = Tmp::new("corrupt");
    let paths = tmp.paths();
    std::fs::write(paths.config(), "{not json").unwrap();
    assert!(store::load(&paths)
        .unwrap_err()
        .contains("compression.json"));
    std::fs::write(paths.config(), r#"{"provider": "bogus"}"#).unwrap();
    assert!(store::read(&paths).unwrap_err().contains("invalid policy"));
    assert_eq!(
        std::fs::read_to_string(paths.config()).unwrap(),
        r#"{"provider": "bogus"}"#
    );
}

#[test]
fn read_never_writes_but_load_persists_the_migration_once() {
    let tmp = Tmp::new("migrate");
    let paths = tmp.paths();
    let original = old_config().to_string();
    std::fs::write(paths.config(), &original).unwrap();
    let read = store::read(&paths).unwrap();
    assert!(!read.notes.is_empty());
    assert_eq!(std::fs::read_to_string(paths.config()).unwrap(), original);
    store::load(&paths).unwrap();
    let again = store::load(&paths).unwrap();
    assert!(again.notes.is_empty());
    assert!(std::fs::read_to_string(paths.config())
        .unwrap()
        .contains("\"provider_version\""));
}

#[test]
fn fnmatch_follows_python_semantics() {
    assert!(fnmatch("*/clients/*", "/Users/x/clients/acme"));
    assert!(fnmatch("/a/*", "/a/b/c/d"));
    assert!(fnmatch("/a/?", "/a/b") && !fnmatch("/a/?", "/a/bc"));
    assert!(fnmatch("/a/[bc]x", "/a/cx") && !fnmatch("/a/[bc]x", "/a/dx"));
    assert!(fnmatch("/a/[!bc]x", "/a/dx") && !fnmatch("/a/[!bc]x", "/a/bx"));
    assert!(fnmatch("[a-c]", "b") && !fnmatch("[a-c]", "d"));
    assert!(fnmatch("a[", "a[") && !fnmatch("exact", "Exact"));
    assert!(!fnmatch("/a", "/a/b"));
}

#[test]
fn context_rule_uses_match_as_the_json_key() {
    let rule = ContextRule::new("*/x/*", Some(ProviderName::RtkOnly), None);
    assert_eq!(
        serde_json::to_value(&rule).unwrap(),
        json!({"match": "*/x/*", "provider": "rtk-only", "preset": null})
    );
}

// ---- shims ----

#[test]
fn shims_define_every_function_without_leading_underscores() {
    for route in [false, true] {
        let snippet = shim_snippet(ShimOptions {
            route_claude: route,
        });
        let defined = defined_functions(&snippet);
        for name in SHIM_FUNCTIONS {
            assert!(defined.iter().any(|d| d == name), "{name} missing");
        }
        assert_eq!(defined.iter().any(|d| d == "claude"), route);
        assert!(defined.iter().all(|d| !d.starts_with('_')), "{defined:?}");
        assert!(!snippet.contains("mcpm"));
        assert!(snippet.contains("toolportctl compression run -- \"$@\""));
    }
}

#[test]
fn shims_golden() {
    assert_eq!(
        shim_snippet(ShimOptions::default()),
        "# Managed by `toolportctl compression` \u{2014} do not edit by hand.\n\
# Source from ~/.zshrc:  source <data dir>/compression-shims.zsh\n\
# (replaces the old hand-maintained ~/.config/headroom-aliases.zsh)\n\
hrclaude() { toolportctl compression run -- \"$@\"; }   # launch claude under the per-dir policy\n\
hrup()     { toolportctl compression proxy up; }        # start the active-preset proxy\n\
hrdown()   { toolportctl compression proxy down; }      # stop it\n\
hrrestart(){ toolportctl compression proxy restart; }   # restart (apply a mode change)\n\
hrstat()   { toolportctl compression status; }\n\
hrupdate() { toolportctl compression update --latest --accept; }  # pin newest headroom + re-snapshot presets\n\
hrperf()   { headroom perf \"$@\"; }              # savings report (headroom passthrough)\n\
hrdash()   { { open \"http://127.0.0.1:8787/dashboard\" || xdg-open \"http://127.0.0.1:8787/dashboard\"; } 2>/dev/null || headroom perf; }\n"
    );
}

#[test]
fn env_snippet_quotes_values_safely() {
    let mut env = OrderedMap::new();
    env.insert("A", "plain".to_string());
    env.insert("B", "a\"b$c`d\\e".to_string());
    let text = shell_env_snippet(&env);
    assert!(text.contains("export A=\"plain\"\n"));
    assert!(text.contains("export B=\"a\\\"b\\$c\\`d\\\\e\"\n"));
}

fn zsh() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("zsh"))
            .find(|c| c.is_file())
    })
}

#[test]
fn generated_zsh_parses_and_defines_the_functions() {
    let Some(zsh) = zsh() else {
        eprintln!("zsh not installed; skipping");
        return;
    };
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = Tmp::new("zsh");
    let mut env = OrderedMap::new();
    env.insert("ANTHROPIC_BASE_URL", "http://127.0.0.1:8787".to_string());
    env.insert("TRICKY", "a\"b$c`d".to_string());
    for (name, text) in [
        ("plain.zsh", shim_snippet(ShimOptions::default())),
        (
            "routed.zsh",
            shim_snippet(ShimOptions { route_claude: true }),
        ),
        ("env.sh", shell_env_snippet(&env)),
    ] {
        let file = tmp.path().join(name);
        std::fs::write(&file, text).unwrap();
        let out = std::process::Command::new(&zsh)
            .arg("-n")
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let probe = format!(
        "source {}; for f in {}; do (( $+functions[$f] )) || {{ echo missing $f; exit 1; }}; done; echo ok",
        tmp.path().join("plain.zsh").display(),
        SHIM_FUNCTIONS.join(" ")
    );
    let out = std::process::Command::new(&zsh)
        .arg("-c")
        .arg(probe)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "ok",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = std::process::Command::new(&zsh)
        .arg("-c")
        .arg(format!(
            "source {}; print -r -- $TRICKY",
            tmp.path().join("env.sh").display()
        ))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "a\"b$c`d");
}

// ---- test_run: launch planning ----

#[test]
fn run_04_headroom_dir_plans_proxy_and_env() {
    let cfg = headroom_config();
    let plan = plan_launch(
        &cfg,
        "/x/agent-batch/run",
        false,
        &args(&["--version"]),
        &FakeProbe(Some("0.29.0")),
    );
    assert!(plan.routed);
    assert_eq!(plan.preset, "agent");
    assert_eq!(plan.proxy.as_ref().unwrap().port, AGENT_PORT);
    assert_eq!(
        plan.proxy.as_ref().unwrap().argv,
        ["headroom", "proxy", "--port", "8788", "--mode", "token"]
    );
    assert_eq!(
        (plan.program.as_str(), plan.argv.clone()),
        ("claude", args(&["--version"]))
    );
    let env = env_of(&plan);
    assert_eq!(
        env["ANTHROPIC_BASE_URL"],
        format!("http://127.0.0.1:{AGENT_PORT}")
    );
    assert_eq!(env["HEADROOM_MODE"], "token");
    assert_eq!(env["HEADROOM_SAVINGS_PROFILE"], "agent-90");
    assert_eq!(plan.installed.as_deref(), Some("0.29.0"));
    assert!(plan.warnings.is_empty());
}

#[test]
fn run_04b_refuses_to_route_when_build_drifts_from_pin() {
    let cfg = headroom_config();
    let plan = plan_launch(
        &cfg,
        "/x/agent-batch/run",
        false,
        &args(&["--version"]),
        &FakeProbe(Some("0.31.0")),
    );
    assert!(!plan.routed && plan.proxy.is_none());
    assert!(!env_of(&plan).contains_key("ANTHROPIC_BASE_URL"));
    assert_eq!(plan.argv, args(&["--version"]));
    assert!(plan.warnings[0].contains("0.31.0") && plan.warnings[0].contains("0.29.0"));
    let missing = plan_launch(&cfg, "/x/agent-batch/run", false, &[], &FakeProbe(None));
    assert!(!missing.routed && missing.warnings[0].contains("not on PATH"));
}

#[test]
fn run_04c_force_routes_through_a_drifted_build() {
    let plan = plan_launch(
        &headroom_config(),
        "/x/agent-batch/run",
        true,
        &args(&["--version"]),
        &FakeProbe(Some("0.31.0")),
    );
    assert!(plan.routed && plan.proxy.is_some());
    assert_eq!(
        env_of(&plan)["ANTHROPIC_BASE_URL"],
        format!("http://127.0.0.1:{AGENT_PORT}")
    );
    assert!(plan.warnings[0].contains("--force"));
}

#[test]
fn run_04d_ledger_records_what_the_launch_actually_did() {
    let cfg = headroom_config();
    let routed = plan_launch(
        &cfg,
        "/x/agent-batch/run",
        false,
        &[],
        &FakeProbe(Some("0.29.0")),
    );
    assert!(routed.ledger.routed);
    assert_eq!(
        (routed.ledger.preset.as_str(), routed.ledger.port),
        ("agent", Some(AGENT_PORT))
    );
    assert_eq!(routed.ledger.pin, "0.29.0");
    let drifted = plan_launch(
        &cfg,
        "/x/agent-batch/run",
        false,
        &[],
        &FakeProbe(Some("0.31.0")),
    );
    assert!(!drifted.ledger.routed);
    assert_eq!(drifted.ledger.port, None);
}

#[derive(Default)]
struct FakeOps {
    proxy: Option<Result<String, String>>,
    recorded: Vec<LedgerEntry>,
    launched: Vec<LaunchPlan>,
    exit: i32,
}

impl LaunchOps for FakeOps {
    fn ensure_proxy(&mut self, _spec: &ProxySpec) -> Result<String, String> {
        self.proxy.clone().unwrap_or_else(|| Ok("up".into()))
    }
    fn record(&mut self, entry: &LedgerEntry) {
        self.recorded.push(entry.clone());
    }
    fn launch(&mut self, plan: &LaunchPlan) -> Result<i32, String> {
        self.launched.push(plan.clone());
        Ok(self.exit)
    }
}

#[test]
fn run_04e_failed_proxy_launches_plain_and_is_not_credited() {
    let plan = plan_launch(
        &headroom_config(),
        "/x/agent-batch/run",
        false,
        &args(&["--version"]),
        &FakeProbe(Some("0.29.0")),
    );
    let mut ops = FakeOps {
        proxy: Some(Err("did not become ready".into())),
        exit: 3,
        ..Default::default()
    };
    let out = run_plan(plan, &mut ops).unwrap();
    assert_eq!(out.exit_code, 3);
    assert!(!out.plan.routed);
    assert!(!env_of(&ops.launched[0]).contains_key("ANTHROPIC_BASE_URL"));
    assert!(!ops.recorded[0].routed && ops.recorded[0].port.is_none());
    assert!(out
        .plan
        .warnings
        .iter()
        .any(|w| w.contains("did not become ready")));
}

#[test]
fn run_plan_routes_when_the_proxy_is_up_and_records_once() {
    let plan = plan_launch(
        &headroom_config(),
        "/x/agent-batch/run",
        false,
        &[],
        &FakeProbe(Some("0.29.0")),
    );
    let mut ops = FakeOps::default();
    let out = run_plan(plan, &mut ops).unwrap();
    assert!(out.plan.routed && ops.recorded.len() == 1 && ops.recorded[0].routed);
    assert_eq!(
        env_of(&ops.launched[0])["ANTHROPIC_BASE_URL"],
        format!("http://127.0.0.1:{AGENT_PORT}")
    );
}

#[test]
fn run_05_none_provider_goes_direct_no_proxy_and_clears_stale_base_url() {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg.contexts = vec![ContextRule::new(
        "*/clients/*",
        Some(ProviderName::None),
        None,
    )];
    let plan = plan_launch(
        &cfg,
        "/x/clients/acme",
        false,
        &args(&["-p", "hi"]),
        &FakeProbe(Some("0.29.0")),
    );
    assert!(!plan.routed && plan.proxy.is_none());
    assert_eq!(plan.installed, None);
    let parent = BTreeMap::from([(
        "ANTHROPIC_BASE_URL".to_string(),
        "http://127.0.0.1:8787".to_string(),
    )]);
    assert!(!plan.child_env(&parent).contains_key("ANTHROPIC_BASE_URL"));
    for p in [ProviderName::RtkOnly, ProviderName::Parsec] {
        let plan = plan_launch(
            &CompressionConfig::with_provider(p),
            "/x",
            false,
            &[],
            &FakeProbe(None),
        );
        assert!(!plan.routed && plan.proxy.is_none());
    }
}

#[test]
fn passthrough_args_including_help_are_untouched() {
    let cfg = CompressionConfig::default();
    let list = args(&["-h", "--", "--plan", "-p", "two words"]);
    let plan = plan_launch(&cfg, "/x", false, &list, &FakeProbe(None));
    assert_eq!(plan.argv, list);
}

#[test]
fn plan_serializes_camel_case_without_the_parent_environment() {
    let plan = plan_launch(
        &headroom_config(),
        "/x/agent-batch/run",
        false,
        &args(&["-p", "hi"]),
        &FakeProbe(Some("0.29.0")),
    );
    let value = serde_json::to_value(&plan).unwrap();
    assert_eq!(value["provider"], "headroom");
    assert_eq!(value["env"]["set"]["HEADROOM_MODE"], "token");
    assert_eq!(value["env"]["unset"], json!([]));
    assert_eq!(value["ledger"]["routed"], true);
    assert!(value.get("proxy").is_some());
}

// ---- system ops (fake executables, localhost only) ----

#[cfg(unix)]
fn fake_exe(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn system_path(dir: &Path) -> std::ffi::OsString {
    let mut parts = vec![dir.to_path_buf()];
    parts.extend(["/usr/bin", "/bin"].map(PathBuf::from));
    std::env::join_paths(parts).unwrap()
}

#[cfg(unix)]
#[test]
fn system_probe_reads_the_version_from_a_fake_headroom() {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = Tmp::new("probe");
    fake_exe(tmp.path(), "headroom", "echo 'headroom, version 0.29.0'");
    let ops = SystemOps {
        path: Some(system_path(tmp.path())),
    };
    assert_eq!(ops.headroom_version().as_deref(), Some("0.29.0"));
    let empty = Tmp::new("probe-none");
    let none = SystemOps {
        path: Some(empty.path().into()),
    };
    assert_eq!(none.headroom_version(), None);
}

#[cfg(unix)]
#[test]
fn system_launch_runs_the_fake_claude_with_the_planned_env_and_argv() {
    let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = Tmp::new("launch");
    let out = tmp.path().join("seen.txt");
    fake_exe(
        tmp.path(),
        "claude",
        &format!(
            "printf '%s|%s|%s\\n' \"$ANTHROPIC_BASE_URL\" \"$HEADROOM_MODE\" \"$*\" > {}; exit 7",
            out.display()
        ),
    );
    let cfg = headroom_config();
    let mut plan = plan_launch(
        &cfg,
        "/x/agent-batch/run",
        false,
        &args(&["-h", "two words"]),
        &FakeProbe(Some("0.29.0")),
    );
    plan.proxy = None;
    let mut ops = SystemOps {
        path: Some(system_path(tmp.path())),
    };
    let code = ops.launch(&plan).unwrap();
    assert_eq!(code, 7);
    assert_eq!(
        std::fs::read_to_string(out).unwrap().trim(),
        format!("http://127.0.0.1:{AGENT_PORT}|token|-h two words")
    );
    let missing = SystemOps {
        path: Some(Tmp::new("launch-none").path().into()),
    };
    let mut missing = missing;
    assert!(missing
        .launch(&plan)
        .unwrap_err()
        .contains("cannot launch claude"));
}

#[test]
fn proxy_ready_reads_health_on_localhost() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let req = server.recv().unwrap();
            let ready = req.url() == "/health";
            let body = if ready {
                r#"{"ready": true, "status": "ok"}"#
            } else {
                r#"{"ready": false}"#
            };
            req.respond(tiny_http::Response::from_string(body)).unwrap();
        }
    });
    assert!(proxy_ready(port));
    drop(handle);
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = closed.local_addr().unwrap().port();
    drop(closed);
    assert!(!proxy_ready(dead));
}

#[test]
fn system_ensure_proxy_fails_when_nothing_listens() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = closed.local_addr().unwrap().port();
    drop(closed);
    let mut ops = SystemOps::new();
    let spec = ProxySpec {
        port: dead,
        mode: "cache".into(),
        argv: vec![],
    };
    assert!(ops.ensure_proxy(&spec).unwrap_err().contains("hrup"));
}
