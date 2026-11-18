//! The `serverRepos` part: each machine writes its own record, a push keeps the others', and a
//! pull reads them into the cache the attention rows and `sync repos` compare against.

use std::path::Path;

use super::config::FILE_NAME;
use super::engine::{self, PullOptions, PushOptions, SetupOptions};
use super::peers::{self, Peers};
use crate::setup::export::TEST_MACHINE;
use crate::setup::tests::{World, PASS};
use crate::setup::{inspect, PartId};
use crate::skills::git::SystemGit;

fn named(name: &str) {
    TEST_MACHINE.with(|m| *m.borrow_mut() = Some(name.to_string()));
}

fn join(world: &World, dir: &Path, parts: &[PartId]) {
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

fn push(world: &World) {
    engine::push(&world.ctx(), &PushOptions::default(), &SystemGit).unwrap();
}

fn pull(world: &World) {
    let plan = engine::pull(&world.ctx(), &PullOptions::default(), &SystemGit).unwrap();
    engine::pull(
        &world.ctx(),
        &PullOptions {
            apply: true,
            plan: Some(plan["plan"].as_str().unwrap().to_string()),
            ..Default::default()
        },
        &SystemGit,
    )
    .unwrap();
}

fn published_machines(shared: &Path, own: &str) -> Vec<String> {
    let plain = shared.join("opened.zip");
    let mut input = std::fs::File::open(shared.join(FILE_NAME)).unwrap();
    let mut out = std::fs::File::create(&plain).unwrap();
    crate::setup::crypto::open(PASS, &mut input, &mut out).unwrap();
    let mut opened = inspect::open(&plain).unwrap();
    let others: Vec<String> = peers::records_of(&mut opened, own)
        .into_iter()
        .map(|(machine, _)| machine)
        .collect();
    std::fs::remove_file(&plain).unwrap();
    others
}

#[test]
fn test01_server_repos_is_on_by_default_and_listed_as_a_sync_part() {
    let world = World::new();
    let shared = world.dir.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    engine::setup(
        &world.ctx(),
        &SetupOptions {
            folder: Some(shared),
            passphrase: Some(PASS.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let status = engine::status(&world.ctx()).unwrap();
    let part = status["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "serverRepos")
        .unwrap();
    assert_eq!(part["on"], true);
    assert_eq!(part["default"], true);
}

#[test]
fn test02_each_machine_writes_its_own_record_and_a_push_keeps_the_others() {
    let (laptop, desktop) = (World::new(), World::new());
    let shared = laptop.dir.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    for world in [&laptop, &desktop] {
        join(world, &shared, &[PartId::ServerRepos]);
    }
    named("laptop");
    push(&laptop);
    assert!(published_machines(&shared, "desktop").contains(&"laptop".to_string()));
    named("desktop");
    pull(&desktop);
    push(&desktop);
    let mut seen = published_machines(&shared, "x");
    seen.sort();
    assert_eq!(seen, ["desktop", "laptop"]);
    named("laptop");
    pull(&laptop);
    push(&laptop);
    let mut seen = published_machines(&shared, "x");
    seen.sort();
    assert_eq!(seen, ["desktop", "laptop"]);
}

#[test]
fn test03_a_pull_keeps_the_other_machines_records_for_the_attention_rows() {
    let (laptop, desktop) = (World::new(), World::new());
    let shared = laptop.dir.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    for world in [&laptop, &desktop] {
        join(world, &shared, &[PartId::ServerRepos]);
    }
    named("laptop");
    push(&laptop);
    named("desktop");
    pull(&desktop);
    let seen = Peers::read(&desktop.ctx().data_dir);
    assert_eq!(seen.machines.len(), 1);
    assert_eq!(seen.machines[0].machine, "laptop");
    assert!(seen.revision.is_some());
}

#[test]
fn test04_the_part_never_enters_an_export_or_the_all_list() {
    let world = World::new();
    let parts = crate::setup::parse_parts("all").unwrap();
    assert!(!parts.contains(&PartId::ServerRepos));
    assert_eq!(PartId::parse("server-repos"), Some(PartId::ServerRepos));
    let listed = crate::setup::export::list(&world.ctx(), &[]).unwrap();
    assert!(!listed["parts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "serverRepos"));
}
