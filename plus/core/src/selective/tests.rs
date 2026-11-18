use super::ops::{self, Env, Options, SetArgs};
use super::store::{self, Kind, Selections, State};
use super::{check, discover};
use crate::plugins::claude::ClaudeRunner;
use crate::plugins::Env as PluginEnv;
use crate::skills::git::GitRunner;
use crate::skills::gitexec::CmdOutput;
use crate::skills::taps::{save_taps, Tap};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tempfile::TempDir;
use toolport_plus_api::memory::MemoryHost;

const TAP: &str = "example-tap";
const FORMAT_HOOK: &str = "\"${CLAUDE_PLUGIN_ROOT}\"/scripts/format.sh";

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn skill(root: &Path, name: &str, body: &str) {
    put(
        &root.join(format!("skills/{name}/SKILL.md")),
        &format!("---\nname: {name}\ndescription: A skill\n---\n{body}\n"),
    );
}

fn hooks_json(command: &str) -> String {
    json!({"hooks": {
        "PostToolUse": [{"matcher": "Write|Edit", "hooks": [
            {"type": "command", "command": command}
        ]}],
        "PreToolUse": [{"matcher": "Bash", "hooks": [
            {"type": "command", "command": "echo checked"},
            {"type": "prompt", "prompt": "never run as a command"}
        ]}]
    }})
    .to_string()
}

fn tree(root: &Path) {
    skill(root, "code-review", "Review a diff.");
    skill(root, "tdd-workflow", "Test first.");
    put(
        &root.join("agents/reviewer.md"),
        "---\nname: reviewer\n---\nReviews.\n",
    );
    put(
        &root.join("commands/deploy.md"),
        "---\ndescription: Deploy\n---\nShip it.\n",
    );
    put(&root.join("rules/style.md"), "Keep functions short.\n");
    put(&root.join("hooks/hooks.json"), &hooks_json(FORMAT_HOOK));
    put(
        &root.join("scripts/format.sh"),
        "#!/bin/sh\necho formatting\n",
    );
}

struct FakeGit {
    upstream: PathBuf,
    sha: RefCell<String>,
    offline: RefCell<bool>,
}

impl GitRunner for FakeGit {
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), String> {
        self.clone_shallow(url, dest)
    }

    fn clone_shallow(&self, _: &str, dest: &Path) -> Result<(), String> {
        if *self.offline.borrow() {
            return Err("could not resolve host".into());
        }
        super::slice::copy_tree(&self.upstream, dest)
    }

    fn pull(&self, repo: &Path) -> Result<String, String> {
        self.head(repo)
    }

    fn head(&self, repo: &Path) -> Result<String, String> {
        Ok(if repo.to_string_lossy().contains("/taps/") {
            "local00".to_string()
        } else {
            self.sha.borrow().clone()
        })
    }
}

struct FakeClaude {
    present: bool,
    fail: Mutex<Option<String>>,
    calls: Mutex<Vec<String>>,
}

impl ClaudeRunner for FakeClaude {
    fn run(&self, args: &[&str], _: Duration) -> Result<CmdOutput, String> {
        self.calls.lock().unwrap().push(args.join(" "));
        Ok(match self.fail.lock().unwrap().clone() {
            Some(why) => CmdOutput {
                code: 1,
                stdout: String::new(),
                stderr: why,
            },
            None => CmdOutput::default(),
        })
    }

    fn present(&self) -> bool {
        self.present
    }
}

