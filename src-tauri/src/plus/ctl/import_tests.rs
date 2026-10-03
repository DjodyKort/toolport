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
    assert_eq!(
        v["data"]["orphans"][0]["reference"],
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

#[test]
fn rename_refs_requires_tools_and_paths() {
    let (code, v) = cli_json(&["import", "rename-refs", "/FAKE/root", "--paths", "/FAKE/p"]);
    assert_eq!(code, 2, "{v}");
    let (code, v) = cli_json(&["import", "rename-refs", "/FAKE/root", "--tools", "/FAKE/t"]);
    assert_eq!(code, 2, "{v}");
}
