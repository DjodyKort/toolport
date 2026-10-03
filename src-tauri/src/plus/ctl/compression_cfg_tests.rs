use super::compression_golden_tests::{Fx, Setup, PROXY_PORT};
use super::emit;
use crate::plus::compression::model::{CompressionConfig, KnobSpec, ProviderName};
use crate::plus::compression::store::{self, Paths};
use crate::plus::testutil::tree_snapshot;
use serde_json::Value;

fn argv(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn cli(fx: &mut Fx, json: bool, list: &[&str]) -> (i32, String, String) {
    let list = argv(list);
    let result = fx.run(&list);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let command = format!("compression {}", list[0]);
    let code = emit(json, &command, result, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn data_of(fx: &mut Fx, list: &[&str]) -> Value {
    let (code, out, err) = cli(fx, true, list);
    assert_eq!(code, 0, "{list:?}: {out}{err}");
    let envelope: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(envelope["ok"], true);
    envelope["data"].clone()
}

#[cfg(unix)]
fn mode_of(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn dry_run_leaves_the_whole_tree_and_the_engine_untouched() {
    let mut fx = Fx::new("cfg-dry");
    fx.setup(&[Setup::Headroom, Setup::Plist, Setup::Drift]);
    std::fs::write(
        fx.legacy_root().join("compression.json"),
        r#"{"provider": "none"}"#,
    )
    .unwrap();
    let before = tree_snapshot(&fx.base);
    let version = fx.engine.version.clone();
    for list in [
        &["enable", "--provider", "rtk-only", "--dry-run"][..],
        &["enable", "--dry-run"],
        &["disable", "--teardown", "--dry-run"],
        &["set-provider", "rtk-only", "--dry-run"],
        &["use", "agent", "--dry-run"],
        &["sync", "--dry-run"],
        &["pin", "0.31.0", "--install", "--refresh", "--dry-run"],
        &["pin", "--refresh", "--dry-run"],
        &["presets", "--refresh", "--dry-run"],
    ] {
        let data = data_of(&mut fx, list);
        let dry = data.get("dryRun").or_else(|| data["refresh"].get("dryRun"));
        assert_eq!(dry, Some(&Value::Bool(true)), "{list:?}: {data}");
        assert_eq!(tree_snapshot(&fx.base), before, "{list:?}");
        assert_eq!(fx.engine.version, version, "{list:?}");
    }
}

#[test]
fn dry_run_of_a_sealed_apply_keeps_the_policy_file() {
    let mut fx = Fx::new("cfg-seal-dry");
    fx.setup(&[Setup::HeadroomPort]);
    let before = tree_snapshot(&fx.base);
    let data = data_of(&mut fx, &["seal", "--apply", "--dry-run"]);
    assert_eq!(data["sealed"], 0);
    assert_eq!(data["declarable"].as_array().unwrap().len(), 3);
    assert_eq!(tree_snapshot(&fx.base), before);
}

#[test]
fn a_bad_argument_is_a_usage_error_before_anything_is_touched() {
    let mut fx = Fx::new("cfg-usage");
    let before = tree_snapshot(&fx.base);
    for (list, text) in [
        (
            &["enable", "--port", "x"][..],
            "--port needs a whole number",
        ),
        (&["enable", "--port"], "--port requires a value"),
        (
            &["enable", "--provider", "x"],
            "unknown provider 'x' (have: headroom, rtk-only, parsec, none)",
        ),
        (
            &["enable", "--mode", "x"],
            "unknown mode 'x' (have: cache, token)",
        ),
        (
            &["enable", "--telemetry", "maybe"],
            "unknown telemetry 'maybe' (have: off, on)",
        ),
        (&["enable", "extra"], "unexpected argument: extra"),
        (&["enable", "--bogus"], "unknown option: --bogus"),
        (&["disable", "--teardown=1"], "unknown option: --teardown"),
        (
            &["set-provider"],
            "set-provider needs a provider (headroom, rtk-only, parsec, none)",
        ),
        (&["set-provider", "a", "b"], "set-provider needs a provider"),
        (&["set-provider", "x"], "unknown provider 'x'"),
        (&["use"], "use needs a preset name"),
        (&["use", "a", "b"], "use needs a preset name"),
        (
            &["use", "ghost"],
            "unknown preset 'ghost' (have: interactive, agent, balanced)",
        ),
        (&["sync", "extra"], "unexpected argument: extra"),
        (&["sync", "--mcpm-root"], "--mcpm-root requires a value"),
        (&["pin", "a", "b"], "unexpected argument: b"),
        (
            &["pin", "latest"],
            "'latest' is not a parseable X.Y.Z version",
        ),
        (&["seal", "a", "b"], "unexpected argument: b"),
        (&["env", "extra"], "unexpected argument: extra"),
        (&["presets", "extra"], "unexpected argument: extra"),
    ] {
        let (code, out, err) = cli(&mut fx, false, list);
        assert_eq!(code, 2, "{list:?}: {out}{err}");
        assert!(out.is_empty(), "{list:?}: {out}");
        assert!(err.contains(text), "{list:?}: {err}");
        assert_eq!(tree_snapshot(&fx.base), before, "{list:?}");
    }
}

#[test]
fn enable_json_carries_the_plan_the_text_is_built_from() {
    let mut fx = Fx::new("cfg-json");
    let data = data_of(&mut fx, &["enable", "--dry-run"]);
    let mut keys: Vec<_> = data.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "actions",
            "adopted",
            "dryRun",
            "nextSteps",
            "preset",
            "provider",
            "removed",
            "runtime",
            "warnings",
            "written"
        ]
    );
    assert_eq!(data["provider"], "headroom");
    assert_eq!(data["runtime"], "proxy");
    assert_eq!(data["adopted"], Value::Null);
    assert_eq!(data["written"].as_array().unwrap().len(), 2);
    let (code, out, _) = cli(&mut fx, false, &["enable", "--dry-run"]);
    assert_eq!(code, 0);
    assert!(out.contains("would write "), "{out}");
    assert!(
        out.contains("would save config (provider=headroom)"),
        "{out}"
    );
    assert!(out.ends_with("(MCP entry already registered)\n"), "{out}");
}

#[test]
fn sync_is_idempotent_and_writes_the_documented_modes() {
    let mut fx = Fx::new("cfg-sync-twice");
    fx.setup(&[Setup::Headroom]);
    let first = data_of(&mut fx, &["sync"]);
    let after_first = tree_snapshot(&fx.base);
    let second = data_of(&mut fx, &["sync"]);
    assert_eq!(tree_snapshot(&fx.base), after_first);
    assert_eq!(first["actions"], second["actions"]);
    #[cfg(unix)]
    {
        let paths = Paths::new(fx.data());
        assert_eq!(mode_of(&paths.shims()), 0o644);
        assert_eq!(mode_of(&paths.env_snippet()), 0o600);
        assert_eq!(mode_of(&paths.config()), 0o600);
    }
}

#[test]
fn sync_adopts_a_legacy_policy_from_an_explicit_root_exactly_once() {
    let mut fx = Fx::new("cfg-legacy-root");
    let other = fx.base.join("elsewhere");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(
        other.join("compression.json"),
        r#"{"provider": "rtk-only", "runtime": "hook", "active_preset": "agent"}"#,
    )
    .unwrap();
    let root = other.to_string_lossy().into_owned();
    let data = data_of(&mut fx, &["sync", "--mcpm-root", &root]);
    assert_eq!(data["provider"], "rtk-only");
    assert_eq!(
        data["adopted"]["from"].as_str().unwrap(),
        other.join("compression.json").to_string_lossy()
    );
    let again = data_of(&mut fx, &["sync", "--mcpm-root", &root]);
    assert_eq!(again["adopted"], Value::Null);
    let kept = std::fs::read_to_string(other.join("compression.json")).unwrap();
    assert!(kept.contains("rtk-only"), "the mcpm file is only read");
}

#[test]
fn a_sealed_posture_survives_a_pin_refresh_and_is_listed_as_kept() {
    let mut fx = Fx::new("cfg-sealed-refresh");
    fx.setup(&[Setup::Headroom]);
    fx.engine.proxy = Some(8788);
    let data = data_of(&mut fx, &["seal", "agent", "--apply"]);
    assert_eq!(data["sealed"], 2);
    fx.engine.version = Some("0.30.1".into());
    let data = data_of(&mut fx, &["presets", "--refresh"]);
    let agent = &data["refresh"]["presets"][0];
    assert_eq!(agent["name"], "agent");
    assert_eq!(
        agent["moved"],
        serde_json::json!([{"knob": "HEADROOM_MAX_ITEMS", "from": "30", "to": "40"}])
    );
    assert_eq!(
        agent["kept"],
        serde_json::json!(["HEADROOM_ACCURACY_GUARD", "HEADROOM_COMPRESS_USER_MESSAGES"])
    );
    let config = store::read(&Paths::new(fx.data())).unwrap().config;
    let knobs = &config.presets.get("agent").unwrap().knobs;
    assert_eq!(knobs.get("HEADROOM_ACCURACY_GUARD").unwrap().value, "1");
    assert_eq!(
        knobs.get("HEADROOM_COMPRESS_USER_MESSAGES").unwrap().value,
        "0"
    );
}

#[test]
fn a_context_rule_for_headroom_keeps_the_proxy_surface_while_the_provider_is_off() {
    let mut fx = Fx::new("cfg-rule");
    let paths = Paths::new(fx.data());
    let mut config = CompressionConfig::with_provider(ProviderName::None);
    config
        .contexts
        .push(crate::plus::compression::model::ContextRule::new(
            "*/hr/*",
            Some(ProviderName::Headroom),
            None,
        ));
    store::save(&paths, &config).unwrap();
    let data = data_of(&mut fx, &["sync"]);
    assert_eq!(data["provider"], "none");
    let actions: Vec<_> = data["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert!(
        actions
            .iter()
            .any(|a| a.starts_with("snapshot preset 'agent'")),
        "{actions:?}"
    );
    let env = data_of(&mut fx, &["env", "--cwd", "/work/hr/app"]);
    assert_eq!(env["launch"], "route");
    let plain = data_of(&mut fx, &["env", "--cwd", "/work/other"]);
    assert_eq!(plain["launch"], "plain");
}

#[cfg(unix)]
#[test]
fn eval_of_env_output_reproduces_values_with_shell_metacharacters() {
    let mut fx = Fx::new("cfg-env-eval");
    fx.setup(&[Setup::Headroom]);
    let paths = Paths::new(fx.data());
    let mut config = store::read(&paths).unwrap().config;
    let tricky = "a\"b$c`d\\e f";
    config
        .presets
        .get_mut("interactive")
        .unwrap()
        .knobs
        .insert("HEADROOM_TRICKY", KnobSpec::new(tricky));
    store::save(&paths, &config).unwrap();
    let (code, out, _) = cli(&mut fx, false, &["env", "--cwd", "/work/app"]);
    assert_eq!(code, 0);
    let script = format!("{out}\nprintf %s \"$HEADROOM_TRICKY\"");
    let ran = std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&ran.stdout), tricky);
}

#[test]
fn doctor_lists_the_checks_and_exits_1_when_one_fails() {
    let mut fx = Fx::new("cfg-doctor");
    fx.setup(&[Setup::Headroom]);
    let (code, out, err) = cli(&mut fx, false, &["doctor"]);
    assert!(
        out.contains("headroom") && out.contains("shims"),
        "{out}{err}"
    );
    let failed = out.contains("FAIL");
    assert_eq!(code, i32::from(failed), "{out}");
    fx.engine.version = Some("0.30.1".into());
    let (code, out, _) = cli(&mut fx, false, &["doctor"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FAIL"), "{out}");
    let (code, out, _) = cli(&mut fx, true, &["doctor"]);
    assert_eq!(code, 1);
    let envelope: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(envelope["error"]["code"], "unhealthy");
    assert_eq!(envelope["data"]["healthy"], false);
    let _ = PROXY_PORT;
}
