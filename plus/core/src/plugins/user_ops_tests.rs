use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use super::claude::NOT_FOUND;
use super::inventory::{self, World};
use super::user_ops::{self, Args, Op};
use super::ClaudeRunner;
use crate::op::ErrorKind;
use crate::skills::gitexec::CmdOutput;

struct Tree {
    dir: tempfile::TempDir,
    home: PathBuf,
    claude: PathBuf,
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn tree(settings: Value) -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let claude = home.join(".claude");
    put(
        &claude.join("plugins/installed_plugins.json"),
        &json!({"version": 2, "plugins": {
            "kit@acme": [{"scope": "user", "version": "1.8.2",
                          "installPath": claude.join("plugins/cache/kit")}],
        }})
        .to_string(),
    );
    put(
        &claude.join("plugins/known_marketplaces.json"),
        &json!({"acme": {"source": {"source": "github", "repo": "acme/skills"},
                         "installLocation": claude.join("plugins/marketplaces/acme")}})
        .to_string(),
    );
    put(
        &claude.join("plugins/marketplaces/acme/.claude-plugin/marketplace.json"),
        &json!({"name": "acme", "plugins": [
            {"name": "kit", "source": {"source": "github", "repo": "acme/kit"}, "version": "1.9.0"}]})
        .to_string(),
    );
    put(&claude.join("settings.json"), &settings.to_string());
    Tree { dir, home, claude }
}

fn world(tree: &Tree) -> World {
    World {
        home: tree.home.clone(),
        claude_home: tree.claude.clone(),
        managed: None,
        data_dir: Some(tree.dir.path().join("data")),
        starts: Vec::new(),
    }
}

#[derive(Default)]
struct Claude {
    calls: Mutex<Vec<String>>,
    fail: Option<&'static str>,
    absent: bool,
}

impl ClaudeRunner for Claude {
    fn run(&self, args: &[&str], _: Duration) -> Result<CmdOutput, String> {
        if args.starts_with(&["plugin", "list"]) {
            return Ok(CmdOutput {
                code: 1,
                ..CmdOutput::default()
            });
        }
        self.calls.lock().unwrap().push(args.join(" "));
        match self.fail {
            Some(why) => Ok(CmdOutput {
                code: 1,
                stderr: why.into(),
                ..CmdOutput::default()
            }),
            None => Ok(CmdOutput::default()),
        }
    }

    fn present(&self) -> bool {
        !self.absent
    }
}

fn go(tree: &Tree, claude: &Claude, op: Op, dry_run: bool) -> Result<Value, crate::op::OpError> {
    user_ops::run(
        &world(tree),
        Some(claude),
        &Args {
            op,
            id: "kit@acme",
            dry_run,
        },
    )
}

fn on() -> Value {
    json!({"enabledPlugins": {"kit@acme": true}})
}

#[test]
fn test01_disable_runs_exactly_one_user_scope_command_after_a_plan_that_runs_nothing() {
    let tree = tree(on());
    let claude = Claude::default();
    let plan = go(&tree, &claude, Op::Disable, true).unwrap();
    assert!(claude.calls.lock().unwrap().is_empty());
    assert_eq!(plan["result"], Value::Null);
    assert_eq!(
        plan["plan"]["steps"][0]["detail"],
        "claude plugin disable kit@acme --scope user"
    );
    assert_eq!(plan["plan"]["undo"], "toolportctl plugins enable kit@acme");
    let done = go(&tree, &claude, Op::Disable, false).unwrap();
    assert_eq!(
        *claude.calls.lock().unwrap(),
        vec!["plugin disable kit@acme --scope user"]
    );
    assert_eq!(done["result"]["applied"], true);
    assert_eq!(done["restartRequired"], true);
    assert!(done["plan"]["warnings"].to_string().contains("restart"));
}

#[test]
fn test02_a_plugin_that_is_already_in_the_wanted_state_is_not_touched() {
    let tree = tree(json!({"enabledPlugins": {"kit@acme": false}}));
    let claude = Claude::default();
    let out = go(&tree, &claude, Op::Disable, false).unwrap();
    assert!(claude.calls.lock().unwrap().is_empty());
    assert_eq!(out["changes"][0]["action"], "none");
    assert_eq!(out["result"]["applied"], false);
    assert_eq!(out["restartRequired"], false);
    go(&tree, &claude, Op::Enable, false).unwrap();
    assert_eq!(
        *claude.calls.lock().unwrap(),
        vec!["plugin enable kit@acme --scope user"]
    );
}

#[test]
fn test03_a_missing_claude_is_a_conflict_also_in_a_dry_run() {
    let tree = tree(on());
    let claude = Claude {
        absent: true,
        ..Claude::default()
    };
    for dry_run in [true, false] {
        let e = go(&tree, &claude, Op::Disable, dry_run).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Conflict);
        assert!(e.message.starts_with(NOT_FOUND), "{}", e.message);
    }
    let none = user_ops::run(
        &world(&tree),
        None,
        &Args {
            op: Op::Update,
            id: "kit@acme",
            dry_run: true,
        },
    )
    .unwrap_err();
    assert_eq!(none.kind, ErrorKind::Conflict);
}

