#![allow(dead_code)]

//! The skills library fixtures of MIG-SRC-3: a bare remote, the library clone the CLI finds through
//! `skills_sync.json`, a second machine that pushes to the same remote and a sibling clone of it.
//! Each function below reshapes the world one step (a hook between two CLI calls); nothing here
//! touches the network or a real credential.

use std::path::PathBuf;

use crate::ctl_fixtures::git;
use crate::ctl_world::{write_json, CtlWorld};

/// A secret-shaped value that is not a credential; spelled in pieces so no scanner reads it here.
pub const SECRET: &str = concat!("gh", "p_", "CANARY0123456789CANARY0123456789CANARY0123456789");

pub fn remote(world: &CtlWorld) -> PathBuf {
    world.base.join("lib-remote.git")
}

pub fn library(world: &CtlWorld) -> PathBuf {
    world.home.join("lib/ai-skills")
}

pub fn copy(world: &CtlWorld) -> PathBuf {
    world.home.join("lib/ai-skills-copy")
}

fn other(world: &CtlWorld) -> PathBuf {
    world.base.join("lib-other")
}

fn skill(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\nBody of {name}.\n")
}

fn put(path: PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn identity(world: &CtlWorld, dir: &PathBuf) {
    git(dir, &world.home, &["config", "user.name", "fixture"]);
    git(dir, &world.home, &["config", "user.email", "fixture@example.invalid"]);
}

fn commit_all(world: &CtlWorld, dir: &PathBuf, message: &str) {
    git(dir, &world.home, &["add", "-A"]);
    git(dir, &world.home, &["commit", "-q", "-m", message]);
}

fn add_skill(world: &CtlWorld, repo: &PathBuf, name: &str) {
    put(
        repo.join("skills").join(name).join("SKILL.md"),
        &skill(name, &format!("The {name} skill")),
    );
    commit_all(world, repo, &format!("Add {name}"));
}

/// A bare remote with two skills, the library cloned from it (level with it, clean) and
/// `skills_sync.json` pointing at the clone.
pub fn library_home(world: &CtlWorld) {
    let bare = remote(world);
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &world.home, &["init", "-q", "--bare", "-b", "main"]);
    let seed = other(world);
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &world.home, &["init", "-q", "-b", "main"]);
    identity(world, &seed);
    add_skill(world, &seed, "review");
    add_skill(world, &seed, "odoo-upgrade");
    git(
        &seed,
        &world.home,
        &["remote", "add", "origin", &world.path(&bare)],
    );
    git(&seed, &world.home, &["push", "-q", "-u", "origin", "main"]);

    let lib = library(world);
    std::fs::create_dir_all(lib.parent().unwrap()).unwrap();
    git(
        &world.home,
        &world.home,
        &["clone", "-q", &world.path(&bare), &world.path(&lib)],
    );
    identity(world, &lib);
    write_json(
        &world.data.join("skills_sync.json"),
        &serde_json::json!({"local_path": world.path(&lib)}),
    );
}

/// One local commit that is not on the remote.
pub fn ahead_one(world: &CtlWorld) {
    add_skill(world, &library(world), "new-local");
}

/// A second clone of the same remote next to the library.
pub fn add_copy(world: &CtlWorld) {
    let target = copy(world);
    git(
        &world.home,
        &world.home,
        &["clone", "-q", &world.path(&remote(world)), &world.path(&target)],
    );
    identity(world, &target);
}

/// The library level with the remote, then two commits from the other machine that it has
/// fetched but not merged; the sibling clone is gone.
pub fn behind_two(world: &CtlWorld) {
    let lib = library(world);
    git(&lib, &world.home, &["reset", "-q", "--hard", "origin/main"]);
    let _ = std::fs::remove_dir_all(copy(world));
    let seed = other(world);
    add_skill(world, &seed, "notes-one");
    add_skill(world, &seed, "notes-two");
    git(&seed, &world.home, &["push", "-q", "origin", "main"]);
    git(&lib, &world.home, &["fetch", "-q"]);
}

/// A changed tracked file and an untracked one.
pub fn dirty(world: &CtlWorld) {
    let lib = library(world);
    let tracked = lib.join("skills/review/SKILL.md");
    let mut text = std::fs::read_to_string(&tracked).unwrap();
    text.push_str("An unsaved edit.\n");
    std::fs::write(tracked, text).unwrap();
    put(
        lib.join("skills/draft/SKILL.md"),
        &skill("draft", "Not committed"),
    );
}

/// Back to the last commit: no changed and no untracked files.
pub fn clean(world: &CtlWorld) {
    let lib = library(world);
    git(&lib, &world.home, &["checkout", "-q", "--", "."]);
    git(&lib, &world.home, &["clean", "-fdq"]);
}

pub fn no_remote(world: &CtlWorld) {
    clean(world);
    git(&library(world), &world.home, &["remote", "remove", "origin"]);
}

/// A local commit that adds a secret-shaped value.
pub fn secret_commit(world: &CtlWorld) {
    let lib = library(world);
    put(
        lib.join("skills/leaky/SKILL.md"),
        &format!(
            "---\nname: leaky\ndescription: Holds a pasted token\n---\ntoken = {SECRET}\n"
        ),
    );
    commit_all(world, &lib, "Add leaky");
}

pub fn drop_last_commit(world: &CtlWorld) {
    git(
        &library(world),
        &world.home,
        &["reset", "-q", "--hard", "HEAD~1"],
    );
}
