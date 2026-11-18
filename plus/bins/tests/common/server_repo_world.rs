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

/// Another machine, `laptop`, published that it is on a pushed branch `feature` of the checkout,
/// which the work clone has fetched.
pub fn server_repo_peers(world: &World) {
    let (other, work) = (
        world.base.join("gamma-other"),
        world.base.join("gamma-work"),
    );
    let remote = world.base.join("gamma-remote.git");
    git(&other, &world.home, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(other.join("feature.js"), "feature").unwrap();
    git(&other, &world.home, &["add", "."]);
    git(
        &other,
        &world.home,
        &["commit", "-q", "-m", "Work on the feature"],
    );
    git(&other, &world.home, &["push", "-q", "origin", "feature"]);
    let head = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&other)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    git(&work, &world.home, &["fetch", "-q", "origin"]);
    let peers = world.data.join("plus").join("sync").join("peers.json");
    std::fs::create_dir_all(peers.parent().unwrap()).unwrap();
    write_json(
        &peers,
        &json!({
            "revision": "r1",
            "seenAt": 1_700_000_000,
            "machines": [{
                "machine": "laptop",
                "madeAt": "2026-11-18T10:00:00Z",
                "heads": [{
                    "server": "gamma",
                    "remote": remote.to_string_lossy(),
                    "branch": "feature",
                    "head": head,
                    "pushed": true,
                }],
            }],
        }),
    );
}

pub fn server_repo_with_peers(world: &World) {
    server_repo(world);
    server_repo_peers(world);
}
