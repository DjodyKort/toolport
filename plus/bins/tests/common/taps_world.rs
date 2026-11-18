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

/// A tap with several kinds of items, for the selective loading cases: two skills, an agent, a
/// command, a rule and a hook that runs a script.
pub fn select_src(world: &World) {
    let dir = world.base.join("select-src");
    put(
        &dir,
        &[
            (
                "skills/code-review/SKILL.md",
                "---\nname: code-review\ndescription: A synthetic skill\n---\nReview a diff\n",
            ),
            (
                "skills/tdd-workflow/SKILL.md",
                "---\nname: tdd-workflow\ndescription: A synthetic skill\n---\nTest first\n",
            ),
            ("agents/reviewer.md", "---\nname: reviewer\n---\nReviews\n"),
            (
                "commands/deploy.md",
                "---\ndescription: Deploy\n---\nShip it\n",
            ),
            ("rules/style.md", "Keep functions short\n"),
            (
                "hooks/hooks.json",
                r#"{"hooks": {"PostToolUse": [{"matcher": "Write|Edit", "hooks": [{"type": "command", "command": "\"${CLAUDE_PLUGIN_ROOT}\"/scripts/format.sh"}]}]}}"#,
            ),
            ("scripts/format.sh", "#!/bin/sh\necho formatting\n"),
        ],
    );
    commit(&dir, world);
}

/// Both repositories, for the cases that add a plain tap and a marketplace tap.
pub fn both(world: &World) {
    tap_src(world);
    plugin_src(world);
}

/// A Library repository in the working folder of the test run, holding one rule of its own.
pub fn library(world: &World) {
    put(
        &world.home,
        &[(
            "rules/mine/SKILL.md",
            "---\nname: mine\ndescription: A synthetic rule\nactivation: always\n---\nMine\n",
        )],
    );
}

/// The Library of [`library`] plus a rule called `style` that Toolport did not write.
pub fn library_with_style(world: &World) {
    library(world);
    put(
        &world.home,
        &[(
            "rules/style/SKILL.md",
            "---\nname: style\ndescription: Not from the tap\nactivation: always\n---\nMine too\n",
        )],
    );
}
