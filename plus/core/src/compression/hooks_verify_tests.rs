#![cfg(unix)]

use super::hooks::{apply, read_ledger, set_paused, summarize};
use super::hooks_testkit::{script, settings_with, world, World};
use super::hooks_verify::{verify, Options};
use super::ledger::now_ms;
use std::time::Duration;

const REWRITE: &str = r#"cat >/dev/null; echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"command":"rtk git status"}}}'"#;
const BLOCK_REPEAT: &str =
    r#"cat >/dev/null; if [ -e seen ]; then echo '{"decision":"block"}'; fi; touch seen"#;

fn with_hooks(rtk_body: &str, read_once_body: &str) -> World {
    let w = world(settings_with(&[]));
    let rtk = script(&w.dir, "rtk-hook.sh", rtk_body);
    let once = script(&w.dir, "read-once-hook.sh", read_once_body);
    let settings = settings_with(&[
        ("Bash", rtk.to_str().unwrap()),
        ("Read", once.to_str().unwrap()),
    ]);
    std::fs::write(&w.file, settings.to_string()).unwrap();
    w
}

fn row<'a>(data: &'a serde_json::Value, provider: &str) -> &'a serde_json::Value {
    data["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["provider"] == provider)
        .unwrap()
}

#[test]
fn test01_both_hooks_pass_with_a_rewrite_and_a_blocked_repeat_read() {
    let w = with_hooks(REWRITE, BLOCK_REPEAT);
    let data = verify(&w.host, &w.paths, None, &Options::default()).unwrap();
    assert_eq!(
        (data["passed"].clone(), data["failed"].clone()),
        (2.into(), 0.into())
    );
    let rtk = row(&data, "rtk");
    assert_eq!(rtk["status"], "pass");
    assert_eq!(rtk["returned"], "rewrote `git status` to `rtk git status`");
    assert_eq!(rtk["calls"][0]["exitCode"], 0);
    assert!(rtk["latencyMs"].as_u64().is_some());
    let once = row(&data, "read-once");
    assert_eq!(once["status"], "pass");
    assert_eq!(once["calls"].as_array().unwrap().len(), 2);
    assert_eq!(once["calls"][0]["answered"], false);
    assert_eq!(once["calls"][1]["answered"], true);
}

#[test]
fn test02_a_hook_that_answers_nothing_or_no_rewrite_fails_with_the_reason() {
    let silent = with_hooks("cat >/dev/null", "cat >/dev/null");
    let data = verify(&silent.host, &silent.paths, None, &Options::default()).unwrap();
    assert_eq!(data["failed"], 2);
    assert!(row(&data, "rtk")["reason"]
        .as_str()
        .unwrap()
        .contains("answered nothing"));
    assert!(row(&data, "read-once")["reason"]
        .as_str()
        .unwrap()
        .contains("repeat read"));
    let same = with_hooks(
        r#"cat >/dev/null; echo '{"hookSpecificOutput":{"updatedInput":{"command":"git status"}}}'"#,
        BLOCK_REPEAT,
    );
    let data = verify(&same.host, &same.paths, Some("rtk"), &Options::default()).unwrap();
    assert!(row(&data, "rtk")["reason"]
        .as_str()
        .unwrap()
        .contains("not with a rewritten command"));
}

#[test]
fn test03_one_hook_can_be_verified_alone_and_a_missing_entry_is_skipped() {
    let w = world(settings_with(&[("Write", "echo unrelated")]));
    let data = verify(&w.host, &w.paths, Some("rtk"), &Options::default()).unwrap();
    assert_eq!(data["rows"].as_array().unwrap().len(), 1);
    assert_eq!(row(&data, "rtk")["status"], "skipped");
    assert_eq!(
        (
            data["passed"].clone(),
            data["failed"].clone(),
            data["skipped"].clone()
        ),
        (0.into(), 0.into(), 1.into())
    );
}

#[test]
fn test04_a_hook_that_does_not_finish_in_time_is_stopped_and_fails() {
    let w = with_hooks("exec sleep 5", BLOCK_REPEAT);
    let options = Options {
        timeout: Duration::from_millis(200),
        path_prepend: None,
    };
    let started = std::time::Instant::now();
    let data = verify(&w.host, &w.paths, Some("rtk"), &options).unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(row(&data, "rtk")["reason"]
        .as_str()
        .unwrap()
        .contains("did not finish"));
}

#[test]
fn test05_a_command_the_shell_cannot_find_fails_as_not_found() {
    let w = world(settings_with(&[("Bash", "/no/such/rtk-hook")]));
    let data = verify(&w.host, &w.paths, Some("rtk"), &Options::default()).unwrap();
    let rtk = row(&data, "rtk");
    assert_eq!(rtk["status"], "fail");
    assert!(
        rtk["reason"]
            .as_str()
            .unwrap()
            .contains("did not find a command"),
        "{rtk}"
    );
}

#[test]
fn test06_a_wrapped_entry_runs_through_its_wrapper_and_counts_nothing() {
    let w = with_hooks(REWRITE, BLOCK_REPEAT);
    apply(&w.host, &w.paths, None, false).unwrap();
    let bin = w.dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin, "toolportctl", r#"shift 4; exec "$@""#);
    let options = Options {
        path_prepend: Some(bin),
        ..Options::default()
    };
    set_paused(&w.paths, true, None, now_ms()).unwrap();
    let data = verify(&w.host, &w.paths, None, &options).unwrap();
    let rtk = row(&data, "rtk");
    assert_eq!(
        (
            rtk["status"].clone(),
            rtk["wrapped"].clone(),
            rtk["paused"].clone()
        ),
        ("pass".into(), true.into(), true.into())
    );
    assert_eq!(data["paused"], true);
    assert_eq!(summarize(&w.paths, "rtk").calls, 0);
    assert_eq!(read_ledger(&w.paths).unwrap().entries.len(), 2);
}

#[test]
fn test07_the_hook_runs_in_a_temporary_directory_that_is_removed_afterwards() {
    let w = world(settings_with(&[]));
    let record = w.dir.join("where");
    let body = format!("cat >/dev/null; pwd > '{}'", record.display());
    let rtk = script(&w.dir, "rtk-hook.sh", &body);
    std::fs::write(
        &w.file,
        settings_with(&[("Bash", rtk.to_str().unwrap())]).to_string(),
    )
    .unwrap();
    verify(&w.host, &w.paths, Some("rtk"), &Options::default()).unwrap();
    let ran_in = std::fs::read_to_string(&record).unwrap();
    assert!(ran_in.contains("toolport-verify-"), "{ran_in}");
    assert!(!std::path::Path::new(ran_in.trim()).exists());
}
