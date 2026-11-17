use super::measure_csv::{to_csv, COLUMNS};
use serde_json::{json, Value};

fn report() -> Value {
    json!({
        "sources": {
            "status": "data", "method": "Size divided by 4",
            "rows": [{"kind": "Read", "share": 0.38}, {"kind": "Bash", "share": 0.27}],
            "population": {"sessions": 461, "from": "2026-10-16", "to": "2026-11-16"},
            "cannotTell": "how much a hook removed",
        },
        "hooks": {
            "status": "data",
            "providers": [
                {"provider": "rtk", "calls": 1842, "answered": 1800, "failed": 2, "p50Ms": 38, "p95Ms": 140,
                 "firstCall": "2026-11-05", "lastCall": "2026-11-16"},
                {"provider": "read-once", "calls": 0, "answered": 0, "failed": 0, "p50Ms": null, "p95Ms": null,
                 "firstCall": null, "lastCall": null}
            ],
            "population": {"hooks": 2, "calls": 1842, "from": "2026-11-05", "to": "2026-11-16"},
            "cannotTell": "billed tokens",
        },
        "outcome": {
            "status": "data",
            "groups": [
                {"label": "Hooks on", "sessions": 118, "relative": 100.0, "relativeLow": 71.0, "relativeHigh": 160.0,
                 "intervals": [{"from": "2026-11-05", "to": null}]},
                {"label": "Paused", "sessions": 4, "relative": 112.0, "relativeLow": null, "relativeHigh": null,
                 "intervals": [{"from": "2026-11-12", "to": "2026-11-12"}]}
            ],
            "verdict": {"status": "refused", "reason": "Needs at least 8 sessions, paused has 4."},
            "population": {"sessions": 122, "minTurns": 5, "unplaced": 3},
            "cannotTell": "a fair comparison",
        },
        "rework": {
            "status": "data",
            "groups": [{"label": "Hooks on", "sessions": 118, "repeatedCommands": 1.9, "repeatedReads": 0.8}],
            "population": {"sessions": 118, "minTurns": 5, "unplaced": 0},
            "cannotTell": "whether a repeat was caused by compression",
        },
    })
}

fn lines(csv: &str) -> Vec<&str> {
    csv.split("\r\n").filter(|l| !l.is_empty()).collect()
}

#[test]
fn test01_every_figure_is_one_row_with_its_population_and_its_limit() {
    let (csv, rows) = to_csv(&report());
    let lines = lines(&csv);
    assert_eq!(lines[0], COLUMNS.join(","));
    assert_eq!(lines.len(), rows + 1);
    let bash = lines.iter().find(|l| l.contains(",Bash,")).unwrap();
    assert!(bash.starts_with(
        "1 Where context tokens come from,share of context-turns,Bash,0.27,share,estimate,"
    ));
    assert!(bash.contains("\"461 sessions on this machine, 2026-10-16 to 2026-11-16, read from Claude Code transcripts\""));
    assert!(bash.ends_with(",how much a hook removed"));
}

#[test]
fn test02_hook_figures_skip_a_hook_without_calls_and_missing_values() {
    let (csv, _) = to_csv(&report());
    let rtk: Vec<_> = lines(&csv)
        .into_iter()
        .filter(|l| l.contains(",rtk,"))
        .collect();
    assert_eq!(rtk.len(), 5);
    assert!(rtk
        .iter()
        .any(|l| l.contains("median latency,rtk,38,ms,mechanical,2026-11-05 to 2026-11-16")));
    assert!(!csv.contains("read-once"));
}

#[test]
fn test03_outcome_rows_name_their_window_and_end_with_the_verdict() {
    let (csv, _) = to_csv(&report());
    let lines = lines(&csv);
    assert!(lines.iter().any(
        |l| l.contains("median cost,Hooks on,100,relative to hooks on = 100,billed,2026-11-05")
    ));
    assert!(!lines
        .iter()
        .any(|l| l.contains("lower quartile of cost,Paused")));
    let verdict = lines.iter().find(|l| l.contains(",verdict,")).unwrap();
    assert!(
        verdict.contains(",refused,text,billed,Needs at least 8 sessions")
            || verdict.contains("refused,text,billed,\"Needs at least 8 sessions")
    );
    assert!(verdict.contains("3 sessions fall in no window"));
}

#[test]
fn test04_a_section_without_data_gives_a_status_row_and_keeps_its_caveat() {
    let mut empty = report();
    empty["sources"] = json!({
        "status": "no_transcripts", "rows": [], "population": {"sessions": 0, "from": null, "to": null},
        "cannotTell": "how much a hook removed",
    });
    empty["hooks"]["status"] = json!("off");
    let (csv, _) = to_csv(&empty);
    let lines = lines(&csv);
    assert!(lines.iter().any(|l| l.starts_with("1 Where context tokens come from,status,,no_transcripts,text,estimate,,no transcripts were found,how much")));
    assert!(lines.iter().any(|l| l
        .contains(",status,,off,text,mechanical,,no hook is written by Toolport,billed tokens")));
}

#[test]
fn test05_a_text_cell_that_starts_like_a_formula_is_shifted_behind_a_quote() {
    let mut evil = report();
    evil["sources"]["rows"] = json!([{"kind": "=HYPERLINK(\"x\")", "share": 1.0}]);
    let (csv, _) = to_csv(&evil);
    assert!(csv.contains("\"'=HYPERLINK(\"\"x\"\")\""), "{csv}");
}
