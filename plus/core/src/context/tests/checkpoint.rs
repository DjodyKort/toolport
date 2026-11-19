use super::world::{world, World};
use crate::context::checkpoint;
use serde_json::Value;

fn transcript(w: &World, session: &str, cwd: &str, model: &str, usage: &[(&str, u64, u64, u64)]) {
    let mut text = String::new();
    for (i, (ts, input, cache_read, cache_creation)) in usage.iter().enumerate() {
        text.push_str(&format!(
            "{{\"sessionId\":\"{session}\",\"cwd\":\"{cwd}\",\"timestamp\":\"{ts}\",\"message\":{{\"id\":\"m{i}\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":{input},\"output_tokens\":50,\"cache_read_input_tokens\":{cache_read},\"cache_creation_input_tokens\":{cache_creation}}}}}}}\n"
        ));
    }
    w.write(
        &format!(".claude/projects/p-{session}/{session}.jsonl"),
        &text,
    );
}

fn status(w: &World, cwd: Option<&str>, env: Option<&str>) -> Value {
    checkpoint::status_with(&w.host, cwd, env).unwrap()
}

#[test]
fn test01_the_gauge_reads_the_last_request_of_the_newest_session() {
    let w = world("gauge-used");
    transcript(
        &w,
        "s1",
        "/work/a",
        "claude-opus-4-8",
        &[
            ("2026-11-19T10:00:00Z", 10, 40_000, 5_000),
            ("2026-11-19T10:05:00Z", 12, 120_000, 22_000),
        ],
    );
    let data = status(&w, None, None);
    assert_eq!(data["session"]["id"], "s1");
    assert_eq!(data["session"]["model"], "claude-opus-4-8");
    assert_eq!(data["usedTokens"], 142_012);
    assert_eq!(data["window"]["tokens"], 200_000);
    assert_eq!(data["window"]["source"], "model name");
    assert_eq!(data["compactionAt"], 200_000);
    assert_eq!(data["remainingToCompaction"], 57_988);
    assert_eq!(data["checkpoint"], Value::Null);
}

#[test]
fn test02_no_transcript_says_so_instead_of_inventing_numbers() {
    let w = world("gauge-none");
    let data = status(&w, None, None);
    assert_eq!(data["session"], Value::Null);
    assert!(data.get("usedTokens").is_none());
    assert!(data["note"].as_str().unwrap().contains("No Claude session"));
}

