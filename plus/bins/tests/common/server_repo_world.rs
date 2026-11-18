#![allow(dead_code)]

//! A world with a server that runs from a git checkout: a bare remote, a clone the server's
//! working directory points at, and one commit on the remote the checkout has not fetched yet.
//! Commits carry fixed dates, so their ids are the same on every run.

use serde_json::json;

use super::library_world::git;
use super::world::{read_json, write_json, World};

fn commit(world: &World, dir: &std::path::PathBuf, file: &str, text: &str, subject: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &world.home, &["add", "."]);
    git(dir, &world.home, &["commit", "-q", "-m", subject]);
    git(dir, &world.home, &["push", "-q", "origin", "HEAD:main"]);
}

pub fn server_repo(world: &World) {
    let (remote, other, work) = (
        world.base.join("gamma-remote.git"),
        world.base.join("gamma-other"),
        world.base.join("gamma-work"),
    );
    git(
        &world.base,
        &world.home,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            &remote.to_string_lossy(),
        ],
    );
    git(
        &world.base,
        &world.home,
        &[
            "clone",
            "-q",
            &remote.to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    commit(world, &other, "README.md", "one", "Start the server");
    git(
        &world.base,
        &world.home,
        &[
            "clone",
            "-q",
            &remote.to_string_lossy(),
            &work.to_string_lossy(),
        ],
    );
    commit(world, &other, "tools.js", "two", "Add the search tool");

    let registry = world.data.join("registry.json");
    let mut value = read_json(&registry);
    value["servers"].as_array_mut().unwrap().push(json!({
        "id": "srv-gamma", "name": "gamma", "transport": "stdio",
        "command": world.mock, "args": [], "cwd": work.to_string_lossy(),
    }));
    write_json(&registry, &value);
}

/// A second commit on the remote and a local change to a tracked file in the checkout.
pub fn server_repo_dirty(world: &World) {
    let other = world.base.join("gamma-other");
    let work = world.base.join("gamma-work");
    git(
        &other,
        &world.home,
        &["pull", "-q", "--ff-only", "origin", "main"],
    );
    commit(world, &other, "more.js", "three", "Add the fetch tool");
    git(&work, &world.home, &["fetch", "-q", "origin"]);
    std::fs::write(work.join("README.md"), "edited by hand").unwrap();
}
