use super::claude::NOT_FOUND;
use super::config::quote;
use super::link::{describe, link, linked_taps, refresh, unlink};
use super::marketplace;
use super::tests::Fx;
use super::ClaudeRunner;
use crate::plus::op::ErrorKind;
use crate::plus::update::exec::CmdOutput;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const TAP: &str = "agent-skills";

struct Recorder {
    calls: Mutex<Vec<String>>,
    list: String,
    fail_on: Option<&'static str>,
    missing: bool,
}

impl Recorder {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            list: "[]".into(),
            fail_on: None,
            missing: false,
        }
    }

    fn listing(mut self, list: Value) -> Self {
        self.list = list.to_string();
        self
    }

    fn failing(mut self, prefix: &'static str) -> Self {
        self.fail_on = Some(prefix);
        self
    }

    fn without_claude(mut self) -> Self {
        self.missing = true;
        self
    }

    fn all(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn writes(&self) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|call| call != "plugin list --json")
            .collect()
    }
}

impl ClaudeRunner for Recorder {
    fn run(&self, args: &[&str], _timeout: Duration) -> Result<CmdOutput, String> {
        let line = args.join(" ");
        self.calls.lock().unwrap().push(line.clone());
        let out = |code: i32, stdout: &str, stderr: &str| {
            Ok(CmdOutput {
                code,
                stdout: stdout.into(),
                stderr: stderr.into(),
            })
        };
        if line == "plugin list --json" {
            return out(0, &self.list, "");
        }
        if self.fail_on.is_some_and(|prefix| line.starts_with(prefix)) {
            return out(1, "", "boom");
        }
        out(0, "done", "")
    }

    fn present(&self) -> bool {
        !self.missing
    }
}

fn marketplace_tap(fx: &Fx) -> PathBuf {
    fx.put_json(
        "data/taps.json",
        json!({TAP: {"repo": "acme/agent-skills", "url": "https://github.com/acme/agent-skills.git"}}),
    );
    let root = format!("data/taps/{TAP}");
    fx.put_json(
        &format!("{root}/.claude-plugin/marketplace.json"),
        json!({
            "name": "acme",
            "plugins": [
                {"name": "core", "source": "./plugins/core", "description": "Stable"},
                {"name": "labs", "source": "./plugins/labs"},
            ],
        }),
    );
    for (plugin, skill) in [("core", "task"), ("labs", "estimate")] {
        fx.put_json(
            &format!("{root}/plugins/{plugin}/.claude-plugin/plugin.json"),
            json!({"name": plugin, "version": "2026.10.14.0"}),
        );
        fx.put(
            &format!("{root}/plugins/{plugin}/skills/{skill}/SKILL.md"),
            &format!("---\nname: {skill}\ndescription: d\n---\n"),
        );
    }
    fx.put(&format!("{root}/plugins/labs/agents/helper.md"), "---\nname: helper\n---\n");
    fx.put(&format!("{root}/plugins/labs/hooks/hooks.json"), "{}");
    fx.path(&root)
}

fn declare(fx: &Fx, source: Value) {
    fx.put_json(
        "home/.claude/plugins/known_marketplaces.json",
        json!({"acme": {"source": source, "lastUpdated": "2026-10-19T07:32:46.936Z"}}),
    );
}

fn declare_here(fx: &Fx, clone: &Path) {
    declare(fx, json!({"source": "directory", "path": clone}));
}

fn install(fx: &Fx, ids: &[&str]) {
    let plugins: serde_json::Map<String, Value> = ids
        .iter()
        .map(|id| {
            (
                id.to_string(),
                json!([{"scope": "user", "version": "2026.10.14.0", "installPath": "/cache"}]),
            )
        })
        .collect();
    fx.put_json(
        "home/.claude/plugins/installed_plugins.json",
        json!({"version": 2, "plugins": plugins}),
    );
}

