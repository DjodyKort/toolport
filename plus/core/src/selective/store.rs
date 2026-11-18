//! The selection store: per source, the pinned ref and every item with its state. A new item is
//! off until the user turns it on; the file is Toolport's own and never touches Claude Code.

use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "selections.json";
const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Skill,
    Agent,
    Command,
    Hook,
    Rule,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Skill => "skill",
            Kind::Agent => "agent",
            Kind::Command => "command",
            Kind::Hook => "hook",
            Kind::Rule => "rule",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [
            Kind::Skill,
            Kind::Agent,
            Kind::Command,
            Kind::Hook,
            Kind::Rule,
        ]
        .into_iter()
        .find(|k| k.as_str() == text)
    }

    /// Rules have no plugin component, so they never enter the slice plugin.
    pub fn in_slice(self) -> bool {
        self != Kind::Rule
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    On,
    Off,
    New,
    Removed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::On => "on",
            State::Off => "off",
            State::New => "new",
            State::Removed => "removed",
        }
    }

    pub fn wanted(self) -> bool {
        matches!(self, State::On | State::Removed)
    }
}

/// A hook command the upstream changed after it was ticked: the old one keeps running until the
/// new one is ticked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub hash: String,
    pub spec: Value,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub hash: String,
    pub state: State,
    /// Whether the generated plugin carries this item now.
    #[serde(default)]
    pub in_slice: bool,
    /// Where the item sits inside the source (a folder for a skill, a file otherwise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The hook as it runs: `{event, matcher, handler}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Item {
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.id)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceSel {
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub pinned: Option<String>,
    #[serde(default)]
    pub slice: String,
    /// Counts regenerations: Claude Code caches a plugin by version, so each one needs a new one.
    #[serde(default)]
    pub rev: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support: Option<String>,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl SourceSel {
    pub fn item(&self, kind: Kind, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.kind == kind && i.id == id)
    }

    pub fn item_mut(&mut self, kind: Kind, id: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.kind == kind && i.id == id)
    }

    pub fn wants_slice(&self) -> bool {
        self.items
            .iter()
            .any(|i| i.kind.in_slice() && i.state.wanted())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Selections {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceSel>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn version() -> u32 {
    VERSION
}

impl Default for Selections {
    fn default() -> Self {
        Self {
            version: VERSION,
            sources: BTreeMap::new(),
            extra: Map::new(),
        }
    }
}

pub fn path(base: &Path) -> PathBuf {
    base.join(FILE)
}

pub fn load(base: &Path) -> Result<Selections, OpError> {
    let file = path(base);
    match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            OpError::conflict(format!(
                "{} is not a selection file ({e}); nothing was changed",
                file.display()
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Selections::default()),
        Err(e) => Err(OpError::conflict(format!("{}: {e}", file.display()))),
    }
}

pub fn save(base: &Path, selections: &Selections) -> Result<(), OpError> {
    let text =
        serde_json::to_string_pretty(selections).map_err(|e| OpError::conflict(e.to_string()))?;
    atomic_write(&path(base), &format!("{text}\n"), Mode::Private)
        .map_err(|e| OpError::conflict(format!("{}: {e}", path(base).display())))
}
