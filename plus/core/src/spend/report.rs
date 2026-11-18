//! The numbers the Spend page, `spend summary` and the CSV export show. Source rule: per
//! session, OTel events when the session has any, otherwise the transcript. Daily totals stand
//! in for OTel events that were pruned.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::sample::Sample;
use super::store::{DailyLine, Store, UNKNOWN_MODEL, UNKNOWN_PROJECT};
use super::transcript::{self, TranscriptSession};
use super::{day_of, MS_PER_DAY};

pub const BADGE_MEASURED: &str = "Measured";
pub const BADGE_COUNTED: &str = "Counted";
pub const BADGE_ESTIMATE: &str = "Estimate";
pub const BADGE_NOT_ITEMISED: &str = "Not itemised";
pub const UNATTRIBUTED: &str = "Unattributed";
pub const INTERACTIVE: &str = "Interactive";
const TASK_MATCH_SLACK_MS: i64 = 60_000;
const TASK_MATCH_OPEN_MS: i64 = 3_600_000;
const LISTED_SESSIONS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Today,
    Days7,
    Days30,
}

impl Range {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "today" | "1d" => Some(Self::Today),
            "7d" => Some(Self::Days7),
            "30d" => Some(Self::Days30),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
        }
    }

    pub fn days(self) -> i64 {
        match self {
            Self::Today => 1,
            Self::Days7 => 7,
            Self::Days30 => 30,
        }
    }
}

/// A task run that may own sessions started inside it.
#[derive(Clone, Debug)]
pub struct RunSpan {
    pub task: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub session: Option<String>,
}

pub struct Inputs<'a> {
    pub store: &'a Store,
    pub transcripts: BTreeMap<String, TranscriptSession>,
    pub runs: Vec<RunSpan>,
    pub now: i64,
}

#[derive(Default, Clone)]
struct Acc {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    cost: f64,
    cost_estimated: f64,
    requests: u64,
    transcript_tokens: u64,
    sessions: BTreeSet<String>,
    by_model: BTreeMap<String, f64>,
}

impl Acc {
    fn add(&mut self, s: &Sample) {
        self.input += s.input;
        self.output += s.output;
        self.cache_read += s.cache_read;
        self.cache_creation += s.cache_creation;
        self.cost += s.cost;
        self.cost_estimated += s.cost_estimated;
        self.requests += s.requests;
        if s.transcript {
            self.transcript_tokens += s.tokens();
        }
        if let Some(id) = &s.session {
            self.sessions.insert(id.clone());
        }
        *self
            .by_model
            .entry(s.model.clone().unwrap_or_else(|| UNKNOWN_MODEL.into()))
            .or_default() += s.cost + s.cost_estimated;
    }

    fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_creation
    }

    fn total_cost(&self) -> f64 {
        self.cost + self.cost_estimated
    }

    fn json(&self, key: &str, share_of: f64) -> Value {
        json!({
            "key": key,
            "tokens": {
                "input": self.input, "output": self.output, "cacheRead": self.cache_read,
                "cacheCreation": self.cache_creation, "total": self.tokens(),
                "badge": BADGE_MEASURED,
                "fromTranscript": self.transcript_tokens,
            },
            "cost": {
                "usd": money(self.total_cost()), "estimatedUsd": money(self.cost_estimated),
                "badge": cost_badge(self.cost, self.cost_estimated),
            },
            "requests": self.requests,
            "sessions": self.sessions.len(),
            "share": if share_of > 0.0 { (self.total_cost() / share_of * 1000.0).round() / 1000.0 } else { 0.0 },
        })
    }
}

fn money(usd: f64) -> f64 {
    (usd * 1_000_000.0).round() / 1_000_000.0
}

fn cost_badge(measured: f64, estimated: f64) -> &'static str {
    if estimated > 0.0 || measured == 0.0 {
        BADGE_ESTIMATE
    } else {
        BADGE_MEASURED
    }
}

fn daily_sample(line: &DailyLine) -> Sample {
    let ts = super::day_start_ms(&line.day).unwrap_or(0) + MS_PER_DAY / 2;
    Sample {
        ts,
        session: None,
        project: Some(line.project.clone()),
        model: Some(line.model.clone()),
        input: line.input,
        output: line.output,
        cache_read: line.cache_read,
        cache_creation: line.cache_creation,
        cost: line.cost,
        cost_estimated: line.cost_estimated,
        requests: line.requests,
        transcript: false,
    }
}

pub struct Collected {
    pub samples: Vec<Sample>,
    pub otel_sessions: usize,
    pub transcript_sessions: usize,
    pub session_start: BTreeMap<String, i64>,
}