fn exec_steps(out: &Value) -> Vec<String> {
    out["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["op"] == "exec")
        .map(|s| s["detail"].as_str().unwrap().to_string())
        .collect()
}

fn clone_arg(clone: &Path) -> String {
    clone.to_string_lossy().into_owned()
}

#[test]
fn marketplace_lists_plugins_with_versions_and_components() {
    let fx = Fx::new("mkt-read");
    let clone = marketplace_tap(&fx);
    let market = marketplace::read(&clone).unwrap().unwrap();
    assert_eq!(market.name, "acme");
    assert!(market.warnings.is_empty(), "{:?}", market.warnings);
    let names: Vec<&str> = market.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["core", "labs"]);
    let core = market.plugin("core").unwrap();
    assert_eq!(core.version.as_deref(), Some("2026.10.14.0"));
    assert_eq!(core.description.as_deref(), Some("Stable"));
    assert_eq!(core.skills, ["task"]);
    assert!(!core.hooks);
    let labs = market.plugin("labs").unwrap();
    assert_eq!(labs.agents, ["helper"]);
    assert!(labs.hooks);
    assert_eq!(market.id_of("core"), "core@acme");
}

#[test]
fn marketplace_skips_remote_escaping_and_repeated_entries() {
    let fx = Fx::new("mkt-skip");
    let clone = marketplace_tap(&fx);
    fx.put_json(
        &format!("data/taps/{TAP}/.claude-plugin/marketplace.json"),
        json!({
            "name": "acme",
            "plugins": [
                {"name": "core", "source": "./plugins/core"},
                {"name": "core", "source": "./plugins/labs"},
                {"name": "remote", "source": {"source": "github", "repo": "a/b"}},
                {"name": "escape", "source": "./../outside"},
                {"name": "bare", "source": "plugins/core"},
                {"name": "bad@name", "source": "./plugins/core"},
            ],
        }),
    );
    let market = marketplace::read(&clone).unwrap().unwrap();
    let names: Vec<&str> = market.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["core"]);
    assert_eq!(market.warnings.len(), 5, "{:?}", market.warnings);
}

#[test]
fn marketplace_needs_a_usable_name_and_valid_json() {
    let fx = Fx::new("mkt-bad");
    let clone = marketplace_tap(&fx);
    let file = format!("data/taps/{TAP}/.claude-plugin/marketplace.json");
    fx.put_json(&file, json!({"name": "a@b", "plugins": []}));
    assert!(marketplace::read(&clone).unwrap().is_err());
    fx.put(&file, "not json");
    assert!(marketplace::read(&clone).unwrap().is_err());
    assert!(marketplace::read(&fx.path("data")).is_none());
}

#[test]
fn names_that_could_pass_as_ids_or_flags_are_not_valid() {
    for bad in ["a@b", "-flag", "..", ".hidden", "", "with space"] {
        assert!(!marketplace::valid_name(bad), "{bad:?}");
    }
    assert!(marketplace::valid_name("cf-labs"));
}

#[test]
fn link_dry_run_plans_every_step_and_runs_nothing() {
    let fx = Fx::new("link-dry");
    let clone = marketplace_tap(&fx);
    let runner = Recorder::new();
    let out = link(&fx.env(), &runner, TAP, &[], true).unwrap();
    assert_eq!(out["dryRun"], true);
    assert!(out["result"].is_null());
    assert_eq!(
        exec_steps(&out),
        [
            format!("claude plugin marketplace add {} --scope user", quote(&clone_arg(&clone))),
            "claude plugin install core@acme --scope user".to_string(),
            "claude plugin install labs@acme --scope user".to_string(),
        ]
    );
    assert!(runner.writes().is_empty(), "{:?}", runner.writes());
    assert_eq!(out["marketplace"], "acme");
    assert_eq!(out["plugins"][0]["linked"], false);
}

