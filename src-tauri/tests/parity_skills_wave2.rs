//! Replays the mcpm golden inputs for the wave-2 skills transpilers (continue, roo-code, trae,
//! amazon-q, jetbrains, aider, codex-cli, gemini-cli, goose-cli, zed) and the vscode registry
//! gap through the full registry and compares every produced file with the golden tree.

mod common;

use common::replay::*;
use conduit_lib::plus::skills::transpilers::{
    register_all_with_home, register_vscode_copilot,
};
use conduit_lib::plus::skills::{
    discover_skills, save_lockfile, sync_skills, FixedClock, SyncOptions, TranspilerRegistry,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const CASES: &[&str] = &[
    "continue-project",
    "continue-global",
    "roo-code-project",
    "roo-code-global",
    "trae-project",
    "trae-global",
    "amazon-q-project",
    "amazon-q-global",
    "jetbrains-project",
    "jetbrains-global",
    "aider-project",
    "aider-global",
    "codex-cli-project",
    "codex-cli-global",
    "gemini-cli-project",
    "gemini-cli-global",
    "goose-cli-project",
    "goose-cli-global",
    "zed-project",
    "zed-append-existing",
    "vscode-registry-gap",
    "vscode-direct-import",
];

fn sync_pass(home: &Path, args: &Value, clock: &FixedClock) {
    let global = args["global"].as_bool().unwrap_or(false);
    let repo = home.join(args["repo"].as_str().unwrap());
    let wanted: Vec<String> = args["clients"]
        .as_array()
        .map(|a| a.iter().map(|c| c.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    let mut registry = TranspilerRegistry::new();
    register_all_with_home(&mut registry, Some(home.to_path_buf()));
    let imports_vscode = args["imports"]
        .as_array()
        .is_some_and(|a| a.iter().any(|m| m.as_str().is_some_and(|m| m.ends_with("vscode_copilot"))));
    if imports_vscode {
        register_vscode_copilot(&mut registry);
    }
    let lock_dir = if global {
        home.join(".config/mcpm")
    } else {
        repo.clone()
    };
    let opts = SyncOptions {
        output_root: if global {
            home.to_path_buf()
        } else {
            repo.clone()
        },
        lock_dir: lock_dir.clone(),
        global_mode: global,
        dry_run: false,
        migrate: args["migrate"].as_bool(),
        client_keys: (!wanted.is_empty()).then_some(wanted),
        clock,
    };
    // D-027 divergence: goldens use mcpm's narrower asset allowlist; see parity_skills_core.rs.
    let result = conduit_lib::plus::skills::with_asset_policy(
        conduit_lib::plus::skills::AssetPolicy::Mcpm,
        || sync_skills(&discover_skills(&repo), &registry, &opts),
    )
    .unwrap();
    save_lockfile(&lock_dir, &result.lockfile).unwrap();
}

fn files(home: &Path) -> BTreeSet<String> {
    snapshot(home).into_keys().collect()
}

fn skills_entry(home: &Path, args: &Value, clock: &FixedClock) -> Extras {
    sync_pass(home, args, clock);
    let Some(remove) = args["then_remove"].as_array() else {
        return Vec::new();
    };
    for rel in remove {
        let p = home.join(rel.as_str().unwrap());
        if p.is_dir() {
            fs::remove_dir_all(&p).unwrap();
        } else if p.exists() {
            fs::remove_file(&p).unwrap();
        }
    }
    let before_second = files(home);
    sync_pass(home, args, clock);
    let after = files(home);
    let text: String = before_second
        .difference(&after)
        .map(|r| format!("{r}\n"))
        .collect();
    vec![("_cleaned.txt".into(), text)]
}

#[test]
fn every_declared_case_has_vendored_inputs_and_a_golden() {
    assert_vendored("skills", CASES);
}

#[test]
fn registry_follows_mcpm_import_order_and_omits_vscode() {
    let mut registry = TranspilerRegistry::new();
    register_all_with_home(&mut registry, None);
    let keys: Vec<&str> = registry.all().map(|t| t.client_key()).collect();
    assert_eq!(
        keys,
        [
            "agents-md",
            "aider",
            "amazon-q",
            "claude-code",
            "cline",
            "codex-cli",
            "continue",
            "cursor",
            "gemini-cli",
            "goose-cli",
            "jetbrains",
            "roo-code",
            "trae",
            "windsurf",
            "zed",
        ]
    );
    assert!(registry.get("vscode").is_none());
}

macro_rules! golden_tests {
    ($($name:ident => [$($case:literal),+ $(,)?]),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                for case in [$($case),+] {
                    replay("skills", case, &skills_entry);
                }
            }
        )+
    };
}

golden_tests! {
    continue_roo_trae_match_golden => [
        "continue-project", "continue-global", "roo-code-project", "roo-code-global",
        "trae-project", "trae-global",
    ],
    amazon_q_jetbrains_aider_match_golden => [
        "amazon-q-project", "amazon-q-global", "jetbrains-project", "jetbrains-global",
        "aider-project", "aider-global",
    ],
    codex_gemini_goose_match_golden => [
        "codex-cli-project", "codex-cli-global", "gemini-cli-project", "gemini-cli-global",
        "goose-cli-project", "goose-cli-global",
    ],
    zed_matches_golden => ["zed-project", "zed-append-existing"],
    vscode_gap_and_direct_import_match_golden => ["vscode-registry-gap", "vscode-direct-import"],
}