struct Fx {
    _dir: TempDir,
    plugins: PluginEnv,
    host: MemoryHost,
    git: FakeGit,
    claude: FakeClaude,
    upstream: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let claude_home = base.join("home/.claude");
        let data_dir = base.join("data");
        std::fs::create_dir_all(&claude_home).unwrap();
        save_taps(
            &data_dir,
            &[Tap {
                name: TAP.into(),
                repo: "example/example-tap".into(),
                url: "https://example.invalid/example-tap.git".into(),
            }],
        )
        .unwrap();
        let upstream = base.join("upstream");
        tree(&upstream);
        tree(&data_dir.join("taps").join(TAP));
        Self {
            host: MemoryHost {
                settings_paths: vec![claude_home.join("settings.json")],
                ..MemoryHost::default()
            },
            git: FakeGit {
                upstream: upstream.clone(),
                sha: RefCell::new("aaaaaaa1".into()),
                offline: RefCell::new(false),
            },
            claude: FakeClaude {
                present: true,
                fail: Mutex::new(None),
                calls: Mutex::new(Vec::new()),
            },
            plugins: PluginEnv {
                claude_home,
                data_dir,
            },
            upstream,
            _dir: dir,
        }
    }

    fn env(&self) -> Env<'_> {
        Env {
            plugins: &self.plugins,
            git: &self.git,
            claude: &self.claude,
            store: &self.host,
            managed: None,
        }
    }

    fn base(&self) -> PathBuf {
        super::base_dir(&self.plugins.data_dir)
    }

    fn settings(&self) -> Value {
        std::fs::read_to_string(self.plugins.user_settings())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null)
    }

    fn set(&self, on: &[&str], tick: &[&str], off: &[&str]) -> Result<Value, String> {
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        let args = SetArgs {
            on: strings(on),
            off: strings(off),
            tick_hook: strings(tick),
        };
        ops::set(&self.env(), TAP, &args, false).map_err(|e| e.message)
    }

    fn plan(&self) -> Value {
        ops::plan(
            &self.env(),
            TAP,
            Options {
                fetch: true,
                switch: false,
            },
        )
        .unwrap()
    }

    fn apply(&self) -> Result<Value, String> {
        let hash = self.plan()["planHash"].as_str().unwrap().to_string();
        ops::apply(
            &self.env(),
            TAP,
            &hash,
            Options {
                fetch: true,
                switch: false,
            },
        )
        .map_err(|e| e.message)
    }

    fn slice(&self) -> PathBuf {
        super::slice::slice_dir(&self.base(), "example-tap-selected")
    }

    fn entry(&self) -> Value {
        super::slice::entries_of(&self.base()).remove(0)
    }

    fn selection(&self) -> Selections {
        store::load(&self.base()).unwrap()
    }

    fn state_of(&self, kind: Kind, id: &str) -> State {
        self.selection().sources[TAP].item(kind, id).unwrap().state
    }
}

const FORMAT_ID: &str = "PostToolUse:Write|Edit";

fn chosen(fx: &Fx) {
    fx.set(
        &["code-review", "agent:reviewer", "deploy", "style"],
        &[FORMAT_ID],
        &[],
    )
    .unwrap();
    fx.apply().unwrap();
}

#[test]
fn test01_selection_file_round_trips_and_keeps_fields_it_does_not_know() {
    let text = r#"{"version":1,"later":{"a":1},"sources":{"t":{"ref":"abc","slice":"t-selected",
        "rev":2,"future":true,"items":[{"id":"x","kind":"skill","hash":"h","state":"on",
        "inSlice":true,"owner":"me"}]}}}"#;
    let parsed: Selections = serde_json::from_str(text).unwrap();
    let again: Selections = serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
    assert_eq!(parsed, again);
    assert_eq!(again.extra["later"], json!({"a": 1}));
    assert_eq!(again.sources["t"].extra["future"], json!(true));
    assert_eq!(again.sources["t"].items[0].extra["owner"], json!("me"));
}

#[test]
fn test02_discovery_lists_every_kind_and_only_command_hooks() {
    let fx = Fx::new();
    let found = discover::discover(&fx.upstream);
    let keys: Vec<String> = found
        .items
        .iter()
        .map(|f| format!("{}:{}", f.kind.as_str(), f.id))
        .collect();
    assert_eq!(
        keys,
        [
            "skill:code-review",
            "skill:tdd-workflow",
            "agent:reviewer",
            "command:deploy",
            "hook:PostToolUse:Write|Edit",
            "hook:PreToolUse:Bash",
            "rule:style",
        ]
    );
    assert!(found
        .warnings
        .iter()
        .any(|w| w.contains("not shell commands")));
}