#[test]
fn link_declares_the_clone_then_installs_each_plugin() {
    let fx = Fx::new("link-apply");
    let clone = marketplace_tap(&fx);
    let runner = Recorder::new();
    let out = link(&fx.env(), &runner, TAP, &[], false).unwrap();
    assert_eq!(
        runner.writes(),
        [
            format!("plugin marketplace add {} --scope user", clone_arg(&clone)),
            "plugin install core@acme --scope user".to_string(),
            "plugin install labs@acme --scope user".to_string(),
        ]
    );
    assert_eq!(out["result"]["applied"], true);
    assert_eq!(out["result"]["undo"], format!("toolportctl skills tap unlink {TAP}"));
    assert_eq!(out["changes"][1]["action"], "install");
}

#[test]
fn link_installs_only_the_named_plugins() {
    let fx = Fx::new("link-only");
    marketplace_tap(&fx);
    let runner = Recorder::new();
    link(&fx.env(), &runner, TAP, &["core".to_string()], false).unwrap();
    let writes = runner.writes();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert_eq!(writes[1], "plugin install core@acme --scope user");

    let unknown = link(&fx.env(), &runner, TAP, &["nope".to_string()], true).unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::Usage);
    assert!(unknown.message.contains("core, labs"), "{}", unknown.message);
}

#[test]
fn link_does_nothing_when_it_is_already_linked() {
    let fx = Fx::new("link-again");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    install(&fx, &["core@acme", "labs@acme"]);
    let runner = Recorder::new();
    let out = link(&fx.env(), &runner, TAP, &[], false).unwrap();
    assert!(runner.writes().is_empty(), "{:?}", runner.writes());
    assert!(exec_steps(&out).is_empty());
    assert_eq!(out["plugins"][0]["linked"], true);
}

#[test]
fn link_installs_the_missing_plugin_of_a_declared_marketplace() {
    let fx = Fx::new("link-partial");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    install(&fx, &["core@acme"]);
    let runner = Recorder::new();
    link(&fx.env(), &runner, TAP, &[], false).unwrap();
    assert_eq!(runner.writes(), ["plugin install labs@acme --scope user"]);
}

