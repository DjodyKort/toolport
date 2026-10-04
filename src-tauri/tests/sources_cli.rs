//! `toolportctl sources` and `skills ls --source` through the real binary, over the sources
//! fixture home of the GUI-wave contract (section 13). The goldens in `ctl_contract.rs` pin the
//! envelope shapes; these tests pin the behaviour a screen relies on.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::process::{Command, Stdio};

use serde_json::Value;

#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/sources_world.rs"]
mod sources_world;

use ctl_world::CtlWorld;

fn world(tag: &str) -> CtlWorld {
    let world = CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"));
    sources_world::build_in(&world.base);
    world
}

fn ctl(world: &CtlWorld, args: &[&str]) -> (i32, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .arg("--json")
        .args(args)
        .env_clear()
        .current_dir(&world.home)
        .stdin(Stdio::null());
    for (key, value) in world.env() {
        command.env(key, value);
    }
    let output = command.output().expect("run toolportctl");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let envelope: Value = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("{args:?}: not an envelope ({e}): {text}"));
    (output.status.code().expect("exit code"), envelope)
}

fn names(rows: &Value) -> BTreeSet<String> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn skills_ls_source_library_returns_only_library_items() {
    let world = world("src-library");
    let (code, library) = ctl(&world, &["skills", "ls", "--source", "library"]);
    assert_eq!(code, 0, "{library}");
    let rows = &library["data"]["skills"];
    assert!(!rows.as_array().unwrap().is_empty());
    for row in rows.as_array().unwrap() {
        assert_eq!(row["origin"]["kind"], "library", "{row}");
        assert_eq!(row["writable"], true, "{row}");
    }
    let found = names(rows);
    for foreign in ["odh", "odh-develop", "v18-local", "risky", "ship", "pdf"] {
        assert!(!found.contains(foreign), "{foreign} is not a library skill");
    }
    assert!(found.contains("odoo-upgrade") && found.contains("review"));

    let hidden: Vec<&Value> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["visible"] == false)
        .collect();
    assert_eq!(
        hidden.len(),
        1,
        "the rejected deployed copy is the only hidden skill"
    );
    assert_eq!(hidden[0]["name"], "old-style");
    assert!(hidden[0]["invisibleReason"]
        .as_str()
        .unwrap()
        .starts_with("the deployed copy is rejected"));
}

#[test]
fn skills_ls_source_names_other_places_and_refuses_unknown_ones() {
    let world = world("src-other");
    let (code, odh) = ctl(&world, &["skills", "ls", "--source", "repo:odh"]);
    assert_eq!(code, 0, "{odh}");
    let rows = &odh["data"]["skills"];
    assert_eq!(
        names(rows),
        BTreeSet::from(["odh".to_string(), "odh-develop".into(), "v18-local".into()])
    );
    let remote_only: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["path"].as_str())
        .filter(|p| p.starts_with("origin/main:"))
        .collect();
    assert_eq!(
        remote_only.len(),
        2,
        "the two skills only the remote branch has"
    );

    let (code, nope) = ctl(&world, &["skills", "ls", "--source", "nope"]);
    assert_eq!(code, 1, "{nope}");
    assert_eq!(nope["ok"], false);
}

#[test]
fn a_forced_zero_budget_skips_the_detector_and_the_scan_says_partial() {
    let world = world("src-budget");
    let (code, report) = ctl(&world, &["sources", "ls", "--budget", "library=0"]);
    assert_eq!(code, 0, "{report}");
    let data = &report["data"];
    assert_eq!(data["partial"], true);
    assert_eq!(data["skipped"][0]["detector"], "library");
    assert!(data["sources"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["id"] != "library"));
    assert!(
        data["sources"].as_array().unwrap().len() >= 8,
        "the other detectors still ran"
    );

    let (code, whole) = ctl(&world, &["sources", "ls"]);
    assert_eq!(code, 0, "{whole}");
    assert_eq!(whole["data"]["partial"], false);
    assert_eq!(whole["data"]["skipped"], Value::Array(vec![]));
}

#[test]
fn a_scan_keeps_its_cache_under_the_data_dir_and_survives_losing_it() {
    let world = world("src-cache");
    let (code, first) = ctl(&world, &["sources", "ls", "--items"]);
    assert_eq!(code, 0, "{first}");
    let cache = world.data.join("plus/cache/sources.json");
    assert!(cache.is_file(), "the scan cache is {}", cache.display());
    let (_, second) = ctl(&world, &["sources", "ls", "--items"]);
    assert_eq!(first["data"]["items"], second["data"]["items"]);
    let (_, refreshed) = ctl(&world, &["sources", "ls", "--items", "--refresh"]);
    assert_eq!(first["data"]["items"], refreshed["data"]["items"]);

    std::fs::remove_file(&cache).unwrap();
    let (code, again) = ctl(&world, &["sources", "ls", "--items"]);
    assert_eq!(code, 0, "{again}");
    assert_eq!(first["data"]["items"], again["data"]["items"]);
}

#[test]
fn source_roots_preview_then_apply_and_defaults_cannot_be_removed() {
    let world = world("src-roots");
    let context = world.home.join(".config/mcpm/context.json");
    let work = world.path(&world.home.join("work"));
    let before = std::fs::read(&context).unwrap();

    let (code, preview) = ctl(&world, &["sources", "root", "add", &work, "--dry-run"]);
    assert_eq!(code, 0, "{preview}");
    assert_eq!(preview["data"]["dryRun"], true);
    assert!(preview["data"]["result"].is_null());
    assert_eq!(
        std::fs::read(&context).unwrap(),
        before,
        "a dry run writes nothing"
    );

    let (code, applied) = ctl(&world, &["sources", "root", "add", &work]);
    assert_eq!(code, 0, "{applied}");
    assert_eq!(applied["data"]["result"]["applied"], true);
    let (_, listed) = ctl(&world, &["sources", "root", "ls"]);
    let paths: Vec<&str> = listed["data"]["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&work.as_str()), "{paths:?}");

    let (code, again) = ctl(&world, &["sources", "root", "add", &work]);
    assert_eq!(code, 1, "adding a root twice is a conflict: {again}");

    let (code, removed) = ctl(&world, &["sources", "root", "rm", &work]);
    assert_eq!(code, 0, "{removed}");
    let (_, listed) = ctl(&world, &["sources", "root", "ls"]);
    assert!(listed["data"]["roots"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["path"] != work.as_str()));

    let default_root = world.path(&world.home.join("work/odh"));
    let (code, refused) = ctl(&world, &["sources", "root", "rm", &default_root]);
    assert_eq!(code, 1, "a default root cannot be removed: {refused}");
    assert_eq!(refused["error"]["code"], "not_found");
}
