//! One list of what wants a decision (MIG-GUI-11, contract section 9). Every feed is a function
//! over local state that returns items; a feed that fails, panics or has nothing contributes
//! nothing and never fails the list. Reading is offline: no network call and no engine process.
//! Only the dismissals are stored (`<data dir>/plus/attention.json`).

pub mod dismissals;
mod feed_auth;
mod feed_compression;
mod feed_context;
mod feed_hooks;
mod feed_library;
mod feed_plugins;
mod feed_secrets;
mod feed_sources;
mod feed_tasks;

#[cfg(test)]
mod tests;

use crate::plus::op::OpError;
use crate::plus::tasks::cron;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    NeedsYou,
    Look,
    Fyi,
}

impl Level {
    pub const NAMES: [&'static str; 3] = ["needs-you", "look", "fyi"];

    pub fn parse(text: &str) -> Option<Level> {
        match text {
            "needs-you" => Some(Level::NeedsYou),
            "look" => Some(Level::Look),
            "fyi" => Some(Level::Fyi),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Target {
    pub route: String,
    pub params: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Action {
    pub label: String,
    pub command: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    pub id: String,
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub from: &'static str,
    pub target: Target,
    pub action: Option<Action>,
    pub since: String,
}

impl Item {
    pub fn new(ctx: &Ctx, id: impl Into<String>, level: Level, from: &'static str, title: impl Into<String>, detail: impl Into<String>) -> Item {
        Item {
            id: id.into(),
            level,
            title: title.into(),
            detail: detail.into(),
            from,
            target: Target { route: String::new(), params: BTreeMap::new() },
            action: None,
            since: cron::rfc3339(ctx.now),
        }
    }

    pub fn target(mut self, route: &str, params: &[(&str, &str)]) -> Item {
        self.target = Target {
            route: route.to_string(),
            params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        self
    }

    pub fn action(mut self, label: &str, argv: &[&str]) -> Item {
        self.action = Some(Action { label: label.to_string(), command: argv.iter().map(|a| a.to_string()).collect() });
        self
    }

    pub fn since_epoch(mut self, epoch: i64) -> Item {
        self.since = cron::rfc3339(epoch);
        self
    }
}

pub struct Ctx {
    pub now: i64,
}

impl Ctx {
    pub fn system() -> Ctx {
        Ctx { now: cron::now() }
    }

    pub fn today(&self) -> String {
        cron::rfc3339(self.now)[..10].to_string()
    }
}

pub type Feed = fn(&Ctx) -> Vec<Item>;

/// A new item adds its feed here and in its own file; the others stay as they are.
pub const FEEDS: &[(&str, Feed)] = &[
    ("auth", feed_auth::collect),
    ("tasks", feed_tasks::collect),
    ("secrets", feed_secrets::collect),
    ("sources", feed_sources::collect),
    ("context", feed_context::collect),
    ("plugins", feed_plugins::collect),
    ("hooks", feed_hooks::collect),
    ("compression", feed_compression::collect),
    ("library", feed_library::collect),
];

pub fn collect(ctx: &Ctx, feeds: &[(&str, Feed)]) -> Vec<Item> {
    let mut items = Vec::new();
    for (_, feed) in feeds {
        let feed = *feed;
        if let Ok(found) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| feed(ctx))) {
            for item in found {
                if !items.iter().any(|seen: &Item| seen.id == item.id) {
                    items.push(item);
                }
            }
        }
    }
    items
}

fn rank(level: Level) -> u8 {
    match level {
        Level::NeedsYou => 0,
        Level::Look => 1,
        Level::Fyi => 2,
    }
}

pub fn ls(ctx: &Ctx, feeds: &[(&str, Feed)], level: Option<Level>) -> Result<Value, OpError> {
    let hidden = dismissals::load(&ctx.today());
    let mut items: Vec<Item> = collect(ctx, feeds).into_iter().filter(|i| !hidden.contains_key(&i.id)).collect();
    items.sort_by(|a, b| rank(a.level).cmp(&rank(b.level)).then(a.since.cmp(&b.since)).then(a.id.cmp(&b.id)));
    let count = |l: Level| items.iter().filter(|i| i.level == l).count();
    let counts = json!({"needsYou": count(Level::NeedsYou), "look": count(Level::Look), "fyi": count(Level::Fyi)});
    let shown: Vec<&Item> = items.iter().filter(|i| level.is_none_or(|l| i.level == l)).collect();
    Ok(json!({"counts": counts, "items": shown}))
}

pub fn ls_host(level: Option<Level>) -> Result<Value, OpError> {
    ls(&Ctx::system(), FEEDS, level)
}

pub fn human(data: &Value) -> String {
    let counts = &data["counts"];
    let mut out = format!(
        "needs you {}, worth a look {}, for your information {}\n",
        counts["needsYou"], counts["look"], counts["fyi"]
    );
    for item in data["items"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<9} {}\n            {}\n",
            item["level"].as_str().unwrap_or(""),
            item["title"].as_str().unwrap_or(""),
            item["detail"].as_str().unwrap_or("")
        ));
    }
    if data["items"].as_array().is_none_or(|a| a.is_empty()) {
        out.push_str("Nothing needs a decision.\n");
    }
    out
}

pub(crate) fn home_short(path: &str) -> String {
    match crate::clients::home() {
        Some(home) => match path.strip_prefix(home.to_string_lossy().as_ref()) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
            _ => path.to_string(),
        },
        None => path.to_string(),
    }
}
