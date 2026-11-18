#![allow(dead_code)]

//! Local git repositories a tap can be added from, so no step reaches a network: `{base}/tap-src`
//! holds one skill, `{base}/plugin-src` is a plugin marketplace with one plugin.

use std::path::Path;

use super::library_world::git;
use super::world::World;

fn put(root: &Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn commit(dir: &Path, world: &World) {
    let (dir, home) = (dir.to_path_buf(), world.home.clone());
    git(&dir, &home, &["init", "-q"]);
    git(&dir, &home, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&dir, &home, &["add", "-A"]);
    git(&dir, &home, &["commit", "-q", "-m", "fixture"]);
}

pub fn tap_src(world: &World) {
    let dir = world.base.join("tap-src");
    put(
        &dir,
        &[(
            "skills/tapskill/SKILL.md",
            "---\nname: tapskill\ndescription: A synthetic tap skill\n---\nTap body\n",
        )],
    );
    commit(&dir, world);
}

pub fn plugin_src(world: &World) {
    let dir = world.base.join("plugin-src");
    put(
        &dir,
        &[
            (
                ".claude-plugin/marketplace.json",
                r#"{"name": "acme", "plugins": [{"name": "core", "source": "./plugins/core"}]}"#,
            ),
            (
                "plugins/core/.claude-plugin/plugin.json",
                r#"{"name": "core", "version": "2026.10.14.0"}"#,
            ),
            (
                "plugins/core/skills/task/SKILL.md",
                "---\nname: task\ndescription: A synthetic plugin skill\n---\nPlugin body\n",
            ),
        ],
    );
    commit(&dir, world);
}

/// Both repositories, for the cases that add a plain tap and a marketplace tap.
pub fn both(world: &World) {
    tap_src(world);
    plugin_src(world);
}
