//! `compression measure`: four questions with their population and what each cannot tell.
//! Where context tokens come from, what the hooks did, the billed outcome per epoch and a rework
//! check. Every input is a file on this machine; two kinds of figure are never added together and
//! no headline number is made.

use super::hooks::{self, Epoch, HOOK_PROVIDERS};
use super::ledger::{self, format_ts, parse_ts};
use super::store::Paths;
use super::verify::{iter_transcripts, parse_record, scan_lines, session_origin, tokens};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const MIN_TURNS: usize = 5;
pub const MIN_VERDICT_SESSIONS: usize = 8;
const BYTES_PER_TOKEN: usize = 4;

/// Billed token kinds relative to one input token, the published price ratios of the Claude
/// models; a cost unit is only ever compared with another cost unit of the same report.
const WEIGHT_INPUT: f64 = 1.0;
const WEIGHT_CACHE_WRITE: f64 = 1.25;
const WEIGHT_CACHE_READ: f64 = 0.1;
const WEIGHT_OUTPUT: f64 = 5.0;

const SOURCES_CANNOT_TELL: &str = "how much of that share a hook removed, or whether the model needed the removed text. Compaction is not modelled, so a long session counts every result for every later turn.";
const HOOKS_CANNOT_TELL: &str = "billed tokens, or whether an output was cut that Claude then asked for again (see the rework check). A PreToolUse hook sees the call and its answer, not the size of the output a rewrite saves.";
const OUTCOME_CANNOT_TELL: &str = "that a paused window is a fair comparison: the task mix of two windows is not the same, and a refusal is shown instead of a percentage when the windows are thin.";
const REWORK_CANNOT_TELL: &str =
    "whether any single repeat was caused by compression. Treat both figures as hints.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Read,
    Bash,
    Mcp,
    Edit,
    Other,
}

const SOURCES: [Source; 5] = [
    Source::Read,
    Source::Bash,
    Source::Mcp,
    Source::Edit,
    Source::Other,
];

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Read => "Read",
            Source::Bash => "Bash",
            Source::Mcp => "MCP results",
            Source::Edit => "Edit, Write",
            Source::Other => "Other",
        }
    }

    fn of(tool: &str) -> Self {
        match tool {
            "Read" => Source::Read,
            "Bash" => Source::Bash,
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => Source::Edit,
            t if t.starts_with("mcp__") => Source::Mcp,
            _ => Source::Other,
        }
    }

    fn index(self) -> usize {
        SOURCES.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// What one transcript says: billed cost, how long each tool result stayed in context, and the
/// repeated commands and reads.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub start_ms: Option<i64>,
    pub turns: usize,
    pub cost: f64,
    context_turns: [f64; 5],
    pub repeated_commands: usize,
    pub repeated_reads: usize,
}

#[derive(Default)]
struct Billed {
    input: u64,
    cache_write: u64,
    cache_read: u64,
    output: u64,
}

fn result_bytes(block: &Value) -> usize {
    match block.get("content") {
        Some(Value::String(text)) => text.len(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .map(str::len)
            .sum(),
        _ => 0,
    }
}

/// A turn is one assistant message id; Claude Code writes a message as several records that
/// repeat its usage, so the last record of an id is the one that counts.
pub fn scan_session(path: &Path) -> Session {
    let mut billed: HashMap<String, Billed> = HashMap::new();
    let mut tools: HashMap<String, Source> = HashMap::new();
    let mut results: Vec<(Source, usize, usize)> = Vec::new();
    let mut commands: HashSet<String> = HashSet::new();
    let mut files: HashSet<String> = HashSet::new();
    let mut session = Session {
        start_ms: session_origin(path).0,
        ..Session::default()
    };
    let mut anonymous = 0usize;
    scan_lines(path, |line| {
        let assistant = line.contains("usage");
        if !assistant && !line.contains("tool_result") {
            return true;
        }
        let Some(record) = parse_record(line) else {
            return true;
        };
        let Some(message) = record.get("message") else {
            return true;
        };
        let blocks = message
            .get("content")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        match record.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                let id = message
                    .get("id")
                    .or_else(|| record.get("uuid"))
                    .and_then(Value::as_str)
                    .map(String::from)
                    .unwrap_or_else(|| {
                        anonymous += 1;
                        format!("#{anonymous}")
                    });
                if let Some(usage) = message.get("usage").filter(|u| u.is_object()) {
                    billed.insert(
                        id,
                        Billed {
                            input: tokens(usage, "input_tokens"),
                            cache_write: tokens(usage, "cache_creation_input_tokens"),
                            cache_read: tokens(usage, "cache_read_input_tokens"),
                            output: tokens(usage, "output_tokens"),
                        },
                    );
                    session.turns = billed.len();
                }
                for block in blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        tools.insert(id.to_string(), Source::of(name));
                    }
                    let input = block.get("input");
                    let repeat = |key: &str, seen: &mut HashSet<String>| {
                        input
                            .and_then(|i| i.get(key))
                            .and_then(Value::as_str)
                            .is_some_and(|value| !seen.insert(value.to_string()))
                    };
                    match name {
                        "Bash" => {
                            session.repeated_commands +=
                                usize::from(repeat("command", &mut commands))
                        }
                        "Read" => {
                            session.repeated_reads += usize::from(repeat("file_path", &mut files))
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for block in blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                {
                    let source = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .and_then(|id| tools.get(id))
                        .copied()
                        .unwrap_or(Source::Other);
                    results.push((
                        source,
                        result_bytes(block).div_ceil(BYTES_PER_TOKEN),
                        billed.len(),
                    ));
                }
            }
            _ => {}
        }
        true
    });
    for (source, estimate, turns_before) in results {
        session.context_turns[source.index()] +=
            estimate as f64 * session.turns.saturating_sub(turns_before) as f64;
    }
    session.cost = billed
        .values()
        .map(|b| {
            b.input as f64 * WEIGHT_INPUT
                + b.cache_write as f64 * WEIGHT_CACHE_WRITE
                + b.cache_read as f64 * WEIGHT_CACHE_READ
                + b.output as f64 * WEIGHT_OUTPUT
        })
        .sum();
    session
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Off,
    On,
    Paused,
}

