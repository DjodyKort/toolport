use super::claude::NOT_FOUND;
use super::installed::{self, Via};
use super::link::{describe, link, linked_taps, refresh, unlink};
use super::{marketplace, ClaudeRunner, Env};
use crate::skills::gitexec::CmdOutput;
use crate::skills::taps::{save_taps, Tap};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tempfile::TempDir;
use toolport_plus_api::memory::MemoryHost;

const TAP: &str = "plugins";

struct FakeClaude {
    present: bool,
    replies: Vec<(&'static str, Result<CmdOutput, String>)>,
    calls: Mutex<Vec<String>>,
}

impl FakeClaude {
    fn absent() -> Self {
        Self {
            present: false,
            replies: Vec::new(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn answering(replies: Vec<(&'static str, Result<CmdOutput, String>)>) -> Self {
        Self {
            present: true,
            replies,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl ClaudeRunner for FakeClaude {
    fn run(&self, args: &[&str], _: Duration) -> Result<CmdOutput, String> {
        let line = args.join(" ");
        self.calls.lock().unwrap().push(line.clone());
        if !self.present {
            return Err(NOT_FOUND.to_string());
        }
        self.replies
            .iter()
            .find(|(prefix, _)| line.starts_with(prefix))
            .map(|(_, reply)| reply.clone())
            .unwrap_or_else(|| Ok(CmdOutput::default()))
    }

    fn present(&self) -> bool {
        self.present
    }
}

fn stdout(text: &str) -> Result<CmdOutput, String> {
    Ok(CmdOutput {
        code: 0,
        stdout: text.to_string(),
        stderr: String::new(),
    })
}

fn failing(message: &str) -> Result<CmdOutput, String> {
    Ok(CmdOutput {
        code: 1,
        stdout: String::new(),
        stderr: message.to_string(),
    })
}

struct Fx {
    _dir: TempDir,
    env: Env,
    host: MemoryHost,
    clone: PathBuf,
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let claude_home = base.join("home/.claude");
        let data_dir = base.join("data");
        std::fs::create_dir_all(&claude_home).unwrap();
        let tap = Tap {
            name: TAP.into(),
            repo: "acme/plugins".into(),
            url: "https://github.com/acme/plugins".into(),
        };
        std::fs::create_dir_all(&data_dir).unwrap();
        save_taps(&data_dir, &[tap]).unwrap();
        let clone = data_dir.join("taps").join(TAP);
        put(
            &clone.join(".claude-plugin/marketplace.json"),
            r#"{"name": "acme", "plugins": [
                {"name": "core", "source": "./plugins/core", "version": "1.0.0"},
                {"name": "extra", "source": "./plugins/extra"},
                {"name": "remote", "source": {"source": "github", "repo": "acme/remote"}}
            ]}"#,
        );
        put(
            &clone.join("plugins/core/skills/task/SKILL.md"),
            "---\nname: task\ndescription: A plugin skill\n---\nBody.\n",
        );
        put(&clone.join("plugins/core/hooks/hooks.json"), "{}");
        put(
            &clone.join("plugins/extra/agents/helper.md"),
            "---\nname: helper\ndescription: A plugin agent\n---\nBody.\n",
        );
        let host = MemoryHost {
            settings_paths: vec![claude_home.join("settings.json")],
            ..MemoryHost::default()
        };
        Self {
            _dir: dir,
            env: Env {
                claude_home,
                data_dir,
            },
            host,
            clone,
        }
    }

    fn settings(&self) -> Value {
        let text = std::fs::read_to_string(self.env.user_settings()).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or(Value::Null)
    }

    fn write_settings(&self, doc: &Value) {
        put(&self.env.user_settings(), &doc.to_string());
    }

    fn known_marketplaces(&self, doc: &Value) {
        put(
            &self.env.claude_home.join("plugins/known_marketplaces.json"),
            &doc.to_string(),
        );
    }

    fn installed(&self, doc: &Value) {
        put(
            &self.env.claude_home.join("plugins/installed_plugins.json"),
            &doc.to_string(),
        );
    }

    fn link(
        &self,
        runner: &dyn ClaudeRunner,
        only: &[&str],
        dry_run: bool,
    ) -> Result<Value, String> {
        let only: Vec<String> = only.iter().map(|s| s.to_string()).collect();
        link(&self.host, &self.env, runner, TAP, &only, dry_run).map_err(|e| e.message)
    }
}

fn directory(path: &Path) -> Value {
    json!({"source": {"source": "directory", "path": path.display().to_string()}})
}

#[test]
fn a_marketplace_lists_the_plugins_inside_the_clone_and_warns_about_the_rest() {
    let fx = Fx::new();
    let market = marketplace::read(&fx.clone).unwrap().unwrap();
    assert_eq!(market.name, "acme");
    let names: Vec<&str> = market.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["core", "extra"]);
    let core = market.plugin("core").unwrap();
    assert_eq!((core.skills.len(), core.hooks), (1, true));
    assert_eq!(market.plugin("extra").unwrap().agents, ["helper"]);
    assert_eq!(market.id_of("core"), "core@acme");
    assert!(market.warnings.iter().any(|w| w.contains("remote")));
}

#[test]
fn a_plugin_source_that_leaves_the_clone_is_skipped() {
    let fx = Fx::new();
    put(
        &fx.clone.join(".claude-plugin/marketplace.json"),
        r#"{"name": "acme", "plugins": [{"name": "evil", "source": "./../outside"}]}"#,
    );
    let market = marketplace::read(&fx.clone).unwrap().unwrap();
    assert!(market.plugins.is_empty());
    assert!(market.warnings[0].contains("not a folder inside the clone"));
}

#[cfg(unix)]
#[test]
fn a_plugin_that_is_a_link_or_holds_links_leaks_nothing_into_the_listing() {
    use std::os::unix::fs::symlink;
    let fx = Fx::new();
    let outside = fx.clone.parent().unwrap().join("outside");
    put(&outside.join("plugin/skills/leak/SKILL.md"), "TOPSECRET");
    put(
        &outside.join("plugin/.claude-plugin/plugin.json"),
        r#"{"version": "TOPSECRET"}"#,
    );
    put(&outside.join("hooks.json"), "{}");
    put(
        &fx.clone.join(".claude-plugin/marketplace.json"),
        r#"{"name": "acme", "plugins": [
            {"name": "linked", "source": "./plugins/linked"},
            {"name": "inner", "source": "./plugins/inner"},
            {"name": "through", "source": "./plugins/linked/skills"},
            {"name": "up", "source": "./plugins/../../outside/plugin"},
            {"name": "slash", "source": "./plugins//inner"},
            {"name": "absolute", "source": "/tmp"}]}"#,
    );
    symlink(outside.join("plugin"), fx.clone.join("plugins/linked")).unwrap();
    put(&fx.clone.join("plugins/inner/skills/own/SKILL.md"), "x");
    symlink(
        outside.join("plugin/skills"),
        fx.clone.join("plugins/inner/agents"),
    )
    .unwrap();
    symlink(
        outside.join("plugin/.claude-plugin/plugin.json"),
        fx.clone.join("plugins/inner/plugin.json"),
    )
    .unwrap();
    std::fs::create_dir_all(fx.clone.join("plugins/inner/.claude-plugin")).unwrap();
    symlink(
        outside.join("plugin/.claude-plugin/plugin.json"),
        fx.clone.join("plugins/inner/.claude-plugin/plugin.json"),
    )
    .unwrap();
    symlink(
        outside.join("hooks.json"),
        fx.clone.join("plugins/inner/hooks"),
    )
    .unwrap();
    let market = marketplace::read(&fx.clone).unwrap().unwrap();
    let names: Vec<&str> = market.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["inner"]);
    let inner = market.plugin("inner").unwrap();
    assert_eq!(inner.skills, ["own"]);
    assert!(inner.agents.is_empty());
    assert_eq!(inner.version, None);
    assert!(!inner.hooks);
    assert_eq!(market.warnings.len(), 5, "{:?}", market.warnings);
}

#[cfg(unix)]
#[test]
fn a_marketplace_file_that_is_a_link_is_not_read() {
    use std::os::unix::fs::symlink;
    let fx = Fx::new();
    let real = fx.clone.parent().unwrap().join("market.json");
    std::fs::rename(fx.clone.join(marketplace::FILE), &real).unwrap();
    symlink(&real, fx.clone.join(marketplace::FILE)).unwrap();
    assert!(marketplace::read(&fx.clone).is_none());
    assert!(!marketplace::is_marketplace(&fx.clone));
}

#[test]
fn a_marketplace_name_that_looks_like_a_flag_or_an_id_is_refused() {
    for bad in ["", "-x", ".hidden", "a@b", "a b"] {
        assert!(!marketplace::valid_name(bad), "{bad:?}");
    }
    assert!(marketplace::valid_name("acme-tools_2.0"));
}

#[test]
fn the_cli_list_wins_and_keeps_the_user_scope_record_of_a_plugin() {
    let listed = installed::parse_cli_list(
        r#"[{"name":"core@acme","version":"1.0.0","scope":"project"},
            {"name":"core@acme","version":"1.1.0","scope":"user","folderVersion":"1.2.0"}]"#,
    )
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].scope.as_deref(), Some("user"));
    assert_eq!(listed[0].folder_version.as_deref(), Some("1.2.0"));
    assert_eq!(listed[0].marketplace.as_deref(), Some("acme"));
    assert!(installed::parse_cli_list("{\"plugins\": 3}").is_err());
}

