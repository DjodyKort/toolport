#![allow(dead_code)]

//! The skills library fixtures of MIG-SRC-3: a bare remote, the library clone the CLI finds through
//! `skills_sync.json`, a second machine that pushes to the same remote and a sibling clone of it.
//! Each function below reshapes the world one step (a hook between two CLI calls); nothing here
//! touches the network or a real credential.

use std::path::PathBuf;

use super::world::{write_json, World};
use std::process::{Command, Stdio};

/// A secret-shaped value that is not a credential; spelled in pieces so no scanner reads it here.
pub const SECRET: &str = concat!(
    "gh",
    "p_",
    "CANARY0123456789CANARY0123456789CANARY0123456789"
);

pub fn remote(world: &World) -> PathBuf {
    world.base.join("lib-remote.git")
}

pub fn library(world: &World) -> PathBuf {
    world.home.join("lib/ai-skills")
}

pub fn copy(world: &World) -> PathBuf {
    world.home.join("lib/ai-skills-copy")
}

fn other(world: &World) -> PathBuf {
    world.base.join("lib-other")
}

pub fn git(dir: &PathBuf, home: &PathBuf, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn skill(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\nBody of {name}.\n")
}

fn put(path: PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn identity(world: &World, dir: &PathBuf) {
    git(dir, &world.home, &["config", "user.name", "fixture"]);
    git(
        dir,
        &world.home,
        &["config", "user.email", "fixture@example.invalid"],
    );
}

fn commit_all(world: &World, dir: &PathBuf, message: &str) {
    git(dir, &world.home, &["add", "-A"]);
    git(dir, &world.home, &["commit", "-q", "-m", message]);
}

fn add_skill(world: &World, repo: &PathBuf, name: &str) {
    put(
        repo.join("skills").join(name).join("SKILL.md"),
        &skill(name, &format!("The {name} skill")),
    );
    commit_all(world, repo, &format!("Add {name}"));
}

/// A bare remote with two skills, the library cloned from it (level with it, clean) and
/// `skills_sync.json` pointing at the clone.
pub fn library_home(world: &World) {
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

/// The bare remote with its two skills and no clone of it: nothing is connected yet, and the data
/// dir holds no `skills_sync.json`.
pub fn remote_only(world: &World) {
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
}

/// An empty bare repository as the library's only remote, for a first push.
pub fn empty_remote(world: &World) {
    let bare = world.base.join("lib-empty.git");
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &world.home, &["init", "-q", "--bare", "-b", "main"]);
    git(
        &library(world),
        &world.home,
        &["remote", "add", "origin", &world.path(&bare)],
    );
}

/// A changed file that holds a secret-shaped value and is not committed.
pub fn secret_dirty(world: &World) {
    put(
        library(world).join("skills/leaky/SKILL.md"),
        &format!("---\nname: leaky\ndescription: Holds a pasted token\n---\ntoken = {SECRET}\n"),
    );
}

/// One local commit that is not on the remote.
pub fn ahead_one(world: &World) {
    add_skill(world, &library(world), "new-local");
}

/// A second clone of the same remote next to the library.
pub fn add_copy(world: &World) {
    let target = copy(world);
    git(
        &world.home,
        &world.home,
        &[
            "clone",
            "-q",
            &world.path(&remote(world)),
            &world.path(&target),
        ],
    );
    identity(world, &target);
}

/// The library level with the remote, then two commits from the other machine that it has
/// fetched but not merged; the sibling clone is gone.
pub fn behind_two(world: &World) {
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
pub fn dirty(world: &World) {
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
pub fn clean(world: &World) {
    let lib = library(world);
    git(&lib, &world.home, &["checkout", "-q", "--", "."]);
    git(&lib, &world.home, &["clean", "-fdq"]);
}

pub fn no_remote(world: &World) {
    clean(world);
    git(
        &library(world),
        &world.home,
        &["remote", "remove", "origin"],
    );
}

/// A local commit that adds a secret-shaped value.
pub fn secret_commit(world: &World) {
    let lib = library(world);
    put(
        lib.join("skills/leaky/SKILL.md"),
        &format!("---\nname: leaky\ndescription: Holds a pasted token\n---\ntoken = {SECRET}\n"),
    );
    commit_all(world, &lib, "Add leaky");
}

pub fn drop_last_commit(world: &World) {
    git(
        &library(world),
        &world.home,
        &["reset", "-q", "--hard", "HEAD~1"],
    );
}

fn git_identity(dir: &PathBuf, home: &PathBuf) {
    git(dir, home, &["config", "user.name", "fixture"]);
    git(
        dir,
        home,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(dir, home, &["config", "commit.gpgsign", "false"]);
}

/// The skills repository of the world has a bare remote and one pushed commit, then an
/// uncommitted file.
pub fn skills_repo_remote_world(world: &World) {
    let remote = world.base.join("skills-remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &world.home, &["init", "-q", "--bare"]);
    git(
        &remote,
        &world.home,
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    let repo = &world.repo;
    git(repo, &world.home, &["init", "-q"]);
    git(
        repo,
        &world.home,
        &["symbolic-ref", "HEAD", "refs/heads/main"],
    );
    git_identity(repo, &world.home);
    git(
        repo,
        &world.home,
        &["remote", "add", "origin", &world.path(&remote)],
    );
    git(repo, &world.home, &["add", "-A"]);
    git(repo, &world.home, &["commit", "-q", "-m", "fixture"]);
    git(repo, &world.home, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(repo.join("skills/demo/extra.md"), "not committed yet\n").unwrap();
}
