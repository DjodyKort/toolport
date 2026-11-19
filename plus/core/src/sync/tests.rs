//! Sync between two synthetic machines: each has its own home and data folder, the transport is a
//! shared folder or a bare git repository on disk. Nothing leaves the temp dir.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use toolport_plus_api::ServerRecord;

use super::config::{self, FILE_NAME};
use super::engine::{self, PullOptions, PushOptions, SetupOptions};
use crate::setup::tests::{World, PASS};
use crate::setup::PartId;
use crate::skills::git::{MockGit, SystemGit};

fn server(id: &str, name: &str, command: &str) -> ServerRecord {
    let mut record = ServerRecord {
        id: id.into(),
        name: name.into(),
        enabled: true,
        ..Default::default()
    };
    record.extra.insert("transport".into(), json!("stdio"));
    record.extra.insert("command".into(), json!(command));
    record
}

fn folder(world: &World) -> PathBuf {
    let dir = world.dir.path().join("shared");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn join_folder(world: &World, dir: &Path, parts: &[PartId]) {
    engine::setup(
        &world.ctx(),
        &SetupOptions {
            folder: Some(dir.to_path_buf()),
            passphrase: Some(PASS.into()),
            parts: Some(parts.iter().copied().collect()),
            ..Default::default()
        },
    )
    .unwrap();
}

fn pull(world: &World, take: &[&str]) -> Value {
    engine::pull(
        &world.ctx(),
        &PullOptions {
            take: take.iter().map(|t| t.to_string()).collect(),
            ..Default::default()
        },
        &SystemGit,
    )
    .unwrap()
}

fn apply(world: &World, take: &[&str]) -> Value {
    let plan = pull(world, take);
    engine::pull(
        &world.ctx(),
        &PullOptions {
            take: take.iter().map(|t| t.to_string()).collect(),
            apply: true,
            plan: Some(plan["plan"].as_str().unwrap().to_string()),
            ..Default::default()
        },
        &SystemGit,
    )
    .unwrap()
}

fn push(world: &World) -> Value {
    engine::push(&world.ctx(), &PushOptions::default(), &SystemGit).unwrap()
}

fn row<'a>(plan: &'a Value, id: &str) -> &'a Value {
    plan["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no row {id} in {plan}"))
}

const SERVERS_ONLY: [PartId; 1] = [PartId::Servers];
const WITH_FILES: [PartId; 3] = [PartId::Servers, PartId::Settings, PartId::ClaudeUser];

/// Everything but sync's own bookkeeping is as it was.
fn assert_untouched(world: &World, before: &std::collections::BTreeMap<String, Option<Vec<u8>>>) {
    let own = |k: &&String| !k.starts_with("data/plus/sync/");
    let after = world.snapshot();
    let changed: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .filter(own)
        .filter(|k| before.get(*k) != after.get(*k))
        .collect();
    assert!(changed.is_empty(), "changed: {changed:?}");
}

fn two_machines() -> (World, World, PathBuf) {
    let a = World::new();
    let b = World::new();
    let shared = folder(&a);
    a.host
        .registry
        .servers
        .borrow_mut()
        .push(server("srv-beta", "beta", "beta-mcp"));
    (a, b, shared)
}

