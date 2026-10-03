//! Replays the recorded mcpm reference outputs (`tests/fixtures/compression/mcpm`, produced by
//! `generator/gen.sh` against the Python reference with stub engine binaries) through the
//! `toolportctl compression` commands on a stub engine. Every difference from mcpm is a named
//! rule in `expected_text`; an unlisted difference fails the test.

use super::compression_cfg as cfg;
use super::output::{CtlError, Output};
use super::{compression, emit};
use crate::plus::compression::apply::LEGACY_PLIST_LABEL;
use crate::plus::compression::engine::EngineOps;
use crate::plus::compression::manage::Ctx;
use crate::plus::compression::mcp_entry::{McpHost, Registration};
use crate::plus::compression::model::{ContextRule, ProviderName};
use crate::plus::compression::store::{self, Paths};
use crate::plus::compression::verify::HealthProbe;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FIXTURES: &str = "tests/fixtures/compression/mcpm";
pub(super) const PROXY_PORT: u16 = 18787;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Setup {
    Headroom,
    HeadroomPort,
    Agent,
    Rtk,
    Drift,
    NoEngine,
    NoUv,
    Legacy,
    Plist,
    Sealed,
    Rule,
}

use Setup::*;

#[derive(Clone, Copy, PartialEq)]
enum Expect {
    Ok,
    Usage,
    Failed,
}

struct Case {
    name: &'static str,
    setups: &'static [Setup],
    expect: Expect,
}

const fn case(name: &'static str, setups: &'static [Setup], expect: Expect) -> Case {
    Case {
        name,
        setups,
        expect,
    }
}

const CASES: &[Case] = &[
    case("enable-headroom", &[], Expect::Ok),
    case("enable-headroom-opts", &[], Expect::Ok),
    case("enable-rtk-only", &[], Expect::Ok),
    case("enable-none", &[], Expect::Ok),
    case("enable-no-engine", &[NoEngine], Expect::Ok),
    case("enable-unknown-preset", &[], Expect::Usage),
    case("disable", &[Headroom], Expect::Ok),
    case("disable-teardown", &[Headroom], Expect::Ok),
    case("set-provider-rtk", &[Headroom], Expect::Ok),
    case("set-provider-headroom", &[], Expect::Ok),
    case("use-agent", &[Headroom], Expect::Ok),
    case("use-unknown", &[Headroom], Expect::Usage),
    case("sync", &[Headroom], Expect::Ok),
    case("sync-fresh", &[], Expect::Ok),
    case("sync-legacy", &[Legacy], Expect::Ok),
    case("sync-plist", &[Headroom, Plist], Expect::Ok),
    case("env-headroom", &[Headroom], Expect::Ok),
    case("env-agent", &[Headroom, Agent], Expect::Ok),
    case("env-routed-plain", &[Headroom, Rule], Expect::Ok),
    case("env-rtk", &[Rtk], Expect::Ok),
    case("env-fresh", &[], Expect::Ok),
    case("pin", &[Headroom], Expect::Ok),
    case("pin-set", &[Headroom], Expect::Ok),
    case("pin-drift", &[Headroom, Drift], Expect::Ok),
    case("pin-install", &[Headroom, Drift], Expect::Ok),
    case("pin-install-refresh", &[Headroom, Drift], Expect::Ok),
    case("pin-set-install-refresh", &[Headroom], Expect::Ok),
    case("pin-install-no-uv", &[Headroom, NoUv], Expect::Failed),
    case("seal-no-proxy", &[Headroom], Expect::Failed),
    case("seal-preview", &[HeadroomPort], Expect::Ok),
    case("seal-apply", &[HeadroomPort], Expect::Ok),
    case("seal-again", &[HeadroomPort, Sealed], Expect::Ok),
    case("seal-unknown", &[HeadroomPort], Expect::Usage),
    case("presets-refresh", &[Headroom, Drift], Expect::Ok),
    case("presets-refresh-same", &[Headroom], Expect::Ok),
    case(
        "presets-refresh-no-engine",
        &[Headroom, NoEngine],
        Expect::Failed,
    ),
];