pub fn collect(inputs: &Inputs) -> Collected {
    let (events, _) = inputs.store.read_events();
    let daily = inputs.store.read_daily();
    let mut samples = super::sample::from_events(&events);
    let mut known: BTreeSet<String> = samples.iter().filter_map(|s| s.session.clone()).collect();
    known.extend(daily.iter().flat_map(|l| l.sessions.iter().cloned()));
    let otel_sessions = known.len();
    samples.extend(daily.iter().map(daily_sample));
    let mut transcript_sessions = 0;
    for (id, session) in &inputs.transcripts {
        if known.contains(id) {
            continue;
        }
        transcript_sessions += 1;
        samples.extend(session.samples.iter().cloned());
    }
    let mut session_start: BTreeMap<String, i64> = BTreeMap::new();
    for s in &samples {
        if let Some(id) = &s.session {
            let slot = session_start.entry(id.clone()).or_insert(s.ts);
            *slot = (*slot).min(s.ts);
        }
    }
    for s in &mut samples {
        let cwd = s.project.clone().or_else(|| {
            s.session
                .as_ref()
                .and_then(|id| inputs.transcripts.get(id))
                .and_then(|t| t.cwd.clone())
        });
        s.project = Some(
            cwd.map(|c| transcript::project_root(&c))
                .unwrap_or_else(|| UNKNOWN_PROJECT.to_string()),
        );
    }
    Collected {
        samples,
        otel_sessions,
        transcript_sessions,
        session_start,
    }
}

pub fn task_of(runs: &[RunSpan], session: &str, first_ts: i64) -> Option<String> {
    if let Some(run) = runs.iter().find(|r| r.session.as_deref() == Some(session)) {
        return Some(run.task.clone());
    }
    runs.iter()
        .filter(|r| r.session.is_none())
        .filter(|r| {
            let end = r.end_ms.unwrap_or(r.start_ms + TASK_MATCH_OPEN_MS);
            first_ts >= r.start_ms - TASK_MATCH_SLACK_MS && first_ts <= end + TASK_MATCH_SLACK_MS
        })
        .max_by_key(|r| r.start_ms)
        .map(|r| r.task.clone())
}

pub fn range_start(now: i64, range: Range) -> i64 {
    (now.div_euclid(MS_PER_DAY) - (range.days() - 1)) * MS_PER_DAY
}

fn delta(now: f64, before: f64) -> Value {
    if before > 0.0 {
        json!(((now - before) / before * 1000.0).round() / 1000.0)
    } else {
        Value::Null
    }
}

/// All three range tiles at once, so the page reads them with one command whatever range it shows.
fn tiles(samples: &[Sample], now: i64, keep: impl Fn(&&Sample) -> bool) -> Value {
    let mut out = serde_json::Map::new();
    for range in [Range::Today, Range::Days7, Range::Days30] {
        let start = range_start(now, range);
        let (mut current, mut previous) = (Acc::default(), Acc::default());
        let mut estimated_days = BTreeSet::new();
        for s in samples
            .iter()
            .filter(|s| s.ts >= start - range.days() * MS_PER_DAY)
            .filter(|s| keep(s))
        {
            if s.ts >= start {
                current.add(s);
                if s.cost_estimated > 0.0 {
                    estimated_days.insert(day_of(s.ts));
                }
            } else {
                previous.add(s);
            }
        }
        out.insert(
            range.name().to_string(),
            json!({
                "tokens": current.tokens(),
                "cost": {
                    "usd": money(current.total_cost()),
                    "estimatedUsd": money(current.cost_estimated),
                    "badge": cost_badge(current.cost, current.cost_estimated),
                },
                "costDelta": delta(current.total_cost(), previous.total_cost()),
                "previousUsd": money(previous.total_cost()),
                "estimatedDays": estimated_days.len(),
                "days": range.days(),
            }),
        );
    }
    Value::Object(out)
}

