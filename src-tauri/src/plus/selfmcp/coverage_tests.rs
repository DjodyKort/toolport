use super::*;
use std::collections::BTreeSet;

const SOURCES: &[(&str, &str)] = &[
    ("apply_update_tests.rs", include_str!("apply_update_tests.rs")),
    ("compression_tests.rs", include_str!("compression_tests.rs")),
    ("context_bundle_tests.rs", include_str!("context_bundle_tests.rs")),
    ("context_tests.rs", include_str!("context_tests.rs")),
    ("direct_tests.rs", include_str!("direct_tests.rs")),
    ("plugins_tests.rs", include_str!("plugins_tests.rs")),
    ("effect_tests.rs", include_str!("effect_tests.rs")),
    (
        "state_agent_style_tests.rs",
        include_str!("state_agent_style_tests.rs"),
    ),
    ("state_tests.rs", include_str!("state_tests.rs")),
    ("tap_tools_tests.rs", include_str!("tap_tools_tests.rs")),
    ("tasks_tests.rs", include_str!("tasks_tests.rs")),
    ("wired_tests.rs", include_str!("wired_tests.rs")),
];

const SKILLS_EDITS: &str = "tier_three_skill_edits_need_confirm_and_validate";
const SKILLS_ROUND_TRIP: &str = "skills_scaffold_sync_and_status_round_trip";
const TAPS_APPLY: &str = "the_tap_tools_apply_when_dry_run_is_false";
const AGENTS_FLOW: &str = "agents_flow_scaffold_sync_edit";
const STYLES_FLOW: &str = "styles_flow_with_apply_and_remove_tiers";
const SERVER_MUTATIONS: &str = "server_mutations_follow_their_tiers";
const SYNC_DRY_RUN: &str =
    "agents_and_styles_sync_write_nothing_on_a_dry_run_and_stay_inside_the_named_client";
const DIRECT_APPLY: &str = "add_ls_and_rm_apply_only_with_dry_run_false";

const PLUGIN_CONTROLS: &str =
    "plugin_control_tools_preview_by_default_and_write_only_inside_the_folder";
const BUNDLE_FLOW: &str = "bundle_tools_follow_their_tiers_and_apply_and_undo_only_inside_the_folder";

const TASKS_FLOW: &str = "task_tools_follow_their_tiers_and_a_run_without_an_approval_starts_nothing";