impl State {
    fn key(self) -> &'static str {
        match self {
            State::Off => "off",
            State::On => "on",
            State::Paused => "paused",
        }
    }

    fn label(self) -> &'static str {
        match self {
            State::Off => "Hooks off",
            State::On => "Hooks on",
            State::Paused => "Paused",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Period {
    pub state: State,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

/// The ledger's wrap, undo, pause and resume events as consecutive periods; a period before the
/// first event has no start and the current one no end.
pub fn periods(epochs: &[Epoch]) -> Vec<Period> {
    let mut events: Vec<(i64, &Epoch)> = epochs
        .iter()
        .filter_map(|e| Some((parse_ts(&e.at)?, e)))
        .collect();
    events.sort_by_key(|(at, _)| *at);
    let mut wrapped: BTreeSet<&str> = BTreeSet::new();
    let mut paused = false;
    let mut out = vec![Period {
        state: State::Off,
        from: None,
        to: None,
    }];
    for (at, event) in events {
        match (event.event.as_str(), event.provider.as_deref()) {
            ("wrap", Some(provider)) => {
                wrapped.insert(provider);
            }
            ("undo", Some(provider)) => {
                wrapped.remove(provider);
            }
            ("undo", None) => wrapped.clear(),
            ("pause", _) => paused = true,
            ("resume", _) => paused = false,
            _ => continue,
        }
        let state = if wrapped.is_empty() {
            State::Off
        } else if paused {
            State::Paused
        } else {
            State::On
        };
        let last = out.last_mut().expect("one period always exists");
        if last.state != state {
            last.to = Some(at);
            out.push(Period {
                state,
                from: Some(at),
                to: None,
            });
        }
    }
    out
}

fn day(ms: i64) -> String {
    format_ts(ms)[..10].to_string()
}

fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    (!sorted.is_empty()).then(|| sorted[((sorted.len() - 1) as f64 * p).round() as usize])
}

fn round(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

fn mean(total: usize, count: usize) -> Value {
    if count == 0 {
        Value::Null
    } else {
        json!(round(total as f64 / count as f64, 2))
    }
}

fn population(sessions: &[Session]) -> Value {
    let starts: Vec<i64> = sessions.iter().filter_map(|s| s.start_ms).collect();
    json!({
        "sessions": sessions.len(),
        "from": starts.iter().min().map(|ms| day(*ms)),
        "to": starts.iter().max().map(|ms| day(*ms)),
    })
}

fn sources(sessions: &[Session]) -> Value {
    let mut totals = [0f64; 5];
    for session in sessions {
        for (total, part) in totals.iter_mut().zip(session.context_turns) {
            *total += part;
        }
    }
    let all: f64 = totals.iter().sum();
    let mut rows: Vec<Value> = SOURCES
        .iter()
        .map(|s| {
            let share = if all > 0.0 {
                totals[s.index()] / all
            } else {
                0.0
            };
            json!({"kind": s.label(), "share": round(share, 4)})
        })
        .collect();
    rows.sort_by(|a, b| {
        b["share"]
            .as_f64()
            .partial_cmp(&a["share"].as_f64())
            .unwrap()
    });
    let bash = totals[Source::Bash.index()];
    json!({
        "status": if sessions.is_empty() { "no_transcripts" } else if all > 0.0 { "data" } else { "no_results" },
        "basis": "estimate",
        "method": "Size of each tool result divided by 4, times the assistant turns that came after it",
        "rows": if all > 0.0 { rows } else { Vec::new() },
        "bashCeiling": (all > 0.0).then(|| round(bash / all, 4)),
        "population": population(sessions),
        "cannotTell": SOURCES_CANNOT_TELL,
    })
}

fn hooks_section(paths: &Paths, owned: &hooks::Ledger, since: Option<i64>) -> Value {
    let providers: Vec<Value> = HOOK_PROVIDERS
        .iter()
        .map(|p| {
            let s = hooks::summarize(paths, p.id);
            json!({
                "provider": p.id,
                "wrapped": owned.entries.iter().any(|e| e.provider == p.id),
                "calls": s.calls,
                "answered": s.answered,
                "failed": s.failed,
                "p50Ms": s.median_ms,
                "p95Ms": s.p95_ms,
                "firstCall": s.first.as_deref().map(|t| t[..10.min(t.len())].to_string()),
                "lastCall": s.last.as_deref().map(|t| t[..10.min(t.len())].to_string()),
            })
        })
        .collect();
    let calls: u64 = providers.iter().filter_map(|p| p["calls"].as_u64()).sum();
    let reported: Vec<Value> = ledger::summarize(&[], &ledger::read_savings(paths), None, since)
        .into_iter()
        .filter(|r| r.savings_entries > 0)
        .map(|r| {
            json!({
                "provider": r.provider,
                "entries": r.savings_entries,
                "tokensBefore": r.tokens_before,
                "tokensAfter": r.tokens_after,
            })
        })
        .collect();
    let wrapped = providers.iter().any(|p| p["wrapped"] == true);
    json!({
        "status": if calls > 0 { "data" } else if wrapped { "no_calls" } else { "off" },
        "basis": "mechanical",
        "providers": providers,
        "outputBytes": {
            "available": false,
            "reason": "A PreToolUse hook sees the call and its answer, not the tool output a rewrite later shrinks.",
        },
        "providerReported": reported,
        "population": {
            "hooks": providers.iter().filter(|p| p["wrapped"] == true).count(),
            "calls": calls,
            "from": providers.iter().filter_map(|p| p["firstCall"].as_str()).min(),
            "to": providers.iter().filter_map(|p| p["lastCall"].as_str()).max(),
        },
        "cannotTell": HOOKS_CANNOT_TELL,
    })
}

fn state_at(periods: &[Period], at: i64) -> Option<usize> {
    periods
        .iter()
        .position(|p| p.from.is_none_or(|f| f <= at) && p.to.is_none_or(|t| at < t))
}

struct Group<'a> {
    state: State,
    intervals: Vec<Value>,
    sessions: Vec<&'a Session>,
}

impl Group<'_> {
    fn costs(&self) -> Vec<f64> {
        let mut costs: Vec<f64> = self.sessions.iter().map(|s| s.cost).collect();
        costs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        costs
    }

    fn median(&self) -> Option<f64> {
        percentile(&self.costs(), 0.5)
    }
}