#[test]
fn test03_a_changed_hook_command_changes_the_item_hash() {
    let one = discover::hook_hash(&json!({"command": "a.sh"}));
    assert_ne!(one, discover::hook_hash(&json!({"command": "a.sh --all"})));
}

#[test]
fn test04_new_items_start_off_and_a_hook_needs_its_own_tick() {
    let fx = Fx::new();
    let seeded = fx.set(&["code-review"], &[], &[]).unwrap();
    assert_eq!(seeded["on"], 1);
    assert_eq!(fx.state_of(Kind::Skill, "tdd-workflow"), State::New);
    let refused = fx.set(&[FORMAT_ID], &[], &[]).unwrap_err();
    assert!(refused.contains("--tick-hook"), "{refused}");
    fx.set(&[], &[FORMAT_ID], &[]).unwrap();
    assert_eq!(fx.state_of(Kind::Hook, FORMAT_ID), State::On);
}

#[test]
fn test05_a_plan_writes_nothing_and_apply_refuses_another_hash() {
    let fx = Fx::new();
    fx.set(&["code-review"], &[], &[]).unwrap();
    let plan = fx.plan();
    assert_eq!(plan["diffs"]["added"][0]["id"], "code-review");
    assert!(!fx.base().join("marketplace").exists());
    assert!(!fx.plugins.user_settings().exists());
    let refused = ops::apply(
        &fx.env(),
        TAP,
        "sha256:0000",
        Options {
            fetch: true,
            switch: false,
        },
    )
    .unwrap_err();
    assert!(
        refused.message.contains("plan changed"),
        "{}",
        refused.message
    );
    assert!(!fx.slice().exists());
}

