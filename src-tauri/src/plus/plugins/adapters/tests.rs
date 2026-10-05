use super::*;

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("adapters-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

const SYNTH: &str = "format: 1\nplugin: synth@market\nknobs:\n  - {key: mode, env: SYNTH_MODE, kind: enum, choices: [a, b], default: a}\n  - {key: extra, env: SYNTH_EXTRA, kind: csv}\n";

#[test]
fn the_embedded_ecc_adapter_has_the_contract_knobs() {
    let reg = Registry::load(None);
    assert!(reg.problems.is_empty(), "{:?}", reg.problems);
    let ecc = reg.for_plugin("ecc@ecc").expect("ecc adapter");
    let keys: Vec<&str> = ecc.knobs.iter().map(|k| k.key.as_str()).collect();
    assert_eq!(keys, ["hook_profile", "hooks_enabled", "gateguard", "gateguard_exempt_globs", "disabled_hooks"]);
    let by = |k: &str| ecc.knob(k).unwrap();
    assert_eq!(by("hook_profile").option.as_deref(), Some("hook_profile"));
    assert_eq!(by("hook_profile").choices.as_ref().unwrap(), &["minimal", "standard", "strict"]);
    assert_eq!(by("hooks_enabled").default.as_deref(), Some("true"));
    assert_eq!(by("gateguard").kind, Kind::BoolOff);
    assert_eq!(by("gateguard").option, None);
    assert_eq!(by("gateguard").default.as_deref(), Some("on"));
    assert_eq!(by("disabled_hooks").kind, Kind::HookIds);
    assert!(ecc.hook_ids.is_some());
}

#[test]
fn user_adapters_load_and_malformed_ones_are_reported_and_ignored() {
    let data = tmp("user");
    let dir = user_dir(&data);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("synth@market.yaml"), SYNTH).unwrap();
    std::fs::write(dir.join("broken@market.yaml"), "format: 1\nplugin: broken@market\nknobs: [{key: x}]\n").unwrap();
    std::fs::write(dir.join("wrong@market.yaml"), SYNTH).unwrap();
    std::fs::write(
        dir.join("enumless@market.yaml"),
        "format: 1\nplugin: enumless@market\nknobs:\n  - {key: m, env: M, kind: enum}\n",
    )
    .unwrap();
    let reg = Registry::load(Some(&data));
    assert!(reg.for_plugin("synth@market").is_some());
    assert!(reg.for_plugin("broken@market").is_none());
    assert!(reg.for_plugin("enumless@market").is_none());
    assert!(reg.for_plugin("ecc@ecc").is_some());
    let files: Vec<&str> = reg.problems.iter().map(|p| p.file.rsplit('/').next().unwrap()).collect();
    assert_eq!(files, ["broken@market.yaml", "enumless@market.yaml", "wrong@market.yaml"]);
    assert!(reg.problems[1].message.contains("enum needs choices"));
    assert!(reg.problems[2].message.contains("named wrong@market.yaml"));
}

#[test]
fn a_user_adapter_for_a_built_in_plugin_replaces_it() {
    let data = tmp("replace");
    let dir = user_dir(&data);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("ecc@ecc.yaml"),
        "format: 1\nplugin: ecc@ecc\nknobs:\n  - {key: only, env: ECC_ONLY, kind: bool}\n",
    )
    .unwrap();
    let reg = Registry::load(Some(&data));
    let ecc = reg.for_plugin("ecc@ecc").unwrap();
    assert_eq!(ecc.knobs.len(), 1);
    assert_eq!(reg.adapters.iter().filter(|a| a.plugin == "ecc@ecc").count(), 1);
}

#[test]
fn bad_adapter_fields_are_refused() {
    for (text, needle) in [
        ("format: 2\nplugin: a@b\nknobs: []\n", "format must be 1"),
        ("format: 1\nplugin: nomarket\nknobs: []\n", "name>@<marketplace"),
        ("format: 1\nplugin: a@b\nknobs: [{key: k, env: 9X, kind: bool}]\n", "environment variable"),
        ("format: 1\nplugin: a@b\nknobs: [{key: k, env: K, kind: odd}]\n", "unknown kind"),
        ("format: 1\nplugin: a@b\nknobs: [{key: k, env: K, kind: bool}, {key: k, env: L, kind: bool}]\n", "listed twice"),
        ("format: 1\nplugin: a@b\nknobs: []\nhook_ids: {file: ../x.json, regex: '(a)'}\n", "inside the plugin"),
        ("format: 1\nplugin: a@b\nknobs: []\nhook_ids: {file: h.json, regex: 'a'}\n", "capture group"),
    ] {
        let err = parse(text).unwrap_err();
        assert!(err.contains(needle), "{text:?} gave {err}");
    }
}

#[test]
fn hook_ids_come_from_the_plugin_hooks_file() {
    let dir = tmp("hooks");
    std::fs::create_dir_all(dir.join("hooks")).unwrap();
    let hooks = serde_json::json!({"hooks": {"PreToolUse": [
        {"matcher": "Bash", "hooks": [{"type": "command", "command": "node run-with-flags.js pre:bash:dispatcher scripts/a.js"}]},
        {"matcher": "Edit", "hooks": [{"type": "command", "command": "node run-with-flags.js pre:edit:gate scripts/b.js"}]},
        {"matcher": "Bash", "hooks": [{"type": "command", "command": "node run-with-flags.js pre:bash:dispatcher again"}]}
    ], "Stop": [{"hooks": [{"type": "command", "command": "echo plain"}]}]}});
    std::fs::write(dir.join("hooks/hooks.json"), hooks.to_string()).unwrap();
    let ecc = Registry::load(None).for_plugin("ecc@ecc").cloned().unwrap();
    assert_eq!(ecc.hook_ids_of(&dir), ["pre:bash:dispatcher", "pre:edit:gate"]);
    assert!(ecc.hook_ids_of(&dir.join("missing")).is_empty());
}
