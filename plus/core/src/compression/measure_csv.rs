//! `compression measure --format csv`: one row per figure of the report, each with the population
//! it was computed over and what it cannot tell. Text cells that start like a formula are
//! shifted behind a quote, so a spreadsheet never runs them.

use serde_json::Value;

pub const COLUMNS: [&str; 9] = [
    "question",
    "figure",
    "group",
    "value",
    "unit",
    "basis",
    "detail",
    "population",
    "cannot_tell",
];

struct Question<'a> {
    name: &'static str,
    basis: &'a str,
    population: String,
    cannot_tell: &'a str,
}

fn cell(text: &str) -> String {
    let guarded = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn number(value: &Value) -> Option<String> {
    let v = value.as_f64()?;
    Some(if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    })
}

fn day_range(from: &Value, to: &Value) -> String {
    match (from.as_str(), to.as_str()) {
        (Some(a), Some(b)) if a == b => a.to_string(),
        (Some(a), Some(b)) => format!("{a} to {b}"),
        (Some(a), None) | (None, Some(a)) => a.to_string(),
        (None, None) => "no dates".into(),
    }
}

#[derive(Default)]
struct Sheet {
    lines: Vec<String>,
}

impl Sheet {
    fn row(
        &mut self,
        q: &Question,
        figure: &str,
        group: &str,
        value: &str,
        unit: &str,
        detail: &str,
    ) {
        let cells = [
            q.name,
            figure,
            group,
            value,
            unit,
            q.basis,
            detail,
            &q.population,
            q.cannot_tell,
        ];
        self.lines
            .push(cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join(","));
    }

    fn figure(
        &mut self,
        q: &Question,
        name: &str,
        group: &str,
        value: &Value,
        unit: &str,
        detail: &str,
    ) {
        if let Some(text) = number(value) {
            self.row(q, name, group, &text, unit, detail);
        }
    }

    fn status(&mut self, q: &Question, status: &str, detail: &str) {
        self.row(q, "status", "", status, "text", detail);
    }
}

fn sources(sheet: &mut Sheet, section: &Value) {
    let population = &section["population"];
    let status = section["status"].as_str().unwrap_or("");
    let q = Question {
        name: "1 Where context tokens come from",
        basis: "estimate",
        population: if status == "no_transcripts" {
            "no transcripts were found".into()
        } else {
            format!(
                "{} sessions on this machine, {}, read from Claude Code transcripts",
                population["sessions"],
                day_range(&population["from"], &population["to"])
            )
        },
        cannot_tell: section["cannotTell"].as_str().unwrap_or(""),
    };
    if status != "data" {
        return sheet.status(&q, status, "");
    }
    let method = section["method"].as_str().unwrap_or("");
    for row in section["rows"].as_array().into_iter().flatten() {
        sheet.figure(
            &q,
            "share of context-turns",
            row["kind"].as_str().unwrap_or(""),
            &row["share"],
            "share",
            method,
        );
    }
}

fn hooks(sheet: &mut Sheet, section: &Value) {
    let population = &section["population"];
    let status = section["status"].as_str().unwrap_or("");
    let q = Question {
        name: "2 What the hooks did",
        basis: "mechanical",
        population: if status == "off" {
            "no hook is written by Toolport".into()
        } else {
            format!(
                "{} hooks, {}, {} calls counted by the hook wrapper",
                population["hooks"],
                day_range(&population["from"], &population["to"]),
                population["calls"]
            )
        },
        cannot_tell: section["cannotTell"].as_str().unwrap_or(""),
    };
    if status != "data" {
        return sheet.status(&q, status, "");
    }
    for p in section["providers"].as_array().into_iter().flatten() {
        if p["calls"].as_u64().unwrap_or(0) == 0 {
            continue;
        }
        let name = p["provider"].as_str().unwrap_or("");
        let detail = day_range(&p["firstCall"], &p["lastCall"]);
        for (figure, key, unit) in [
            ("calls", "calls", "count"),
            ("answered", "answered", "count"),
            ("failed", "failed", "count"),
            ("median latency", "p50Ms", "ms"),
            ("95th percentile latency", "p95Ms", "ms"),
        ] {
            sheet.figure(&q, figure, name, &p[key], unit, &detail);
        }
    }
}