pub fn summary(inputs: &Inputs, range: Range, project: Option<&str>) -> Value {
    let collected = collect(inputs);
    let start = range_start(inputs.now, range);
    let previous_start = start - range.days() * MS_PER_DAY;
    let keep = |s: &&Sample| project.is_none_or(|p| s.project.as_deref() == Some(p));
    let current: Vec<&Sample> = collected
        .samples
        .iter()
        .filter(|s| s.ts >= start)
        .filter(keep)
        .collect();
    let mut previous = Acc::default();
    for s in collected
        .samples
        .iter()
        .filter(|s| s.ts >= previous_start && s.ts < start)
        .filter(keep)
    {
        previous.add(s);
    }
    let mut total = Acc::default();
    let mut by_day: BTreeMap<String, Acc> = BTreeMap::new();
    let mut by_project: BTreeMap<String, Acc> = BTreeMap::new();
    let mut by_model: BTreeMap<String, Acc> = BTreeMap::new();
    let mut by_task: BTreeMap<String, Acc> = BTreeMap::new();
    let mut sessions: BTreeMap<String, Acc> = BTreeMap::new();
    for s in &current {
        total.add(s);
        by_day.entry(day_of(s.ts)).or_default().add(s);
        by_project
            .entry(s.project.clone().unwrap_or_else(|| UNKNOWN_PROJECT.into()))
            .or_default()
            .add(s);
        by_model
            .entry(s.model.clone().unwrap_or_else(|| UNKNOWN_MODEL.into()))
            .or_default()
            .add(s);
        let task = match s.session.as_deref() {
            Some(id) => task_of(
                &inputs.runs,
                id,
                collected.session_start.get(id).copied().unwrap_or(s.ts),
            )
            .unwrap_or_else(|| INTERACTIVE.to_string()),
            None => UNATTRIBUTED.to_string(),
        };
        by_task.entry(task).or_default().add(s);
        if let Some(id) = &s.session {
            sessions.entry(id.clone()).or_default().add(s);
        }
    }
    let grand = total.total_cost();
    let rows = |map: &BTreeMap<String, Acc>| {
        let mut list: Vec<(&String, &Acc)> = map.iter().collect();
        list.sort_by(|a, b| {
            b.1.total_cost()
                .total_cmp(&a.1.total_cost())
                .then(b.1.tokens().cmp(&a.1.tokens()))
                .then(a.0.cmp(b.0))
        });
        Value::Array(list.into_iter().map(|(k, a)| a.json(k, grand)).collect())
    };
    let days: Vec<Value> = by_day
        .iter()
        .map(|(day, a)| {
            let mut row = a.json(day, grand);
            row["byModel"] = json!(a
                .by_model
                .iter()
                .map(|(m, v)| (m.clone(), money(*v)))
                .collect::<BTreeMap<_, _>>());
            row
        })
        .collect();
    let input_side = total.input + total.cache_read + total.cache_creation;
    let mut out = json!({
        "range": range.name(),
        "project": project,
        "since": super::format_ts(start),
        "totals": total.json("total", grand),
        "previous": {
            "costDelta": delta(total.total_cost(), previous.total_cost()),
            "tokensDelta": delta(total.tokens() as f64, previous.tokens() as f64),
        },
        "cacheShare": {
            "value": if input_side > 0 { (total.cache_read as f64 / input_side as f64 * 1000.0).round() / 1000.0 } else { 0.0 },
            "badge": BADGE_MEASURED,
        },
        "byDay": days,
        "byProject": rows(&by_project),
        "byModel": rows(&by_model),
        "byTask": rows(&by_task),
        "sources": {
            "otelSessions": collected.otel_sessions,
            "transcriptSessions": collected.transcript_sessions,
            "estimatedShare": if grand > 0.0 { (total.cost_estimated / grand * 1000.0).round() / 1000.0 } else { 0.0 },
        },
        "tiles": tiles(&collected.samples, inputs.now, keep),
        "notes": ["Spend is never added to upstream's saved-tokens number"],
    });
    if project.is_some() {
        let mut list: Vec<(&String, &Acc)> = sessions.iter().collect();
        list.sort_by_key(|(id, _)| std::cmp::Reverse(collected.session_start.get(*id).copied()));
        out["sessionList"] = Value::Array(
            list.into_iter()
                .take(LISTED_SESSIONS)
                .map(|(id, a)| {
                    let mut row = a.json(id, grand);
                    row["start"] = json!(super::format_ts(
                        collected.session_start.get(id).copied().unwrap_or(0)
                    ));
                    row["source"] = json!(if a.transcript_tokens > 0 {
                        "transcript"
                    } else {
                        "otel"
                    });
                    row
                })
                .collect(),
        );
    }
    out
}

/// One row per day, project and model for the CSV export.
pub fn rows_for_csv(inputs: &Inputs, range: Range) -> Vec<[String; 11]> {
    let collected = collect(inputs);
    let start = range_start(inputs.now, range);
    let mut grouped: BTreeMap<(String, String, String), (Acc, bool)> = BTreeMap::new();
    for s in collected.samples.iter().filter(|s| s.ts >= start) {
        let key = (
            day_of(s.ts),
            s.project.clone().unwrap_or_default(),
            s.model.clone().unwrap_or_else(|| UNKNOWN_MODEL.into()),
        );
        let entry = grouped.entry(key).or_default();
        entry.0.add(s);
        entry.1 |= s.transcript;
    }
    grouped
        .into_iter()
        .map(|((day, project, model), (a, transcript))| {
            [
                day,
                project,
                model,
                a.input.to_string(),
                a.output.to_string(),
                a.cache_read.to_string(),
                a.cache_creation.to_string(),
                format!("{:.6}", a.total_cost()),
                cost_badge(a.cost, a.cost_estimated).to_string(),
                a.requests.to_string(),
                if transcript { "transcript" } else { "otel" }.to_string(),
            ]
        })
        .collect()
}