pub(super) struct StubEngine {
    pub(super) version: Option<String>,
    pub(super) on_path: bool,
    pub(super) has_uv: bool,
    pub(super) proxy: Option<u16>,
}

impl StubEngine {
    fn new() -> Self {
        Self {
            version: Some("0.29.0".into()),
            on_path: true,
            has_uv: true,
            proxy: None,
        }
    }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn live_health(proxy: Option<u16>, port: u16) -> Option<Value> {
    (proxy == Some(port)).then(|| {
        json!({"ready": true, "status": "healthy", "version": "0.29.0", "config": {
            "max_items_after_crush": 30, "accuracy_guard": true, "protect_recent": null,
            "compress_user_messages": false}})
    })
}

struct StubProbe {
    version: Option<String>,
    proxy: Option<u16>,
}

impl HealthProbe for StubProbe {
    fn binary(&self, name: &str) -> Option<String> {
        self.version.as_ref().map(|_| format!("/usr/bin/{name}"))
    }
    fn installed_version(&self) -> Option<String> {
        self.version.clone()
    }
    fn proxy_health(&self, port: u16) -> Option<Value> {
        live_health(self.proxy, port)
    }
}

impl EngineOps for StubEngine {
    fn proxy_health(&self, port: u16) -> Option<Value> {
        live_health(self.proxy, port)
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
        self.on_path.then(|| self.version.clone()).flatten()
    }
    fn latest_version(&self, _package: &str) -> Option<String> {
        None
    }
    fn install(&mut self, requirement: &str) -> Result<(Option<String>, Option<String>), String> {
        let before = self.installed_version();
        if !self.has_uv {
            return Err("uv not on PATH: install the engine manually".into());
        }
        let after = requirement.rsplit("==").next().map(str::to_string);
        self.version = after.clone();
        Ok((before, after))
    }
    fn agent_savings(&self, profile: &str) -> Result<Vec<(String, String)>, String> {
        if !self.on_path {
            return Err("headroom is not on PATH: cannot snapshot profile env. \
                        Install the pinned build: toolportctl compression pin --install"
                .into());
        }
        let old = self.version.as_deref() == Some("0.29.0");
        match (profile, old) {
            ("agent-90", true) => Ok(pairs(&[
                ("HEADROOM_MODE", "token"),
                ("HEADROOM_SAVINGS_PROFILE", "agent-90"),
                ("HEADROOM_MAX_ITEMS", "30"),
                ("HEADROOM_FORCE_KOMPRESS", "1"),
            ])),
            ("agent-90", false) => Ok(pairs(&[
                ("HEADROOM_MODE", "token"),
                ("HEADROOM_SAVINGS_PROFILE", "agent-90"),
                ("HEADROOM_MAX_ITEMS", "40"),
                ("HEADROOM_NEW_KNOB", "1"),
            ])),
            ("balanced", _) => Ok(pairs(&[
                ("HEADROOM_MODE", "token"),
                ("HEADROOM_SAVINGS_PROFILE", "balanced"),
                ("HEADROOM_MAX_ITEMS", "60"),
            ])),
            _ => Err(format!(
                "agent-savings --profile {profile}: unknown profile"
            )),
        }
    }
    fn run_headroom(&mut self, args: &[&str]) -> Result<String, String> {
        match args {
            ["mcp", "uninstall"] => Ok("removed headroom mcp registration".into()),
            ["unwrap", client] => Ok(format!("unwrapped {client}")),
            _ => Err("headroom not on PATH".into()),
        }
    }
}

#[derive(Default)]
struct StubMcp {
    present: bool,
}

impl McpHost for StubMcp {
    fn register(&mut self, dry_run: bool) -> Result<Registration, String> {
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
        let was = self.present;
        self.present &= dry_run;
        Ok(was.then(|| "headroom".to_string()))
    }
}

const LEGACY: &str = r#"{
  "provider": "rtk-only",
  "runtime": "hook",
  "presets": {"agent": {"mode": "token", "savings_profile": "agent-90",
    "env": {"HEADROOM_MODE": "token"}, "code_aware": false, "port": 8788}},
  "active_preset": "agent"
}"#;