fn outcome(sheet: &mut Sheet, section: &Value) {
    let population = &section["population"];
    let status = section["status"].as_str().unwrap_or("");
    let min_turns = &population["minTurns"];
    let mut text = if status == "no_sessions" {
        format!("no session has {min_turns} or more turns")
    } else {
        format!("sessions with at least {min_turns} turns; cost units are relative (hooks-on median = 100)")
    };
    if population["unplaced"].as_u64().unwrap_or(0) > 0 {
        text.push_str(&format!(
            "; {} sessions fall in no window",
            population["unplaced"]
        ));
    }
    let q = Question {
        name: "3 Billed outcome, by day",
        basis: "billed",
        population: text,
        cannot_tell: section["cannotTell"].as_str().unwrap_or(""),
    };
    if status != "data" {
        return sheet.status(&q, status, "");
    }
    for g in section["groups"].as_array().into_iter().flatten() {
        let label = g["label"].as_str().unwrap_or("");
        let windows: Vec<String> = g["intervals"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|i| day_range(&i["from"], &i["to"]))
            .collect();
        let detail = windows.join("; ");
        sheet.figure(&q, "sessions", label, &g["sessions"], "count", &detail);
        for (figure, key) in [
            ("median cost", "relative"),
            ("lower quartile of cost", "relativeLow"),
            ("upper quartile of cost", "relativeHigh"),
        ] {
            sheet.figure(
                &q,
                figure,
                label,
                &g[key],
                "relative to hooks on = 100",
                &detail,
            );
        }
    }
    let verdict = &section["verdict"];
    sheet.row(
        &q,
        "verdict",
        "",
        verdict["status"].as_str().unwrap_or(""),
        "text",
        verdict["reason"].as_str().unwrap_or(""),
    );
}

fn rework(sheet: &mut Sheet, section: &Value) {
    let population = &section["population"];
    let status = section["status"].as_str().unwrap_or("");
    let q = Question {
        name: "4 Rework check",
        basis: "transcripts",
        population: if status == "no_sessions" {
            format!("no session has {} or more turns", population["minTurns"])
        } else {
            format!(
                "the sessions of question 3, with at least {} turns",
                population["minTurns"]
            )
        },
        cannot_tell: section["cannotTell"].as_str().unwrap_or(""),
    };
    if status != "data" {
        return sheet.status(&q, status, "");
    }
    for g in section["groups"].as_array().into_iter().flatten() {
        let label = g["label"].as_str().unwrap_or("");
        sheet.figure(&q, "sessions", label, &g["sessions"], "count", "");
        sheet.figure(
            &q,
            "repeated Bash commands per session",
            label,
            &g["repeatedCommands"],
            "count per session",
            "",
        );
        sheet.figure(
            &q,
            "files read again per session",
            label,
            &g["repeatedReads"],
            "count per session",
            "",
        );
    }
}

/// The report of `measure::report` as CSV text with CRLF rows, and how many figure rows it has.
pub fn to_csv(report: &Value) -> (String, usize) {
    let mut sheet = Sheet::default();
    sources(&mut sheet, &report["sources"]);
    hooks(&mut sheet, &report["hooks"]);
    outcome(&mut sheet, &report["outcome"]);
    rework(&mut sheet, &report["rework"]);
    let rows = sheet.lines.len();
    let mut text = COLUMNS.join(",");
    text.push_str("\r\n");
    for line in sheet.lines {
        text.push_str(&line);
        text.push_str("\r\n");
    }
    (text, rows)
}
