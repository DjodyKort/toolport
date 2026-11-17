//! The scheduler: a ledger of the last cron slot handled per task, the due computation and
//! the catch-up of slots a quiet period let pass.
//!
//! Two kinds of process tick: the desktop app's thread and every gateway process (the shared
//! daemon exits after 300 s idle, so the app is the always-on one). Both run [`tick`], which
//! reads and rewrites the ledger under one lock, so a slot is handled by exactly one of them.
//! A run itself starts under a per-task lock (`triggers::start`), so a manual start and a
//! scheduled one cannot double up either.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use toolport_plus_api::TaskHost;

use super::cron::{parse_rfc3339, rfc3339, Cron};
use super::model::Task;
use super::store::{self, Missed};
use super::triggers::{self, Raised};
use crate::op::OpError;

/// A slot older than this is "missed": the process was not there when it was due.
pub const MISSED_GRACE_SECS: i64 = 120;
/// Slots missed longer ago than this are recorded and not run; run the task by hand.
pub const CATCH_UP_WINDOW_SECS: i64 = 24 * 3600;
/// Catch-up runs one tick may start; the remaining tasks follow on the next ticks.
pub const MAX_CATCH_UPS_PER_TICK: usize = 3;
const MAX_LEDGER_MISSED: usize = 50;
const SLOT_SCAN_CAP: usize = 5000;
const LEDGER_WAIT: Duration = Duration::from_secs(2);
pub const APP_TICK_EVERY: Duration = Duration::from_secs(20);

/// Who ticked: the desktop app (every [`APP_TICK_EVERY`]) or a gateway process (once a minute).
pub const TICKER_APP: &str = "app";
pub const TICKER_GATEWAY: &str = "gateway";

pub const OUTCOME_CAUGHT_UP: &str = "caught-up";
pub const OUTCOME_TOO_OLD: &str = "too-old";
pub const OUTCOME_BUSY: &str = "busy";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    handled_slot: i64,
}

/// One scheduled slot, or a run of them, that did not start a run on time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissedRecord {
    pub task: String,
    pub slots: u32,
    pub first_due: String,
    pub last_due: String,
    pub outcome: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    #[serde(default)]
    tasks: BTreeMap<String, Entry>,
    #[serde(default)]
    pub missed: Vec<MissedRecord>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    pub raised: Vec<Raised>,
    pub missed: Vec<MissedRecord>,
}

pub fn disabled() -> bool {
    std::env::var("TOOLPORT_TASKS_SCHEDULER")
        .is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

/// The last time a process looked at the schedule, for the "last checked" line of the screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    pub at: String,
    pub by: String,
    pub interval_secs: u64,
}

fn heartbeat_path(base: &Path) -> PathBuf {
    store::plus_dir(base).join("task-heartbeat.json")
}

pub fn read_heartbeat(base: &Path) -> Option<Heartbeat> {
    serde_json::from_str(&std::fs::read_to_string(heartbeat_path(base)).ok()?).ok()
}

fn beat(base: &Path, now: i64, by: &str) {
    let interval = if by == TICKER_APP {
        APP_TICK_EVERY.as_secs()
    } else {
        60
    };
    let beat = Heartbeat {
        at: rfc3339(now),
        by: by.into(),
        interval_secs: interval,
    };
    let text = serde_json::to_string(&beat).unwrap_or_default() + "\n";
    let _ = store::write(&heartbeat_path(base), &text);
}

pub fn ledger_path(base: &Path) -> PathBuf {
    store::plus_dir(base).join("task-schedule.json")
}

fn ledger_lock_path(base: &Path) -> PathBuf {
    store::plus_dir(base).join(".task-schedule.lock")
}

