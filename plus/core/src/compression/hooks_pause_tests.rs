use super::hooks::{
    effective_epochs, parse_duration, pause_active, pause_end, read_ledger, run_wrapper,
    set_paused, status, Ledger, LEDGER_FILE,
};
use super::hooks_testkit::{rtk_settings, world};
use super::ledger::{format_ts, now_ms, parse_ts};
use super::measure::{periods, State};
use serde_json::json;

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

fn expired_pause(w: &super::hooks_testkit::World) {
    let ledger = json!({
        "pausedSince": "2020-01-01T00:00:00.000Z",
        "pausedUntil": "2020-01-02T00:00:00.000Z",
        "epochs": [
            {"event": "wrap", "at": "2019-12-31T00:00:00.000Z", "provider": "rtk"},
            {"event": "pause", "at": "2020-01-01T00:00:00.000Z"}
        ],
    });
    std::fs::write(w.paths.dir.join(LEDGER_FILE), ledger.to_string()).unwrap();
}

#[test]
fn test01_durations_read_days_hours_minutes_and_their_sum() {
    assert_eq!(parse_duration("1d"), Some(DAY));
    assert_eq!(parse_duration("90m"), Some(90 * 60_000));
    assert_eq!(parse_duration("1d12h"), Some(DAY + 12 * HOUR));
    assert_eq!(parse_duration("6H"), Some(6 * HOUR));
    for bad in ["", "d", "1", "1x", "0h", "1d 2", "-1h", "1.5h"] {
        assert_eq!(parse_duration(bad), None, "{bad:?}");
    }
}

#[test]
fn test02_pause_end_takes_a_duration_or_a_time_and_refuses_the_rest() {
    let now = parse_ts("2026-11-17T10:00:00Z").unwrap();
    assert_eq!(pause_end(None, None, now), Ok(None));
    assert_eq!(pause_end(Some("1d"), None, now), Ok(Some(now + DAY)));
    assert_eq!(
        pause_end(None, Some("2026-11-18T09:12+01:00"), now),
        Ok(parse_ts("2026-11-18T08:12:00Z"))
    );
    assert_eq!(
        pause_end(None, Some("2026-11-18T09:12"), now),
        Ok(parse_ts("2026-11-18T09:12:00Z"))
    );
    assert_eq!(
        pause_end(None, Some("2026-11-20"), now),
        Ok(parse_ts("2026-11-20T00:00:00Z"))
    );
    let refused = |a, b| pause_end(a, b, now).unwrap_err();
    assert!(refused(Some("1d"), Some("2026-11-18")).contains("not both"));
    assert!(refused(Some("soon"), None).contains("not a duration"));
    assert!(refused(None, Some("tomorrow")).contains("not a time"));
    assert!(refused(None, Some("2026-11-17T09:00Z")).contains("before it starts"));
    assert!(refused(Some("31d"), None).contains("at most 30 days"));
}

#[test]
fn test03_a_pause_with_an_end_is_over_once_the_end_has_passed() {
    let at = |ts: &str| parse_ts(ts).unwrap();
    let mut ledger = Ledger {
        paused_since: Some("2026-11-17T10:00:00.000Z".into()),
        ..Ledger::default()
    };
    assert!(pause_active(&ledger, at("2030-01-01T00:00:00Z")), "no end");
    ledger.paused_until = Some("2026-11-18T10:00:00.000Z".into());
    assert!(pause_active(&ledger, at("2026-11-18T09:59:59Z")));
    assert!(!pause_active(&ledger, at("2026-11-18T10:00:00Z")));
    ledger.paused_until = Some("not a time".into());
    assert!(
        !pause_active(&ledger, at("2026-11-17T10:00:01Z")),
        "unreadable end"
    );
    assert!(!pause_active(&Ledger::default(), 0));
}

#[test]
fn test04_pause_for_records_the_end_and_status_shows_it() {
    let w = world(rtk_settings());
    let now = now_ms();
    let data = set_paused(&w.paths, true, Some(now + DAY), now).unwrap();
    assert_eq!(data["changed"], true);
    assert_eq!(data["pausedUntil"], format_ts(now + DAY));
    let shown = status(&w.host, &w.paths).unwrap();
    assert_eq!(shown["paused"], true);
    assert_eq!(shown["pausedUntil"], format_ts(now + DAY));
}

#[cfg(unix)]
#[test]
fn test05_an_expired_pause_lets_the_hook_answer_again_without_writing_anything() {
    let w = world(rtk_settings());
    expired_pause(&w);
    let before = w.ledger_bytes();
    let argv: Vec<String> = ["/bin/sh", "-c", "echo answered"]
        .map(String::from)
        .to_vec();
    let out = run_wrapper(&w.paths, "rtk", &argv, b"");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "answered\n");
    let shown = status(&w.host, &w.paths).unwrap();
    assert_eq!(shown["paused"], false);
    assert_eq!(shown["pausedUntil"], serde_json::Value::Null);
    assert_eq!(w.ledger_bytes(), before, "the hook and status only read");
}

#[test]
fn test06_an_expired_pause_closes_at_its_end_in_the_epochs() {
    let w = world(rtk_settings());
    expired_pause(&w);
    let ledger = read_ledger(&w.paths).unwrap();
    let epochs = effective_epochs(&ledger, now_ms());
    assert_eq!(epochs.last().unwrap().event, "resume");
    assert_eq!(epochs.last().unwrap().at, "2020-01-02T00:00:00.000Z");
    let states: Vec<_> = periods(&epochs).iter().map(|p| p.state).collect();
    assert_eq!(states, [State::Off, State::On, State::Paused, State::On]);
}

#[test]
fn test07_pausing_after_an_expired_pause_records_both_epochs_in_order() {
    let w = world(rtk_settings());
    expired_pause(&w);
    let now = now_ms();
    set_paused(&w.paths, true, Some(now + HOUR), now).unwrap();
    let events: Vec<_> = read_ledger(&w.paths)
        .unwrap()
        .epochs
        .into_iter()
        .map(|e| (e.event, e.at))
        .collect();
    assert_eq!(
        events[2],
        ("resume".into(), "2020-01-02T00:00:00.000Z".into())
    );
    assert_eq!(events[3], ("pause".into(), format_ts(now)));
}

#[test]
fn test08_resume_now_ends_a_pause_and_a_second_resume_changes_nothing() {
    let w = world(rtk_settings());
    let now = now_ms();
    set_paused(&w.paths, true, Some(now + DAY), now).unwrap();
    let data = set_paused(&w.paths, false, None, now + HOUR).unwrap();
    assert_eq!(
        (data["changed"].clone(), data["pausedUntil"].clone()),
        (json!(true), json!(null))
    );
    assert_eq!(
        set_paused(&w.paths, false, None, now + HOUR).unwrap()["changed"],
        false
    );
    assert_eq!(status(&w.host, &w.paths).unwrap()["paused"], false);
}

#[test]
fn test09_pausing_again_moves_the_end_only_when_it_differs() {
    let w = world(rtk_settings());
    let now = now_ms();
    set_paused(&w.paths, true, Some(now + HOUR), now).unwrap();
    let same = set_paused(&w.paths, true, Some(now + HOUR), now).unwrap();
    assert_eq!(same["changed"], false);
    let longer = set_paused(&w.paths, true, Some(now + DAY), now).unwrap();
    assert_eq!(longer["changed"], true);
    assert_eq!(longer["pausedUntil"], format_ts(now + DAY));
    let pauses = read_ledger(&w.paths)
        .unwrap()
        .epochs
        .iter()
        .filter(|e| e.event == "pause")
        .count();
    assert_eq!(pauses, 1);
}