pub(super) struct Fx {
    pub(super) base: PathBuf,
    pub(super) engine: StubEngine,
    mcp: StubMcp,
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

impl Fx {
    pub(super) fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("tp-golden-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("home").join(".config").join("mcpm")).unwrap();
        Self {
            base,
            engine: StubEngine::new(),
            mcp: StubMcp::default(),
        }
    }

    pub(super) fn data(&self) -> PathBuf {
        self.base.join("data")
    }

    pub(super) fn home(&self) -> PathBuf {
        self.base.join("home")
    }

    pub(super) fn legacy_root(&self) -> PathBuf {
        self.home().join(".config").join("mcpm")
    }

    pub(super) fn plist(&self) -> PathBuf {
        self.home()
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LEGACY_PLIST_LABEL}.plist"))
    }

    pub(super) fn run(&mut self, argv: &[String]) -> Result<Output, CtlError> {
        let paths = Paths::new(self.data());
        let probe = StubProbe {
            version: self.engine.installed_version(),
            proxy: self.engine.proxy,
        };
        let mut cx = Ctx {
            paths: &paths,
            mcpm_root: Some(self.legacy_root()),
            legacy_plist: Some(self.plist()),
            engine: &mut self.engine,
            mcp: &mut self.mcp,
        };
        let rest = &argv[1..];
        match argv[0].as_str() {
            "doctor" => cfg::doctor_with(&mut cx, rest, &probe),
            "enable" => cfg::enable_with(&mut cx, rest),
            "disable" => cfg::disable_with(&mut cx, rest),
            "set-provider" => cfg::set_provider_with(&mut cx, rest),
            "use" => cfg::use_with(&mut cx, rest),
            "sync" => cfg::sync_with(&mut cx, rest),
            "env" => cfg::env_with(&mut cx, rest),
            "pin" => cfg::pin_with(&mut cx, rest),
            "seal" => cfg::seal_with(&mut cx, rest),
            "presets" => compression::presets_with(&mut cx, rest),
            other => panic!("unmapped command {other}"),
        }
    }