const SIDE_EFFECT_TESTS: &[(&str, &str, &str)] = &[
    ("tasks_run", "tasks_tests.rs", TASKS_FLOW),
    ("tasks_cancel", "tasks_tests.rs", TASKS_FLOW),
    ("context_bundle_apply", "context_bundle_tests.rs", BUNDLE_FLOW),
    ("context_bundle_undo", "context_bundle_tests.rs", BUNDLE_FLOW),
    ("plugins_config", "plugins_tests.rs", PLUGIN_CONTROLS),
    ("plugins_mcp", "plugins_tests.rs", PLUGIN_CONTROLS),
    (
        "context_measure",
        "context_tests.rs",
        "a_call_measures_the_variants_and_a_second_call_is_answered_from_the_cache",
    ),
    ("skills_scaffold", "wired_tests.rs", SKILLS_ROUND_TRIP),
    ("skills_sync", "wired_tests.rs", SKILLS_ROUND_TRIP),
    ("skills_tap_add", "tap_tools_tests.rs", TAPS_APPLY),
    ("skills_tap_remove", "tap_tools_tests.rs", TAPS_APPLY),
    ("skills_tap_update", "tap_tools_tests.rs", TAPS_APPLY),
    ("skills_install", "tap_tools_tests.rs", TAPS_APPLY),
    (
        "skills_bundle",
        "state_tests.rs",
        "skills_bundle_plans_by_default_and_never_overwrites_or_leaves_the_directory_unchecked",
    ),
    (
        "skills_unbundle",
        "state_tests.rs",
        "skills_unbundle_round_trips_a_bundle_and_refuses_files_outside_the_skill_trees",
    ),
    (
        "skills_clean",
        "state_tests.rs",
        "skills_clean_previews_by_default_and_removes_what_sync_wrote_on_apply",
    ),
    (
        "skills_uninstall",
        "state_tests.rs",
        "skills_uninstall_removes_the_skill_its_outputs_and_its_lock_entry_only_when_applied",
    ),
    (
        "skills_resolve",
        "state_tests.rs",
        "skills_resolve_reports_shadowing_files_and_replaces_them_only_when_migrating",
    ),
    ("skills_edit_body", "wired_tests.rs", SKILLS_EDITS),
    ("skills_edit_frontmatter", "wired_tests.rs", SKILLS_EDITS),
    ("skills_delete", "wired_tests.rs", SKILLS_EDITS),
    (
        "skills_git_push",
        "wired_tests.rs",
        "skills_git_push_commits_and_pushes_only_when_confirmed",
    ),
    ("agents_scaffold", "wired_tests.rs", AGENTS_FLOW),
    ("agents_sync", "effect_tests.rs", SYNC_DRY_RUN),
    ("agents_edit_body", "wired_tests.rs", AGENTS_FLOW),
    (
        "agents_clean",
        "state_agent_style_tests.rs",
        "agents_clean_previews_by_default_and_keeps_the_lockfile_on_apply",
    ),
    (
        "agents_uninstall",
        "state_agent_style_tests.rs",
        "agents_uninstall_removes_the_agent_its_outputs_and_its_lock_entry_only_when_applied",
    ),
    ("styles_scaffold", "wired_tests.rs", STYLES_FLOW),
    ("styles_sync_tier1", "effect_tests.rs", SYNC_DRY_RUN),
    ("styles_apply", "wired_tests.rs", STYLES_FLOW),
    ("styles_remove", "wired_tests.rs", STYLES_FLOW),
    ("styles_edit_body", "wired_tests.rs", STYLES_FLOW),
    (
        "styles_clean",
        "state_agent_style_tests.rs",
        "styles_clean_previews_by_default_and_clears_the_synced_outputs_on_apply",
    ),
    (
        "compression_enable",
        "compression_tests.rs",
        "compression_enable_previews_by_default_and_applies_only_with_confirm",
    ),
    (
        "compression_disable",
        "compression_tests.rs",
        "enabling_headroom_registers_its_server_and_disable_removes_it_again",
    ),
    (
        "compression_set_provider",
        "compression_tests.rs",
        "compression_set_provider_and_use_preview_by_default",
    ),
    (
        "compression_use",
        "compression_tests.rs",
        "compression_set_provider_and_use_preview_by_default",
    ),
    (
        "compression_sync",
        "compression_tests.rs",
        "compression_sync_adopts_a_legacy_policy_only_when_applied",
    ),
    (
        "compression_seal",
        "compression_tests.rs",
        "compression_seal_records_the_live_posture_only_when_applied",
    ),
    ("servers_add_profile_tag", "wired_tests.rs", SERVER_MUTATIONS),
    ("servers_remove_profile_tag", "wired_tests.rs", SERVER_MUTATIONS),
    ("servers_install", "wired_tests.rs", SERVER_MUTATIONS),
    ("servers_update_config", "wired_tests.rs", SERVER_MUTATIONS),
    ("servers_set_mode", "wired_tests.rs", SERVER_MUTATIONS),
    ("servers_uninstall", "wired_tests.rs", SERVER_MUTATIONS),
    (
        "servers_apply_update",
        "apply_update_tests.rs",
        "applying_an_update_fast_forwards_the_named_checkout_and_writes_only_its_entry",
    ),
    (
        "servers_fork_sync",
        "wired_tests.rs",
        "fork_sync_replays_local_commits_onto_a_second_remote",
    ),
    (
        "servers_auth",
        "wired_tests.rs",
        "servers_auth_captures_the_consent_url_from_stderr",
    ),
    (
        "clients_sync",
        "effect_tests.rs",
        "clients_sync_writes_the_gateway_entry_and_prunes_orphans_only_when_applied",
    ),
    ("client_direct_add", "direct_tests.rs", DIRECT_APPLY),
    ("client_direct_rm", "direct_tests.rs", DIRECT_APPLY),
    (
        "sync_push",
        "effect_tests.rs",
        "sync_push_commits_an_encrypted_bundle_only_when_confirmed",
    ),
];

fn body_of(file: &str, test: &str) -> &'static str {
    let (_, text) = SOURCES
        .iter()
        .find(|(name, _)| *name == file)
        .unwrap_or_else(|| panic!("{file} is not a listed test source"));
    let header = format!("fn {test}()");
    let at = text
        .find(&header)
        .unwrap_or_else(|| panic!("{file} has no fn {test}"));
    assert!(
        text[..at].trim_end().ends_with("#[test]"),
        "{file}::{test} is not a test"
    );
    let rest = &text[at..];
    &rest[..rest.find("\n}\n").unwrap_or(rest.len())]
}

#[test]
fn every_tool_with_a_side_effect_is_driven_by_a_named_test() {
    let ledger: BTreeSet<&str> = SIDE_EFFECT_TESTS.iter().map(|(tool, _, _)| *tool).collect();
    assert_eq!(ledger.len(), SIDE_EFFECT_TESTS.len(), "a tool is listed twice");
    let writers: BTreeSet<&str> = TOOLS
        .iter()
        .filter(|t| t.tier >= 2)
        .map(|t| t.name)
        .collect();
    assert_eq!(
        ledger, writers,
        "a tool of tier 2 or above needs a behavior test listed here"
    );
    for (tool, file, test) in SIDE_EFFECT_TESTS {
        let body = body_of(file, test);
        assert!(
            body.contains(&format!("\"{tool}\"")),
            "{file}::{test} does not call {tool}"
        );
    }
}
