//! Context bundles through the real `toolportctl` (MIG-CTX-10): what the golden envelopes cannot
//! show. Nothing outside the folder where Claude starts may change, every foreign byte of the
//! local files survives apply and undo, and `context sync` applies a bound bundle only when the
//! auto-apply switch is on and only to the matching folders that have none.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

#[path = "common/claude_stub.rs"]
mod claude_stub;
#[path = "common/ctl_fixtures.rs"]
mod ctl_fixtures;
#[path = "common/ctl_world.rs"]
mod ctl_world;
#[path = "common/exec.rs"]
mod exec;
#[path = "common/loads_world.rs"]
mod loads_world;

use ctl_fixtures::{bundle_home, ctl_data, FOREIGN_SETTINGS};
use ctl_world::{read_json, CtlWorld};

const USER_NOTES: &str = "# My notes\nKeep the invoices in order.\n";

fn world(tag: &str) -> CtlWorld {
    let world = CtlWorld::new(tag, env!("CARGO_BIN_EXE_mock-mcp-server"));
    bundle_home(&world);
    world
}

fn client(world: &CtlWorld) -> PathBuf {
    world.home.join("work/erp/clients/acme-erp")
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn ledger(world: &CtlWorld) -> PathBuf {
    world.data.join("plus/profile-ledger.json")
}

/// Every file of the world except the ledger Toolport keeps, by path.
fn files(world: &CtlWorld) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut all = world.snapshot();
    all.remove(&ledger(world));
    all
}

fn changed(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> Vec<PathBuf> {
    after
        .iter()
        .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
        .map(|(path, _)| path.clone())
        .chain(before.keys().filter(|p| !after.contains_key(*p)).cloned())
        .collect()
}

fn argv<'a>(parts: &[&'a str], cwd: &'a str) -> Vec<&'a str> {
    parts.iter().copied().chain(["--cwd", cwd]).collect()
}

#[test]
fn nothing_above_the_start_folder_changes_and_undo_restores_every_foreign_byte() {
    let world = world("bundle-nested");
    let client = client(&world);
    let workspace = world.home.join("work/erp");
    let workspace_exclude = workspace.join(".git/info/exclude");
    std::fs::create_dir_all(workspace_exclude.parent().unwrap()).unwrap();
    std::fs::write(&workspace_exclude, "# workspace repository\n").unwrap();
    let cwd = world.path(&client);

    let before = files(&world);
    let plan = ctl_data(&world, &argv(&["context", "bundle", "apply", "acme-dev", "--dry-run"], &cwd));
    assert_eq!(plan["dryRun"], true);
    assert_eq!(files(&world), before, "a dry run writes nothing");

    let result = ctl_data(&world, &argv(&["context", "bundle", "apply", "acme-dev"], &cwd));
    assert_eq!(result["result"]["applied"], true);
    let after = files(&world);
    for path in changed(&before, &after) {
        assert!(
            path.starts_with(&client),
            "{} is outside the folder where Claude starts",
            path.display()
        );
    }
    assert_eq!(text(&workspace_exclude), "# workspace repository\n");
    assert!(
        !workspace.join("clients/.claude").exists() && !workspace.join("clients/CLAUDE.local.md").exists()
    );

    let settings_text = text(&client.join(".claude/settings.local.json"));
    assert!(
        settings_text.contains("\"allow\": [\"Bash(git status)\", \"Read(./docs/**)\"]"),
        "the foreign permissions keep their bytes:\n{settings_text}"
    );
    let settings = read_json(&client.join(".claude/settings.local.json"));
    assert_eq!(settings["model"], "sonnet");
    assert_eq!(settings["enabledPlugins"]["user-notes@notes-market"], true);
    assert_eq!(settings["enabledPlugins"]["tools-pack@tools-market"], false);
    assert_eq!(settings["skillOverrides"]["scratch-one"], "off");
    assert_eq!(settings["skillOverrides"]["long-guide"], "name-only");
    let deny: Vec<&str> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(deny, ["Bash(rm -rf *)", "Agent(reviewer-bot)"]);
    let notes = text(&client.join("CLAUDE.local.md"));
    assert!(notes.starts_with(USER_NOTES) && notes.contains("toolport:bundle:begin acme-dev"));
    let exclude = text(&client.join(".git/info/exclude"));
    assert!(exclude.lines().any(|l| l == ".claude/settings.local.json"));
    assert!(exclude.lines().any(|l| l == "CLAUDE.local.md"));

    let status = ctl_data(&world, &["context", "bundle", "status", "--cwd", &cwd]);
    assert_eq!(status["applied"]["bundle"], "acme-dev");
    assert_eq!(status["applied"]["drift"], false);
    assert!(!text(&ledger(&world)).contains("Bash(rm -rf *)"));

    let undone = ctl_data(&world, &argv(&["context", "bundle", "undo"], &cwd));
    assert_eq!(undone["conflicts"].as_array().unwrap().len(), 0);
    assert_eq!(text(&client.join(".claude/settings.local.json")), FOREIGN_SETTINGS);
    assert_eq!(text(&client.join("CLAUDE.local.md")), USER_NOTES);
    assert_eq!(files(&world), before, "undo leaves the world as it was");
    assert_eq!(text(&workspace_exclude), "# workspace repository\n");
    let status = ctl_data(&world, &["context", "bundle", "status", "--cwd", &cwd]);
    assert!(status["applied"].is_null());
}

