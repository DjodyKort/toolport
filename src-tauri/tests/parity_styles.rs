//! Replays the mcpm golden inputs for output styles: tier-1 sync (claude-code, roomodes merge)
//! and tier-2 apply / switch / remove across the thirteen apply-remove clients.

mod common;

use common::replay::*;
use conduit_lib::plus::skills::styles::{
    apply_style, discover_styles, remove_style, sync_styles, StyleOptions,
};
use conduit_lib::plus::skills::{load_lockfile, save_lockfile, FixedClock};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

const CASES: &[&str] = &[
    "tier1-project",
    "tier1-sync-all-project",
    "tier2-apply-project",
    "tier2-apply-selected",
    "tier2-switch-project",
    "tier2-remove-project",
    "tier2-zed-existing",
];

fn clients(args: &Value) -> Option<Vec<String>> {
    let wanted: Vec<String> = args["clients"]
        .as_array()
        .map(|a| a.iter().map(|c| c.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    (!wanted.is_empty()).then_some(wanted)
}

fn styles_sync_entry(home: &Path, args: &Value, clock: &FixedClock) -> Extras {
    let repo = home.join(args["repo"].as_str().unwrap());
    let opts = StyleOptions {
        client_keys: clients(args),
        dry_run: false,
        clock,
    };
    let lock = sync_styles(&discover_styles(&repo), &repo, load_lockfile(&repo), &opts).unwrap();
    save_lockfile(&repo, &lock).unwrap();
    Vec::new()
}

fn files(home: &Path) -> BTreeSet<String> {
    snapshot(home).into_keys().collect()
}

fn styles_tier2_entry(home: &Path, args: &Value, clock: &FixedClock) -> Extras {
    let repo = home.join(args["repo"].as_str().unwrap());
    let opts = StyleOptions {
        client_keys: clients(args),
        dry_run: false,
        clock,
    };
    let styles = discover_styles(&repo);
    let mut lock = load_lockfile(&repo);
    let mut extras = Vec::new();
    for step in args["steps"].as_array().unwrap() {
        match step["op"].as_str().unwrap() {
            "apply" => {
                let name = step["style"].as_str().unwrap();
                let style = styles.iter().find(|s| s.name() == name).unwrap();
                lock = Some(apply_style(style, &repo, lock, &opts).unwrap());
            }
            "remove" => {
                let before = files(home);
                lock = Some(remove_style(&repo, lock, &opts).unwrap());
                let after = files(home);
                let text: String = before.difference(&after).map(|r| format!("{r}\n")).collect();
                extras.push(("_cleaned.txt".to_string(), text));
            }
            other => panic!("unknown step {other}"),
        }
    }
    save_lockfile(&repo, &lock.unwrap()).unwrap();
    extras
}

#[test]
fn every_declared_case_has_vendored_inputs_and_a_golden() {
    assert_vendored("styles", CASES);
}

#[test]
fn tier1_sync_matches_golden() {
    for case in ["tier1-project", "tier1-sync-all-project"] {
        replay("styles", case, &styles_sync_entry);
    }
}

#[test]
fn tier2_apply_and_switch_match_golden() {
    for case in [
        "tier2-apply-project",
        "tier2-apply-selected",
        "tier2-switch-project",
        "tier2-zed-existing",
    ] {
        replay("styles", case, &styles_tier2_entry);
    }
}

#[test]
fn tier2_remove_matches_golden() {
    replay("styles", "tier2-remove-project", &styles_tier2_entry);
}
