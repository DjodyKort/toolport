use super::world::{world, World};
use crate::context::presync::{self, Opts, OrgTool};
use serde_json::Value;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Noop;

impl OrgTool for Noop {
    fn pull(&self, _: &Path) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct Recording {
    pulled: RefCell<Vec<PathBuf>>,
    fail: Option<&'static str>,
}

impl OrgTool for Recording {
    fn pull(&self, dir: &Path) -> Result<(), String> {
        self.pulled.borrow_mut().push(dir.to_path_buf());
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