#[test]
fn test01_the_pushed_file_is_sealed_as_a_whole_and_names_nothing() {
    let (a, _b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    let data = push(&a);
    assert_eq!(data["pushed"], true);
    let file = shared.join(FILE_NAME);
    let bytes = std::fs::read(&file).unwrap();
    assert!(crate::setup::crypto::is_sealed_file(&file));
    for needle in [
        "manifest.json",
        "servers.json",
        "beta",
        "alpha",
        "toolport-setup",
    ] {
        assert!(
            !bytes.windows(needle.len()).any(|w| w == needle.as_bytes()),
            "{needle} is readable in the sealed file"
        );
    }
    let error = crate::setup::inspect::run(&file).unwrap_err();
    assert!(error.message.contains("sealed as a whole"));
}

#[test]
fn test02_a_second_machine_plans_first_and_adds_held_servers_only_with_the_hash() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    push(&a);
    join_folder(&b, &shared, &SERVERS_ONLY);
    let before = b.snapshot();
    let plan = pull(&b, &[]);
    assert_untouched(&b, &before);
    assert_eq!(plan["dryRun"], true);
    assert_eq!(row(&plan, "server:beta")["state"], "add");
    assert_eq!(row(&plan, "server:alpha")["state"], "here");
    let stale = engine::pull(
        &b.ctx(),
        &PullOptions {
            apply: true,
            plan: Some("0000000000".into()),
            ..Default::default()
        },
        &SystemGit,
    )
    .unwrap_err();
    assert!(stale.message.contains("changed since the plan"));
    let applied = apply(&b, &[]);
    assert_eq!(applied["failed"], 0);
    let names: Vec<String> = b
        .host
        .registry
        .servers
        .borrow()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    assert!(names.contains(&"beta".to_string()));
    let again = pull(&b, &[]);
    assert_eq!(row(&again, "server:beta")["state"], "here");
}

#[test]
fn test03_a_change_on_the_other_machine_alone_is_incoming_and_undo_offers_it_again() {
    let (a, b, shared) = two_machines();
    a.host
        .registry
        .unknown_fields
        .borrow_mut()
        .insert("safety".into(), json!({"mode": "ask"}));
    b.host
        .registry
        .unknown_fields
        .borrow_mut()
        .insert("safety".into(), json!({"mode": "ask"}));
    join_folder(&a, &shared, &WITH_FILES);
    join_folder(&b, &shared, &WITH_FILES);
    push(&a);
    apply(&b, &[]);
    a.host
        .registry
        .unknown_fields
        .borrow_mut()
        .insert("safety".into(), json!({"mode": "strict"}));
    push(&a);
    let plan = pull(&b, &[]);
    assert_eq!(row(&plan, "settings")["state"], "incoming");
    assert_eq!(plan["summary"]["conflict"], 0);
    apply(&b, &[]);
    assert_eq!(
        b.host.registry.unknown_fields.borrow()["safety"],
        json!({"mode": "strict"})
    );
    engine::undo(&b.ctx(), false).unwrap();
    assert_eq!(
        b.host.registry.unknown_fields.borrow()["safety"],
        json!({"mode": "ask"})
    );
    assert_eq!(row(&pull(&b, &[]), "settings")["state"], "incoming");
}

#[test]
fn test04_both_machines_changing_a_file_is_a_conflict_that_keeps_this_machine_until_taken() {
    let (a, b, shared) = two_machines();
    a.claude("CLAUDE.md", "shared rules\n");
    b.claude("CLAUDE.md", "shared rules\n");
    join_folder(&a, &shared, &WITH_FILES);
    join_folder(&b, &shared, &WITH_FILES);
    push(&a);
    apply(&b, &[]);
    a.claude("CLAUDE.md", "rules edited on a\n");
    b.claude("CLAUDE.md", "rules edited on b\n");
    push(&a);
    let plan = pull(&b, &[]);
    assert_eq!(row(&plan, "claude:CLAUDE.md")["state"], "conflict");
    apply(&b, &[]);
    let path = b.home.join(".claude/CLAUDE.md");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "rules edited on b\n"
    );
    let state = config::read_state(&b.ctx()).unwrap();
    assert_eq!(state.pending, vec!["claude:CLAUDE.md".to_string()]);
    let refused = engine::push(&b.ctx(), &PushOptions::default(), &SystemGit).unwrap_err();
    assert!(refused.message.contains("claude:CLAUDE.md"));
    apply(&b, &["claude:CLAUDE.md"]);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "rules edited on a\n"
    );
    assert!(config::read_state(&b.ctx()).unwrap().pending.is_empty());
}

#[test]
fn test05_a_push_over_a_remote_this_machine_has_not_pulled_is_refused() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    join_folder(&b, &shared, &SERVERS_ONLY);
    push(&a);
    let error = engine::push(&b.ctx(), &PushOptions::default(), &SystemGit).unwrap_err();
    assert!(error.message.contains("pull first"), "{}", error.message);
    apply(&b, &[]);
    assert_eq!(push(&b)["pushed"], true);
    let error = engine::push(&a.ctx(), &PushOptions::default(), &SystemGit).unwrap_err();
    assert!(error.message.contains("pull first"));
}

