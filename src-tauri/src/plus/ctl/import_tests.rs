use super::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn cli_json(list: &[&str]) -> (i32, Value) {
    let mut full = vec!["--json"];
    full.extend_from_slice(list);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args(&full), &mut out, &mut err);
    let text = String::from_utf8(if out.is_empty() { err } else { out }).unwrap();
    (code, serde_json::from_str(text.trim()).expect(&text))
}

fn cli_text(list: &[&str]) -> (i32, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args(list), &mut out, &mut err);
    let text = String::from_utf8(if out.is_empty() { err } else { out }).unwrap();
    (code, text)
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import_mcpm/input")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn rename_refs_dry_run_apply_and_rerun_golden() {
    let dir = std::env::temp_dir().join(format!("ctl-rename-refs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("skills")).unwrap();
    let settings = dir.join("settings.json");
    let skill = dir.join("skills/SKILL.md");
    let untouched = dir.join("skills/plain.md");
    let settings_text = r#"{"allow":["mcp__mcpm_anna-mcp__search","mcp__mcpm_FAKE-ghost__tool"]}"#;
    std::fs::write(&settings, settings_text).unwrap();
    std::fs::write(&skill, "use mcp__mcpm_anna-mcp__list-items now\n").unwrap();
    std::fs::write(&untouched, "nothing to see\n").unwrap();

    let root = fixture_root();
    let tools = root.join("tools.json");
    let (root_s, tools_s) = (root.to_string_lossy(), tools.to_string_lossy());
    let (settings_s, skills_s) = (
        settings.to_string_lossy().to_string(),
        dir.join("skills").to_string_lossy().to_string(),
    );
    let base = [
        "import",
        "rename-refs",
        &root_s,
        "--tools",
        &tools_s,
        "--home",
        "/FAKE/home",
        "--paths",
        &settings_s,
        &skills_s,
    ];

    let mut dry = base.to_vec();
    dry.push("--dry-run");
    let (code, v) = cli_json(&dry);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["dryRun"], true);
    assert_eq!(v["data"]["scanned"], 3);
    assert_eq!(v["data"]["files"].as_array().unwrap().len(), 2);
    assert_eq!(v["data"]["replaced"], 2);
    assert_eq!(v["data"]["orphans"].as_array().unwrap().len(), 0);
    assert_eq!(
        v["data"]["dead"][0]["reference"],
        "mcp__mcpm_FAKE-ghost__tool"
    );
    assert_eq!(read(&settings), settings_text);
    assert!(read(&skill).contains("mcp__mcpm_anna-mcp__list-items"));

    let (code, v) = cli_json(&base);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["dryRun"], false);
    assert_eq!(v["data"]["replaced"], 2);
    assert_eq!(
        read(&settings),
        r#"{"allow":["mcp__toolport__anna__search","mcp__mcpm_FAKE-ghost__tool"]}"#
    );
    assert_eq!(read(&skill), "use mcp__toolport__anna__list_items now\n");
    assert_eq!(read(&untouched), "nothing to see\n");

    let (code, v) = cli_json(&base);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["files"].as_array().unwrap().len(), 0);
    assert_eq!(v["data"]["replaced"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

const WILDCARD_SETTINGS: &str = r#"{"permissions":{"allow":["mcp__mcpm_anna-mcp__search","mcp__mcpm_playwright__browser_click"],"ask":["mcp__mcpm_anna-mcp__create-*","mcp__mcpm_anna-mcp__list-*","mcp__mcpm_anna-*","mcp__mcpm_playwright__*"],"deny":[]}}"#;

#[test]
fn rename_refs_maps_wildcard_gates_reports_dead_servers_and_refuses_to_drop_a_gate() {
    let dir = std::env::temp_dir().join(format!("ctl-rename-wild-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let settings = dir.join("settings.json");
    std::fs::write(&settings, WILDCARD_SETTINGS).unwrap();

    let root = fixture_root();
    let tools = root.join("tools.json");
    let (root_s, tools_s) = (root.to_string_lossy(), tools.to_string_lossy());
    let settings_s = settings.to_string_lossy().to_string();
    let base = [
        "import",
        "rename-refs",
        &root_s,
        "--tools",
        &tools_s,
        "--home",
        "/FAKE/home",
        "--paths",
        &settings_s,
    ];

    let mut dry = base.to_vec();
    dry.push("--dry-run");
    let (code, v) = cli_json(&dry);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["replaced"], 3);
    assert_eq!(v["data"]["orphans"][0]["reference"], "mcp__mcpm_anna-*");
    assert_eq!(v["data"]["orphans"][0]["rule"], "ask");
    assert!(v["data"]["orphans"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("cuts a server name"));
    let dead: Vec<&str> = v["data"]["dead"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["reference"].as_str().unwrap())
        .collect();
    assert_eq!(
        dead,
        ["mcp__mcpm_playwright__*", "mcp__mcpm_playwright__browser_click"]
    );
    let (code, text) = cli_text(&dry);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("1 orphans, 2 dead"), "{text}");
    assert!(text.contains("orphan mcp__mcpm_anna-* in "), "{text}");
    assert!(text.contains("[ask rule]"), "{text}");
    assert!(text.contains("dead mcp__mcpm_playwright__* in "), "{text}");
    assert!(text.contains("error: apply would refuse"), "{text}");
    assert_eq!(read(&settings), WILDCARD_SETTINGS);

    let (code, v) = cli_json(&base);
    assert_ne!(code, 0, "{v}");
    assert!(v.to_string().contains("would be left without a replacement"), "{v}");
    assert_eq!(read(&settings), WILDCARD_SETTINGS, "nothing written on refusal");

    std::fs::write(
        &settings,
        WILDCARD_SETTINGS.replace(r#""mcp__mcpm_anna-*","#, ""),
    )
    .unwrap();
    let (code, v) = cli_json(&base);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["replaced"], 3);
    assert_eq!(
        read(&settings),
        r#"{"permissions":{"allow":["mcp__toolport__anna__search","mcp__mcpm_playwright__browser_click"],"ask":["mcp__toolport__anna__create_*","mcp__toolport__anna__list_*","mcp__mcpm_playwright__*"],"deny":[]}}"#
    );
    assert_eq!(v["data"]["dead"].as_array().unwrap().len(), 2);

    let (code, v) = cli_json(&base);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["replaced"], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_refs_requires_tools_and_paths() {
    let (code, v) = cli_json(&["import", "rename-refs", "/FAKE/root", "--paths", "/FAKE/p"]);
    assert_eq!(code, 2, "{v}");
    let (code, v) = cli_json(&["import", "rename-refs", "/FAKE/root", "--tools", "/FAKE/t"]);
    assert_eq!(code, 2, "{v}");
}

const LEGACY_POLICY: &str = r#"{
  "provider": "rtk-only",
  "runtime": "hook",
  "presets": {"agent": {"mode": "token", "savings_profile": "agent-90",
    "env": {"HEADROOM_MODE": "token"}, "code_aware": false, "port": 8788}},
  "active_preset": "agent"
}"#;

fn import_world(tag: &str, legacy: Option<&str>) -> (crate::plus::testutil::DataDirFx, PathBuf) {
    let fx = crate::plus::testutil::DataDirFx::with_data_subdir("ctl-import-adopt", tag, "data")
        .with_secret_key("test-secret-key-for-import-adopt");
    let root = fx.dir.join("mcpm");
    std::fs::create_dir_all(&root).unwrap();
    for entry in std::fs::read_dir(fixture_root()).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            std::fs::copy(&path, root.join(path.file_name().unwrap())).unwrap();
        }
    }
    if let Some(text) = legacy {
        std::fs::write(root.join("compression.json"), text).unwrap();
    }
    (fx, root)
}

fn import_args<'a>(root: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut list = vec!["import", "mcpm", root, "--skip-clients", "--home", "/FAKE/home"];
    list.extend_from_slice(extra);
    list
}

