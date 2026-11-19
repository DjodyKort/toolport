use super::world::{world, World};
use crate::context::presync::{self, Opts, OrgCommand, OrgTool};
use serde_json::Value;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Noop;

impl OrgTool for Noop {
    fn pull(&self, _: &Path, _: &OrgCommand) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct Recording {
    pulled: RefCell<Vec<PathBuf>>,
    commands: RefCell<Vec<OrgCommand>>,
    fail: Option<&'static str>,
}

impl OrgTool for Recording {
    fn pull(&self, dir: &Path, command: &OrgCommand) -> Result<(), String> {
        self.pulled.borrow_mut().push(dir.to_path_buf());
        self.commands.borrow_mut().push(command.clone());
        self.fail.map_or(Ok(()), |why| Err(why.to_string()))
    }
}

fn install_org_tool(w: &World) {
    w.write(".local/share/corp-dev-tools/.git/HEAD", "ref\n");
    w.write(".local/share/corp-dev-tools/.venv/bin/python", "");
}

fn record(w: &World) -> Value {
    serde_json::from_str(&fs::read_to_string(w.data.join("plus/context-presync.json")).unwrap())
        .unwrap()
}

const INTERVAL: i64 = 14_400;

#[test]
fn test01_the_first_run_syncs_and_records_when_and_how_long() {
    let w = world("presync-first");
    let out = presync::run(&w.host, &Noop, Opts::default()).unwrap();
    assert_eq!(out["ran"], true);
    assert_eq!(out["ok"], true);
    assert_eq!(out["orgPulled"], false);
    let rec = record(&w);
    assert_eq!(rec["ok"], true);
    assert_eq!(rec["lastAttemptAt"], 1_800_000_000i64);
    assert_eq!(rec["lastOkAt"], 1_800_000_000i64);
}

#[test]
fn test02_inside_the_interval_nothing_runs_and_nothing_is_written() {
    let w = world("presync-throttle");
    presync::run(&w.host, &Noop, Opts::default()).unwrap();
    let before = fs::read(w.data.join("plus/context-presync.json")).unwrap();
    w.host.now.set(1_800_000_000 + INTERVAL - 1);
    let out = presync::run(&w.host, &Noop, Opts::default()).unwrap();
    assert_eq!(out["ran"], false);
    assert_eq!(out["nextAfter"], 1_800_000_000i64 + INTERVAL);
    assert_eq!(
        fs::read(w.data.join("plus/context-presync.json")).unwrap(),
        before
    );
    w.host.now.set(1_800_000_000 + INTERVAL);
    assert_eq!(
        presync::run(&w.host, &Noop, Opts::default()).unwrap()["ran"],
        true
    );
}

#[test]
fn test03_force_runs_inside_the_interval() {
    let w = world("presync-force");
    presync::run(&w.host, &Noop, Opts::default()).unwrap();
    w.host.now.set(1_800_000_060);
    let out = presync::run(&w.host, &Noop, Opts { force: true }).unwrap();
    assert_eq!(out["ran"], true);
    assert_eq!(record(&w)["lastOkAt"], 1_800_000_060i64);
}

#[test]
fn test04_the_org_tool_is_pulled_once_per_interval_and_only_when_it_is_installed() {
    let w = world("presync-org");
    let tool = Recording::default();
    presync::run(&w.host, &tool, Opts::default()).unwrap();
    assert!(tool.pulled.borrow().is_empty(), "not installed: no pull");

    install_org_tool(&w);
    w.host.now.set(1_800_000_100);
    let out = presync::run(&w.host, &tool, Opts { force: true }).unwrap();
    assert_eq!(out["orgPulled"], true);
    assert_eq!(tool.pulled.borrow().len(), 1);
    assert_eq!(w.read(".claude/.last_auto_sync").trim(), "1800000100");

    w.host.now.set(1_800_000_200);
    presync::run(&w.host, &tool, Opts { force: true }).unwrap();
    assert_eq!(
        tool.pulled.borrow().len(),
        1,
        "the org tool's own stamp throttles the pull"
    );

    w.host.now.set(1_800_000_100 + INTERVAL);
    presync::run(&w.host, &tool, Opts::default()).unwrap();
    assert_eq!(tool.pulled.borrow().len(), 2);
}

#[test]
fn test05_a_pull_that_fails_is_a_failed_presync_with_the_output_and_the_last_good_run_kept() {
    let w = world("presync-pullfail");
    presync::run(&w.host, &Noop, Opts::default()).unwrap();
    install_org_tool(&w);
    w.host.now.set(1_800_000_000 + INTERVAL + 5);
    let tool = Recording {
        fail: Some("could not reach the remote"),
        ..Default::default()
    };
    let err = presync::run(&w.host, &tool, Opts::default()).unwrap_err();
    assert_eq!(err.kind.code(), "presync_failed");
    let rec = record(&w);
    assert_eq!(rec["ok"], false);
    assert_eq!(rec["lastOkAt"], 1_800_000_000i64);
    assert_eq!(rec["output"], "could not reach the remote");
    assert!(rec["warnings"][0]
        .as_str()
        .unwrap()
        .contains("could not be pulled"));
}

#[test]
fn test06_a_failing_part_of_the_sync_fails_the_presync_and_the_next_launch_tries_again() {
    let w = world("presync-syncfail");
    w.clients_root("clients");
    w.layer(
        "client-a",
        "scope: folder\nfolders: [\"~/clients/a\"]\nimports: [\"~/missing.md\"]",
        "text",
    );
    fs::create_dir_all(w.home.join("clients/a")).unwrap();
    let err = presync::run(&w.host, &Noop, Opts::default()).unwrap_err();
    assert_eq!(err.kind.code(), "presync_failed");
    assert!(err.message.contains("client-a"), "{}", err.message);
    assert_eq!(record(&w)["ok"], false);
    assert_eq!(record(&w).get("lastOkAt"), None);
    w.host.now.set(1_800_000_001);
    let again = presync::run(&w.host, &Noop, Opts::default());
    assert!(
        again.is_err(),
        "a presync that never succeeded is due at every launch"
    );
}

#[test]
fn test07_the_failure_text_is_scrubbed_and_cut() {
    let w = world("presync-scrub");
    install_org_tool(&w);
    let long = "x".repeat(10_000);
    let tool = Recording {
        fail: Some(Box::leak(long.into_boxed_str())),
        ..Default::default()
    };
    let _ = presync::run(&w.host, &tool, Opts::default());
    assert!(record(&w)["output"].as_str().unwrap().len() <= 4096);
}

fn set_context_json(w: &World, text: &str) {
    w.write(".config/mcpm/context.json", text);
}

fn org_command(w: &World) -> Result<OrgCommand, String> {
    let config = crate::sources::config::load(&w.home.join(".config/mcpm/context.json"));
    OrgCommand::from_config(&config)
}

#[test]
fn test08_without_a_setting_the_built_in_pull_is_the_command() {
    let w = world("presync-default-command");
    assert_eq!(org_command(&w), Ok(OrgCommand::BuiltIn));
    set_context_json(&w, "{\"orgPull\": {}}\n");
    assert_eq!(org_command(&w), Ok(OrgCommand::BuiltIn));

    install_org_tool(&w);
    let tool = Recording::default();
    presync::run(&w.host, &tool, Opts::default()).unwrap();
    assert_eq!(*tool.commands.borrow(), vec![OrgCommand::BuiltIn]);
}

#[test]
fn test09_a_configured_command_is_read_as_an_argv_and_handed_to_the_pull() {
    let w = world("presync-configured-command");
    set_context_json(
        &w,
        "{\"orgPull\": {\"command\": [\"org-pull\", \"--quiet\", \"two words\"]}}\n",
    );
    let want = OrgCommand::Argv(vec![
        "org-pull".into(),
        "--quiet".into(),
        "two words".into(),
    ]);
    assert_eq!(org_command(&w), Ok(want.clone()));

    w.write(".local/share/corp-dev-tools/README", "x\n");
    let tool = Recording::default();
    let out = presync::run(&w.host, &tool, Opts::default()).unwrap();
    assert_eq!(
        out["orgPulled"], true,
        "a configured command needs only the folder"
    );
    assert_eq!(*tool.commands.borrow(), vec![want]);
}

#[test]
fn test10_the_interval_comes_from_the_setting_before_the_environment_default() {
    let w = world("presync-interval-setting");
    set_context_json(&w, "{\"orgPull\": {\"intervalSecs\": 600}}\n");
    let config = crate::sources::config::load(&w.home.join(".config/mcpm/context.json"));
    assert_eq!(presync::interval_secs(&config), 600);
    presync::run(&w.host, &Noop, Opts::default()).unwrap();
    w.host.now.set(1_800_000_000 + 599);
    assert_eq!(
        presync::run(&w.host, &Noop, Opts::default()).unwrap()["ran"],
        false
    );
    w.host.now.set(1_800_000_000 + 600);
    assert_eq!(
        presync::run(&w.host, &Noop, Opts::default()).unwrap()["ran"],
        true
    );
}

#[test]
fn test11_an_unusable_setting_names_the_problem_and_the_pull_is_not_attempted() {
    let cases = [
        ("[\"x\"]", "must be an object"),
        ("{\"command\": \"git pull\"}", "not a shell string"),
        ("{\"command\": []}", "command is empty"),
        ("{\"command\": [\"\"]}", "first item of command is empty"),
        ("{\"command\": [\"git\", 3]}", "only strings"),
        ("{\"command\": 7}", "must be a list"),
        (
            "{\"command\": [\"/bin/sh\", \"-c\", \"git pull\"]}",
            "not a shell",
        ),
        ("{\"command\": [\"BASH\", \"-c\", \"x\"]}", "not a shell"),
        ("{\"command\": [\"git pull --quiet\"]}", "holds a space"),
        ("{\"intervalSecs\": 0}", "intervalSecs must be"),
        ("{\"intervalSecs\": \"4h\"}", "intervalSecs must be"),
    ];
    for (i, (setting, expect)) in cases.iter().enumerate() {
        let w = world(&format!("presync-bad-setting-{i}"));
        set_context_json(&w, &format!("{{\"orgPull\": {setting}}}\n"));
        let why = org_command(&w).unwrap_err();
        assert!(why.starts_with("orgPull in context.json: "), "{why}");
        assert!(why.contains(expect), "{setting}: {why}");

        install_org_tool(&w);
        let tool = Recording::default();
        let err = presync::run(&w.host, &tool, Opts::default()).unwrap_err();
        assert_eq!(err.kind.code(), "presync_failed", "{setting}");
        assert!(tool.pulled.borrow().is_empty(), "{setting}");
        assert!(record(&w)["output"].as_str().unwrap().contains(expect));
        assert!(
            !w.exists(".claude/.last_auto_sync"),
            "no stamp: tried again"
        );
    }
}

#[test]
fn test12_an_absolute_program_path_with_a_space_is_a_program_not_a_shell_string() {
    let w = world("presync-spaced-path");
    set_context_json(
        &w,
        "{\"orgPull\": {\"command\": [\"/Users/me/My Tools/pull\"]}}\n",
    );
    assert_eq!(
        org_command(&w),
        Ok(OrgCommand::Argv(vec!["/Users/me/My Tools/pull".into()]))
    );
}

#[test]
fn test13_a_bad_setting_fails_the_presync_even_when_the_org_tool_is_not_installed() {
    let w = world("presync-bad-setting-sync");
    set_context_json(&w, "{\"orgPull\": {\"command\": []}}\n");
    let err = presync::run(&w.host, &Noop, Opts::default()).unwrap_err();
    assert_eq!(err.kind.code(), "presync_failed");
    assert!(
        err.message.contains("org file was not pulled"),
        "{}",
        err.message
    );
    assert_eq!(record(&w)["ok"], false);
}

#[cfg(unix)]
#[test]
fn test14_the_configured_argv_runs_as_given_in_the_org_folder_without_a_shell() {
    let w = world("presync-fake-binary");
    let bin = w.home.join("bin/fake-pull");
    let log = w.home.join("pull.log");
    super::world::script(
        &bin,
        &format!(
            "pwd > '{log}'\nfor a in \"$@\"; do printf '[%s]\\n' \"$a\" >> '{log}'; done",
            log = log.display()
        ),
    );
    let tricky = "a b; touch pwned $(id)";
    set_context_json(
        &w,
        &serde_json::json!({
            "orgPull": { "command": [bin.to_string_lossy(), "--flag", tricky] }
        })
        .to_string(),
    );
    w.write(".local/share/corp-dev-tools/README", "x\n");

    let out = presync::run(&w.host, &presync::ProcessOrgTool, Opts::default()).unwrap();
    assert_eq!(out["orgPulled"], true);
    let said = fs::read_to_string(&log).unwrap();
    let dir = crate::context::fsx::canonical(&w.home.join(".local/share/corp-dev-tools"));
    let mut lines = said.lines();
    assert_eq!(
        crate::context::fsx::canonical(Path::new(lines.next().unwrap())),
        dir
    );
    assert_eq!(
        lines.collect::<Vec<_>>(),
        vec!["[--flag]".to_string(), format!("[{tricky}]")]
    );
    assert!(!dir.join("pwned").exists());
    assert_eq!(w.read(".claude/.last_auto_sync").trim(), "1800000000");
}

#[cfg(unix)]
#[test]
fn test15_a_configured_command_that_fails_reports_its_own_output() {
    let w = world("presync-fake-failing");
    let bin = w.home.join("bin/failing-pull");
    super::world::script(&bin, "echo 'remote said no' >&2\nexit 3");
    set_context_json(
        &w,
        &serde_json::json!({ "orgPull": { "command": [bin.to_string_lossy()] } }).to_string(),
    );
    w.write(".local/share/corp-dev-tools/README", "x\n");
    let err = presync::run(&w.host, &presync::ProcessOrgTool, Opts::default()).unwrap_err();
    assert_eq!(err.kind.code(), "presync_failed");
    let output = record(&w)["output"].as_str().unwrap().to_string();
    assert!(output.contains("failing-pull exited with"), "{output}");
    assert!(output.contains("remote said no"), "{output}");

    set_context_json(
        &w,
        "{\"orgPull\": {\"command\": [\"/nonexistent/org-pull\"]}}\n",
    );
    w.host.now.set(1_800_000_000 + INTERVAL + 1);
    presync::run(&w.host, &presync::ProcessOrgTool, Opts::default()).unwrap_err();
    assert!(record(&w)["output"]
        .as_str()
        .unwrap()
        .contains("/nonexistent/org-pull"));
}
