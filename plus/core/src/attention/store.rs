//! `<data dir>/plus/attention.json`: what the person told the list (snooze, skip, dismiss and the
//! one step to undo), what the notifier already announced, the switches, and when the one check
//! that asks the network last ran. Rows themselves are never stored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::model::{Item, Urgency};
use crate::fsx::{atomic_write, Mode};
use crate::tasks::cron::rfc3339;

const FILE: &str = "attention.json";
pub const NEEDS_YOU_SNOOZE_MAX: i64 = 7 * 24 * 3600;
const UNSEEN_KEEP: i64 = 30 * 24 * 3600;
/// One notification per row per this many seconds.
pub const NOTIFY_WINDOW_SECS: i64 = 6 * 3600;

/// Checks that exist as a switch, in the order the Manage page shows them.
pub const CHECKS: [&str; 5] = ["credentials", "updates", "access", "tasks", "library"];

/// Whether a category notifies when the master switch is on and the person has not chosen:
/// update and library rows are never urgent enough to wake the person.
pub fn notifies_by_default(category: &str) -> bool {
    matches!(category, "credentials" | "tasks")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HideKind {
    Snooze,
    Skip,
    Dismiss,
}

impl HideKind {
    pub fn word(self) -> &'static str {
        match self {
            HideKind::Snooze => "snoozed",
            HideKind::Skip => "skipped",
            HideKind::Dismiss => "dismissed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hide {
    pub kind: HideKind,
    /// Epoch seconds a snooze ends at; a skip and a dismissal have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<i64>,
    /// The urgency the row had when it was hidden: a row that rises past it comes back.
    pub urgency: Urgency,
    pub at: i64,
    /// The last time a feed returned the row; entries nobody returns for 30 days are dropped.
    #[serde(default)]
    pub seen: i64,
}

impl Hide {
    pub fn to_json(&self) -> Value {
        let mut value = json!({"kind": self.kind, "at": rfc3339(self.at)});
        if let Some(until) = self.until {
            value["until"] = json!(rfc3339(until));
        }
        value
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Undo {
    pub id: String,
    /// What the id held before the last change; `None` when it held nothing.
    pub previous: Option<Hide>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCheck {
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesState {
    #[serde(default)]
    pub checked_at: Option<i64>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceCheck>,
}

/// What the Access check last saw of the registry, so a server that appeared after the access
/// sets last changed can be told from one that has always been unreached.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessLedger {
    /// When the registry was first observed; none of its servers count as added before this.
    #[serde(default)]
    pub observed_at: Option<i64>,
    /// Server id to the time it was first seen; servers present at the first observation are 0.
    #[serde(default)]
    pub servers: BTreeMap<String, i64>,
    /// Set id to what it lists, to notice a set that changed.
    #[serde(default)]
    pub sets: BTreeMap<String, String>,
    #[serde(default)]
    pub sets_changed_at: Option<i64>,
}

impl AccessLedger {
    /// The fingerprint of a set: its name and members, so renaming and editing both count.
    pub fn fingerprint(name: &str, server_ids: &[String]) -> String {
        let mut ids: Vec<&str> = server_ids.iter().map(String::as_str).collect();
        ids.sort_unstable();
        format!("{name}\n{}", ids.join(","))
    }

    /// When `server` first appeared, if that was after the last change to any access set. A set
    /// that differs from the last observation has just changed. Nothing counts as added before
    /// the first observation.
    pub fn added_after_sets(
        &self,
        server: &str,
        sets: &BTreeMap<String, String>,
        now: i64,
    ) -> Option<i64> {
        self.observed_at?;
        let changed = if &self.sets == sets {
            self.sets_changed_at.unwrap_or(0)
        } else {
            now
        };
        let first_seen = self.servers.get(server).copied().unwrap_or(now);
        (first_seen > changed).then_some(first_seen)
    }

    /// Records the registry; true when the ledger changed and has to be written.
    pub fn observe(
        &mut self,
        servers: &[String],
        sets: BTreeMap<String, String>,
        now: i64,
    ) -> bool {
        let before = self.clone();
        let first = self.observed_at.is_none();
        if first {
            self.observed_at = Some(now);
            self.sets_changed_at = Some(now);
        } else if self.sets != sets {
            self.sets_changed_at = Some(now);
        }
        self.sets = sets;
        let known = std::mem::take(&mut self.servers);
        self.servers = servers
            .iter()
            .map(|id| {
                let seen = known
                    .get(id)
                    .copied()
                    .unwrap_or(if first { 0 } else { now });
                (id.clone(), seen)
            })
            .collect();
        *self != before
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notify {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub categories: BTreeMap<String, bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub checks: BTreeMap<String, bool>,
    #[serde(default)]
    pub notify: Notify,
}

impl Settings {
    pub fn check_on(&self, name: &str) -> bool {
        self.checks.get(name).copied().unwrap_or(true)
    }

    pub fn category_on(&self, category: &str) -> bool {
        self.notify
            .categories
            .get(category)
            .copied()
            .unwrap_or_else(|| notifies_by_default(category))
    }

    pub fn to_json(&self) -> Value {
        let checks: Vec<Value> = CHECKS
            .iter()
            .map(|name| json!({"name": name, "on": self.check_on(name)}))
            .collect();
        let categories: Vec<Value> = CHECKS
            .iter()
            .map(|name| json!({"name": name, "on": self.category_on(name)}))
            .collect();
        json!({
            "checks": checks,
            "notify": {"enabled": self.notify.enabled, "categories": categories},
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub hidden: BTreeMap<String, Hide>,
    #[serde(default)]
    pub undo: Option<Undo>,
    #[serde(default)]
    pub notified: BTreeMap<String, i64>,
    /// The needs-you ids the last notifier pass saw; only an id that was not there is news.
    #[serde(default)]
    pub announced: Option<Vec<String>>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub updates: UpdatesState,
    #[serde(default)]
    pub access: AccessLedger,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

pub fn path(base: &Path) -> PathBuf {
    base.join("plus").join(FILE)
}

impl Store {
    /// No file is an empty store; a file that cannot be read is an error, so rows it may hide
    /// are not shown as if nothing were hidden.
    pub fn read(base: &Path) -> Result<Store, String> {
        let file = path(base);
        match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| format!("{} is not valid: {e}", file.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
            Err(e) => Err(format!("{}: {e}", file.display())),
        }
    }

    pub fn write(&self, base: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).unwrap_or_default() + "\n";
        atomic_write(&path(base), &text, Mode::Private)
            .map_err(|e| format!("{}: {e}", path(base).display()))
    }

    /// The entry that keeps `item` out of the list right now.
    pub fn hiding(&self, item: &Item, now: i64) -> Option<&Hide> {
        let hide = self.hidden.get(&item.id)?;
        let live = match hide.kind {
            HideKind::Snooze => {
                hide.until.is_some_and(|until| until > now) && hide.urgency == item.urgency
            }
            HideKind::Skip => item.skippable() && item.urgency == Urgency::WorthALook,
            HideKind::Dismiss => !item.skippable() && item.urgency == Urgency::WorthALook,
        };
        live.then_some(hide)
    }

    /// Hides `item`. A needs-you row can be snoozed for at most a week and never skipped or
    /// dismissed; a skip needs an id that carries the state it skips.
    pub fn hide(
        &mut self,
        item: &Item,
        kind: HideKind,
        until: Option<i64>,
        now: i64,
    ) -> Result<(), String> {
        match kind {
            HideKind::Snooze => {
                let until = until.ok_or("a snooze needs an end")?;
                if until <= now {
                    return Err("the snooze would end before it starts".into());
                }
                if item.urgency == Urgency::NeedsYou && until - now > NEEDS_YOU_SNOOZE_MAX {
                    return Err("a row that needs you can be snoozed for at most 7 days".into());
                }
            }
            HideKind::Skip => {
                if !item.skippable() {
                    return Err(format!(
                        "{} carries no version or commit range to skip; snooze it or dismiss it",
                        item.id
                    ));
                }
                if item.urgency == Urgency::NeedsYou {
                    return Err("a row that needs you cannot be skipped; snooze it".into());
                }
            }
            HideKind::Dismiss => {
                if item.urgency == Urgency::NeedsYou {
                    return Err("a row that needs you cannot be dismissed; snooze it".into());
                }
                if item.skippable() {
                    return Err(format!(
                        "{} is about one state; skip it so it returns when that changes",
                        item.id
                    ));
                }
            }
        }
        let previous = self.hidden.insert(
            item.id.clone(),
            Hide {
                kind,
                until: (kind == HideKind::Snooze).then_some(until).flatten(),
                urgency: item.urgency,
                at: now,
                seen: now,
            },
        );
        self.undo = Some(Undo {
            id: item.id.clone(),
            previous,
        });
        Ok(())
    }

    /// True when the id held an entry.
    pub fn restore(&mut self, id: &str) -> bool {
        match self.hidden.remove(id) {
            Some(previous) => {
                self.undo = Some(Undo {
                    id: id.into(),
                    previous: Some(previous),
                });
                true
            }
            None => false,
        }
    }

    /// Puts back what the last change replaced; the id it touched.
    pub fn undo(&mut self) -> Option<String> {
        let undo = self.undo.take()?;
        match undo.previous {
            Some(previous) => self.hidden.insert(undo.id.clone(), previous),
            None => self.hidden.remove(&undo.id),
        };
        Some(undo.id)
    }

    /// Marks the ids a feed still returns and drops entries nobody returned for 30 days, and
    /// notified marks older than the notification window.
    pub fn tidy(&mut self, live: &[&str], now: i64) {
        for (id, hide) in self.hidden.iter_mut() {
            if live.contains(&id.as_str()) {
                hide.seen = now;
            }
        }
        self.hidden.retain(|_, hide| now - hide.seen < UNSEEN_KEEP);
        self.notified.retain(|_, at| now - *at < NOTIFY_WINDOW_SECS);
    }
}