#[test]
fn a_start_folder_inside_a_repository_gets_a_warning_and_the_repository_root_stays_as_it_was() {
    let world = world("bundle-subfolder");
    let client = client(&world);
    let sub = client.join("addons/billing");
    let cwd = world.path(&sub);
    let before = files(&world);

    let plan = ctl_data(&world, &argv(&["context", "bundle", "apply", "acme-dev", "--dry-run"], &cwd));
    let warnings: Vec<&str> = plan["plan"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("not git-ignored")),
        "the plan says the files would not be git-ignored: {warnings:?}"
    );
    ctl_data(&world, &argv(&["context", "bundle", "apply", "acme-dev"], &cwd));
    let after = files(&world);
    for path in changed(&before, &after) {
        assert!(path.starts_with(&sub), "{} is outside {}", path.display(), sub.display());
    }
    assert!(sub.join(".claude/settings.local.json").is_file());
    assert_eq!(text(&client.join(".claude/settings.local.json")), FOREIGN_SETTINGS);

    ctl_data(&world, &argv(&["context", "bundle", "undo"], &cwd));
    let left = changed(&before, &files(&world));
    assert!(left.is_empty(), "{left:?}");
}

fn bind_world(world: &CtlWorld) {
    let library = world.home.join(".config/mcpm/skills_repo/profiles");
    std::fs::write(
        library.join("erp-bound.yaml"),
        "format: 1\nname: erp-bound\nskills:\n  off: [scratch-one]\nbind: [\"~/work/erp/clients/acme-*\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(world.home.join("work/erp/clients/internal/.git")).unwrap();
}

#[test]
fn sync_applies_a_bound_bundle_only_when_auto_apply_is_on_and_only_to_matching_folders_without_one() {
    let world = world("bundle-sync");
    bind_world(&world);
    let acme = client(&world);
    let two = world.home.join("work/erp/clients/acme-two");
    let internal = world.home.join("work/erp/clients/internal");
    let cwd_two = world.path(&two);

    let config = ctl_data(&world, &["context", "bundle", "config"]);
    assert_eq!(config["autoApply"], false);
    let synced = ctl_data(&world, &["context", "sync"]);
    assert!(synced.get("bundles").is_none(), "no bundle work while the switch is off");
    assert_eq!(text(&acme.join(".claude/settings.local.json")), FOREIGN_SETTINGS);
    assert!(!internal.join(".claude").exists());

    ctl_data(&world, &["context", "bundle", "config", "--auto-apply", "on"]);
    ctl_data(&world, &argv(&["context", "bundle", "apply", "acme-dev"], &cwd_two));
    let before = files(&world);
    let dry = ctl_data(&world, &["context", "sync", "--dry-run"]);
    assert_eq!(dry["dryRun"], true);
    assert!(dry.get("bundles").is_none());
    assert_eq!(text(&acme.join(".claude/settings.local.json")), FOREIGN_SETTINGS);

    let synced = ctl_data(&world, &["context", "sync"]);
    let bundles = synced["bundles"].as_array().expect("the bundles key when the switch is on");
    assert_eq!(bundles.len(), 1, "{bundles:?}");
    assert_eq!(bundles[0]["bundle"], "erp-bound");
    assert_eq!(bundles[0]["applied"], true);
    assert!(bundles[0]["folder"].as_str().unwrap().ends_with("clients/acme-erp"));
    let settings = read_json(&acme.join(".claude/settings.local.json"));
    assert_eq!(settings["skillOverrides"]["scratch-one"], "off");
    assert_eq!(settings["enabledPlugins"]["user-notes@notes-market"], true);
    let status = ctl_data(&world, &["context", "bundle", "status", "--cwd", &cwd_two]);
    assert_eq!(status["applied"]["bundle"], "acme-dev", "a folder that has a bundle keeps it");
    assert!(!internal.join(".claude").exists(), "a folder the pattern does not match is left alone");
    for path in changed(&before, &files(&world)) {
        assert!(
            path.starts_with(&acme) || path.starts_with(world.home.join(".claude")) || path.starts_with(&world.data),
            "{} changed although nothing there should",
            path.display()
        );
    }

    let again = ctl_data(&world, &["context", "sync"]);
    assert_eq!(again["bundles"].as_array().unwrap().len(), 0, "every match has a bundle now");

    ctl_data(&world, &["context", "bundle", "config", "--auto-apply", "off"]);
    ctl_data(&world, &argv(&["context", "bundle", "undo"], &world.path(&acme)));
    let synced = ctl_data(&world, &["context", "sync"]);
    assert!(synced.get("bundles").is_none());
    assert_eq!(text(&acme.join(".claude/settings.local.json")), FOREIGN_SETTINGS);
}

#[test]
fn context_sync_keeps_the_block_in_a_layer_managed_claude_local_md() {
    let world = world("bundle-managed");
    let client = client(&world);
    let cwd = world.path(&client);
    let local = client.join("CLAUDE.local.md");
    std::fs::remove_file(&local).unwrap();
    ctl_data(&world, &["context", "sync"]);
    let managed = text(&local);
    assert!(managed.contains("Managed by"), "the client layer deploys the file:\n{managed}");

    ctl_data(&world, &argv(&["context", "bundle", "apply", "acme-dev"], &cwd));
    let applied = text(&local);
    assert!(applied.starts_with(&managed) && applied.contains("toolport:bundle:begin acme-dev"));

    ctl_data(&world, &["context", "sync"]);
    assert_eq!(text(&local), applied, "a layer deploy carries the bundle block over");
    let status = ctl_data(&world, &["context", "bundle", "status", "--cwd", &cwd]);
    assert_eq!(status["applied"]["drift"], false, "{status}");

    ctl_data(&world, &argv(&["context", "bundle", "undo"], &cwd));
    assert_eq!(text(&local), managed);
    ctl_data(&world, &["context", "sync"]);
    assert_eq!(text(&local), managed);
}
