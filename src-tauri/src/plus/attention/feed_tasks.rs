//! Tasks: the records a schedule or a failed login raised, a run that waits for you and the last
//! run of a task when it failed. Reads the task files only; nothing is started.

use super::{Ctx, Item, Level};
use crate::plus::tasks::cron;
use crate::plus::tasks::store::{self, Status};

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let mut items = Vec::new();
    for record in store::attention() {
        let level = Level::parse(&record.level).unwrap_or(Level::NeedsYou);
        let mut item = Item::new(ctx, record.id, level, "tasks", record.title, record.detail)
            .target("tasks", &[("tab", "tasks"), ("task", &record.task)]);
        if let Some(action) = record.action {
            let label = action["label"].as_str().unwrap_or("Open");
            let argv: Vec<&str> = action["command"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
            if !argv.is_empty() {
                item = item.action(label, &argv);
            }
        }
        if let Some(epoch) = cron::parse_rfc3339(&record.since) {
            item = item.since_epoch(epoch);
        }
        items.push(item);
    }
    let runs = store::list_runs(None).unwrap_or_default();
    let mut seen: Vec<&str> = Vec::new();
    for run in &runs {
        if seen.contains(&run.task.as_str()) {
            continue;
        }
        seen.push(&run.task);
        let (key, title, detail, label, argv): (&str, String, &str, &str, Vec<&str>) = match run.status {
            Status::Waiting => (
                "waiting",
                format!("Task {} is waiting for you", run.task),
                "It stopped at a step that only you can do. Continue it when you are ready.",
                "Continue",
                vec!["toolportctl", "task", "resume", &run.id],
            ),
            Status::Failed => (
                "failed",
                format!("Task {} failed", run.task),
                "Its last run ended with an error. The run history says at which step.",
                "Run again",
                vec!["toolportctl", "task", "run", &run.task],
            ),
            _ => continue,
        };
        let mut item = Item::new(ctx, format!("tasks:{}:{key}", run.task), Level::NeedsYou, "tasks", title, detail)
            .target("tasks", &[("tab", "tasks"), ("task", &run.task), ("run", &run.id)])
            .action(label, &argv);
        if let Some(epoch) = cron::parse_rfc3339(&run.started_at) {
            item = item.since_epoch(epoch);
        }
        items.push(item);
    }
    items
}