fn verdict(on: Option<&Group>, paused: Option<&Group>) -> Value {
    let Some(paused) = paused else {
        return json!({"status": "none", "reason": "No paused window yet: pause the hooks for a day to compare matched days."});
    };
    let on_count = on.map_or(0, |g| g.sessions.len());
    if on_count < MIN_VERDICT_SESSIONS || paused.sessions.len() < MIN_VERDICT_SESSIONS {
        return json!({
            "status": "refused",
            "reason": format!(
                "Needs at least {MIN_VERDICT_SESSIONS} sessions with {MIN_TURNS} or more turns in each window; hooks on has {on_count}, paused has {}.",
                paused.sessions.len()
            ),
            "needsSessions": MIN_VERDICT_SESSIONS,
        });
    }
    let (on_costs, paused_costs) = (on.map(Group::costs).unwrap_or_default(), paused.costs());
    let band = |costs: &[f64]| (percentile(costs, 0.25), percentile(costs, 0.75));
    let ((a_lo, a_hi), (b_lo, b_hi)) = (band(&on_costs), band(&paused_costs));
    let overlap = a_lo <= b_hi && b_lo <= a_hi;
    json!({
        "status": "compared",
        "overlap": overlap,
        "reason": if overlap {
            "The middle halves of both windows overlap: no difference can be told from these sessions."
        } else {
            "The middle halves of the windows do not overlap, which is a hint, not proof: the task mix differs between days."
        },
    })
}