#[test]
fn link_refuses_a_marketplace_name_declared_from_elsewhere() {
    let fx = Fx::new("link-clash");
    marketplace_tap(&fx);
    declare(&fx, json!({"source": "github", "repo": "other/place"}));
    let runner = Recorder::new();
    let error = link(&fx.env(), &runner, TAP, &[], false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert!(
        error.message.contains("already declared from github:other/place"),
        "{}",
        error.message
    );
    assert!(runner.writes().is_empty());
}

#[test]
fn link_warns_that_the_clone_replaces_a_synced_copy_and_that_hooks_run() {
    let fx = Fx::new("link-warn");
    marketplace_tap(&fx);
    let runner = Recorder::new().listing(json!([
        {"id": "core@synced", "version": "2026.10.14.0", "scope": "user", "enabled": true},
        {"id": "other@synced", "version": "1", "scope": "user", "enabled": true},
    ]));
    let out = link(&fx.env(), &runner, TAP, &[], true).unwrap();
    let text = out["plan"]["warnings"].to_string();
    assert!(text.contains("core@synced is the same plugin as core@acme"), "{text}");
    assert!(!text.contains("other@synced"), "{text}");
    assert!(text.contains("labs ship hooks"), "{text}");
    assert!(text.contains("sessions that are already running"), "{text}");
}

#[test]
fn link_names_the_undo_when_a_later_step_fails() {
    let fx = Fx::new("link-fail");
    marketplace_tap(&fx);
    let runner = Recorder::new().failing("plugin install labs");
    let error = link(&fx.env(), &runner, TAP, &[], false).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert!(error.message.starts_with("boom"), "{}", error.message);
    assert!(error.message.contains("2 of 3 steps ran"), "{}", error.message);
    assert!(
        error.message.contains("toolportctl skills tap unlink agent-skills"),
        "{}",
        error.message
    );
}

#[test]
fn link_needs_claude_a_registered_tap_and_a_marketplace() {
    let fx = Fx::new("link-pre");
    marketplace_tap(&fx);
    let missing = link(&fx.env(), &Recorder::new().without_claude(), TAP, &[], false).unwrap_err();
    assert!(missing.message.contains(NOT_FOUND), "{}", missing.message);

    let unknown = link(&fx.env(), &Recorder::new(), "nope", &[], true).unwrap_err();
    assert_eq!(unknown.kind, ErrorKind::NotFound);

    std::fs::remove_file(fx.path(&format!("data/taps/{TAP}/.claude-plugin/marketplace.json"))).unwrap();
    let plain = link(&fx.env(), &Recorder::new(), TAP, &[], true).unwrap_err();
    assert_eq!(plain.kind, ErrorKind::Usage);
    assert!(plain.message.contains("not a plugin marketplace"), "{}", plain.message);
}

#[test]
fn only_an_apply_that_has_something_to_run_needs_claude() {
    let fx = Fx::new("no-claude");
    let clone = marketplace_tap(&fx);
    let runner = Recorder::new().without_claude();
    let planned = link(&fx.env(), &runner, TAP, &[], true).unwrap();
    assert_eq!(exec_steps(&planned).len(), 3);
    let nothing = unlink(&fx.env(), &runner, TAP, false).unwrap();
    assert!(exec_steps(&nothing).is_empty());
    assert!(runner.writes().is_empty());

    declare_here(&fx, &clone);
    let error = unlink(&fx.env(), &runner, TAP, false).unwrap_err();
    assert!(error.message.contains(NOT_FOUND), "{}", error.message);
}

#[test]
fn unlink_uninstalls_the_plugins_before_it_removes_the_marketplace() {
    let fx = Fx::new("unlink");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    install(&fx, &["core@acme", "labs@acme", "x@elsewhere"]);
    let runner = Recorder::new();
    let out = unlink(&fx.env(), &runner, TAP, false).unwrap();
    assert_eq!(
        runner.writes(),
        [
            "plugin uninstall core@acme --scope user",
            "plugin uninstall labs@acme --scope user",
            "plugin marketplace remove acme",
        ]
    );
    assert_eq!(out["result"]["undo"], format!("toolportctl skills tap link {TAP}"));
    assert_eq!(out["marketplaces"][0], "acme");
}

#[test]
fn unlink_leaves_a_marketplace_declared_from_elsewhere_alone() {
    let fx = Fx::new("unlink-foreign");
    marketplace_tap(&fx);
    declare(&fx, json!({"source": "github", "repo": "other/place"}));
    install(&fx, &["core@acme"]);
    let runner = Recorder::new();
    let out = unlink(&fx.env(), &runner, TAP, false).unwrap();
    assert!(runner.writes().is_empty());
    assert!(exec_steps(&out).is_empty());
}

#[test]
fn unlink_still_works_after_the_clone_is_gone() {
    let fx = Fx::new("unlink-gone");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    std::fs::remove_dir_all(&clone).unwrap();
    let runner = Recorder::new();
    unlink(&fx.env(), &runner, TAP, false).unwrap();
    assert_eq!(runner.writes(), ["plugin marketplace remove acme"]);
}

#[test]
fn unlink_dry_run_runs_nothing() {
    let fx = Fx::new("unlink-dry");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    let runner = Recorder::new();
    let out = unlink(&fx.env(), &runner, TAP, true).unwrap();
    assert!(runner.writes().is_empty());
    assert!(out["result"].is_null());
    assert_eq!(exec_steps(&out), ["claude plugin marketplace remove acme"]);
}

fn drifted_listing() -> Value {
    json!([
        {"id": "core@acme", "version": "2026.10.14.0", "folderVersion": "2026.10.19.0", "scope": "user", "enabled": true},
        {"id": "labs@acme", "version": "2026.10.14.0", "folderVersion": "2026.10.14.0", "scope": "user", "enabled": true},
        {"id": "ecc@ecc", "version": "2.2.3", "folderVersion": "2.3.0", "scope": "user", "enabled": true},
    ])
}

#[test]
fn refresh_updates_only_linked_plugins_whose_clone_moved_on() {
    let fx = Fx::new("refresh");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    let runner = Recorder::new().listing(drifted_listing());
    let rows = refresh(&fx.env(), &runner, &[TAP.to_string()], false);
    assert_eq!(runner.writes(), ["plugin update core@acme --scope user"]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "core@acme");
    assert_eq!(rows[0]["from"], "2026.10.14.0");
    assert_eq!(rows[0]["to"], "2026.10.19.0");
    assert_eq!(rows[0]["outcome"], "updated");
}

#[test]
fn refresh_dry_run_reports_without_updating() {
    let fx = Fx::new("refresh-dry");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    let runner = Recorder::new().listing(drifted_listing());
    let rows = refresh(&fx.env(), &runner, &[TAP.to_string()], true);
    assert!(runner.writes().is_empty());
    assert_eq!(rows[0]["outcome"], "would update");
}

#[test]
fn refresh_reports_a_failed_update_as_a_row() {
    let fx = Fx::new("refresh-fail");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    let runner = Recorder::new().listing(drifted_listing()).failing("plugin update");
    let rows = refresh(&fx.env(), &runner, &[TAP.to_string()], false);
    assert_eq!(rows[0]["error"], "boom");
    assert!(rows[0].get("outcome").is_none());
}

#[test]
fn refresh_never_starts_claude_for_a_tap_that_is_not_linked() {
    let fx = Fx::new("refresh-unlinked");
    marketplace_tap(&fx);
    let runner = Recorder::new().listing(drifted_listing());
    let rows = refresh(&fx.env(), &runner, &[TAP.to_string(), "unknown".to_string()], false);
    assert!(rows.is_empty());
    assert!(runner.all().is_empty(), "{:?}", runner.all());
}

#[test]
fn refresh_turns_a_failed_listing_into_a_row_per_linked_tap() {
    let fx = Fx::new("refresh-nolist");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    let runner = Recorder::new().listing(json!("not a list"));
    let rows = refresh(&fx.env(), &runner, &[TAP.to_string()], false);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["tap"], TAP);
    assert!(rows[0]["error"].as_str().unwrap().contains("plugin list"), "{}", rows[0]);
}

