use super::hooks::{Epoch, Ledger, LedgerEntry};
use super::ledger::parse_ts;
use super::measure::*;
use super::store::Paths;
use crate::randutil::ScratchDir;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn assistant(id: &str, at: &str, usage: (u64, u64, u64, u64), blocks: Value) -> String {
    json!({
        "type": "assistant",
        "uuid": format!("u-{id}"),
        "cwd": "/work/demo",
        "timestamp": at,
        "message": {
            "id": id,
            "role": "assistant",
            "content": blocks,
            "usage": {
                "input_tokens": usage.0,
                "cache_creation_input_tokens": usage.1,
                "cache_read_input_tokens": usage.2,
                "output_tokens": usage.3,
            },
        },
    })
    .to_string()
}

fn result(tool_use_id: &str, at: &str, text: &str) -> String {
    json!({
        "type": "user",
        "timestamp": at,
        "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": tool_use_id, "content": text}
        ]},
    })
    .to_string()
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!([{"type": "tool_use", "id": id, "name": name, "input": input}])
}

fn write(root: &Path, name: &str, lines: &[String]) -> PathBuf {
    let dir = root.join("demo");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jsonl"));
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

const T: &str = "2026-11-02T10:00:00.000Z";
const FLAT: (u64, u64, u64, u64) = (100, 0, 0, 10);

/// A session with `turns` assistant messages of `usage`, started at `at`.
fn plain_session(root: &Path, name: &str, at: &str, turns: usize, usage: (u64, u64, u64, u64)) {
    let lines: Vec<String> = (0..turns)
        .map(|n| {
            assistant(
                &format!("{name}-{n}"),
                at,
                usage,
                json!([{"type": "text", "text": "x"}]),
            )
        })
        .collect();
    write(root, name, &lines);
}

#[test]
fn test01_a_turn_is_one_message_id_and_its_last_usage_is_billed() {
    let scratch = ScratchDir::new("measure");
    let lines = vec![
        assistant(
            "m1",
            T,
            (10, 0, 0, 1),
            json!([{"type": "text", "text": "a"}]),
        ),
        assistant(
            "m1",
            T,
            (10, 0, 0, 40),
            json!([{"type": "text", "text": "a"}]),
        ),
        assistant(
            "m2",
            T,
            (20, 0, 0, 5),
            json!([{"type": "text", "text": "b"}]),
        ),
    ];
    let path = write(scratch.path(), "s1", &lines);
    let session = scan_session(&path);
    assert_eq!(session.turns, 2);
    assert_eq!(session.cost, 10.0 + 40.0 * 5.0 + 20.0 + 5.0 * 5.0);
}

#[test]
fn test02_cache_tokens_are_weighted_by_the_price_ratios() {
    let scratch = ScratchDir::new("measure");
    plain_session(scratch.path(), "s1", T, 1, (0, 1000, 10_000, 0));
    let session = scan(scratch.path(), None).remove(0);
    assert_eq!(session.cost, 1000.0 * 1.25 + 10_000.0 * 0.1);
}

#[test]
fn test03_a_result_counts_for_every_turn_after_it() {
    let scratch = ScratchDir::new("measure");
    let big = "x".repeat(400);
    let lines = vec![
        assistant(
            "m1",
            T,
            FLAT,
            tool_use("t1", "Bash", json!({"command": "ls"})),
        ),
        result("t1", T, &big),
        assistant("m2", T, FLAT, json!([{"type": "text", "text": "ok"}])),
        assistant(
            "m3",
            T,
            FLAT,
            tool_use("t2", "Read", json!({"file_path": "/a"})),
        ),
        result("t2", T, &"y".repeat(40)),
        assistant("m4", T, FLAT, json!([{"type": "text", "text": "done"}])),
    ];
    write(scratch.path(), "s1", &lines);
    let paths = Paths::new(scratch.path().join("data"));
    let report = report(
        &paths,
        &Options {
            root: scratch.path().to_path_buf(),
            min_turns: 1,
            since_ms: None,
        },
    )
    .unwrap();
    let rows = report["sources"]["rows"].as_array().unwrap();
    let share = |kind: &str| rows.iter().find(|r| r["kind"] == kind).unwrap()["share"].clone();
    // Bash: 100 tokens x 3 later turns; Read: 10 tokens x 1 later turn.
    assert_eq!(share("Bash"), json!(0.9677));
    assert_eq!(share("Read"), json!(0.0323));
    assert_eq!(rows[0]["kind"], "Bash");
    assert_eq!(report["sources"]["bashCeiling"], json!(0.9677));
}

#[test]
fn test04_mcp_edit_and_unknown_results_land_in_their_own_rows() {
    let scratch = ScratchDir::new("measure");
    let lines = vec![
        assistant("m1", T, FLAT, tool_use("t1", "mcp__srv__list", json!({}))),
        result("t1", T, &"a".repeat(40)),
        assistant("m2", T, FLAT, tool_use("t2", "Edit", json!({}))),
        result("t2", T, &"b".repeat(40)),
        assistant("m3", T, FLAT, tool_use("t3", "Grep", json!({}))),
        result("t3", T, &"c".repeat(40)),
        assistant("m4", T, FLAT, json!([{"type": "text", "text": "x"}])),
    ];
    write(scratch.path(), "s1", &lines);
    let paths = Paths::new(scratch.path().join("data"));
    let report = report(
        &paths,
        &Options {
            root: scratch.path().to_path_buf(),
            min_turns: 1,
            since_ms: None,
        },
    )
    .unwrap();
    let shares: Vec<(String, f64)> = report["sources"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["kind"].as_str().unwrap().to_string(),
                r["share"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(shares[0], ("MCP results".to_string(), 0.5));
    assert!(shares.contains(&("Edit, Write".to_string(), 0.3333)));
    assert!(shares.contains(&("Other".to_string(), 0.1667)));
}

#[test]
fn test05_a_repeated_command_and_a_repeated_read_count_once_per_repeat() {
    let scratch = ScratchDir::new("measure");
    let lines = vec![
        assistant(
            "m1",
            T,
            FLAT,
            tool_use("t1", "Bash", json!({"command": "ls"})),
        ),
        assistant(
            "m2",
            T,
            FLAT,
            tool_use("t2", "Bash", json!({"command": "ls"})),
        ),
        assistant(
            "m3",
            T,
            FLAT,
            tool_use("t3", "Bash", json!({"command": "ls"})),
        ),
        assistant(
            "m4",
            T,
            FLAT,
            tool_use("t4", "Read", json!({"file_path": "/a"})),
        ),
        assistant(
            "m5",
            T,
            FLAT,
            tool_use("t5", "Read", json!({"file_path": "/a"})),
        ),
        assistant(
            "m6",
            T,
            FLAT,
            tool_use("t6", "Read", json!({"file_path": "/b"})),
        ),
    ];
    let path = write(scratch.path(), "s1", &lines);
    let session = scan_session(&path);
    assert_eq!((session.repeated_commands, session.repeated_reads), (2, 1));
}

fn epoch(event: &str, at: &str, provider: Option<&str>) -> Epoch {
    Epoch {
        event: event.into(),
        at: at.into(),
        provider: provider.map(String::from),
    }
}

#[test]
fn test06_periods_follow_wrap_pause_resume_and_undo() {
    let periods = periods(&[
        epoch("wrap", "2026-11-05T09:00:00.000Z", Some("rtk")),
        epoch("pause", "2026-11-12T09:00:00.000Z", None),
        epoch("resume", "2026-11-13T09:00:00.000Z", None),
        epoch("undo", "2026-11-20T09:00:00.000Z", None),
    ]);
    let states: Vec<_> = periods.iter().map(|p| p.state).collect();
    assert_eq!(
        states,
        [State::Off, State::On, State::Paused, State::On, State::Off]
    );
    assert_eq!(periods[0].from, None);
    assert_eq!(periods[1].to, periods[2].from);
    assert_eq!(periods[4].to, None);
}

#[test]
fn test07_a_partial_undo_keeps_the_other_hook_on() {
    let periods = periods(&[
        epoch("wrap", "2026-11-05T09:00:00.000Z", Some("rtk")),
        epoch("wrap", "2026-11-05T09:00:00.000Z", Some("read-once")),
        epoch("undo", "2026-11-06T09:00:00.000Z", Some("rtk")),
        epoch("undo", "2026-11-07T09:00:00.000Z", Some("read-once")),
    ]);
    let states: Vec<_> = periods.iter().map(|p| p.state).collect();
    assert_eq!(states, [State::Off, State::On, State::Off]);
}

fn ledger_with(paths: &Paths, epochs: Vec<Epoch>) {
    let ledger = Ledger {
        entries: vec![LedgerEntry {
            file: "/settings.json".into(),
            provider: "rtk".into(),
            original: "rtk hook claude".into(),
            wrapped: "toolportctl compression hook rtk -- x".into(),
            at: "2026-11-05T09:00:00.000Z".into(),
        }],
        epochs,
        ..Ledger::default()
    };
    std::fs::create_dir_all(&paths.dir).unwrap();
    std::fs::write(
        paths.dir.join(super::hooks::LEDGER_FILE),
        serde_json::to_string(&ledger).unwrap(),
    )
    .unwrap();
}

fn measured(scratch: &ScratchDir, paths: &Paths) -> Value {
    report(
        paths,
        &Options {
            root: scratch.path().to_path_buf(),
            min_turns: MIN_TURNS,
            since_ms: None,
        },
    )
    .unwrap()
}

#[test]
fn test08_a_paused_window_with_too_few_sessions_gets_a_refusal() {
    let scratch = ScratchDir::new("measure");
    let paths = Paths::new(scratch.path().join("data"));
    ledger_with(
        &paths,
        vec![
            epoch("wrap", "2026-11-05T00:00:00.000Z", Some("rtk")),
            epoch("pause", "2026-11-12T00:00:00.000Z", None),
            epoch("resume", "2026-11-13T00:00:00.000Z", None),
        ],
    );
    for n in 0..9 {
        plain_session(
            scratch.path(),
            &format!("on{n}"),
            "2026-11-08T10:00:00.000Z",
            MIN_TURNS,
            FLAT,
        );
    }
    for n in 0..4 {
        plain_session(
            scratch.path(),
            &format!("paused{n}"),
            "2026-11-12T10:00:00.000Z",
            MIN_TURNS,
            (110, 0, 0, 10),
        );
    }
    let report = measured(&scratch, &paths);
    assert_eq!(report["outcome"]["verdict"]["status"], "refused");
    let reason = report["outcome"]["verdict"]["reason"].as_str().unwrap();
    assert!(reason.contains("hooks on has 9, paused has 4"), "{reason}");
    let groups = report["outcome"]["groups"].as_array().unwrap();
    let on = groups.iter().find(|g| g["state"] == "on").unwrap();
    assert_eq!(
        (on["sessions"].clone(), on["relative"].clone()),
        (json!(9), json!(100.0))
    );
}

#[test]
fn test09_overlapping_middle_halves_are_named_as_no_difference() {
    let scratch = ScratchDir::new("measure");
    let paths = Paths::new(scratch.path().join("data"));
    ledger_with(
        &paths,
        vec![
            epoch("wrap", "2026-11-05T00:00:00.000Z", Some("rtk")),
            epoch("pause", "2026-11-12T00:00:00.000Z", None),
            epoch("resume", "2026-11-13T00:00:00.000Z", None),
        ],
    );
    for n in 0..8 {
        plain_session(
            scratch.path(),
            &format!("on{n}"),
            "2026-11-08T10:00:00.000Z",
            MIN_TURNS,
            (100 + n, 0, 0, 10),
        );
        plain_session(
            scratch.path(),
            &format!("paused{n}"),
            "2026-11-12T10:00:00.000Z",
            MIN_TURNS,
            (103 + n, 0, 0, 10),
        );
    }
    let verdict = &measured(&scratch, &paths)["outcome"]["verdict"];
    assert_eq!(verdict["status"], "compared");
    assert_eq!(verdict["overlap"], true);
}

#[test]
fn test10_without_a_paused_window_there_is_nothing_to_compare() {
    let scratch = ScratchDir::new("measure");
    let paths = Paths::new(scratch.path().join("data"));
    plain_session(scratch.path(), "s1", T, MIN_TURNS, FLAT);
    let report = measured(&scratch, &paths);
    assert_eq!(report["outcome"]["verdict"]["status"], "none");
    assert_eq!(report["outcome"]["groups"][0]["state"], "off");
}

#[test]
fn test11_a_session_below_the_turn_floor_is_left_out_of_outcome_and_rework() {
    let scratch = ScratchDir::new("measure");
    let paths = Paths::new(scratch.path().join("data"));
    plain_session(scratch.path(), "short", T, MIN_TURNS - 1, FLAT);
    let report = measured(&scratch, &paths);
    assert_eq!(report["outcome"]["status"], "no_sessions");
    assert_eq!(report["rework"]["status"], "no_sessions");
    assert_eq!(report["sources"]["population"]["sessions"], 1);
}

#[test]
fn test12_no_transcripts_leave_every_transcript_card_empty() {
    let scratch = ScratchDir::new("measure");
    let paths = Paths::new(scratch.path().join("data"));
    let report = measured(&scratch, &paths);
    assert_eq!(report["sources"]["status"], "no_transcripts");
    assert_eq!(report["outcome"]["status"], "no_sessions");
    assert_eq!(report["hooks"]["status"], "off");
}

#[test]
fn test13_since_leaves_out_sessions_that_started_earlier() {
    let scratch = ScratchDir::new("measure");
    plain_session(scratch.path(), "old", "2026-10-01T10:00:00.000Z", 2, FLAT);
    plain_session(scratch.path(), "new", "2026-11-10T10:00:00.000Z", 2, FLAT);
    let since = parse_ts("2026-11-01T00:00:00.000Z");
    assert_eq!(scan(scratch.path(), since).len(), 1);
    assert_eq!(scan(scratch.path(), None).len(), 2);
}

#[test]
fn test14_the_hook_counters_report_the_median_and_the_95th_percentile() {
    let scratch = ScratchDir::new("measure");
    let paths = Paths::new(scratch.path().join("data"));
    ledger_with(
        &paths,
        vec![epoch("wrap", "2026-11-05T00:00:00.000Z", Some("rtk"))],
    );
    let rows: Vec<String> = (1..=20)
        .map(|n| {
            json!({
                "at": format!("2026-11-0{}T10:00:00.000Z", 5 + n % 3),
                "provider": "rtk",
                "bytesIn": 10,
                "bytesOut": 5,
                "latencyMs": n * 10,
                "answered": n % 2 == 0,
                "ok": true,
            })
            .to_string()
        })
        .collect();
    std::fs::write(
        paths.dir.join(super::hooks::COUNTERS_FILE),
        rows.join("\n") + "\n",
    )
    .unwrap();
    let hooks = &measured(&scratch, &paths)["hooks"];
    assert_eq!(hooks["status"], "data");
    let rtk = &hooks["providers"][0];
    assert_eq!(
        (
            rtk["calls"].clone(),
            rtk["answered"].clone(),
            rtk["p50Ms"].clone(),
            rtk["p95Ms"].clone()
        ),
        (json!(20), json!(10), json!(110), json!(190))
    );
    assert_eq!(hooks["providers"][1]["wrapped"], false);
    assert_eq!(hooks["outputBytes"]["available"], false);
}
