#![allow(dead_code)]

//! The fixture home of the `setup` contract: repositories under `~/code` with a bare remote each,
//! one that is ahead of it and dirty, one without a remote, a Claude Code folder with a CLAUDE.md,
//! settings that hold a secret-shaped key, auto memory and one session. Synthetic and offline.

use std::path::{Path, PathBuf};

use super::library_world::git;
use super::world::World;

pub const PASSPHRASE: &str = "correct horse battery\n";

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// How Claude Code spells a folder in the name of a directory it keeps per project.
fn slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn identity(world: &World, dir: &PathBuf) {
    git(dir, &world.home, &["config", "user.name", "fixture"]);
    git(
        dir,
        &world.home,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(dir, &world.home, &["config", "commit.gpgsign", "false"]);
}

fn pushed_repo(world: &World, name: &str) -> PathBuf {
    let bare = world.base.join("remotes").join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    git(&bare, &world.home, &["init", "-q", "--bare", "-b", "main"]);
    let repo = world.home.join("code").join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &world.home, &["init", "-q", "-b", "main"]);
    identity(world, &repo);
    put(&repo.join("README.md"), &format!("# {name}\n"));
    git(&repo, &world.home, &["add", "-A"]);
    git(&repo, &world.home, &["commit", "-q", "-m", "first"]);
    git(
        &repo,
        &world.home,
        &["remote", "add", "origin", &world.path(&bare)],
    );
    git(&repo, &world.home, &["push", "-q", "-u", "origin", "main"]);
    repo
}

/// `web` is two things ahead of its remote: a commit and an untracked file. `docs` is clean and
/// pushed. `scratch` has no remote. Claude Code keeps a memory folder and a session for `web`.
pub fn setup_home(world: &World) {
    let web = pushed_repo(world, "web");
    put(&web.join("a.txt"), "one\n");
    git(&web, &world.home, &["add", "-A"]);
    git(&web, &world.home, &["commit", "-q", "-m", "not pushed"]);
    put(&web.join("notes.txt"), "untracked\n");
    put(&web.join("CLAUDE.local.md"), "my local notes\n");
    pushed_repo(world, "docs");
    let scratch = world.home.join("code/scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    git(&scratch, &world.home, &["init", "-q", "-b", "main"]);
    identity(world, &scratch);
    put(&scratch.join("a.txt"), "x\n");
    git(&scratch, &world.home, &["add", "-A"]);
    git(&scratch, &world.home, &["commit", "-q", "-m", "only here"]);

    let claude = world.home.join(".claude");
    put(&claude.join("CLAUDE.md"), "global rules\n");
    put(&claude.join("rules/style.md"), "short sentences\n");
    put(
        &claude.join("settings.json"),
        "{\n  \"model\": \"opus\",\n  \"env\": {\"MY_VAR\": \"x\"},\n  \"apiKeyHelper\": \"echo\"\n}\n",
    );
    let project = claude.join("projects").join(slug(&web));
    put(
        &project.join("memory/MEMORY.md"),
        "- the user prefers short answers\n",
    );
    put(&project.join("abc.jsonl"), "{\"type\":\"user\"}\n");
    std::fs::create_dir_all(world.base.join("out")).unwrap();
}