#[test]
fn test06_apply_generates_a_manifestless_slice_that_lists_exactly_the_chosen_items() {
    let fx = Fx::new();
    chosen(&fx);
    let entry = fx.entry();
    assert_eq!(entry["strict"], false);
    assert_eq!(entry["name"], "example-tap-selected");
    assert_eq!(entry["source"], "./example-tap-selected");
    assert_eq!(entry["skills"], json!(["./skills/code-review"]));
    assert_eq!(entry["agents"], json!(["./agents/reviewer.md"]));
    assert_eq!(entry["commands"], json!(["./commands/deploy.md"]));
    assert_eq!(
        entry["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
        FORMAT_HOOK
    );
    assert!(entry["hooks"].get("PreToolUse").is_none());
    let slice = fx.slice();
    assert!(slice.join("skills/code-review/SKILL.md").is_file());
    assert!(!slice.join("skills/tdd-workflow").exists());
    assert!(slice.join("scripts/format.sh").is_file());
    assert!(!slice.join(".claude-plugin").exists());
    assert!(!slice.join("rules").exists());
    let settings = fx.settings();
    assert_eq!(
        settings["enabledPlugins"]["example-tap-selected@toolport-selections"],
        true
    );
    assert_eq!(
        settings["extraKnownMarketplaces"]["toolport-selections"]["source"]["source"],
        "directory"
    );
    let calls = fx.claude.calls.lock().unwrap().clone();
    assert!(calls[0].starts_with("plugin validate "), "{calls:?}");
}

#[test]
fn test07_rules_that_are_on_stay_out_of_the_slice_and_are_named_in_the_plan() {
    let fx = Fx::new();
    fx.set(&["code-review", "style"], &[], &[]).unwrap();
    let plan = fx.plan();
    assert_eq!(plan["diffs"]["rules"][0]["id"], "style");
    let text = plan["plan"]["steps"].to_string();
    assert!(text.contains("link them through the Library"), "{text}");
}

#[test]
fn test08_applying_the_same_plan_again_changes_nothing() {
    let fx = Fx::new();
    chosen(&fx);
    let before = std::fs::read_to_string(super::slice::market_file(&fx.base())).unwrap();
    let rev = fx.selection().sources[TAP].rev;
    let again = fx.plan();
    assert_eq!(again["waiting"], false);
    fx.apply().unwrap();
    assert_eq!(
        std::fs::read_to_string(super::slice::market_file(&fx.base())).unwrap(),
        before
    );
    assert_eq!(fx.selection().sources[TAP].rev, rev);
}

#[test]
fn test09_an_update_is_a_plan_and_changes_nothing_until_applied() {
    let fx = Fx::new();
    chosen(&fx);
    skill(
        &fx.upstream,
        "code-review",
        "Review a diff, now with more care.",
    );
    skill(&fx.upstream, "perf-profile", "Profile it.");
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let plan = fx.plan();
    assert_eq!(plan["diffs"]["changed"][0]["id"], "code-review");
    assert_eq!(plan["diffs"]["fresh"][0]["id"], "perf-profile");
    assert_eq!(plan["ref"]["pinned"], "aaaaaaa1");
    let kept = std::fs::read_to_string(fx.slice().join("skills/code-review/SKILL.md")).unwrap();
    assert!(kept.contains("Review a diff.") && !kept.contains("more care"));
    assert_eq!(
        fx.selection().sources[TAP].pinned.as_deref(),
        Some("aaaaaaa1")
    );
}

#[test]
fn test10_applying_an_update_refreshes_chosen_items_and_leaves_new_ones_off() {
    let fx = Fx::new();
    chosen(&fx);
    skill(
        &fx.upstream,
        "code-review",
        "Review a diff, now with more care.",
    );
    skill(&fx.upstream, "perf-profile", "Profile it.");
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    fx.apply().unwrap();
    let body = std::fs::read_to_string(fx.slice().join("skills/code-review/SKILL.md")).unwrap();
    assert!(body.contains("more care"));
    assert!(!fx.slice().join("skills/perf-profile").exists());
    assert_eq!(fx.state_of(Kind::Skill, "perf-profile"), State::New);
    assert_eq!(
        fx.selection().sources[TAP].pinned.as_deref(),
        Some("bbbbbbb2")
    );
    assert_eq!(fx.entry()["version"], "0.1.2");
}

#[test]
fn test11_a_changed_hook_keeps_the_old_command_until_it_is_ticked_again() {
    let fx = Fx::new();
    chosen(&fx);
    put(
        &fx.upstream.join("hooks/hooks.json"),
        &hooks_json(&format!("{FORMAT_HOOK} --all")),
    );
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let plan = fx.plan();
    assert_eq!(plan["diffs"]["retick"][0]["id"], FORMAT_ID);
    fx.apply().unwrap();
    let command = &fx.entry()["hooks"]["PostToolUse"][0]["hooks"][0]["command"];
    assert_eq!(command, FORMAT_HOOK);
    let listed = ops::ls(&fx.env(), Some(TAP)).unwrap();
    let hook = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == FORMAT_ID)
        .unwrap();
    assert!(hook["pending"]["command"]
        .as_str()
        .unwrap()
        .ends_with("--all"));
    fx.set(&[], &[FORMAT_ID], &[]).unwrap();
    fx.apply().unwrap();
    let command = fx.entry()["hooks"]["PostToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(command.ends_with("--all"), "{command}");
}

#[test]
fn test12_an_item_removed_upstream_keeps_its_pinned_copy_until_turned_off() {
    let fx = Fx::new();
    chosen(&fx);
    std::fs::remove_dir_all(fx.upstream.join("commands")).unwrap();
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let plan = fx.plan();
    assert_eq!(plan["diffs"]["gone"][0]["id"], "deploy");
    fx.apply().unwrap();
    assert_eq!(fx.state_of(Kind::Command, "deploy"), State::Removed);
    assert!(fx.slice().join("commands/deploy.md").is_file());
    assert_eq!(fx.entry()["commands"], json!(["./commands/deploy.md"]));
    fx.set(&[], &[], &["deploy"]).unwrap();
    fx.apply().unwrap();
    assert!(!fx.slice().join("commands/deploy.md").exists());
    assert!(fx.entry().get("commands").is_none());
}

#[test]
fn test13_a_rename_is_matched_by_content_hash() {
    let fx = Fx::new();
    chosen(&fx);
    std::fs::rename(
        fx.upstream.join("commands/deploy.md"),
        fx.upstream.join("commands/ship.md"),
    )
    .unwrap();
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let plan = fx.plan();
    assert!(plan["diffs"]["gone"][0]["note"]
        .as_str()
        .unwrap()
        .contains("ship"));
    assert!(plan["diffs"]["fresh"][0]["note"]
        .as_str()
        .unwrap()
        .contains("deploy"));
}

#[test]
fn test14_a_source_that_cannot_be_fetched_is_planned_from_the_local_clone() {
    let fx = Fx::new();
    *fx.git.offline.borrow_mut() = true;
    fx.set(&["code-review"], &[], &[]).unwrap();
    let plan = fx.plan();
    assert_eq!(plan["checked"], false);
    assert!(plan["fetchError"]
        .as_str()
        .unwrap()
        .contains("resolve host"));
    assert_eq!(plan["ref"]["latest"], "local00");
}

#[test]
fn test15_a_marketplace_the_validator_refuses_leaves_no_trace() {
    let fx = Fx::new();
    *fx.claude.fail.lock().unwrap() = Some("entry skills: path not found".into());
    fx.set(&["code-review"], &[], &[]).unwrap();
    let error = fx.apply().unwrap_err();
    assert!(
        error.contains("refused the generated marketplace"),
        "{error}"
    );
    assert!(!fx.slice().exists());
    assert!(!fx.plugins.user_settings().exists());
    assert!(fx.selection().sources[TAP].pinned.is_none());
}

#[test]
fn test16_without_claude_the_apply_goes_ahead_and_says_it_did_not_validate() {
    let mut fx = Fx::new();
    fx.claude.present = false;
    fx.set(&["code-review"], &[], &[]).unwrap();
    let done = fx.apply().unwrap();
    assert!(done["warnings"][0]
        .as_str()
        .unwrap()
        .contains("not validated"));
    assert!(fx.slice().join("skills/code-review/SKILL.md").is_file());
}

#[test]
fn test17_undo_puts_back_the_record_the_folder_and_the_settings() {
    let fx = Fx::new();
    fx.set(&["code-review"], &[], &[]).unwrap();
    fx.apply().unwrap();
    fx.set(&["tdd-workflow"], &[], &[]).unwrap();
    fx.apply().unwrap();
    assert!(fx.slice().join("skills/tdd-workflow").is_dir());
    ops::undo(&fx.env(), TAP).unwrap();
    assert!(!fx.slice().join("skills/tdd-workflow").exists());
    assert!(fx.slice().join("skills/code-review").is_dir());
    assert_eq!(fx.entry()["skills"], json!(["./skills/code-review"]));
    ops::undo(&fx.env(), TAP).unwrap_err();
}

#[test]
fn test18_undoing_the_first_apply_removes_the_slice_and_the_settings_entries() {
    let fx = Fx::new();
    fx.set(&["code-review"], &[], &[]).unwrap();
    fx.apply().unwrap();
    ops::undo(&fx.env(), TAP).unwrap();
    assert!(!fx.slice().exists());
    let settings = fx.settings();
    assert!(settings["enabledPlugins"]
        .get("example-tap-selected@toolport-selections")
        .is_none());
    assert!(settings["extraKnownMarketplaces"]
        .get("toolport-selections")
        .is_none());
}

#[test]
fn test19_turning_every_item_off_removes_the_slice_plugin_from_the_settings() {
    let fx = Fx::new();
    fx.set(&["code-review"], &[], &[]).unwrap();
    fx.apply().unwrap();
    fx.set(&[], &[], &["code-review"]).unwrap();
    fx.apply().unwrap();
    assert!(!fx.slice().exists());
    assert!(fx.settings()["enabledPlugins"]
        .get("example-tap-selected@toolport-selections")
        .is_none());
}

fn marketplace_tap(fx: &Fx) {
    let clone = fx.plugins.data_dir.join("taps").join(TAP);
    std::fs::remove_dir_all(&clone).unwrap();
    put(
        &clone.join(".claude-plugin/marketplace.json"),
        r#"{"name": "acme", "plugins": [{"name": "core", "source": "./plugins/core"}]}"#,
    );
    tree(&clone.join("plugins/core"));
    std::fs::remove_dir_all(&fx.upstream).unwrap();
    put(
        &fx.upstream.join(".claude-plugin/marketplace.json"),
        r#"{"name": "acme", "plugins": [{"name": "core", "source": "./plugins/core"}]}"#,
    );
    tree(&fx.upstream.join("plugins/core"));
}

#[test]
fn test20_a_plugin_installed_whole_gets_an_offer_and_switch_turns_it_off() {
    let fx = Fx::new();
    marketplace_tap(&fx);
    put(
        &fx.plugins.user_settings(),
        r#"{"enabledPlugins": {"core@acme": true}}"#,
    );
    let listed = ops::ls(&fx.env(), None).unwrap();
    assert_eq!(listed["offers"][0]["source"], "example-tap/core");
    assert_eq!(listed["offers"][0]["offer"]["whole"], "core@acme");
    let source = "example-tap/core";
    let args = SetArgs {
        on: vec!["code-review".into()],
        ..SetArgs::default()
    };
    ops::set(&fx.env(), source, &args, false).unwrap();
    let opts = Options {
        fetch: true,
        switch: false,
    };
    let plain = ops::plan(&fx.env(), source, opts).unwrap();
    assert_eq!(plain["offer"]["kind"], "switch-to-chosen-items");
    ops::apply(&fx.env(), source, plain["planHash"].as_str().unwrap(), opts).unwrap();
    assert_eq!(fx.settings()["enabledPlugins"]["core@acme"], true);
    let switch = Options {
        fetch: true,
        switch: true,
    };
    let plan = ops::plan(&fx.env(), source, switch).unwrap();
    assert!(plan["plan"]["steps"]
        .to_string()
        .contains("disable core@acme"));
    ops::apply(
        &fx.env(),
        source,
        plan["planHash"].as_str().unwrap(),
        switch,
    )
    .unwrap();
    assert_eq!(fx.settings()["enabledPlugins"]["core@acme"], false);
    assert_eq!(
        fx.settings()["enabledPlugins"]["core-selected@toolport-selections"],
        true
    );
    ops::undo(&fx.env(), source).unwrap();
    assert_eq!(fx.settings()["enabledPlugins"]["core@acme"], true);
}

#[test]
fn test21_a_marketplace_tap_with_several_plugins_must_be_named() {
    let fx = Fx::new();
    marketplace_tap(&fx);
    let clone = fx.plugins.data_dir.join("taps").join(TAP);
    put(
        &clone.join(".claude-plugin/marketplace.json"),
        r#"{"name": "acme", "plugins": [{"name": "core", "source": "./plugins/core"},
            {"name": "extra", "source": "./plugins/core"}]}"#,
    );
    let error = ops::ls(&fx.env(), Some(TAP)).unwrap_err();
    assert!(
        error.message.contains("example-tap/<plugin>"),
        "{}",
        error.message
    );
}

