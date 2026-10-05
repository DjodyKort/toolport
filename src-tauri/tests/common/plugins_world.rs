#![allow(dead_code)]

//! The plugin and hook fixture of contract section 14: a synthetic plugin `ecc@ecc`, a user
//! settings file with 27 hooks, a skills lock and three project folders, plus a stub `claude`
//! that answers with output recorded from the real `claude plugin list|details|configure` over
//! the same tree (`tests/fixtures/plugins/claude-recorded`, paths replaced by `@CLAUDE_HOME@`).
//! Nothing in it is a real credential, plugin or hook.
//!
//! Layout under `<base>`: `home/.claude` (the user's Claude home), `data/` (the Toolport data
//! directory, with a cached `context measure` result for `acme-erp`) and
//! `home/work/{acme-erp,side-project,quiet}` (project folders).

use std::path::{Path, PathBuf};

pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plugins");

pub struct PluginsWorld {
    pub claude_home: PathBuf,
    pub plugin: PathBuf,
    pub plain: PathBuf,
    pub side: PathBuf,
    pub quiet: PathBuf,
    pub recorded: PathBuf,
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn fixture(rel: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(rel)).unwrap()
}

pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Builds the world below `base`; the stub `claude` is written to `claude`.
pub fn build_in(base: &Path, claude: &Path) -> PluginsWorld {
    let home = base.join("home");
    let claude_home = home.join(".claude");
    let home_text = home.to_string_lossy().into_owned();
    let claude_text = claude_home.to_string_lossy().into_owned();
    let plugin = claude_home.join("plugins/cache/ecc/ecc/2.2.0");
    copy_tree(&Path::new(FIXTURES).join("ecc"), &plugin);

    let market = claude_home.join("plugins/marketplaces/ecc");
    put(
        &market.join(".claude-plugin/marketplace.json"),
        &serde_json::json!({
            "name": "ecc",
            "owner": {"name": "Synthetic"},
            "plugins": [{"name": "ecc", "source": "./", "version": "2.2.0"}]
        })
        .to_string(),
    );
    put(
        &claude_home.join("plugins/known_marketplaces.json"),
        &serde_json::json!({"ecc": {
            "source": {"source": "github", "repo": "acme/ecc-plugins"},
            "installLocation": market,
            "lastUpdated": "2026-08-31T10:00:00.000Z"
        }})
        .to_string(),
    );
    put(
        &claude_home.join("plugins/installed_plugins.json"),
        &serde_json::json!({"version": 2, "plugins": {"ecc@ecc": [{
            "scope": "user",
            "installPath": plugin,
            "version": "2.2.0",
            "installedAt": "2026-08-31T10:00:00.000Z",
            "lastUpdated": "2026-08-31T10:00:00.000Z"
        }]}})
        .to_string(),
    );
    put(
        &claude_home.join("settings.json"),
        &fixture("home/settings.json").replace("@HOME@", &home_text),
    );
    put(
        &base.join("data/mcpm-skills.lock"),
        &fixture("home/skills.lock").replace("@HOME@", &home_text),
    );

    let project = |name: &str, git: bool| {
        let dir = home.join("work").join(name);
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        if git {
            std::fs::create_dir_all(dir.join(".git")).unwrap();
        }
        dir
    };
    let plain = project("acme-erp", true);
    put(
        &plain.join(".claude/settings.json"),
        r#"{"permissions": {"allow": ["Bash(git status)"]}}"#,
    );
    let side = project("side-project", true);
    put(
        &side.join(".claude/settings.local.json"),
        &serde_json::json!({
            "enabledPlugins": {"ecc@ecc": false},
            "deniedMcpServers": [{"serverName": "plugin:ecc:chrome-devtools"}],
        })
        .to_string(),
    );
    put(
        &side.join(".claude/settings.json"),
        &serde_json::json!({"hooks": {"Stop": [{"hooks": [{
            "type": "command", "command": "./scripts/stop-check.sh", "timeout": 20
        }]}]}})
        .to_string(),
    );
    let quiet = project("quiet", true);
    put(
        &quiet.join(".claude/settings.local.json"),
        r#"{"disableAllHooks": true}"#,
    );

    put(
        &base.join("data/plus/cache/measure/acme-erp.json"),
        &serde_json::json!({
            "cwd": plain,
            "measuredAt": "2026-09-01T10:00:00Z",
            "deltas": [
                {"label": "without plugin:ecc@ecc", "tokens": -1240},
                {"label": "without plugin:other@elsewhere", "tokens": -90}
            ]
        })
        .to_string(),
    );

    let recorded = base.join("claude-recorded");
    for name in ["list.json", "details-ecc.txt", "configure-ecc.json"] {
        put(
            &recorded.join(name),
            &fixture(&format!("claude-recorded/{name}")).replace("@CLAUDE_HOME@", &claude_text),
        );
    }
    super::exec::write_executable(
        claude,
        &format!(
            "#!/bin/sh\nd=\"{}\"\ncase \"$1 $2 $3 $4\" in\n\
             \"plugin list --json \"*) cat \"$d/list.json\";;\n\
             \"plugin details ecc@ecc \"*) cat \"$d/details-ecc.txt\";;\n\
             \"plugin configure ecc@ecc --json\") cat \"$d/configure-ecc.json\";;\n\
             \"plugin configure ecc@ecc --values-stdin\") cat > \"$d/values-stdin.json\"; echo \"$*\" > \"$d/values-argv.txt\"; echo configured;;\n\
             \"plugin marketplace update \"*) echo refreshed;;\n\
             *) echo \"unexpected: $*\" >&2; exit 9;;\nesac\n",
            recorded.display()
        ),
    );
    PluginsWorld {
        claude_home,
        plugin,
        plain,
        side,
        quiet,
        recorded,
    }
}
