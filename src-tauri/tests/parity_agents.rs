//! Replays the mcpm golden inputs for agents sync (claude-code, codex-cli TOML, cursor,
//! gemini-cli, vscode, roomodes) and compares every produced file with the golden tree.

mod common;

use common::replay::*;
use conduit_lib::plus::skills::agents::{discover_agents, sync_agents, AgentSyncOptions};
use conduit_lib::plus::skills::{load_lockfile, save_lockfile, FixedClock};
use serde_json::Value;
use std::path::Path;

const CASES: &[&str] = &[
    "claude-code-project",
    "codex-cli-project",
    "codex-toml-quirks",
    "cursor-project",
    "gemini-cli-project",
    "vscode-project",
    "roomodes-project",
    "roomodes-existing-file",
    "all-clients-project",
    "all-clients-global",
];

fn agents_entry(home: &Path, args: &Value, clock: &FixedClock) -> Extras {
    let global = args["global"].as_bool().unwrap_or(false);
    let repo = home.join(args["repo"].as_str().unwrap());
    let wanted: Vec<String> = args["clients"]
        .as_array()
        .map(|a| a.iter().map(|c| c.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    let lock_dir = if global {
        home.join(".config/mcpm")
    } else {
        repo.clone()
    };
    let opts = AgentSyncOptions {
        output_root: if global {
            home.to_path_buf()
        } else {
            repo.clone()
        },
        global_mode: global,
        dry_run: false,
        client_keys: (!wanted.is_empty()).then_some(wanted),
        clock,
    };
    let lock = sync_agents(&discover_agents(&repo), load_lockfile(&lock_dir), &opts).unwrap();
    save_lockfile(&lock_dir, &lock).unwrap();
    Vec::new()
}

#[test]
fn every_declared_case_has_vendored_inputs_and_a_golden() {
    assert_vendored("agents", CASES);
}

#[test]
fn per_client_agents_match_golden() {
    for case in [
        "claude-code-project",
        "codex-cli-project",
        "cursor-project",
        "gemini-cli-project",
        "vscode-project",
        "roomodes-project",
    ] {
        replay("agents", case, &agents_entry);
    }
}

#[test]
fn quirks_match_golden() {
    for case in ["codex-toml-quirks", "roomodes-existing-file"] {
        replay("agents", case, &agents_entry);
    }
}

#[test]
fn all_clients_match_golden() {
    for case in ["all-clients-project", "all-clients-global"] {
        replay("agents", case, &agents_entry);
    }
}