    pub(super) fn setup(&mut self, setups: &[Setup]) {
        for s in setups {
            let argv: Option<&[&str]> = match s {
                Headroom => Some(&["enable"]),
                HeadroomPort => {
                    self.engine.proxy = Some(PROXY_PORT);
                    Some(&["enable", "--port", "18787"])
                }
                Agent => Some(&["use", "agent"]),
                Rtk => Some(&["enable", "--provider", "rtk-only"]),
                Sealed => Some(&["seal", "--apply"]),
                Drift => {
                    self.engine.version = Some("0.30.1".into());
                    None
                }
                NoEngine => {
                    self.engine.on_path = false;
                    None
                }
                NoUv => {
                    self.engine.has_uv = false;
                    None
                }
                Legacy => {
                    std::fs::write(self.legacy_root().join("compression.json"), LEGACY).unwrap();
                    None
                }
                Plist => {
                    std::fs::create_dir_all(self.plist().parent().unwrap()).unwrap();
                    std::fs::write(self.plist(), "<plist/>").unwrap();
                    None
                }
                Rule => {
                    let paths = Paths::new(self.data());
                    let mut config = store::read(&paths).unwrap().config;
                    config.contexts.push(ContextRule::new(
                        "*/plain/*",
                        Some(ProviderName::None),
                        None,
                    ));
                    config
                        .contexts
                        .push(ContextRule::new("*/agent/*", None, Some("agent")));
                    store::save(&paths, &config).unwrap();
                    None
                }
            };
            if let Some(argv) = argv {
                let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
                self.run(&argv)
                    .unwrap_or_else(|e| panic!("setup {argv:?}: {}", e.message));
            }
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(FIXTURES)
        .join(name)
}

fn read(name: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(fixture(name).join(file)).ok()
}

fn split_exit(text: &str) -> (String, i32) {
    let trimmed = text.trim_end();
    let (body, last) = trimmed.rsplit_once('\n').unwrap_or(("", trimmed));
    let code = last
        .strip_prefix("[exit ")
        .and_then(|c| c.strip_suffix(']'))
        .and_then(|c| c.parse().ok())
        .expect("fixture ends with [exit N]");
    (body.to_string(), code)
}

fn is_table_header(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("preset ") && t.ends_with("snapshot")
}

/// The text `toolportctl` is expected to print for the same mcpm output: mcpm's text with every
/// intended difference applied. Nothing else may differ.
fn expected_text(name: &str, expect: Expect, mcpm: &str) -> (String, i32) {
    let (body, code) = split_exit(mcpm);
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<String> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("  ! no installed target clients to propagate to") {
            continue;
        }
        if let Some(path) = line.strip_prefix("  \u{2713} removed artifact ") {
            let rewritten = format!("  \u{2713} wrote {path}");
            if lines[i + 1..].iter().any(|l| l.starts_with(&rewritten)) {
                continue;
            }
        }
        let line = line
            .replace("mcpm compression", "toolportctl compression")
            .replace("mcpm client sync", "toolportctl client sync")
            .replace(
                "in mcpm servers.json",
                "in the registry (enabled in profile 'default')",
            )
            .replace("from mcpm servers.json", "from the registry")
            .replace(
                "(MCP presence already propagated)",
                "(MCP entry already registered)",
            )
            .replace(
                "headroom is not on PATH \u{2014} cannot snapshot",
                "headroom is not on PATH: cannot snapshot",
            )
            .replace(
                "uv not on PATH \u{2014} install headroom manually",
                "uv not on PATH: install the engine manually",
            );
        out.push(line);
    }
    if name.starts_with("presets-") && expect == Expect::Ok {
        if let Some(at) = out.iter().position(|l| is_table_header(l)) {
            out.truncate(at);
        }
    }
    match expect {
        Expect::Ok => (out.join("\n"), code),
        Expect::Usage => {
            let msg = out.last().cloned().unwrap_or_default();
            (
                format!(
                    "toolportctl: {}\nRun `toolportctl --help` for usage.",
                    msg.trim_start_matches("  \u{2717} ")
                ),
                2,
            )
        }
        Expect::Failed => {
            let msg = out.last().cloned().unwrap_or_default();
            (
                format!("toolportctl: {}", msg.trim_start_matches("  \u{2717} ")),
                code,
            )
        }
    }
}

fn scrub(text: &str, fx: &Fx) -> String {
    let data = fx.data().to_string_lossy().into_owned();
    let legacy = fx.legacy_root().to_string_lossy().into_owned();
    let home = fx.home().to_string_lossy().into_owned();
    text.replace(&data, "<DATA>")
        .replace(&legacy, "<LEGACY>")
        .replace(&home, "<HOME>")
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn actual(case: &Case, argv: &[String], fx: &mut Fx) -> (String, i32) {
    let command = format!("compression {}", argv[0]);
    let result = fx.run(argv);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = emit(false, &command, result, &mut out, &mut err);
    let mut text = String::from_utf8(out).unwrap();
    text.push_str(&String::from_utf8(err).unwrap());
    let mut text = scrub(text.trim_end(), fx);
    if case.name == "sync-legacy" {
        text = text
            .lines()
            .filter(|l| {
                !l.starts_with("  \u{2713} adopted legacy policy from ")
                    && !l.starts_with("  \u{2713} migrated ")
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    if case.name.starts_with("presets-") && case.expect == Expect::Ok {
        let mut kept = Vec::new();
        for line in text.lines() {
            if is_table_header(line) {
                break;
            }
            kept.push(line);
        }
        text = kept.join("\n");
    }
    (text, code)
}

fn policy_value(fx: &Fx) -> Value {
    let loaded = store::read(&Paths::new(fx.data())).unwrap();
    serde_json::from_str(&store::render(&loaded.config).unwrap()).unwrap()
}

fn same_policy(name: &str, mine: &Value, mcpm: &Value) {
    if name.starts_with("disable") {
        for key in ["provider", "runtime"] {
            assert_eq!(mine[key], mcpm[key], "{name}: {key}");
        }
        return;
    }
    assert_eq!(mine, mcpm, "{name}: persisted policy");
}

#[test]
fn every_recorded_mcpm_case_is_replayed() {
    let mut recorded: Vec<String> =
        std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURES))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
    recorded.sort();
    let mut known: Vec<&str> = CASES.iter().map(|c| c.name).collect();
    known.sort();
    assert_eq!(recorded, known);
}

#[test]
fn text_and_persisted_policy_match_mcpm_apart_from_the_named_differences() {
    let mut failures = Vec::new();
    for c in CASES {
        let args = read(c.name, "args.txt").expect("args.txt");
        let argv: Vec<String> = args.split_whitespace().map(String::from).collect();
        let mcpm = read(c.name, "output.txt").expect("output.txt");
        let mut fx = Fx::new(c.name);
        fx.setup(c.setups);
        let (want, want_code) = expected_text(c.name, c.expect, &mcpm);
        let (got, got_code) = actual(c, &argv, &mut fx);
        if got != want || got_code != want_code {
            failures.push(format!(
                "--- {} ({})\nexpected (exit {want_code}):\n{want}\nactual (exit {got_code}):\n{got}\n",
                c.name, args.trim()
            ));
            continue;
        }
        if c.expect == Expect::Ok && !argv[0].starts_with("env") {
            if let Some(text) = read(c.name, "compression.json") {
                let theirs: Value = serde_json::from_str(&text).unwrap();
                same_policy(c.name, &policy_value(&fx), &theirs);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn written_shims_and_env_snippet_match_mcpm_apart_from_the_command_name() {
    for name in ["enable-headroom", "enable-headroom-opts", "use-agent"] {
        let args = read(name, "args.txt").unwrap();
        let argv: Vec<String> = args.split_whitespace().map(String::from).collect();
        let c = CASES.iter().find(|c| c.name == name).unwrap();
        let mut fx = Fx::new(&format!("files-{name}"));
        fx.setup(c.setups);
        fx.run(&argv).unwrap();
        let paths = Paths::new(fx.data());
        let shims = std::fs::read_to_string(paths.shims()).unwrap();
        let env = std::fs::read_to_string(paths.env_snippet()).unwrap();
        let want_env = read(name, "compression-env.sh")
            .unwrap()
            .replace("mcpm compression", "toolportctl compression");
        assert_eq!(env, want_env, "{name}: env snippet");
        if name == "enable-headroom" {
            let want = read(name, "compression-shims.zsh")
                .unwrap()
                .replace("mcpm compression", "toolportctl compression")
                .replace(
                    "source ~/.config/mcpm/compression-shims.zsh",
                    "source <data dir>/compression-shims.zsh",
                )
                .replace(
                    "hrdash()   { open \"http://127.0.0.1:8787/dashboard\" 2>/dev/null || headroom perf; }",
                    "hrdash()   { { open \"http://127.0.0.1:8787/dashboard\" || xdg-open \"http://127.0.0.1:8787/dashboard\"; } 2>/dev/null || headroom perf; }",
                );
            assert_eq!(shims, want, "shims");
        }
    }
}