pub fn read_ledger(base: &Path) -> Ledger {
    std::fs::read_to_string(ledger_path(base))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn scheduled(base: &Path) -> Vec<(Task, Cron)> {
    store::list_tasks(base)
        .into_iter()
        .flatten()
        .filter(|t| t.enabled)
        .filter_map(|t| {
            let cron = Cron::parse(&t.triggers.schedule.as_ref()?.cron).ok()?;
            Some((t, cron))
        })
        .collect()
}

/// Where a task seen for the first time counts from: its newest scheduled run, so work missed
/// before the ledger existed is caught up, or now, so a new task is not back-filled.
fn first_sight(base: &Path, task: &Task, now: i64) -> i64 {
    store::list_runs(base, Some(&task.id))
        .iter()
        .filter(|r| r.trigger == "schedule" || r.trigger == "catch-up")
        .find_map(|r| parse_rfc3339(&r.started_at))
        .map_or(now, |started| started.min(now))
}

pub fn tick(host: &dyn TaskHost) -> TickReport {
    tick_as(host, TICKER_GATEWAY)
}

pub fn tick_as(host: &dyn TaskHost, by: &str) -> TickReport {
    let mut report = TickReport::default();
    host.unattended(&mut || report = tick_pass(host, by));
    report
}

fn tick_pass(host: &dyn TaskHost, by: &str) -> TickReport {
    let mut report = TickReport::default();
    if disabled() {
        return report;
    }
    let Some(base) = host.tasks_base() else {
        return report;
    };
    let due = scheduled(&base);
    if due.is_empty() {
        return report;
    }
    beat(&base, host.now(), by);
    let Some(_guard) = store::lock(&ledger_lock_path(&base), LEDGER_WAIT) else {
        return report;
    };
    let now = host.now();
    let mut ledger = read_ledger(&base);
    let before = ledger.clone();
    let mut catch_ups = 0;
    for (task, cron) in &due {
        let Some(entry) = ledger.tasks.get(&task.id).cloned() else {
            let start = first_sight(&base, task, now);
            ledger.tasks.insert(
                task.id.clone(),
                Entry {
                    handled_slot: start,
                },
            );
            if start == now {
                continue;
            }
            handle(
                host,
                &base,
                task,
                cron,
                start,
                now,
                &mut ledger,
                &mut catch_ups,
                &mut report,
            );
            continue;
        };
        handle(
            host,
            &base,
            task,
            cron,
            entry.handled_slot,
            now,
            &mut ledger,
            &mut catch_ups,
            &mut report,
        );
    }
    if ledger != before {
        let overflow = ledger.missed.len().saturating_sub(MAX_LEDGER_MISSED);
        ledger.missed.drain(..overflow);
        let text = serde_json::to_string_pretty(&ledger).unwrap_or_default() + "\n";
        let _ = store::write(&ledger_path(&base), &text);
    }
    report
}

#[allow(clippy::too_many_arguments)]
fn handle(
    host: &dyn TaskHost,
    base: &Path,
    task: &Task,
    cron: &Cron,
    after: i64,
    now: i64,
    ledger: &mut Ledger,
    catch_ups: &mut usize,
    report: &mut TickReport,
) {
    let slots = cron.slots_in(after, now, SLOT_SCAN_CAP);
    let (Some(&first), Some(&last)) = (slots.first(), slots.last()) else {
        return;
    };
    let late = slots.iter().any(|s| now - s > MISSED_GRACE_SECS);
    if late && *catch_ups >= MAX_CATCH_UPS_PER_TICK {
        return;
    }
    ledger
        .tasks
        .insert(task.id.clone(), Entry { handled_slot: last });
    let count = u32::try_from(slots.len()).unwrap_or(u32::MAX);
    let record = |outcome: &str, run: Option<String>| MissedRecord {
        task: task.id.clone(),
        slots: count,
        first_due: rfc3339(first),
        last_due: rfc3339(last),
        outcome: outcome.into(),
        at: rfc3339(now),
        run,
    };
    if late && now - last > CATCH_UP_WINDOW_SECS {
        let missed = record(OUTCOME_TOO_OLD, None);
        ledger.missed.push(missed.clone());
        report.missed.push(missed);
        return;
    }
    if triggers::allowed(task, "schedule").is_err() {
        let _ = triggers::attention(
            base,
            task,
            "due",
            format!("task {} is due", task.id),
            triggers::refused_detail(task),
            now,
        );
        report.raised.push(Raised::Attention(task.id.clone()));
        return;
    }
    let trigger = if late { "catch-up" } else { "schedule" };
    let note = late.then(|| Missed {
        slots: count,
        first_due: rfc3339(first),
        last_due: rfc3339(last),
    });
    match triggers::start(task, trigger, note, host) {
        Ok(run) => {
            if late {
                *catch_ups += 1;
                let missed = record(OUTCOME_CAUGHT_UP, Some(run.id.clone()));
                ledger.missed.push(missed.clone());
                report.missed.push(missed);
            }
            report.raised.push(Raised::Started(run.id));
        }
        Err(OpError { message, .. }) => {
            let missed = record(OUTCOME_BUSY, None);
            let _ = message;
            ledger.missed.push(missed.clone());
            report.missed.push(missed);
        }
    }
}

static LAST_MINUTE: AtomicI64 = AtomicI64::new(i64::MIN);

/// True once per wall-clock minute per process, so a loop that calls every second ticks once.
pub fn claim_minute(epoch: i64) -> bool {
    let minute = epoch.div_euclid(60);
    LAST_MINUTE.swap(minute, Ordering::SeqCst) != minute
}

/// For a loop that calls often (the gateway watch loop): at most one tick a minute, on its own
/// thread, so the caller never waits for the ledger lock or a spawn. The handle is for tests; the
/// loop lets it go. `None` when this call did not tick.
pub fn tick_in_thread<H: TaskHost + Send + 'static>(
    host: H,
) -> Option<std::thread::JoinHandle<()>> {
    if disabled() || !claim_minute(host.now()) {
        return None;
    }
    std::thread::Builder::new()
        .name("task-schedule".into())
        .spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tick(&host);
            }));
        })
        .ok()
}

/// The desktop app's ticker: the first pass runs at once, so a run missed while the app was off
/// is caught up at launch, then every [`APP_TICK_EVERY`].
pub fn spawn_app_ticker<H: TaskHost + Send + 'static>(host: H) {
    let _ = std::thread::Builder::new()
        .name("task-app-ticker".into())
        .spawn(move || loop {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tick_as(&host, TICKER_APP);
            }));
            std::thread::sleep(APP_TICK_EVERY);
        });
}

/// Scheduled, enabled tasks that would start on their own, for the doctor's "launch at login"
/// warning.
pub fn auto_scheduled_count(base: &Path) -> usize {
    scheduled(base)
        .iter()
        .filter(|(task, _)| triggers::auto_runs(task))
        .count()
}

/// Counts a task from now on: a task turned on again does not catch up the slots that came
/// due while it was off.
pub fn skip_ahead(host: &dyn TaskHost, id: &str) {
    let Some(base) = host.tasks_base() else {
        return;
    };
    let Some(_guard) = store::lock(&ledger_lock_path(&base), LEDGER_WAIT) else {
        return;
    };
    let mut ledger = read_ledger(&base);
    ledger.tasks.insert(
        id.to_string(),
        Entry {
            handled_slot: host.now(),
        },
    );
    let text = serde_json::to_string_pretty(&ledger).unwrap_or_default() + "\n";
    let _ = store::write(&ledger_path(&base), &text);
}