fn outcome_and_rework(
    sessions: &[Session],
    periods: &[Period],
    min_turns: usize,
) -> (Value, Value) {
    let mut groups: Vec<Group> = [State::Off, State::On, State::Paused]
        .iter()
        .map(|state| Group {
            state: *state,
            intervals: periods
                .iter()
                .filter(|p| p.state == *state)
                .map(|p| json!({"from": p.from.map(day), "to": p.to.map(day)}))
                .collect(),
            sessions: Vec::new(),
        })
        .collect();
    let mut unplaced = 0;
    let qualifying: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.turns >= min_turns && s.cost > 0.0)
        .collect();
    for session in &qualifying {
        match session.start_ms.and_then(|at| state_at(periods, at)) {
            Some(i) => groups[periods[i].state as usize].sessions.push(session),
            None => unplaced += 1,
        }
    }
    groups.retain(|g| !g.intervals.is_empty());
    let reference = groups
        .iter()
        .find(|g| g.state == State::On)
        .and_then(Group::median);
    let rows: Vec<Value> = groups
        .iter()
        .map(|g| {
            let costs = g.costs();
            json!({
                "state": g.state.key(),
                "label": g.state.label(),
                "intervals": g.intervals,
                "sessions": g.sessions.len(),
                "medianCost": percentile(&costs, 0.5).map(|v| round(v, 0)),
                "lowerQuartile": percentile(&costs, 0.25).map(|v| round(v, 0)),
                "upperQuartile": percentile(&costs, 0.75).map(|v| round(v, 0)),
                "relative": reference
                    .filter(|r| *r > 0.0)
                    .zip(percentile(&costs, 0.5))
                    .map(|(r, m)| round(m / r * 100.0, 0)),
            })
        })
        .collect();
    let find = |state| groups.iter().find(|g| g.state == state);
    let population = json!({
        "sessions": qualifying.len(),
        "minTurns": min_turns,
        "unplaced": unplaced,
    });
    let outcome = json!({
        "status": if qualifying.is_empty() { "no_sessions" } else { "data" },
        "timeline": periods.iter().map(|p| json!({
            "state": p.state.key(),
            "label": p.state.label(),
            "from": p.from.map(day),
            "to": p.to.map(day),
        })).collect::<Vec<_>>(),
        "groups": rows,
        "referenceState": "on",
        "verdict": verdict(find(State::On), find(State::Paused)),
        "population": population,
        "costUnits": {
            "inputToken": WEIGHT_INPUT,
            "cacheWriteToken": WEIGHT_CACHE_WRITE,
            "cacheReadToken": WEIGHT_CACHE_READ,
            "outputToken": WEIGHT_OUTPUT,
            "relativeTo": "the median of the hooks-on window = 100",
        },
        "cannotTell": OUTCOME_CANNOT_TELL,
    });
    let rework = json!({
        "status": if qualifying.is_empty() { "no_sessions" } else { "data" },
        "groups": groups.iter().map(|g| json!({
            "state": g.state.key(),
            "label": g.state.label(),
            "sessions": g.sessions.len(),
            "repeatedCommands": mean(g.sessions.iter().map(|s| s.repeated_commands).sum(), g.sessions.len()),
            "repeatedReads": mean(g.sessions.iter().map(|s| s.repeated_reads).sum(), g.sessions.len()),
        })).collect::<Vec<_>>(),
        "population": population,
        "cannotTell": REWORK_CANNOT_TELL,
    });
    (outcome, rework)
}

/// `2026-11-01` or an RFC 3339 timestamp.
pub fn parse_since(text: &str) -> Option<i64> {
    parse_ts(&format!("{text}T00:00:00Z")).or_else(|| parse_ts(text))
}

#[derive(Clone, Debug)]
pub struct Options {
    pub root: PathBuf,
    pub min_turns: usize,
    pub since_ms: Option<i64>,
}

pub fn scan(root: &Path, since_ms: Option<i64>) -> Vec<Session> {
    iter_transcripts(root)
        .iter()
        .filter(|path| {
            since_ms.is_none_or(|since| session_origin(path).0.is_some_and(|at| at >= since))
        })
        .map(|path| scan_session(path))
        .filter(|s| s.turns > 0)
        .collect()
}

pub fn report(paths: &Paths, options: &Options) -> Result<Value, String> {
    let owned = hooks::read_ledger(paths)?;
    let sessions = scan(&options.root, options.since_ms);
    let periods = periods(&owned.epochs);
    let (outcome, rework) = outcome_and_rework(&sessions, &periods, options.min_turns);
    Ok(json!({
        "transcripts": {
            "root": options.root.to_string_lossy(),
            "sessions": sessions.len(),
            "since": options.since_ms.map(day),
        },
        "sources": sources(&sessions),
        "hooks": hooks_section(paths, &owned, options.since_ms),
        "outcome": outcome,
        "rework": rework,
        "upstream": {
            "note": "Upstream's tokens-saved figure counts tool-definition exposure of MCP servers only. It says nothing about Bash, Read or billed tokens and is never added to the figures here.",
        },
    }))
}