#[test]
fn test04_a_plugin_the_administrator_decides_cannot_be_switched() {
    let tree = tree(on());
    let managed = tree.dir.path().join("managed.json");
    put(
        &managed,
        &json!({"enabledPlugins": {"kit@acme": true}}).to_string(),
    );
    let mut world = world(&tree);
    world.managed = Some(managed);
    let claude = Claude::default();
    let e = user_ops::run(
        &world,
        Some(&claude),
        &Args {
            op: Op::Disable,
            id: "kit@acme",
            dry_run: false,
        },
    )
    .unwrap_err();
    assert!(e.message.contains("administrator"), "{}", e.message);
    assert!(claude.calls.lock().unwrap().is_empty());
}

#[test]
fn test05_an_unknown_plugin_is_not_found_and_a_failing_claude_names_why() {
    let tree = tree(on());
    let claude = Claude::default();
    let e = user_ops::run(
        &world(&tree),
        Some(&claude),
        &Args {
            op: Op::Enable,
            id: "ghost@x",
            dry_run: false,
        },
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::NotFound);
    let failing = Claude {
        fail: Some("plugin is not installed"),
        ..Claude::default()
    };
    let e = go(&tree, &failing, Op::Disable, false).unwrap_err();
    assert!(
        e.message.contains("plugin is not installed"),
        "{}",
        e.message
    );
}

#[test]
fn test06_update_names_the_versions_runs_the_installed_scope_and_says_there_is_no_undo() {
    let tree = tree(on());
    let claude = Claude::default();
    let plan = go(&tree, &claude, Op::Update, true).unwrap();
    assert_eq!(plan["changes"][0]["from"], "1.8.2");
    assert_eq!(plan["changes"][0]["to"], "1.9.0");
    assert!(plan["plan"]["undo"].as_str().unwrap().contains("1.8.2"));
    assert!(claude.calls.lock().unwrap().is_empty());
    go(&tree, &claude, Op::Update, false).unwrap();
    assert_eq!(
        *claude.calls.lock().unwrap(),
        vec!["plugin update kit@acme --scope user"]
    );
}

#[test]
fn test07_an_up_to_date_plugin_runs_no_update() {
    let tree = tree(on());
    put(
        &tree
            .claude
            .join("plugins/marketplaces/acme/.claude-plugin/marketplace.json"),
        &json!({"name": "acme", "plugins": [{"name": "kit", "source": "x", "version": "1.8.2"}]})
            .to_string(),
    );
    let claude = Claude::default();
    let out = go(&tree, &claude, Op::Update, false).unwrap();
    assert!(claude.calls.lock().unwrap().is_empty());
    assert_eq!(out["changes"][0]["action"], "none");
}

#[test]
fn test08_a_blocked_marketplace_makes_the_update_blocked_and_refuses_it() {
    let tree = tree(on());
    let managed = tree.dir.path().join("managed.json");
    put(
        &managed,
        &json!({"blockedMarketplaces": [{"source": "github", "repo": "acme/skills"}]}).to_string(),
    );
    let mut world = world(&tree);
    world.managed = Some(managed);
    let data = inventory::ls(&world, None, None, false);
    assert_eq!(data["plugins"][0]["update"]["state"], "blocked");
    assert_eq!(data["plugins"][0]["update"]["available"], "1.9.0");
    assert!(data["plugins"][0]["update"]["reason"]
        .as_str()
        .unwrap()
        .contains("blocks the acme"));
    assert_eq!(data["updatesAvailable"], 0);
    let claude = Claude::default();
    let e = user_ops::run(
        &world,
        Some(&claude),
        &Args {
            op: Op::Update,
            id: "kit@acme",
            dry_run: false,
        },
    )
    .unwrap_err();
    assert!(e.message.contains("cannot be updated"), "{}", e.message);
}

#[test]
fn test09_strict_known_marketplaces_block_what_they_do_not_list() {
    let tree = tree(on());
    let managed = tree.dir.path().join("managed.json");
    put(
        &managed,
        &json!({"strictKnownMarketplaces": [{"source": "github", "repo": "corp/tools"}]})
            .to_string(),
    );
    let mut world = world(&tree);
    world.managed = Some(managed.clone());
    assert_eq!(
        inventory::ls(&world, None, None, false)["plugins"][0]["update"]["state"],
        "blocked"
    );
    put(
        &managed,
        &json!({"strictKnownMarketplaces": [{"source": "github", "repo": "acme/skills"}]})
            .to_string(),
    );
    assert_eq!(
        inventory::ls(&world, None, None, false)["plugins"][0]["update"]["state"],
        "update"
    );
}

#[test]
fn test10_restart_is_required_only_while_a_session_older_than_a_plugin_file_runs() {
    let tree = tree(on());
    let mut world = world(&tree);
    assert_eq!(
        inventory::ls(&world, None, None, false)["restartRequired"],
        false
    );
    let changed = crate::context::fsx::mtime_secs(&tree.claude.join("settings.json")).unwrap();
    world.starts = vec![changed + 3600];
    assert_eq!(
        inventory::ls(&world, None, None, false)["restartRequired"],
        false
    );
    world.starts = vec![changed - 3600, changed + 3600];
    assert_eq!(
        inventory::ls(&world, None, None, false)["restartRequired"],
        true
    );
}