#[test]
fn test22_the_scheduled_check_records_what_waits_and_applies_nothing() {
    let fx = Fx::new();
    chosen(&fx);
    assert!(check::pending(&fx.base()).is_empty());
    skill(&fx.upstream, "code-review", "Changed upstream.");
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let out = check::run(&fx.env(), true).unwrap();
    assert_eq!(out["waiting"], 1);
    assert_eq!(out["applied"], false);
    assert_eq!(check::pending(&fx.base())[0]["source"], TAP);
    let kept = std::fs::read_to_string(fx.slice().join("skills/code-review/SKILL.md")).unwrap();
    assert!(!kept.contains("Changed upstream"));
    assert_eq!(
        fx.selection().sources[TAP].pinned.as_deref(),
        Some("aaaaaaa1")
    );
}

#[test]
fn test23_two_sources_cannot_share_one_generated_plugin_name() {
    let fx = Fx::new();
    fx.set(&["code-review"], &[], &[]).unwrap();
    let mut selections = fx.selection();
    let mut other = selections.sources[TAP].clone();
    other.slice = "example-tap-selected".into();
    selections.sources.insert("zzz".into(), other);
    store::save(&fx.base(), &selections).unwrap();
    let error = ops::plan(&fx.env(), TAP, Options::default()).unwrap_err();
    assert!(error.message.contains("already uses"), "{}", error.message);
}