fn data_policy(fx: &crate::plus::testutil::DataDirFx) -> PathBuf {
    fx.dir.join("data/compression.json")
}

#[test]
fn import_adopts_the_mcpm_compression_policy_and_a_dry_run_only_reports_it() {
    let (fx, root) = import_world("adopt", Some(LEGACY_POLICY));
    let root_s = root.to_string_lossy().to_string();

    let (code, v) = cli_json(&import_args(&root_s, &["--dry-run"]));
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["compression"]["applied"], false);
    assert!(v["data"]["compression"]["adopted"]["from"]
        .as_str()
        .unwrap()
        .ends_with("compression.json"));
    assert!(!data_policy(&fx).exists());

    let legacy_bytes = std::fs::read(root.join("compression.json")).unwrap();
    let (code, v) = cli_json(&import_args(&root_s, &[]));
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["compression"]["applied"], true);
    assert!(!v["data"]["compression"]["adopted"]["notes"]
        .as_array()
        .unwrap()
        .is_empty());
    let saved: Value = serde_json::from_str(&read(&data_policy(&fx))).unwrap();
    assert_eq!(saved["provider"], "rtk-only");
    assert_eq!(saved["active_preset"], "agent");
    assert_eq!(
        std::fs::read(root.join("compression.json")).unwrap(),
        legacy_bytes,
        "the mcpm file is only read"
    );

    let (code, v) = cli_json(&import_args(&root_s, &[]));
    assert_eq!(code, 0, "{v}");
    assert!(v["data"].get("compression").is_none(), "{v}");
    assert_eq!(saved, serde_json::from_str::<Value>(&read(&data_policy(&fx))).unwrap());
}

#[test]
fn import_keeps_an_existing_compression_policy() {
    let (fx, root) = import_world("keep", Some(LEGACY_POLICY));
    let root_s = root.to_string_lossy().to_string();
    let ours = r#"{"provider": "none"}"#;
    std::fs::write(data_policy(&fx), ours).unwrap();
    let (code, v) = cli_json(&import_args(&root_s, &[]));
    assert_eq!(code, 0, "{v}");
    assert!(v["data"].get("compression").is_none(), "{v}");
    assert_eq!(read(&data_policy(&fx)), ours);
}

#[test]
fn import_without_an_mcpm_compression_policy_leaves_the_data_dir_alone() {
    let (fx, root) = import_world("absent", None);
    let root_s = root.to_string_lossy().to_string();
    let (code, v) = cli_json(&import_args(&root_s, &[]));
    assert_eq!(code, 0, "{v}");
    assert!(v["data"].get("compression").is_none(), "{v}");
    assert!(!data_policy(&fx).exists());
}

#[test]
fn import_reports_an_unreadable_compression_policy_and_still_imports() {
    let (fx, root) = import_world("broken", Some("{not json"));
    let root_s = root.to_string_lossy().to_string();
    let (code, v) = cli_json(&import_args(&root_s, &[]));
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["compression"]["applied"], false);
    assert!(v["data"]["compression"]["adopted"].is_null());
    assert!(v["data"]["compression"]["warnings"][0]
        .as_str()
        .unwrap()
        .starts_with("ignored legacy "));
    assert!(!data_policy(&fx).exists());
}
