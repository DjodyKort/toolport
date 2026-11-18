use crate::tasks::builtin::SKILLS_AUTO_SYNC;
use crate::tasks::cron::parse_rfc3339;
use crate::tasks::schedule::{MissedRecord, OUTCOME_TOO_OLD};
use crate::tasks::store::{AttentionRecord, Run, Status};

use super::feed_credentials::one_line;
use super::model::{Action, Item, Screen, Urgency};

const SOURCE: &str = "Tasks";
const DAY: i64 = 86_400;
/// A failure older than this is history, not something waiting for the person.
const FAILED_WINDOW_SECS: i64 = 7 * DAY;
const MISSED_WINDOW_SECS: i64 = 3 * DAY;
/// Two failures in a row mean the schedule's own retry did not help.
const FAILED_IN_A_ROW: usize = 2;

pub struct TaskFacts {
    pub id: String,
    pub title: String,
    pub enabled: bool,
}

pub struct TaskInputs {
    pub tasks: Vec<TaskFacts>,
    /// Newest first.
    pub runs: Vec<Run>,
    pub missed: Vec<MissedRecord>,
    pub records: Vec<AttentionRecord>,
}

fn started(run: &Run) -> Option<i64> {
    parse_rfc3339(&run.started_at)
}

fn ended(run: &Run) -> Option<i64> {
    run.ended_at
        .as_deref()
        .and_then(parse_rfc3339)
        .or_else(|| started(run))
}

fn waiting(task: &TaskFacts, run: &Run) -> Item {
    let at = run
        .steps
        .iter()
        .position(|step| step.status == Status::Waiting);
    let detail = match at {
        Some(index) => format!(
            "Stopped at step {} of {}: {}.",
            index + 1,
            run.steps.len(),
            run.steps[index].title
        ),
        None => "It stopped at a step that only you can do.".to_string(),
    };
    let since = at
        .and_then(|index| run.steps[index].started_at.as_deref())
        .and_then(parse_rfc3339)
        .or_else(|| started(run));
    Item {
        id: format!("tasks:waiting:{}", run.id),
        urgency: Urgency::NeedsYou,
        title: format!("Task {} is waiting for you", task.title),
        detail,
        source: SOURCE,
        skip: None,
        since,
        action: Some(Action::run(
            "Continue",
            Screen::Tasks,
            &["toolportctl", "task", "resume", &run.id],
        )),
    }
}

/// The failed runs a task ended with, newest first, until a run that did not fail.
fn failed_streak<'a>(runs: &[&'a Run]) -> Vec<&'a Run> {
    runs.iter()
        .copied()
        .filter(|run| run.status.finished())
        .take_while(|run| run.status == Status::Failed)
        .collect()
}

fn failed(task: &TaskFacts, streak: &[&Run], now: i64) -> Option<Item> {
    let last = streak.first()?;
    let at = ended(last)?;
    if now - at > FAILED_WINDOW_SECS {
        return None;
    }
    let reason = last
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| step.status == Status::Failed)
        .map(|(index, step)| {
            format!(
                "Step {} of {} ({}) ended with an error.",
                index + 1,
                last.steps.len(),
                step.title
            )
        })
        .or_else(|| last.error.as_deref().map(one_line))
        .unwrap_or_else(|| "The last run ended with an error.".to_string());
    let repeated = streak.len() >= FAILED_IN_A_ROW;
    Some(Item {
        id: format!("tasks:failed:{}", task.id),
        urgency: if repeated {
            Urgency::NeedsYou
        } else {
            Urgency::WorthALook
        },
        title: if repeated {
            format!("Task {} failed {} times in a row", task.title, streak.len())
        } else {
            format!("Task {} failed", task.title)
        },
        detail: reason,
        source: SOURCE,
        skip: None,
        since: Some(at),
        action: Some(Action::run(
            "Run again",
            Screen::Tasks,
            &["toolportctl", "task", "run", &task.id],
        )),
    })
}

fn missed(task: &TaskFacts, runs: &[&Run], records: &[&MissedRecord], now: i64) -> Option<Item> {
    let record = records
        .iter()
        .filter(|m| m.outcome == OUTCOME_TOO_OLD)
        .filter_map(|m| Some((parse_rfc3339(&m.at)?, *m)))
        .max_by_key(|(at, _)| *at)?;
    let (at, record) = record;
    let ran_since = runs.iter().any(|run| started(run).is_some_and(|s| s >= at));
    if ran_since || now - at > MISSED_WINDOW_SECS {
        return None;
    }
    let what = if record.slots == 1 {
        "1 scheduled run".to_string()
    } else {
        format!("{} scheduled runs", record.slots)
    };
    Some(Item {
        id: format!("tasks:missed:{}", task.id),
        urgency: Urgency::WorthALook,
        title: format!("Task {} missed {what}", task.title),
        detail: format!(
            "Due {} to {} while Toolport was not running, too long ago to catch up.",
            record.first_due, record.last_due
        ),
        source: SOURCE,
        skip: None,
        since: Some(at),
        action: Some(Action::run(
            "Run now",
            Screen::Tasks,
            &["toolportctl", "task", "run", &task.id],
        )),
    })
}

fn raised(record: &AttentionRecord, now: i64) -> Item {
    let command = record
        .action
        .as_ref()
        .and_then(|action| action.get("command"))
        .and_then(|command| command.as_array())
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.as_str().map(String::from))
                .collect::<Vec<_>>()
        });
    let label = record
        .action
        .as_ref()
        .and_then(|action| action.get("label"))
        .and_then(|label| label.as_str())
        .unwrap_or("Open Tasks");
    Item {
        id: record.id.clone(),
        urgency: if record.level == "needs-you" {
            Urgency::NeedsYou
        } else {
            Urgency::WorthALook
        },
        title: record.title.clone(),
        detail: record.detail.clone(),
        source: SOURCE,
        skip: None,
        since: parse_rfc3339(&record.since).map(|at| at.min(now)),
        action: Some(Action {
            label: label.to_string(),
            screen: Screen::Tasks,
            command,
        }),
    }
}

/// The library's own unattended sync is a task too; the Library feed reports it, so it is left
/// out here and a failed sync is one row, not two.
pub fn items(inputs: &TaskInputs, now: i64) -> Vec<Item> {
    let mut out = Vec::new();
    for task in inputs.tasks.iter().filter(|t| t.id != SKILLS_AUTO_SYNC) {
        let runs: Vec<&Run> = inputs.runs.iter().filter(|r| r.task == task.id).collect();
        for run in runs.iter().filter(|r| r.status == Status::Waiting) {
            out.push(waiting(task, run));
        }
        if task.enabled {
            out.extend(failed(task, &failed_streak(&runs), now));
            let ledger: Vec<&MissedRecord> =
                inputs.missed.iter().filter(|m| m.task == task.id).collect();
            out.extend(missed(task, &runs, &ledger, now));
        }
    }
    out.extend(
        inputs
            .records
            .iter()
            .filter(|r| r.task != SKILLS_AUTO_SYNC)
            .filter(|r| inputs.tasks.iter().any(|t| t.id == r.task))
            .map(|r| raised(r, now)),
    );
    out
}
