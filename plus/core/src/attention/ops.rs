//! What the person does to the list: snooze, skip, dismiss, restore, undo, the switches, and the
//! network pass. The command line, the self-MCP tools and the app all call these, so a row is
//! hidden once and is gone everywhere.

use std::path::PathBuf;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::store::{HideKind, Store, CHECKS, STALE_DAYS_RANGE};
use super::{compute, read, updates, Item, Snapshot};
use crate::compression::hooks::{parse_duration, parse_until};
use crate::op::OpError;

pub const DEFAULT_SNOOZE_SECS: i64 = 24 * 3600;
pub const UNDO_COMMAND: [&str; 3] = ["toolportctl", "attention", "undo"];

fn base(host: &dyn Host) -> Result<PathBuf, OpError> {
    host.data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "data directory could not be resolved"))
}

fn open_store(host: &dyn Host) -> Result<(PathBuf, Store), OpError> {
    let base = base(host)?;
    let store = Store::read(&base).map_err(|e| OpError::failed("attention_store", e))?;
    Ok((base, store))
}

fn save(base: &std::path::Path, store: &Store) -> Result<(), OpError> {
    store
        .write(base)
        .map_err(|e| OpError::failed("attention_store", e))
}

fn live(snapshot: &Snapshot) -> Vec<&str> {
    snapshot
        .items
        .iter()
        .chain(snapshot.hidden.iter().map(|h| &h.item))
        .map(|item| item.id.as_str())
        .collect()
}

fn find(snapshot: &Snapshot, id: &str) -> Result<Item, OpError> {
    snapshot
        .items
        .iter()
        .chain(snapshot.hidden.iter().map(|h| &h.item))
        .find(|item| item.id == id)
        .cloned()
        .ok_or_else(|| {
            OpError::not_found(format!(
                "no row {id}; list the ids with `toolportctl attention ls`"
            ))
        })
}

/// When a snooze ends: `--for` is a duration (`1h`, `2d`), `--until` a timestamp with an offset,
/// without an offset it is UTC. Neither is a day.
pub fn snooze_end(duration: Option<&str>, until: Option<&str>, now: i64) -> Result<i64, OpError> {
    match (duration, until) {
        (Some(_), Some(_)) => Err(OpError::usage("pass either --for or --until, not both")),
        (Some(text), None) => parse_duration(text)
            .map(|ms| now + ms / 1000)
            .ok_or_else(|| {
                OpError::usage(format!(
                    "'{text}' is not a duration (use 1h, 2d, 90m or 1d12h)"
                ))
            }),
        (None, Some(text)) => parse_until(text).map(|ms| ms / 1000).ok_or_else(|| {
            OpError::usage(format!(
                "'{text}' is not a time (use 2026-11-18T09:12+01:00, 2026-11-18T09:12 in UTC or a date)"
            ))
        }),
        (None, None) => Ok(now + DEFAULT_SNOOZE_SECS),
    }
}

pub fn hide(
    host: &dyn Host,
    id: &str,
    kind: HideKind,
    until: Option<i64>,
    dry_run: bool,
) -> Result<Value, OpError> {
    let (base, mut store) = open_store(host)?;
    let now = host.now_unix();
    let snapshot = compute(&read(host));
    let item = find(&snapshot, id)?;
    store
        .hide(&item, kind, until, now)
        .map_err(OpError::usage)?;
    if !dry_run {
        store.tidy(&live(&snapshot), now);
        save(&base, &store)?;
    }
    let mut data = json!({
        "id": id,
        "title": item.title,
        "hidden": store.hidden[id].to_json(),
        "dryRun": dry_run,
        "undo": UNDO_COMMAND,
    });
    data["hidden"]["state"] = json!(kind.word());
    Ok(data)
}

pub fn restore(host: &dyn Host, id: &str) -> Result<Value, OpError> {
    let (base, mut store) = open_store(host)?;
    if !store.restore(id) {
        return Err(OpError::not_found(format!(
            "{id} is not hidden; list the hidden rows with `toolportctl attention ls --hidden`"
        )));
    }
    save(&base, &store)?;
    Ok(json!({"id": id, "restored": true, "undo": UNDO_COMMAND}))
}

pub fn undo(host: &dyn Host) -> Result<Value, OpError> {
    let (base, mut store) = open_store(host)?;
    let id = store
        .undo()
        .ok_or_else(|| OpError::not_found("there is nothing to undo"))?;
    save(&base, &store)?;
    Ok(json!({"id": id, "undone": true}))
}

pub fn settings(host: &dyn Host) -> Result<Value, OpError> {
    let (base, store) = open_store(host)?;
    let mut settings = store.settings.to_json();
    settings["notify"]["permission"] = super::notifier_state::read(&base);
    Ok(settings)
}

/// `check.<name>` switches a check, `notify` the master switch and `notify.<name>` one category.
pub fn set_switch(host: &dyn Host, key: &str, on: bool) -> Result<Value, OpError> {
    let (base, mut store) = open_store(host)?;
    let named = |prefix: &str| {
        key.strip_prefix(prefix)
            .filter(|name| CHECKS.contains(name))
            .map(str::to_string)
    };
    if key == "notify" {
        store.settings.notify.enabled = on;
    } else if let Some(name) = named("check.") {
        store.settings.checks.insert(name, on);
    } else if let Some(name) = named("notify.") {
        store.settings.notify.categories.insert(name, on);
    } else {
        return Err(OpError::usage(format!(
            "unknown switch {key}; use notify, check.<name> or notify.<name> with one of {}",
            CHECKS.join(", ")
        )));
    }
    save(&base, &store)?;
    Ok(store.settings.to_json())
}

/// Days without a successful call after which a server that an access set reaches is stale.
pub fn set_stale_days(host: &dyn Host, days: u32) -> Result<Value, OpError> {
    if !STALE_DAYS_RANGE.contains(&days) {
        return Err(OpError::usage(format!(
            "stale-days is a number of days from {} to {}",
            STALE_DAYS_RANGE.start(),
            STALE_DAYS_RANGE.end()
        )));
    }
    let (base, mut store) = open_store(host)?;
    store.settings.stale_days = Some(days);
    save(&base, &store)?;
    Ok(store.settings.to_json())
}

pub fn check(host: &dyn Host) -> Result<updates::Report, OpError> {
    let _ = super::observe_access(host);
    let base = base(host)?;
    let base = crate::skills::repo::resolve_path(&base);
    let _paths = crate::fsx::enter(crate::fsx::Paths {
        home: None,
        data_dir: Some(base.clone()),
    });
    let servers: Vec<(String, PathBuf)> = crate::servers::repo::repos(host)?
        .into_iter()
        .filter(|repo| repo.state.remote.is_some())
        .map(|repo| (repo.source(), repo.state.root))
        .collect();
    let packages = crate::servers::package::targets(&crate::servers::package::pins(
        &crate::servers::read_servers(host)?,
    ));
    updates::check_with(
        &base,
        host.now_unix(),
        &updates::fetch,
        &servers,
        &crate::servers::repo::fetch_tracked,
        &packages,
        &|target| crate::servers::package::lookup::latest(host, target),
    )
    .map_err(|e| OpError::failed("attention_check", e))
}

pub fn hidden_json(snapshot: &Snapshot) -> Value {
    json!(snapshot
        .hidden
        .iter()
        .map(|h| h.to_json())
        .collect::<Vec<_>>())
}
