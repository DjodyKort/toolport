use serde::Serialize;
use serde_json::{json, Value};

use crate::tasks::cron::rfc3339;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Urgency {
    NeedsYou,
    WorthALook,
}

/// The screen that owns the condition; the app opens it when the row's button is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Screen {
    Servers,
    Tasks,
    Library,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Action {
    pub label: String,
    pub screen: Screen,
    /// The `toolportctl` argv that fixes the row without opening the screen, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

impl Action {
    pub fn open(label: &str, screen: Screen) -> Self {
        Self {
            label: label.into(),
            screen,
            command: None,
        }
    }

    pub fn run(label: &str, screen: Screen, argv: &[&str]) -> Self {
        Self {
            command: Some(argv.iter().map(|part| part.to_string()).collect()),
            ..Self::open(label, screen)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub urgency: Urgency,
    pub title: String,
    pub detail: String,
    pub source: &'static str,
    /// Epoch seconds; `None` when the store behind the row keeps no time for it.
    pub since: Option<i64>,
    pub action: Option<Action>,
}

impl Item {
    pub fn to_json(&self) -> Value {
        let mut value = json!({
            "id": self.id,
            "urgency": self.urgency,
            "title": self.title,
            "detail": self.detail,
            "source": self.source,
        });
        if let Some(since) = self.since {
            value["since"] = json!(rfc3339(since));
        }
        if let Some(action) = &self.action {
            value["action"] = json!(action);
        }
        value
    }
}

/// Whether one feed could be read; a feed that could not is named next to the rows so an empty
/// list is never mistaken for an all-clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub checked_at: i64,
    pub checks: Vec<Check>,
    pub items: Vec<Item>,
}

impl Snapshot {
    pub fn count(&self, urgency: Urgency) -> usize {
        self.items.iter().filter(|i| i.urgency == urgency).count()
    }

    pub fn all_checked(&self) -> bool {
        self.checks.iter().all(|check| check.error.is_none())
    }

    pub fn to_json(&self) -> Value {
        let checks: Vec<Value> = self
            .checks
            .iter()
            .map(|check| match &check.error {
                Some(error) => json!({"name": check.name, "ok": false, "error": error}),
                None => json!({"name": check.name, "ok": true}),
            })
            .collect();
        json!({
            "checkedAt": rfc3339(self.checked_at),
            "counts": {
                "needsYou": self.count(Urgency::NeedsYou),
                "worthALook": self.count(Urgency::WorthALook),
            },
            "checks": checks,
            "items": self.items.iter().map(Item::to_json).collect::<Vec<_>>(),
        })
    }
}
