use super::engine::EngineOps;
use super::legacy;
use super::manage::*;
use super::mcp_entry::{self, McpHost, Registration};
use super::model::*;
use super::shims::SHIM_FUNCTIONS;
use super::store::{self, Paths};
use super::verify::HealthProbe;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct EnvGuard(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl EnvGuard {
    pub(crate) fn set(vars: &[(&'static str, &Path)]) -> Self {
        let saved = vars
            .iter()
            .map(|(k, v)| {
                let old = std::env::var_os(k);
                std::env::set_var(k, v);
                (*k, old)
            })
            .collect();
        Self(saved)
    }

    pub(crate) fn home(dir: &Path) -> Self {
        Self::set(&[
            ("HOME", dir),
            ("USERPROFILE", dir),
            ("MCPM_CONFIG_DIR", dir),
        ])
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, old) in self.0.drain(..) {
            match old {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "tp-manage-{tag}-{}-{}",
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
        Paths::new(self.0.join("data"))
    }

    fn mcpm(&self) -> PathBuf {
        self.0.join("mcpm")
    }

    fn plist(&self) -> PathBuf {
        self.0.join("LaunchAgents").join("legacy.plist")
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct FakeEngine {
    version: Option<String>,
    savings: BTreeMap<String, Vec<(String, String)>>,
    health: Option<Value>,
    install_result: Option<(Option<String>, Option<String>)>,
    installs: Vec<String>,
    ran: Vec<String>,
    queried: usize,
}

impl EngineOps for FakeEngine {
    fn proxy_health(&self, _port: u16) -> Option<Value> {
        self.health.clone()
    }
    fn spawn_proxy(
        &mut self,
        _port: u16,
        _mode: &str,
        _env: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        panic!("a configuration command must not start a proxy");
    }
    fn listening_pids(&self, _port: u16) -> Vec<u32> {
        Vec::new()
    }
    fn terminate(&mut self, _pid: u32) -> Result<(), String> {
        panic!("a configuration command must not stop a proxy");
    }
    fn sleep(&mut self, _seconds: u64) {}
    fn installed_version(&self) -> Option<String> {
        self.version.clone()
    }
    fn latest_version(&self, _package: &str) -> Option<String> {
        None
    }
    fn install(&mut self, requirement: &str) -> Result<(Option<String>, Option<String>), String> {
        self.installs.push(requirement.into());
        self.install_result
            .clone()
            .ok_or_else(|| "uv not on PATH".into())
    }
    fn agent_savings(&self, profile: &str) -> Result<Vec<(String, String)>, String> {
        let _ = self.queried;
        self.savings
            .get(profile)
            .cloned()
            .ok_or_else(|| format!("no profile {profile}"))
    }
    fn run_headroom(&mut self, args: &[&str]) -> Result<String, String> {
        self.ran.push(args.join(" "));
        Ok("done".into())
    }
}

#[derive(Default)]
struct FakeMcp {
    present: bool,
    calls: Vec<String>,
    fail: Option<String>,
}

impl McpHost for FakeMcp {
    fn register(&mut self, dry_run: bool) -> Result<Registration, String> {
        self.calls.push(format!("register dry={dry_run}"));
        if let Some(why) = &self.fail {
            return Err(why.clone());
        }
        let created = !self.present;
        self.present |= !dry_run;
        Ok(Registration {
            id: "headroom".into(),
            profile: "default".into(),
            created,
            changed: created,
        })
    }
    fn unregister(&mut self, dry_run: bool) -> Result<Option<String>, String> {
        self.calls.push(format!("unregister dry={dry_run}"));
        let was = self.present;
        self.present &= dry_run;
        Ok(was.then(|| "headroom".to_string()))
    }
}

struct World {
    tmp: Tmp,
    engine: FakeEngine,
    mcp: FakeMcp,
}

impl World {
    fn new(tag: &str) -> Self {
        let mut engine = FakeEngine {
            version: Some("0.29.0".into()),
            ..FakeEngine::default()
        };
        engine.savings.insert(
            "agent-90".into(),
            vec![
                ("HEADROOM_Z".into(), "9".into()),
                ("HEADROOM_A".into(), "1".into()),
            ],
        );
        engine
            .savings
            .insert("balanced".into(), vec![("HEADROOM_B".into(), "2".into())]);
        Self {
            tmp: Tmp::new(tag),
            engine,
            mcp: FakeMcp::default(),
        }
    }

    fn paths(&self) -> Paths {
        self.tmp.paths()
    }

    fn run<T>(&mut self, f: impl FnOnce(&mut Ctx) -> Result<T, CmdError>) -> Result<T, CmdError> {
        let paths = self.tmp.paths();
        f(&mut Ctx {
            paths: &paths,
            mcpm_root: Some(self.tmp.mcpm()),
            legacy_plist: Some(self.tmp.plist()),
            engine: &mut self.engine,
            mcp: &mut self.mcp,
        })
    }

    fn config(&self) -> CompressionConfig {
        store::read(&self.paths()).unwrap().config
    }

    fn snapshot(&self) -> BTreeMap<String, Option<Vec<u8>>> {
        tree_snapshot(self.tmp.path())
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn headroom_req() -> EnableReq {
    EnableReq {
        provider: Some(ProviderName::Headroom),
        ..EnableReq::default()
    }
}

#[test]
fn enable_headroom_writes_the_shims_and_env_snippet_and_registers_the_entry() {
    let mut w = World::new("enable");
    let data = w.run(|cx| enable(cx, &headroom_req())).unwrap();
    let paths = w.paths();
    let shims = std::fs::read_to_string(paths.shims()).unwrap();
    for name in SHIM_FUNCTIONS {
        assert!(shims.contains(&format!("{name}()")), "{name}");
    }
    assert!(shims.contains("toolportctl compression run --"));
    let env = std::fs::read_to_string(paths.env_snippet()).unwrap();
    assert!(env.contains("export ANTHROPIC_BASE_URL=\"http://127.0.0.1:8787\""));
    #[cfg(unix)]
    {
        assert_eq!(mode_of(&paths.shims()), 0o644);
        assert_eq!(mode_of(&paths.env_snippet()), 0o600);
        assert_eq!(mode_of(&paths.config()), 0o600);
    }
    let config = w.config();
    assert_eq!(
        (config.provider, config.runtime),
        (ProviderName::Headroom, RuntimeKind::Proxy)
    );
    assert!(w.mcp.present);
    assert_eq!(data["provider"], "headroom");
    assert_eq!(data["preset"]["name"], "interactive");
    assert_eq!(data["dryRun"], false);
    assert_eq!(data["warnings"], json!([]));
    let actions = strings(&data["actions"]);
    assert!(
        actions.contains(&"saved config (provider=headroom)".to_string()),
        "{actions:?}"
    );
    assert!(actions
        .iter()
        .any(|a| a.starts_with("registered MCP server 'headroom'")));
    assert!(actions
        .iter()
        .any(|a| a.starts_with("wrote ") && a.contains("compression-shims.zsh")));
    assert_eq!(strings(&data["written"]).len(), 2);
    assert!(strings(&data["nextSteps"])[0].contains(&paths.shims().display().to_string()));
}

#[test]
fn enable_snapshots_the_empty_presets_once_and_warns_when_the_engine_is_missing() {
    let mut w = World::new("snapshot");
    let data = w.run(|cx| enable(cx, &headroom_req())).unwrap();
    let config = w.config();
    let agent = config.presets.get("agent").unwrap();
    assert_eq!(
        agent.knobs.keys().cloned().collect::<Vec<_>>(),
        ["HEADROOM_Z", "HEADROOM_A"]
    );
    assert_eq!(agent.snapshot_version.as_deref(), Some("0.29.0"));
    assert!(strings(&data["actions"])
        .iter()
        .any(|a| a == "snapshot preset 'agent' knobs from headroom 0.29.0 (profile 'agent-90')"));
    w.engine
        .savings
        .insert("agent-90".into(), vec![("HEADROOM_NEW".into(), "x".into())]);
    w.run(|cx| sync(cx, None, false)).unwrap();
    assert_eq!(
        w.config().presets.get("agent").unwrap().knobs.len(),
        2,
        "an existing snapshot is never rewritten by apply"
    );

    let mut bare = World::new("snapshot-missing");
    bare.engine.savings.clear();
    let data = bare.run(|cx| enable(cx, &headroom_req())).unwrap();
    assert_eq!(
        strings(&data["warnings"])[0],
        "preset 'agent': no profile agent-90"
    );
    assert!(bare.paths().config().exists());
}

#[test]
fn a_second_sync_changes_nothing() {
    let mut w = World::new("idempotent");
    w.run(|cx| enable(cx, &headroom_req())).unwrap();
    let before = w.snapshot();
    let data = w.run(|cx| sync(cx, None, false)).unwrap();
    assert_eq!(w.snapshot(), before);
    assert_eq!(data["warnings"], json!([]));
    let again = w.run(|cx| sync(cx, None, false)).unwrap();
    assert_eq!(again["actions"], data["actions"]);
}

#[test]
fn dry_run_reports_the_plan_and_writes_nothing() {
    let mut w = World::new("dry");
    let req = EnableReq {
        dry_run: true,
        ..headroom_req()
    };
    let data = w.run(|cx| enable(cx, &req)).unwrap();
    assert!(!w.paths().dir.exists(), "{:?}", w.snapshot());
    assert_eq!(data["dryRun"], true);
    assert!(!w.mcp.present);
    assert_eq!(w.mcp.calls, ["register dry=true"]);
    let actions = strings(&data["actions"]);
    assert!(
        actions.iter().all(|a| a.starts_with("would ")),
        "{actions:?}"
    );
    assert_eq!(
        actions[0],
        "would snapshot preset 'agent' knobs from headroom 0.29.0 (profile 'agent-90')"
    );
    assert!(actions.contains(&"would save config (provider=headroom)".to_string()));
    assert!(actions.iter().any(|a| a.starts_with("would write ")));

    w.run(|cx| enable(cx, &headroom_req())).unwrap();
    let before = w.snapshot();
    for result in [
        w.run(|cx| disable(cx, true, true)),
        w.run(|cx| set_provider(cx, ProviderName::RtkOnly, true)),
        w.run(|cx| use_preset(cx, "agent", true)),
        w.run(|cx| sync(cx, None, true)),
    ] {
        assert_eq!(result.unwrap()["dryRun"], true);
        assert_eq!(w.snapshot(), before);
    }
    assert!(w.mcp.present);
    assert!(w.engine.ran.is_empty());
}

#[test]
fn switching_to_rtk_only_removes_the_env_snippet_and_the_entry_and_keeps_the_shims() {
    let mut w = World::new("rtk");
    w.run(|cx| enable(cx, &headroom_req())).unwrap();
    let paths = w.paths();
    let data = w
        .run(|cx| set_provider(cx, ProviderName::RtkOnly, false))
        .unwrap();
    assert!(!paths.env_snippet().exists());
    assert!(paths.shims().exists(), "mcpm leaves the shims in place");
    assert!(!w.mcp.present);
    let actions = strings(&data["actions"]);
    assert!(actions.contains(&"removed MCP server 'headroom' from the registry".to_string()));
    assert!(actions
        .iter()
        .any(|a| a.starts_with("removed artifact ") && a.ends_with("compression-env.sh")));
    assert_eq!(w.config().provider, ProviderName::RtkOnly);
    assert_eq!(w.config().runtime, RuntimeKind::Hook);
}

#[test]
fn disable_keeps_the_policy_and_teardown_runs_the_engine_removal() {
    let mut w = World::new("disable");
    let mut cfg = CompressionConfig::default();
    cfg.contexts
        .push(ContextRule::new("*/x/*", Some(ProviderName::RtkOnly), None));
    cfg.provider_version.pin = "0.30.1".into();
    store::save(&w.paths(), &cfg).unwrap();
    w.run(|cx| enable(cx, &headroom_req())).unwrap();
    let data = w.run(|cx| disable(cx, false, false)).unwrap();
    let kept = w.config();
    assert_eq!(kept.provider, ProviderName::None);
    assert_eq!(kept.provider_version.pin, "0.30.1");
    assert_eq!(kept.contexts.len(), 1);
    assert!(w.engine.ran.is_empty());
    assert_eq!(data["teardown"], false);

    let data = w.run(|cx| disable(cx, true, false)).unwrap();
    assert_eq!(w.engine.ran, ["mcp uninstall", "unwrap claude"]);
    let actions = strings(&data["actions"]);
    assert!(actions.contains(&"headroom mcp uninstall \u{2014} done".to_string()));
    assert!(actions.contains(&"headroom unwrap claude \u{2014} done".to_string()));
}

#[test]
fn teardown_is_skipped_while_a_context_rule_still_selects_headroom() {
    let mut w = World::new("teardown-kept");
    let mut cfg = CompressionConfig::default();
    cfg.contexts.push(ContextRule::new(
        "*/x/*",
        Some(ProviderName::Headroom),
        None,
    ));
    store::save(&w.paths(), &cfg).unwrap();
    w.run(|cx| disable(cx, true, false)).unwrap();
    assert!(w.engine.ran.is_empty());
}

#[test]
fn parsec_is_selectable_but_its_plugin_is_not_driven() {
    let mut w = World::new("parsec");
    let data = w
        .run(|cx| set_provider(cx, ProviderName::Parsec, false))
        .unwrap();
    assert_eq!(w.config().provider, ProviderName::Parsec);
    assert!(strings(&data["warnings"])[0].contains("claude plugin"));
    assert!(w.engine.ran.is_empty());
}

#[test]
fn a_registry_failure_is_a_warning_not_an_error() {
    let mut w = World::new("mcp-fail");
    w.mcp.fail = Some("registry locked".into());
    let data = w.run(|cx| enable(cx, &headroom_req())).unwrap();
    assert_eq!(
        strings(&data["warnings"]),
        ["MCP registration failed (registry locked)"]
    );
    assert!(w.paths().shims().exists());
}

#[test]
fn use_switches_the_preset_and_rejects_an_unknown_one() {
    let mut w = World::new("use");
    let data = w.run(|cx| use_preset(cx, "agent", false)).unwrap();
    assert_eq!(data["preset"]["name"], "agent");
    assert_eq!(data["preset"]["mode"], "token");
    assert_eq!(data["preset"]["port"], 8788);
    assert_eq!(w.config().active_preset, "agent");
    let err = w.run(|cx| use_preset(cx, "ghost", false)).unwrap_err();
    assert_eq!(err.kind, Kind::Usage);
    assert_eq!(
        err.message,
        "unknown preset 'ghost' (have: interactive, agent, balanced)"
    );
    assert_eq!(w.config().active_preset, "agent");
}

#[test]
fn enable_applies_the_options_and_keeps_what_the_file_already_had() {
    let mut w = World::new("enable-opts");
    let mut cfg = CompressionConfig::default();
    cfg.contexts
        .push(ContextRule::new("*/c/*", Some(ProviderName::None), None));
    cfg.options.insert("custom".into(), json!(1));
    store::save(&w.paths(), &cfg).unwrap();
    let req = EnableReq {
        provider: Some(ProviderName::RtkOnly),
        port: Some(9100),
        telemetry: Some("on".into()),
        preset: Some("agent".into()),
        mode: Some(CompressionMode::Cache),
        dry_run: false,
    };
    w.run(|cx| enable(cx, &req)).unwrap();
    let got = w.config();
    assert_eq!(got.provider, ProviderName::RtkOnly);
    assert_eq!(got.active_preset, "agent");
    let agent = got.presets.get("agent").unwrap();
    assert_eq!((agent.mode, agent.port), (CompressionMode::Cache, 9100));
    assert_eq!(got.telemetry(), "on");
    assert_eq!(got.options["custom"], 1);
    assert_eq!(got.contexts.len(), 1);

    let bad = EnableReq {
        preset: Some("ghost".into()),
        ..EnableReq::default()
    };
    assert_eq!(w.run(|cx| enable(cx, &bad)).unwrap_err().kind, Kind::Usage);
    assert_eq!(w.config().provider, ProviderName::RtkOnly);
}

fn legacy_file(w: &World, text: &str) {
    std::fs::create_dir_all(w.tmp.mcpm()).unwrap();
    std::fs::write(w.tmp.mcpm().join("compression.json"), text).unwrap();
}

const OLD_SHAPE: &str = r#"{
  "provider": "rtk-only",
  "runtime": "hook",
  "presets": {"agent": {"mode": "token", "savings_profile": "agent-90",
    "env": {"HEADROOM_MODE": "token"}, "code_aware": false, "port": 8788}},
  "active_preset": "agent"
}"#;

#[test]
fn sync_adopts_a_legacy_policy_once_and_migrates_its_shape() {
    let mut w = World::new("legacy");
    legacy_file(&w, OLD_SHAPE);
    let legacy_bytes = std::fs::read(w.tmp.mcpm().join("compression.json")).unwrap();
    let data = w.run(|cx| sync(cx, None, false)).unwrap();
    let got = w.config();
    assert_eq!(got.provider, ProviderName::RtkOnly);
    assert_eq!(got.active_preset, "agent");
    assert_eq!(got.provider_version.pin, DEFAULT_PIN);
    let knobs: Vec<_> = got
        .presets
        .get("agent")
        .unwrap()
        .knobs
        .keys()
        .cloned()
        .collect();
    assert_eq!(knobs, ["HEADROOM_MODE", "HEADROOM_CODE_AWARE_ENABLED"]);
    assert_eq!(
        std::fs::read(w.tmp.mcpm().join("compression.json")).unwrap(),
        legacy_bytes,
        "the mcpm file is only read"
    );
    let actions = strings(&data["actions"]);
    assert!(actions[0].starts_with("adopted legacy policy from "));
    assert!(
        actions
            .iter()
            .any(|a| a
                == "migrated seeded provider_version (exact pin; was an unpinned '>=' install)")
    );
    assert!(data["adopted"]["from"]
        .as_str()
        .unwrap()
        .ends_with("compression.json"));
    assert!(!data["adopted"]["notes"].as_array().unwrap().is_empty());

    let again = w.run(|cx| sync(cx, None, false)).unwrap();
    assert_eq!(again["adopted"], Value::Null);
    assert!(!strings(&again["actions"])
        .iter()
        .any(|a| a.contains("adopted")));
}

#[test]
fn a_policy_already_in_the_data_dir_is_never_replaced_by_the_legacy_one() {
    let mut w = World::new("legacy-keep");
    let mut cfg = CompressionConfig::default();
    cfg.active_preset = "balanced".into();
    store::save(&w.paths(), &cfg).unwrap();
    legacy_file(&w, OLD_SHAPE);
    let data = w.run(|cx| sync(cx, None, false)).unwrap();
    assert_eq!(data["adopted"], Value::Null);
    assert_eq!(w.config().active_preset, "balanced");
    assert_eq!(w.config().provider, ProviderName::None);
}

#[test]
fn every_mutating_command_starts_from_the_legacy_policy_not_from_defaults() {
    let mut w = World::new("legacy-use");
    legacy_file(&w, OLD_SHAPE);
    w.run(|cx| use_preset(cx, "agent", false)).unwrap();
    assert_eq!(w.config().provider, ProviderName::RtkOnly);
}

#[test]
fn a_dry_run_adopts_in_memory_only_and_an_unreadable_legacy_file_is_a_warning() {
    let mut w = World::new("legacy-dry");
    legacy_file(&w, OLD_SHAPE);
    let before = w.snapshot();
    let data = w.run(|cx| sync(cx, None, true)).unwrap();
    assert_eq!(w.snapshot(), before);
    assert!(strings(&data["actions"])[0].starts_with("would adopt legacy policy from "));
    assert_eq!(data["provider"], "rtk-only");

    let mut broken = World::new("legacy-broken");
    legacy_file(&broken, "{oops");
    let data = broken.run(|cx| sync(cx, None, false)).unwrap();
    assert!(strings(&data["warnings"])[0].starts_with("ignored legacy "));
    assert_eq!(broken.config().provider, ProviderName::None);
}

#[test]
fn an_explicit_mcpm_root_overrides_the_default_location() {
    let mut w = World::new("legacy-root");
    let other = w.tmp.path().join("elsewhere");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("compression.json"), OLD_SHAPE).unwrap();
    w.run(|cx| sync(cx, Some(&other), false)).unwrap();
    assert_eq!(w.config().provider, ProviderName::RtkOnly);
}

#[test]
fn the_default_mcpm_root_follows_the_environment() {
    let fx = DataDirFx::new("tp-manage", "root-env");
    let home = fx.dir.join("home");
    let _env = EnvGuard::set(&[("HOME", &home), ("USERPROFILE", &home)]);
    let previous = std::env::var_os("MCPM_CONFIG_DIR");
    std::env::remove_var("MCPM_CONFIG_DIR");
    assert_eq!(
        legacy::default_root(),
        Some(home.join(".config").join("mcpm"))
    );
    let custom = fx.dir.join("custom");
    let _mcpm = EnvGuard::set(&[("MCPM_CONFIG_DIR", &custom)]);
    assert_eq!(legacy::default_root(), Some(custom.clone()));
    drop(_mcpm);
    if let Some(v) = previous {
        std::env::set_var("MCPM_CONFIG_DIR", v);
    }
}

#[test]
fn a_leftover_launchd_job_is_removed_by_the_next_apply() {
    let mut w = World::new("plist");
    std::fs::create_dir_all(w.tmp.plist().parent().unwrap()).unwrap();
    std::fs::write(w.tmp.plist(), "<plist/>").unwrap();
    let data = w.run(|cx| sync(cx, None, true)).unwrap();
    assert!(w.tmp.plist().exists());
    assert!(strings(&data["actions"])
        .iter()
        .any(|a| a.starts_with("would remove artifact ")));
    let data = w.run(|cx| sync(cx, None, false)).unwrap();
    assert!(!w.tmp.plist().exists());
    assert_eq!(strings(&data["removed"]).len(), 1);
}

fn routed_config() -> CompressionConfig {
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    cfg.contexts.push(ContextRule::new(
        "*/plain/*",
        Some(ProviderName::None),
        None,
    ));
    cfg.presets
        .get_mut("agent")
        .unwrap()
        .knobs
        .insert("HEADROOM_Q", KnobSpec::new("a\"b$c"));
    cfg
}

#[test]
fn env_prints_the_proxy_env_for_a_routed_directory_and_plain_otherwise() {
    let mut w = World::new("env");
    store::save(&w.paths(), &routed_config()).unwrap();
    let data = w.run(|cx| env(cx, "/work/app")).unwrap();
    let lines = strings(&data["lines"]);
    assert_eq!(data["launch"], "route");
    assert!(lines.contains(&"export ANTHROPIC_BASE_URL=\"http://127.0.0.1:8787\"".to_string()));
    assert_eq!(
        lines[lines.len() - 3..],
        [
            "HRCOMPRESS_PORT=8787",
            "HRCOMPRESS_PRESET=interactive",
            "HRCOMPRESS_LAUNCH=route"
        ]
    );
    let plain = w.run(|cx| env(cx, "/work/plain/app")).unwrap();
    assert_eq!(plain["lines"], json!(["HRCOMPRESS_LAUNCH=plain"]));
    assert_eq!(plain["launch"], "plain");

    let mut cfg = routed_config();
    cfg.active_preset = "agent".into();
    store::save(&w.paths(), &cfg).unwrap();
    let data = w.run(|cx| env(cx, "/work/app")).unwrap();
    assert!(strings(&data["lines"]).contains(&"export HEADROOM_Q=\"a\\\"b\\$c\"".to_string()));
    assert_eq!(data["port"], 8788);
    assert!(!w.paths().shims().exists(), "env is read-only");
}

fn live_config(extra: Value) -> Value {
    let mut config = json!({
        "ready": true,
        "config": {"max_items_after_crush": 50, "protect_recent": null, "accuracy_guard": true}
    });
    if let Value::Object(map) = extra {
        for (k, v) in map {
            config["config"][k] = v;
        }
    }
    config
}

#[test]
fn seal_previews_by_default_and_writes_the_posture_as_policy_with_apply() {
    let mut w = World::new("seal");
    store::save(&w.paths(), &CompressionConfig::default()).unwrap();
    let err = w.run(|cx| seal(cx, &SealReq::default())).unwrap_err();
    assert_eq!(err.kind, Kind::Failed("no_proxy"));
    assert!(err
        .message
        .starts_with("no proxy on :8787: start one first"));

    w.engine.health = Some(live_config(json!({})));
    let before = w.snapshot();
    let data = w.run(|cx| seal(cx, &SealReq::default())).unwrap();
    assert_eq!(w.snapshot(), before);
    assert_eq!(data["complete"], false);
    assert_eq!(
        data["declarable"],
        json!([
            {"knob": "HEADROOM_MAX_ITEMS", "value": "50"},
            {"knob": "HEADROOM_ACCURACY_GUARD", "value": "1"}
        ])
    );
    assert_eq!(data["unset"], json!(["HEADROOM_PROTECT_RECENT"]));
    assert_eq!(data["sealed"], 0);

    let dry = SealReq {
        apply: true,
        dry_run: true,
        ..SealReq::default()
    };
    w.run(|cx| seal(cx, &dry)).unwrap();
    assert_eq!(w.snapshot(), before);

    let apply = SealReq {
        apply: true,
        ..SealReq::default()
    };
    let data = w.run(|cx| seal(cx, &apply)).unwrap();
    assert_eq!(data["sealed"], 2);
    let sealed = w.config();
    let knob = sealed
        .presets
        .get("interactive")
        .unwrap()
        .knobs
        .get("HEADROOM_MAX_ITEMS")
        .unwrap();
    assert_eq!(
        (knob.value.as_str(), knob.source),
        ("50", KnobSource::Policy)
    );
    let data = w.run(|cx| seal(cx, &apply)).unwrap();
    assert_eq!(data["declarable"], json!([]));
    assert_eq!(data["complete"], false, "an unset setting stays unsealable");
    assert_eq!(data["sealed"], 0);

    let err = w
        .run(|cx| {
            seal(
                cx,
                &SealReq {
                    preset: Some("ghost".into()),
                    ..SealReq::default()
                },
            )
        })
        .unwrap_err();
    assert_eq!(err.kind, Kind::Usage);
}

#[test]
fn seal_reports_a_fully_declared_posture_as_complete() {
    let mut w = World::new("seal-complete");
    w.engine.health = Some(json!({"ready": true, "config": {"max_items_after_crush": 50}}));
    let mut cfg = CompressionConfig::default();
    cfg.presets
        .get_mut("interactive")
        .unwrap()
        .knobs
        .insert("HEADROOM_MAX_ITEMS", KnobSpec::new("50"));
    store::save(&w.paths(), &cfg).unwrap();
    let data = w.run(|cx| seal(cx, &SealReq::default())).unwrap();
    assert_eq!(data["complete"], true);
}

#[test]
fn pin_shows_sets_installs_and_refreshes() {
    let mut w = World::new("pin");
    store::save(
        &w.paths(),
        &CompressionConfig::with_provider(ProviderName::Headroom),
    )
    .unwrap();
    let before = w.snapshot();
    let view = w.run(|cx| pin(cx, &PinReq::default())).unwrap();
    assert_eq!(w.snapshot(), before, "a bare pin is read-only");
    assert_eq!(view["pin"], DEFAULT_PIN);
    assert_eq!(view["requirement"], "headroom-ai[proxy,code,ml]==0.29.0");
    assert_eq!(view["installed"], "0.29.0");
    assert_eq!(view["drift"], false);
    assert_eq!(view["install"], Value::Null);

    w.engine.version = Some("0.28.0".into());
    let drift = w.run(|cx| pin(cx, &PinReq::default())).unwrap();
    assert_eq!(drift["drift"], true);

    let set = PinReq {
        version: Some("0.30.1".into()),
        ..PinReq::default()
    };
    let data = w.run(|cx| pin(cx, &set)).unwrap();
    assert_eq!(data["set"], true);
    assert_eq!(w.config().provider_version.pin, "0.30.1");
    for bad in ["latest", "1.x", "x"] {
        let req = PinReq {
            version: Some(bad.into()),
            ..PinReq::default()
        };
        let err = w.run(|cx| pin(cx, &req)).unwrap_err();
        assert_eq!(err.kind, Kind::Usage, "{bad}");
    }
    assert_eq!(w.config().provider_version.pin, "0.30.1");

    let dry = PinReq {
        install: true,
        refresh: true,
        dry_run: true,
        ..PinReq::default()
    };
    let before = w.snapshot();
    let data = w.run(|cx| pin(cx, &dry)).unwrap();
    assert_eq!(w.snapshot(), before);
    assert!(w.engine.installs.is_empty());
    assert_eq!(data["install"]["dryRun"], true);
    assert_eq!(data["refresh"]["saved"], false);

    w.engine.install_result = Some((Some("0.28.0".into()), Some("0.30.1".into())));
    w.engine.version = Some("0.30.1".into());
    let install = PinReq {
        install: true,
        refresh: true,
        ..PinReq::default()
    };
    let data = w.run(|cx| pin(cx, &install)).unwrap();
    assert_eq!(w.engine.installs, ["headroom-ai[proxy,code,ml]==0.30.1"]);
    assert_eq!(data["install"]["detail"], "0.28.0 \u{2192} 0.30.1");
    assert_eq!(data["restartProxies"], true);
    let agent = w.config().presets.get("agent").unwrap().clone();
    assert_eq!(agent.snapshot_version.as_deref(), Some("0.30.1"));
    assert_eq!(agent.knobs.len(), 2);
    assert_eq!(data["refresh"]["presets"][0]["name"], "agent");
    assert_eq!(
        data["refresh"]["presets"][0]["added"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn pin_install_failure_keeps_the_new_pin_and_refresh_failure_keeps_the_file() {
    let mut w = World::new("pin-fail");
    store::save(
        &w.paths(),
        &CompressionConfig::with_provider(ProviderName::Headroom),
    )
    .unwrap();
    let req = PinReq {
        version: Some("0.31.0".into()),
        install: true,
        ..PinReq::default()
    };
    let err = w.run(|cx| pin(cx, &req)).unwrap_err();
    assert_eq!(err.kind, Kind::Failed("install_failed"));
    assert_eq!(w.config().provider_version.pin, "0.31.0");

    let bytes = std::fs::read(w.paths().config()).unwrap();
    w.engine.savings.remove("balanced");
    let refresh = PinReq {
        refresh: true,
        ..PinReq::default()
    };
    let err = w.run(|cx| pin(cx, &refresh)).unwrap_err();
    assert_eq!(err.kind, Kind::Failed("snapshot_failed"));
    assert_eq!(std::fs::read(w.paths().config()).unwrap(), bytes);
}

#[test]
fn presets_refresh_keeps_policy_knobs_and_reports_the_diff() {
    let mut w = World::new("refresh");
    let mut cfg = CompressionConfig::with_provider(ProviderName::Headroom);
    let agent = cfg.presets.get_mut("agent").unwrap();
    agent.knobs.insert(
        "HEADROOM_OLD",
        KnobSpec::with_source("1", KnobSource::Profile),
    );
    agent.knobs.insert(
        "HEADROOM_A",
        KnobSpec::with_source("0", KnobSource::Profile),
    );
    agent.knobs.insert("HEADROOM_MINE", KnobSpec::new("keep"));
    store::save(&w.paths(), &cfg).unwrap();
    let data = w.run(|cx| refresh_presets(cx, false)).unwrap();
    let agent = &data["presets"][0];
    assert_eq!(
        agent["added"],
        json!([{"knob": "HEADROOM_Z", "value": "9"}])
    );
    assert_eq!(
        agent["removed"],
        json!([{"knob": "HEADROOM_OLD", "was": "1"}])
    );
    assert_eq!(
        agent["moved"],
        json!([{"knob": "HEADROOM_A", "from": "0", "to": "1"}])
    );
    assert_eq!(agent["kept"], json!(["HEADROOM_MINE"]));
    assert!(w
        .config()
        .presets
        .get("agent")
        .unwrap()
        .knobs
        .contains_key("HEADROOM_MINE"));
    let second = w.run(|cx| refresh_presets(cx, false)).unwrap();
    assert!(second["presets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["changed"] == false));
}

struct Healthy;

impl HealthProbe for Healthy {
    fn binary(&self, _name: &str) -> Option<String> {
        Some("/usr/bin/headroom".into())
    }
    fn installed_version(&self) -> Option<String> {
        Some("0.29.0".into())
    }
    fn proxy_health(&self, _port: u16) -> Option<Value> {
        None
    }
}

#[test]
fn doctor_lists_the_checks_and_the_migration_notes() {
    let mut w = World::new("doctor");
    std::fs::create_dir_all(w.paths().dir.clone()).unwrap();
    std::fs::write(w.paths().config(), OLD_SHAPE).unwrap();
    let data = w.run(|cx| doctor(cx, &Healthy)).unwrap();
    assert_eq!(data["provider"], "rtk-only");
    assert!(!data["migrated"].as_array().unwrap().is_empty());
    assert!(data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "shims"));
    assert!(data["healthy"].is_boolean());
}

fn registry_with_default_profile(fx: &DataDirFx) {
    fx.write_registry(&json!({
        "version": 1,
        "servers": [],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
        "activeProfileId": "default"
    }));
}

#[test]
fn the_registry_host_registers_once_and_removes_the_entry() {
    let fx = DataDirFx::new("tp-manage", "registry");
    registry_with_default_profile(&fx);
    let path = fx.dir.join("registry.json");
    let mut host = mcp_entry::RegistryHost;

    let plan = host.register(true).unwrap();
    assert!(plan.created && plan.changed);
    assert!(!std::fs::read_to_string(&path).unwrap().contains("headroom"));

    let done = host.register(false).unwrap();
    assert_eq!(
        (done.id.as_str(), done.profile.as_str(), done.created),
        ("headroom", "default", true)
    );
    let reg: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let server = &reg["servers"][0];
    assert_eq!(server["command"], "headroom");
    assert_eq!(server["args"], json!(["mcp", "serve"]));
    assert_eq!(server["transport"], "stdio");
    assert_eq!(server["source"], mcp_entry::MCP_SOURCE);
    assert_eq!(reg["profiles"][0]["enabledServerIds"], json!(["headroom"]));

    let bytes = std::fs::read(&path).unwrap();
    let again = host.register(false).unwrap();
    assert!(!again.changed && !again.created);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "an unchanged entry is not rewritten"
    );

    assert_eq!(host.unregister(true).unwrap().as_deref(), Some("headroom"));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(host.unregister(false).unwrap().as_deref(), Some("headroom"));
    assert_eq!(host.unregister(false).unwrap(), None);
    let reg: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(reg["servers"], json!([]));
    assert_eq!(reg["profiles"][0]["enabledServerIds"], json!([]));
}

#[test]
fn the_registry_host_adopts_an_imported_entry_and_reenables_it() {
    let fx = DataDirFx::new("tp-manage", "registry-import");
    fx.write_registry(&json!({
        "version": 1,
        "servers": [{"id": "headroom", "name": "headroom", "transport": "stdio",
                     "command": "headroom", "args": ["mcp", "serve", "--old"],
                     "source": "imported:mcpm"}],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
        "activeProfileId": "default"
    }));
    let done = mcp_entry::RegistryHost.register(false).unwrap();
    assert!(!done.created && done.changed);
    let reg: Value =
        serde_json::from_str(&std::fs::read_to_string(fx.dir.join("registry.json")).unwrap())
            .unwrap();
    assert_eq!(reg["servers"].as_array().unwrap().len(), 1);
    assert_eq!(reg["servers"][0]["args"], json!(["mcp", "serve"]));
    assert_eq!(reg["profiles"][0]["enabledServerIds"], json!(["headroom"]));
    assert_eq!(
        reg["servers"][0]["source"], "imported:mcpm",
        "an imported entry keeps its origin"
    );
}

#[test]
fn the_handlers_run_the_same_cores_on_the_real_data_dir() {
    let fx = DataDirFx::new("tp-manage", "handlers");
    let home = fx.dir.join("home");
    let _env = EnvGuard::home(&home);
    registry_with_default_profile(&fx);
    let dry = crate::plus::dispatch("plus.compression.sync", json!({"dryRun": true})).unwrap();
    assert_eq!(dry["dryRun"], true);
    assert!(!fx.dir.join("compression.json").exists());

    let data = crate::plus::dispatch(
        "plus.compression.enable",
        json!({"provider": "rtk-only", "preset": "agent", "port": "9100", "telemetry": "on", "mode": "cache"}),
    )
    .unwrap();
    assert_eq!(data["provider"], "rtk-only");
    let config = store::read(&Paths::new(&fx.dir)).unwrap().config;
    assert_eq!(config.presets.get("agent").unwrap().port, 9100);

    let data =
        crate::plus::dispatch("plus.compression.use", json!({"preset": "balanced"})).unwrap();
    assert_eq!(data["preset"]["name"], "balanced");
    let data =
        crate::plus::dispatch("plus.compression.setProvider", json!({"provider": "none"})).unwrap();
    assert_eq!(data["provider"], "none");
    let data =
        crate::plus::dispatch("plus.compression.disable", json!({"teardown": false})).unwrap();
    assert_eq!(data["teardown"], false);
    let data = crate::plus::dispatch("plus.compression.pin", json!({})).unwrap();
    assert_eq!(data["pin"], DEFAULT_PIN);
    let data = crate::plus::dispatch("plus.compression.env", json!({"cwd": "/w"})).unwrap();
    assert_eq!(data["launch"], "plain");
    let data = crate::plus::dispatch("plus.compression.doctor", json!({})).unwrap();
    assert!(data["checks"].is_array());

    for (command, args, needle) in [
        (
            "plus.compression.use",
            json!({"preset": "ghost"}),
            "unknown preset 'ghost'",
        ),
        ("plus.compression.use", json!({}), "preset is required"),
        (
            "plus.compression.setProvider",
            json!({"provider": "x"}),
            "unknown provider 'x'",
        ),
        (
            "plus.compression.enable",
            json!({"mode": "x"}),
            "unknown mode 'x'",
        ),
        (
            "plus.compression.enable",
            json!({"telemetry": "x"}),
            "unknown telemetry 'x'",
        ),
        (
            "plus.compression.enable",
            json!({"port": 70000}),
            "port needs a whole number",
        ),
        (
            "plus.compression.pin",
            json!({"version": "x"}),
            "not a parseable",
        ),
    ] {
        let err = crate::plus::dispatch(command, args).unwrap_err();
        assert!(err.contains(needle), "{command}: {err}");
    }
}

#[cfg(unix)]
#[test]
fn the_written_shims_parse_and_define_the_functions_in_zsh() {
    let zsh = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("zsh"))
            .find(|c| c.is_file())
    });
    let Some(zsh) = zsh else {
        eprintln!("zsh not installed; skipping");
        return;
    };
    let mut w = World::new("zsh");
    w.run(|cx| enable(cx, &headroom_req())).unwrap();
    for file in [w.paths().shims(), w.paths().env_snippet()] {
        let out = std::process::Command::new(&zsh)
            .arg("-n")
            .arg(&file)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}: {}",
            file.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let probe = format!(
        "source {}; for f in {}; do (( $+functions[$f] )) || {{ echo missing $f; exit 1; }}; done; echo ok",
        w.paths().shims().display(),
        SHIM_FUNCTIONS.join(" ")
    );
    let out = std::process::Command::new(&zsh)
        .arg("-c")
        .arg(probe)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}