#[test]
fn test24_the_plan_counts_the_lines_that_changed_in_an_item_that_is_on() {
    let fx = Fx::new();
    chosen(&fx);
    skill(
        &fx.upstream,
        "code-review",
        "Review a diff.\nNow check the tests too.\nAnd the docs.",
    );
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let plan = fx.plan();
    let lines = &plan["lines"]["skill:code-review"];
    assert_eq!(lines["added"], 2);
    assert_eq!(lines["removed"], 0);
    assert_eq!(lines["files"], 1);
    assert!(plan["lines"].get("skill:tdd-workflow").is_none());
}

#[test]
fn test25_disable_all_hooks_in_the_user_settings_is_reported_and_warned_about() {
    let fx = Fx::new();
    chosen(&fx);
    assert!(fx.plan()["policy"]["hooksOff"].is_null());
    put(
        &fx.plugins.user_settings(),
        &json!({"disableAllHooks": true, "enabledPlugins": fx.settings()["enabledPlugins"],
            "extraKnownMarketplaces": fx.settings()["extraKnownMarketplaces"]})
        .to_string(),
    );
    let plan = fx.plan();
    assert_eq!(plan["policy"]["hooksOff"], "your Claude Code settings");
    assert!(plan["plan"]["warnings"]
        .to_string()
        .contains("hooks are switched off by your Claude Code settings"));
    let listed = ops::ls(&fx.env(), Some(TAP)).unwrap();
    assert_eq!(listed["policy"]["hooksOff"], "your Claude Code settings");
}

