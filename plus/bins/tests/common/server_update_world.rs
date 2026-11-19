#![allow(dead_code)]

//! A world with servers whose package the update check has already looked up: an npm pin the
//! check found a newer version of, one it found nothing for, a pin that comes from a launch recipe
//! and a GitHub release whose source names no file to take. Nothing here needs the network, so the
//! plan and the update run against what the check left on disk.

use serde_json::{json, Value};

use super::world::{read_json, write_json, World};

const CHECKED_AT: i64 = 1_792_000_000;

pub fn server_package(world: &World) {
    let registry = world.data.join("registry.json");
    let mut value = read_json(&registry);
    let servers = value["servers"].as_array_mut().unwrap();
    servers.push(json!({
        "id": "srv-files", "name": "files", "transport": "stdio", "command": "npx",
        "args": ["-y", "@scope/files@1.4.2", "/data"], "cwd": "/data/files",
    }));
    servers.push(json!({
        "id": "srv-bare", "name": "bare", "transport": "stdio", "command": "uvx",
        "args": ["bare-mcp==0.9.1"],
    }));
    servers.push(json!({
        "id": "srv-recipe", "name": "recipe", "transport": "stdio", "command": "npx",
        "args": ["-y", "@scope/recipe@2.0.0"],
        "launch": {"inputs": [], "bindings": [], "template": "recipe-template", "revision": 1},
    }));
    servers.push(json!({
        "id": "srv-golf", "name": "golf", "transport": "stdio", "command": "/opt/golf/golf",
        "args": [],
        "mcpmSource": {"type": "github-release", "repo": "acme/golf-mcp",
                       "path": "/opt/golf/golf", "current_version": "1.3.0"},
    }));
    write_json(&registry, &value);
    let source = |latest: Option<&str>| match latest {
        Some(latest) => json!({"at": CHECKED_AT, "latest": latest}),
        None => json!({"at": CHECKED_AT}),
    };
    write_json(
        &world.data.join("plus/attention.json"),
        &json!({"updates": {"checkedAt": CHECKED_AT, "sources": {
            "package:npm:@scope/files": source(Some("1.5.0")),
            "package:npm:@scope/recipe": source(Some("2.1.0")),
            "package:github:acme/golf-mcp": source(Some("v1.4.0")),
            "package:pypi:bare-mcp": source(None),
        }}}),
    );
}

/// The arguments of the `files` server as the registry holds them.
pub fn files_args(world: &World) -> Value {
    world.registry()["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "srv-files")
        .unwrap()["args"]
        .clone()
}

const DIGEST_OLD: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_NEW: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Two more servers with an update waiting, so a plan over several servers has something to
/// move: a uvx pin and an image pinned by digest that follows its tag.
pub fn server_package_more(world: &World) {
    let registry = world.data.join("registry.json");
    let mut value = read_json(&registry);
    let servers = value["servers"].as_array_mut().unwrap();
    servers.push(json!({
        "id": "srv-tools", "name": "tools", "transport": "stdio", "command": "uvx",
        "args": ["tools-mcp==0.9.1"],
    }));
    servers.push(json!({
        "id": "srv-img", "name": "img", "transport": "stdio", "command": "docker",
        "args": ["run", "-i", "--rm", format!("acme/files:latest@{DIGEST_OLD}")],
    }));
    write_json(&registry, &value);
    let store = world.data.join("plus/attention.json");
    let mut checked = read_json(&store);
    checked["updates"]["sources"]["package:pypi:tools-mcp"] =
        json!({"at": CHECKED_AT, "latest": "1.0.0"});
    checked["updates"]["sources"]["package:docker:acme/files:@latest"] =
        json!({"at": CHECKED_AT, "latest": DIGEST_NEW});
    write_json(&store, &checked);
}