#[test]
fn linked_taps_names_only_taps_declared_from_their_own_clone() {
    let fx = Fx::new("linked");
    let clone = marketplace_tap(&fx);
    let names = vec![TAP.to_string(), "unknown".to_string()];
    assert!(linked_taps(&fx.env(), &names).is_empty());
    declare_here(&fx, &clone);
    assert_eq!(linked_taps(&fx.env(), &names), [TAP]);
}

#[test]
fn describe_shows_the_state_of_each_plugin() {
    let fx = Fx::new("describe");
    let clone = marketplace_tap(&fx);
    declare_here(&fx, &clone);
    install(&fx, &["core@acme"]);
    let shown = describe(&fx.env(), TAP).unwrap();
    assert_eq!(shown["name"], "acme");
    assert_eq!(shown["declared"], "here");
    assert_eq!(shown["plugins"][0]["linked"], true);
    assert_eq!(shown["plugins"][1]["linked"], false);
    assert_eq!(shown["plugins"][1]["hooks"], true);

    declare(&fx, json!({"source": "github", "repo": "other/place"}));
    assert_eq!(describe(&fx.env(), TAP).unwrap()["declared"], "elsewhere: github:other/place");

    std::fs::remove_file(clone.join(".claude-plugin/marketplace.json")).unwrap();
    assert!(describe(&fx.env(), TAP).is_none());
}