#[test]
fn test26_a_managed_allowlist_that_omits_the_marketplace_blocks_the_source() {
    let fx = Fx::new();
    let managed = fx.plugins.claude_home.join("managed-settings.json");
    let root = super::slice::market_root(&fx.base());
    let read = |doc: Value| {
        put(&managed, &doc.to_string());
        let mut env = fx.env();
        env.managed = Some(managed.clone());
        super::policy::read(&env, &root)
    };
    assert_eq!(read(json!({})).blocked, None);
    assert_eq!(
        read(json!({"strictKnownMarketplaces": [{"source": "github", "repo": "a/b"}]})).blocked,
        Some("the managed policy")
    );
    let ours = json!({"source": "directory", "path": root.to_string_lossy()});
    assert_eq!(
        read(json!({"strictKnownMarketplaces": [ours.clone()]})).blocked,
        None
    );
    assert_eq!(
        read(json!({"blockedMarketplaces": [ours]})).blocked,
        Some("the managed policy")
    );
    let by_pattern = json!({"source": "pathPattern", "pathPattern": "selective/marketplace$"});
    assert_eq!(
        read(json!({"strictKnownMarketplaces": [by_pattern]})).blocked,
        None
    );
    assert_eq!(
        read(json!({"disableAllHooks": true})).hooks_off,
        Some("the managed policy")
    );
}

#[test]
fn test27_the_plan_carries_the_commands_of_hooks_that_changed_or_arrived() {
    let fx = Fx::new();
    chosen(&fx);
    put(
        &fx.upstream.join("hooks/hooks.json"),
        &json!({"hooks": {"PostToolUse": [{"matcher": "Write|Edit", "hooks": [
            {"type": "command", "command": format!("{FORMAT_HOOK} --all")}
        ]}], "PreToolUse": [{"matcher": "Bash", "hooks": [
            {"type": "command", "command": "echo checked"},
            {"type": "command", "command": "echo audited"}
        ]}]}})
        .to_string(),
    );
    *fx.git.sha.borrow_mut() = "bbbbbbb2".into();
    let hooks = &fx.plan()["hooks"];
    assert_eq!(hooks["hook:PostToolUse:Write|Edit"]["was"], FORMAT_HOOK);
    assert_eq!(
        hooks["hook:PostToolUse:Write|Edit"]["command"],
        format!("{FORMAT_HOOK} --all")
    );
    assert_eq!(hooks["hook:PreToolUse:Bash#2"]["command"], "echo audited");
    assert!(hooks["hook:PreToolUse:Bash#2"]["was"].is_null());
}