#[test]
fn test03_the_window_follows_the_model_name_then_the_usage_then_the_settings_then_the_environment()
{
    let w = world("gauge-window");
    transcript(
        &w,
        "s1",
        "/work/a",
        "claude-opus-4-8[1m]",
        &[("2026-11-19T10:00:00Z", 1, 1_000, 0)],
    );
    assert_eq!(status(&w, None, None)["window"]["tokens"], 1_000_000);
    transcript(
        &w,
        "s1",
        "/work/a",
        "claude-opus-4-8",
        &[("2026-11-19T10:00:00Z", 1, 250_000, 0)],
    );
    let big = status(&w, None, None);
    assert_eq!(big["window"]["tokens"], 1_000_000);
    assert_eq!(big["window"]["source"], "usage above 200,000");
    transcript(
        &w,
        "s1",
        "/work/a",
        "claude-opus-4-8",
        &[("2026-11-19T10:00:00Z", 1, 1_000, 0)],
    );
    w.write(".claude/settings.json", r#"{"autoCompactWindow": 150000}"#);
    let set = status(&w, None, None);
    assert_eq!(
        (
            set["window"]["tokens"].as_u64(),
            set["window"]["source"].as_str()
        ),
        (Some(150_000), Some("setting"))
    );
    w.write(
        ".claude/settings.json",
        r#"{"autoCompactWindow": 150000, "modelSettings": {"claude-opus-4-8": {"autoCompactWindow": 120000}}}"#,
    );
    assert_eq!(status(&w, None, None)["window"]["tokens"], 120_000);
    let env = status(&w, None, Some("110000"));
    assert_eq!(
        (
            env["window"]["tokens"].as_u64(),
            env["window"]["source"].as_str()
        ),
        (Some(110_000), Some("environment"))
    );
    w.write(".claude/settings.json", r#"{"autoCompactWindow": 900000}"#);
    assert_eq!(
        status(&w, None, None)["window"]["tokens"],
        200_000,
        "never larger than the model's window"
    );
}

#[test]
fn test04_the_checkpoint_is_the_compaction_point_minus_the_tokens_the_person_set() {
    let w = world("gauge-checkpoint");
    transcript(
        &w,
        "s1",
        "/work/a",
        "claude-opus-4-8",
        &[("2026-11-19T10:00:00Z", 1, 150_000, 0)],
    );
    let plan = checkpoint::set(&w.host, Some(44_000), true).unwrap();
    assert_eq!(
        plan["plan"]["undo"],
        "toolportctl context checkpoint-config --off"
    );
    assert!(
        status(&w, None, None)["checkpoint"].is_null(),
        "a plan writes nothing"
    );
    let done = checkpoint::set(&w.host, Some(44_000), false).unwrap();
    assert_eq!(done["result"]["applied"], true);
    let data = status(&w, None, None);
    assert_eq!(data["checkpoint"]["point"], 156_000);
    assert_eq!(data["checkpoint"]["reached"], false);
    assert_eq!(data["checkpoint"]["remaining"], 5_999);
    transcript(
        &w,
        "s1",
        "/work/a",
        "claude-opus-4-8",
        &[("2026-11-19T10:00:00Z", 1, 160_000, 0)],
    );
    assert_eq!(status(&w, None, None)["checkpoint"]["reached"], true);
    checkpoint::set(&w.host, None, false).unwrap();
    assert!(status(&w, None, None)["checkpoint"].is_null());
}

#[test]
fn test05_setting_the_checkpoint_keeps_every_other_key_of_context_json() {
    let w = world("gauge-keys");
    w.write(
        ".config/mcpm/context.json",
        r#"{"clients_root": "~/c", "bundleAutoApply": true}"#,
    );
    checkpoint::set(&w.host, Some(30_000), false).unwrap();
    let text = w.read(".config/mcpm/context.json");
    assert!(
        text.contains("clients_root") && text.contains("bundleAutoApply") && text.contains("30000"),
        "{text}"
    );
    checkpoint::set(&w.host, None, false).unwrap();
    assert!(!w.read(".config/mcpm/context.json").contains("checkpointAt"));
}

#[test]
fn test06_a_checkpoint_of_zero_or_more_than_the_largest_window_is_refused() {
    let w = world("gauge-range");
    for bad in [0, 1_000_000, 5_000_000] {
        let err = checkpoint::set(&w.host, Some(bad), false).unwrap_err();
        assert_eq!(err.kind.code(), "usage", "{bad}");
    }
}

#[test]
fn test07_a_folder_selects_the_newest_session_that_ran_there() {
    let w = world("gauge-cwd");
    let a = w.home.join("work/a");
    let b = w.home.join("work/b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    transcript(
        &w,
        "sa",
        &a.display().to_string(),
        "claude-opus-4-8",
        &[("2026-11-19T10:00:00Z", 1, 10_000, 0)],
    );
    std::thread::sleep(std::time::Duration::from_millis(30));
    transcript(
        &w,
        "sb",
        &b.display().to_string(),
        "claude-opus-4-8",
        &[("2026-11-19T11:00:00Z", 1, 20_000, 0)],
    );
    assert_eq!(status(&w, None, None)["session"]["id"], "sb");
    assert_eq!(
        status(&w, Some(&a.display().to_string()), None)["session"]["id"],
        "sa"
    );
    assert_eq!(
        status(&w, Some(&w.home.display().to_string()), None)["session"],
        Value::Null
    );
}

#[test]
fn test08_an_old_session_is_marked_stale() {
    let w = world("gauge-stale");
    transcript(
        &w,
        "s1",
        "/work/a",
        "claude-opus-4-8",
        &[("2026-11-19T10:00:00Z", 1, 10_000, 0)],
    );
    w.host.now.set(1_800_000_000);
    assert_eq!(status(&w, None, None)["session"]["stale"], true);
    let last = crate::spend::parse_ts("2026-11-19T10:00:00Z").unwrap() / 1000;
    w.host.now.set(last + 60);
    assert_eq!(status(&w, None, None)["session"]["stale"], false);
}