#[test]
fn test06_a_file_removed_here_since_the_last_sync_stays_removed() {
    let (a, b, shared) = two_machines();
    a.claude("rules/extra.md", "extra\n");
    join_folder(&a, &shared, &WITH_FILES);
    join_folder(&b, &shared, &WITH_FILES);
    push(&a);
    apply(&b, &[]);
    let path = b.home.join(".claude/rules/extra.md");
    assert!(path.is_file());
    std::fs::remove_file(&path).unwrap();
    let plan = pull(&b, &[]);
    assert_eq!(row(&plan, "claude:rules/extra.md")["state"], "mine");
    apply(&b, &[]);
    assert!(!path.exists());
}

#[test]
fn test07_the_wrong_passphrase_changes_nothing() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    push(&a);
    engine::setup(
        &b.ctx(),
        &SetupOptions {
            folder: Some(shared),
            passphrase: Some("another long passphrase".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let before = b.snapshot();
    let error = engine::pull(&b.ctx(), &PullOptions::default(), &SystemGit).unwrap_err();
    assert!(error.message.contains("does not open"));
    assert_untouched(&b, &before);
}

#[test]
fn test08_upstream_sync_keeps_the_servers_part_out_with_a_reason() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &[PartId::Servers, PartId::Settings]);
    a.host.sync.upstream_active.set(true);
    let data = push(&a);
    assert_eq!(data["skipped"][0]["part"], "servers");
    assert!(data["skipped"][0]["reason"]
        .as_str()
        .unwrap()
        .contains("upstream Sync"));
    let status = engine::status(&a.ctx()).unwrap();
    let servers = status["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "servers")
        .unwrap();
    assert_eq!(servers["handledBy"], "upstream");
    assert_eq!(servers["on"], false);
    join_folder(&b, &shared, &[PartId::Servers, PartId::Settings]);
    let plan = pull(&b, &[]);
    assert!(plan["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["part"] != "servers"));
}

#[test]
fn test09_an_unattended_pull_only_plans_and_skips_while_the_keyring_is_locked() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    push(&a);
    join_folder(&b, &shared, &SERVERS_ONLY);
    let unattended = PullOptions {
        unattended: true,
        ..Default::default()
    };
    b.host.sync.keyring_locked.set(true);
    let before = b.snapshot();
    let skipped = engine::pull(&b.ctx(), &unattended, &SystemGit).unwrap();
    assert_eq!(skipped["skipped"], true);
    assert!(skipped["reason"].as_str().unwrap().contains("locked"));
    assert_untouched(&b, &before);
    b.host.sync.keyring_locked.set(false);
    let plan = engine::pull(&b.ctx(), &unattended, &SystemGit).unwrap();
    assert_eq!(plan["dryRun"], true);
    let state = config::read_state(&b.ctx()).unwrap();
    assert_eq!(state.last_check.as_ref().unwrap()["changes"], 1);
    let refused = engine::pull(
        &b.ctx(),
        &PullOptions {
            unattended: true,
            apply: true,
            plan: Some("x".into()),
            ..Default::default()
        },
        &SystemGit,
    )
    .unwrap_err();
    assert!(refused.message.contains("only plans"));
    assert!(!b
        .host
        .registry
        .servers
        .borrow()
        .iter()
        .any(|s| s.name == "beta"));
}

#[test]
fn test10_the_passphrase_lives_in_the_keychain_and_in_no_file() {
    let (a, _b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    push(&a);
    assert_eq!(a.host.sync.passphrase.borrow().as_deref(), Some(PASS));
    for (name, bytes) in a.snapshot() {
        let Some(bytes) = bytes else { continue };
        assert!(
            !bytes.windows(PASS.len()).any(|w| w == PASS.as_bytes()),
            "the passphrase is in {name}"
        );
    }
    let status = engine::status(&a.ctx()).unwrap();
    assert_eq!(status["passphrase"], "stored");
    assert!(!status.to_string().contains(PASS));
}

#[test]
fn test11_setup_refuses_what_cannot_sync_and_a_short_passphrase() {
    let a = World::new();
    let shared = folder(&a);
    let error = engine::setup(
        &a.ctx(),
        &SetupOptions {
            folder: Some(shared.clone()),
            parts: Some(BTreeSet::from([PartId::Library])),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.message.contains("does not sync"));
    let error = engine::setup(
        &a.ctx(),
        &SetupOptions {
            folder: Some(shared),
            passphrase: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.message.contains("at least"));
    assert!(config::read_config(&a.ctx()).unwrap().is_none());
}

#[test]
fn test12_the_hosted_service_is_never_a_transport() {
    let a = World::new();
    for url in [
        "https://teams.toolport.app/sync.git",
        "git@toolport.app:me/sync.git",
    ] {
        let error = engine::setup(
            &a.ctx(),
            &SetupOptions {
                git_url: Some(url.into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.message.contains("egress to toolport.app"), "{url}");
    }
    let git = MockGit::default();
    let cfg = config::Config::new(config::TransportCfg::Git {
        url: "https://teams.toolport.app/x.git".into(),
        branch: "main".into(),
    });
    config::write_config(&a.ctx(), &cfg).unwrap();
    toolport_plus_api::SetupHost::sync_store_passphrase(&a.host, PASS).unwrap();
    let error = engine::pull(&a.ctx(), &PullOptions::default(), &git).unwrap_err();
    assert!(error.message.contains("egress to toolport.app"));
    assert!(git.calls.borrow().is_empty());
}

#[test]
fn test13_git_is_a_transport_with_history_and_never_forces() {
    let (a, b, _) = two_machines();
    let bare = a.bare("sync");
    let url = format!("file://{}", bare.display());
    let setup = |w: &World| {
        engine::setup(
            &w.ctx(),
            &SetupOptions {
                git_url: Some(url.clone()),
                passphrase: Some(PASS.into()),
                parts: Some(BTreeSet::from([PartId::Servers])),
                ..Default::default()
            },
        )
        .unwrap();
    };
    setup(&a);
    setup(&b);
    let first = push(&a);
    assert_eq!(first["pushed"], true);
    let log = a.git(&bare, &["log", "--format=%s", "main"]);
    assert_eq!(log.trim(), "toolport sync");
    let tracked = a.git(&bare, &["ls-tree", "--name-only", "main"]);
    assert_eq!(tracked.trim(), FILE_NAME);
    let plan = pull(&b, &[]);
    assert_eq!(row(&plan, "server:beta")["state"], "add");
    apply(&b, &[]);
    assert_eq!(push(&b)["pushed"], true);
    let error = engine::push(&a.ctx(), &PushOptions::default(), &SystemGit).unwrap_err();
    assert!(error.message.contains("pull first"));
    let log = a.git(&bare, &["log", "--format=%s", "main"]);
    assert_eq!(log.lines().count(), 2);
}

fn set_command(world: &World, name: &str, command: &str) {
    let mut servers = world.host.registry.servers.borrow_mut();
    let server = servers.iter_mut().find(|s| s.name == name).unwrap();
    server.extra.insert("command".into(), json!(command));
}

fn command_of(world: &World, name: &str) -> String {
    let servers = world.host.registry.servers.borrow();
    let server = servers.iter().find(|s| s.name == name).unwrap();
    server.text("command").unwrap().to_string()
}

#[test]
fn test14_a_server_changed_on_the_other_machine_alone_is_replaced_held_and_undone() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    join_folder(&b, &shared, &SERVERS_ONLY);
    push(&a);
    apply(&b, &[]);
    set_command(&a, "alpha", "alpha-mcp-2");
    push(&a);
    let plan = pull(&b, &[]);
    assert_eq!(row(&plan, "server:alpha")["state"], "incoming");
    assert_eq!(command_of(&b, "alpha"), "alpha-mcp");
    let done = apply(&b, &[]);
    assert_eq!(done["failed"], 0);
    assert_eq!(command_of(&b, "alpha"), "alpha-mcp-2");
    let held = b.host.registry.servers.borrow()[0].enabled;
    assert!(!held, "a changed command is held until it is reviewed");
    assert_eq!(row(&pull(&b, &[]), "server:alpha")["state"], "here");
    engine::undo(&b.ctx(), false).unwrap();
    assert_eq!(command_of(&b, "alpha"), "alpha-mcp");
    assert!(b.host.registry.servers.borrow()[0].enabled);
    assert_eq!(row(&pull(&b, &[]), "server:alpha")["state"], "incoming");
}

#[test]
fn test15_a_server_changed_on_both_machines_stays_until_it_is_taken_from_the_other() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    join_folder(&b, &shared, &SERVERS_ONLY);
    push(&a);
    apply(&b, &[]);
    set_command(&a, "alpha", "alpha-mcp-a");
    set_command(&b, "alpha", "alpha-mcp-b");
    push(&a);
    assert_eq!(row(&pull(&b, &[]), "server:alpha")["state"], "conflict");
    apply(&b, &[]);
    assert_eq!(command_of(&b, "alpha"), "alpha-mcp-b");
    assert_eq!(
        config::read_state(&b.ctx()).unwrap().pending,
        vec!["server:alpha".to_string()]
    );
    apply(&b, &["server:alpha"]);
    assert_eq!(command_of(&b, "alpha"), "alpha-mcp-a");
    assert!(config::read_state(&b.ctx()).unwrap().pending.is_empty());
    engine::undo(&b.ctx(), false).unwrap();
    assert_eq!(command_of(&b, "alpha"), "alpha-mcp-b");
}

#[test]
fn test16_keeping_this_machine_settles_a_conflict_row_and_lets_the_push_through() {
    let (a, b, shared) = two_machines();
    join_folder(&a, &shared, &SERVERS_ONLY);
    join_folder(&b, &shared, &SERVERS_ONLY);
    push(&a);
    apply(&b, &[]);
    set_command(&a, "alpha", "alpha-mcp-a");
    set_command(&b, "alpha", "alpha-mcp-b");
    push(&a);
    let plan = pull(&b, &[]);
    engine::pull(
        &b.ctx(),
        &PullOptions {
            keep: ["server:alpha".to_string()].into(),
            apply: true,
            plan: Some(plan["plan"].as_str().unwrap().to_string()),
            ..Default::default()
        },
        &SystemGit,
    )
    .unwrap();
    assert!(config::read_state(&b.ctx()).unwrap().pending.is_empty());
    assert_eq!(command_of(&b, "alpha"), "alpha-mcp-b");
    assert_eq!(row(&pull(&b, &[]), "server:alpha")["state"], "mine");
    assert_eq!(push(&b)["pushed"], true);
}

#[test]
fn test17_a_sync_undo_that_could_not_finish_keeps_the_run_to_try_again() {
    let (a, b, shared) = two_machines();
    a.claude("CLAUDE.md", "v1\n");
    b.claude("CLAUDE.md", "v1\n");
    join_folder(&a, &shared, &WITH_FILES);
    join_folder(&b, &shared, &WITH_FILES);
    push(&a);
    apply(&b, &[]);
    a.claude("CLAUDE.md", "v2\n");
    push(&a);
    apply(&b, &[]);
    let path = b.home.join(".claude/CLAUDE.md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2\n");
    let run = config::read_state(&b.ctx())
        .unwrap()
        .last_apply
        .unwrap()
        .run;
    let backup = b.data.join("plus/setup/runs").join(run).join("backup/0");
    let saved = std::fs::read(&backup).unwrap();
    std::fs::remove_file(&backup).unwrap();

    let stuck = engine::undo(&b.ctx(), false).unwrap();
    assert_eq!(stuck["undone"], false, "{stuck}");
    assert!(config::read_state(&b.ctx()).unwrap().last_apply.is_some());

    std::fs::write(&backup, saved).unwrap();
    let done = engine::undo(&b.ctx(), false).unwrap();
    assert_eq!(done["undone"], true, "{done}");
    assert!(config::read_state(&b.ctx()).unwrap().last_apply.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "v1\n");
}