#[test]
fn the_files_answer_when_claude_is_not_installed() {
    let fx = Fx::new();
    fx.installed(&json!({"version": 2, "plugins": {
        "core@acme": [{"scope": "user", "installPath": "/x/core", "version": "1.0.0"}]
    }}));
    let listing = installed::list(Some(&FakeClaude::absent()), &fx.env.claude_home);
    assert_eq!(listing.via, Via::Files);
    assert_eq!(listing.cli_error, None);
    assert_eq!(listing.plugins[0].id, "core@acme");
    let failed = installed::list(
        Some(&FakeClaude::answering(vec![(
            "plugin list",
            failing("boom"),
        )])),
        &fx.env.claude_home,
    );
    assert_eq!(failed.cli_error.as_deref(), Some("boom"));
}

#[test]
fn a_cli_listing_borrows_the_install_path_the_files_record() {
    let fx = Fx::new();
    fx.installed(&json!({"plugins": {"core@acme": [{"installPath": "/x/core"}]}}));
    let claude = FakeClaude::answering(vec![("plugin list", stdout(r#"[{"name":"core@acme"}]"#))]);
    let listing = installed::list(Some(&claude), &fx.env.claude_home);
    assert_eq!(listing.via, Via::ClaudeCli);
    assert_eq!(
        listing.plugins[0].install_path.as_deref(),
        Some(Path::new("/x/core"))
    );
}

#[test]
fn a_dry_run_link_plans_both_settings_keys_and_writes_nothing() {
    let fx = Fx::new();
    let before = json!({"theme": "dark"});
    fx.write_settings(&before);
    let data = fx.link(&FakeClaude::absent(), &[], true).unwrap();
    assert_eq!(fx.settings(), before);
    assert_eq!(data["result"], Value::Null);
    assert_eq!(data["dryRun"], json!(true));
    assert_eq!(data["marketplace"], json!("acme"));
    let keys: Vec<&str> = data["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "extraKnownMarketplaces.acme",
            "enabledPlugins.core@acme",
            "enabledPlugins.extra@acme"
        ]
    );
    let steps = data["plan"]["steps"].as_array().unwrap();
    assert_eq!(steps[0]["op"], json!("merge"));
    assert_eq!(steps[0]["keys"], json!(["extraKnownMarketplaces"]));
    assert_eq!(
        data["plan"]["undo"],
        json!("toolportctl skills tap unlink plugins")
    );
}

#[test]
fn link_declares_the_clone_and_switches_the_plugins_on_leaving_other_keys_alone() {
    let fx = Fx::new();
    fx.write_settings(&json!({"theme": "dark", "enabledPlugins": {"other@x": true}}));
    let data = fx.link(&FakeClaude::absent(), &["core"], false).unwrap();
    let settings = fx.settings();
    assert_eq!(settings["theme"], json!("dark"));
    assert_eq!(settings["enabledPlugins"]["other@x"], json!(true));
    assert_eq!(settings["enabledPlugins"]["core@acme"], json!(true));
    assert!(settings["enabledPlugins"].get("extra@acme").is_none());
    assert_eq!(
        settings["extraKnownMarketplaces"]["acme"],
        directory(&fx.clone)
    );
    assert_eq!(data["result"]["applied"], json!(true));
    assert_eq!(data["result"]["changed"].as_array().unwrap().len(), 2);
}

#[test]
fn linking_twice_writes_nothing_the_second_time() {
    let fx = Fx::new();
    fx.link(&FakeClaude::absent(), &[], false).unwrap();
    let first = fx.settings();
    let data = fx.link(&FakeClaude::absent(), &[], false).unwrap();
    assert_eq!(fx.settings(), first);
    assert!(data["changes"].as_array().unwrap().is_empty());
    assert!(data["plan"]["steps"][0]["detail"]
        .as_str()
        .unwrap()
        .starts_with("already linked"));
    assert_eq!(data["plugins"][0]["linked"], json!(true));
}

#[test]
fn a_plugin_switched_off_by_hand_is_switched_on_again_by_link() {
    let fx = Fx::new();
    fx.write_settings(&json!({"enabledPlugins": {"core@acme": false}}));
    fx.link(&FakeClaude::absent(), &["core"], false).unwrap();
    assert_eq!(fx.settings()["enabledPlugins"]["core@acme"], json!(true));
}

#[test]
fn link_refuses_a_marketplace_name_declared_from_somewhere_else() {
    let fx = Fx::new();
    fx.known_marketplaces(&json!({"acme": {"source": {"source": "github", "repo": "acme/other"}}}));
    let message = fx.link(&FakeClaude::absent(), &[], false).unwrap_err();
    assert!(
        message.contains("already declared from github:acme/other"),
        "{message}"
    );
    assert!(!fx.env.user_settings().exists());
}

#[test]
fn link_refuses_an_extra_marketplace_of_the_same_name_with_another_path() {
    let fx = Fx::new();
    fx.write_settings(
        &json!({"extraKnownMarketplaces": {"acme": directory(Path::new("/elsewhere"))}}),
    );
    let message = fx.link(&FakeClaude::absent(), &[], true).unwrap_err();
    assert!(
        message.contains("already declared from /elsewhere"),
        "{message}"
    );
}

#[test]
fn a_marketplace_registered_through_claude_is_not_declared_again() {
    let fx = Fx::new();
    fx.known_marketplaces(&json!({"acme": directory(&fx.clone)}));
    let data = fx.link(&FakeClaude::absent(), &["core"], false).unwrap();
    let keys: Vec<&str> = data["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["enabledPlugins.core@acme"]);
    assert!(fx.settings().get("extraKnownMarketplaces").is_none());
}

#[test]
fn link_names_unknown_plugins_and_taps_that_are_not_marketplaces() {
    let fx = Fx::new();
    let message = fx.link(&FakeClaude::absent(), &["nope"], true).unwrap_err();
    assert!(
        message.contains("nope is not a plugin of acme; it has: core, extra"),
        "{message}"
    );
    std::fs::remove_file(fx.clone.join(marketplace::FILE)).unwrap();
    let message = fx.link(&FakeClaude::absent(), &[], true).unwrap_err();
    assert!(message.contains("is not a plugin marketplace"), "{message}");
    let missing = link(&fx.host, &fx.env, &FakeClaude::absent(), "ghost", &[], true).unwrap_err();
    assert_eq!(missing.message, "Tap 'ghost' not found.");
}

#[test]
fn link_warns_about_hooks_a_synced_twin_and_the_restart() {
    let fx = Fx::new();
    put(
        &fx.env
            .claude_home
            .join("plugins/synced/org_user/core/.keep"),
        "",
    );
    let data = fx.link(&FakeClaude::absent(), &[], true).unwrap();
    let warnings: Vec<&str> = data["plan"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    assert!(warnings
        .iter()
        .any(|w| w.contains("core@synced is the same plugin as core@acme")));
    assert!(warnings.iter().any(|w| w.contains("core ship hooks")));
    assert!(warnings.iter().any(|w| w.contains("when a session starts")));
}

#[test]
fn a_settings_file_that_cannot_be_read_stops_link_before_anything_is_written() {
    let fx = Fx::new();
    put(&fx.env.user_settings(), "{not json");
    let message = fx.link(&FakeClaude::absent(), &[], false).unwrap_err();
    assert!(message.contains("cannot be read"), "{message}");
    assert!(message.contains("nothing was changed"), "{message}");
    assert_eq!(
        std::fs::read_to_string(fx.env.user_settings()).unwrap(),
        "{not json"
    );
}

#[test]
fn unlink_removes_only_what_belongs_to_the_marketplace() {
    let fx = Fx::new();
    fx.write_settings(&json!({
        "theme": "dark",
        "extraKnownMarketplaces": {"acme": directory(&fx.clone), "mine": directory(Path::new("/mine"))},
        "enabledPlugins": {"core@acme": true, "extra@acme": false, "other@mine": true}
    }));
    let data = unlink(&fx.host, &fx.env, &FakeClaude::absent(), TAP, false).unwrap();
    let settings = fx.settings();
    assert_eq!(settings["theme"], json!("dark"));
    assert_eq!(
        settings["extraKnownMarketplaces"],
        json!({"mine": directory(Path::new("/mine"))})
    );
    assert_eq!(settings["enabledPlugins"], json!({"other@mine": true}));
    assert_eq!(data["marketplaces"], json!(["acme"]));
    assert_eq!(
        data["plan"]["undo"],
        json!("toolportctl skills tap link plugins")
    );
}

#[test]
fn unlink_asks_claude_to_forget_a_marketplace_it_registered() {
    let fx = Fx::new();
    fx.known_marketplaces(&json!({"acme": directory(&fx.clone)}));
    let claude = FakeClaude::answering(Vec::new());
    let data = unlink(&fx.host, &fx.env, &claude, TAP, false).unwrap();
    assert_eq!(claude.calls(), ["plugin marketplace remove acme"]);
    assert_eq!(
        data["changes"][0]["command"],
        json!(["claude", "plugin", "marketplace", "remove", "acme"])
    );
    let dry = FakeClaude::answering(Vec::new());
    unlink(&fx.host, &fx.env, &dry, TAP, true).unwrap();
    assert!(dry.calls().is_empty());
}

#[test]
fn unlink_without_claude_edits_the_settings_and_says_what_is_left() {
    let fx = Fx::new();
    fx.known_marketplaces(&json!({"acme": directory(&fx.clone)}));
    fx.write_settings(&json!({"enabledPlugins": {"core@acme": true}}));
    let data = unlink(&fx.host, &fx.env, &FakeClaude::absent(), TAP, false).unwrap();
    assert_eq!(fx.settings()["enabledPlugins"], json!({}));
    let warnings = data["plan"]["warnings"].to_string();
    assert!(
        warnings.contains("Claude Code still lists acme"),
        "{warnings}"
    );
}

#[test]
fn a_failing_marketplace_remove_names_the_settings_that_already_changed() {
    let fx = Fx::new();
    fx.known_marketplaces(&json!({"acme": directory(&fx.clone)}));
    let claude = FakeClaude::answering(vec![("plugin marketplace remove", failing("denied"))]);
    let error = unlink(&fx.host, &fx.env, &claude, TAP, false).unwrap_err();
    assert!(error.message.starts_with("denied"), "{}", error.message);
    assert!(
        error.message.contains("settings were already changed"),
        "{}",
        error.message
    );
}

#[test]
fn unlinking_a_tap_that_was_never_linked_is_a_note() {
    let fx = Fx::new();
    let data = unlink(&fx.host, &fx.env, &FakeClaude::absent(), TAP, true).unwrap();
    assert!(data["changes"].as_array().unwrap().is_empty());
    assert!(data["plan"]["steps"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("is not linked to Claude Code"));
}

#[test]
fn linked_taps_names_only_taps_declared_from_their_own_clone() {
    let fx = Fx::new();
    let names = vec![TAP.to_string()];
    assert!(linked_taps(&fx.host, &fx.env, &names).is_empty());
    fx.write_settings(
        &json!({"extraKnownMarketplaces": {"acme": directory(Path::new("/elsewhere"))}}),
    );
    assert!(linked_taps(&fx.host, &fx.env, &names).is_empty());
    fx.write_settings(&json!({"extraKnownMarketplaces": {"acme": directory(&fx.clone)}}));
    assert_eq!(linked_taps(&fx.host, &fx.env, &names), [TAP]);
}

#[test]
fn describe_shows_the_link_state_of_each_plugin() {
    let fx = Fx::new();
    let shown = describe(&fx.host, &fx.env, TAP).unwrap();
    assert_eq!(shown["declared"], json!("no"));
    assert_eq!(shown["plugins"][0]["linked"], json!(false));
    fx.link(&FakeClaude::absent(), &["core"], false).unwrap();
    let shown = describe(&fx.host, &fx.env, TAP).unwrap();
    assert_eq!(shown["declared"], json!("here"));
    assert_eq!(shown["plugins"][0]["linked"], json!(true));
    assert_eq!(shown["plugins"][1]["linked"], json!(false));
    put(&fx.clone.join(marketplace::FILE), "nope");
    assert!(describe(&fx.host, &fx.env, TAP).unwrap()["error"].is_string());
    std::fs::remove_file(fx.clone.join(marketplace::FILE)).unwrap();
    assert!(describe(&fx.host, &fx.env, TAP).is_none());
}

const DRIFTED: &str = r#"[
    {"name":"core@acme","version":"1.0.0","folderVersion":"1.1.0","scope":"user"},
    {"name":"extra@acme","version":"2.0.0","folderVersion":"2.0.0","scope":"user"},
    {"name":"core@other","version":"1.0.0","folderVersion":"9.9.9","scope":"user"}
]"#;

fn linked_fx() -> Fx {
    let fx = Fx::new();
    fx.write_settings(&json!({"extraKnownMarketplaces": {"acme": directory(&fx.clone)}}));
    fx
}

#[test]
fn refresh_updates_only_the_linked_plugins_whose_clone_moved_on() {
    let fx = linked_fx();
    let claude = FakeClaude::answering(vec![("plugin list", stdout(DRIFTED))]);
    let rows = refresh(&fx.host, &fx.env, &claude, &[TAP.to_string()], false);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], json!("core@acme"));
    assert_eq!(
        (rows[0]["from"].as_str(), rows[0]["to"].as_str()),
        (Some("1.0.0"), Some("1.1.0"))
    );
    assert_eq!(rows[0]["outcome"], json!("updated"));
    assert_eq!(
        claude.calls(),
        ["plugin list --json", "plugin update core@acme --scope user"]
    );
}

#[test]
fn a_dry_run_refresh_reports_without_updating() {
    let fx = linked_fx();
    let claude = FakeClaude::answering(vec![("plugin list", stdout(DRIFTED))]);
    let rows = refresh(&fx.host, &fx.env, &claude, &[TAP.to_string()], true);
    assert_eq!(rows[0]["outcome"], json!("would update"));
    assert_eq!(claude.calls(), ["plugin list --json"]);
}

#[test]
fn a_failed_update_is_a_row_and_an_unreadable_list_is_one_row_per_tap() {
    let fx = linked_fx();
    let claude = FakeClaude::answering(vec![
        ("plugin list", stdout(DRIFTED)),
        ("plugin update", failing("network down")),
    ]);
    let rows = refresh(&fx.host, &fx.env, &claude, &[TAP.to_string()], false);
    assert_eq!(rows[0]["error"], json!("network down"));
    let rows = refresh(
        &fx.host,
        &fx.env,
        &FakeClaude::absent(),
        &[TAP.to_string()],
        false,
    );
    assert_eq!(rows, [json!({"tap": TAP, "error": NOT_FOUND})]);
}

#[test]
fn refresh_leaves_unlinked_taps_alone_without_asking_claude() {
    let fx = Fx::new();
    let claude = FakeClaude::answering(vec![("plugin list", stdout(DRIFTED))]);
    assert!(refresh(&fx.host, &fx.env, &claude, &[TAP.to_string()], false).is_empty());
    assert!(claude.calls().is_empty());
}
