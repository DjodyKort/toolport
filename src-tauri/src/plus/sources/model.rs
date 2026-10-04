//! The wire types of `sources ls` (contract section 1) and the helpers that build them.

use serde::Serialize;

pub const KINDS: [&str; 5] = ["skill", "command", "agent", "rule", "memory"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Origin {
    pub kind: &'static str,
    pub name: String,
}

impl Origin {
    pub fn new(kind: &'static str, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Tokens {
    pub value: u64,
    pub basis: &'static str,
}

impl Tokens {
    pub fn estimate(value: u64) -> Self {
        Self {
            value,
            basis: "estimate",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: &'static str,
    pub detail: String,
    pub checked_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Freshness {
    #[serde(rename = "ref")]
    pub reference: String,
    pub behind: u64,
    pub ahead: u64,
    pub in_checkout: bool,
    pub last_sync: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub skill: usize,
    pub command: usize,
    pub agent: usize,
    pub rule: usize,
    pub memory: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Visible {
    pub skill: usize,
    pub skill_total: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub origin: Origin,
    pub detector: &'static str,
    pub root: Option<String>,
    pub owner: &'static str,
    pub writable: bool,
    pub managed_by: Option<String>,
    pub status: Status,
    pub freshness: Option<Freshness>,
    pub counts: Counts,
    pub tokens: Tokens,
    pub visible: Visible,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub kind: &'static str,
    pub name: String,
    pub path: String,
    pub source_id: String,
    pub origin: Origin,
    pub writable: bool,
    pub lazy: bool,
    pub shadowed_by: Option<String>,
    pub audit: &'static str,
    pub tokens: Tokens,
    #[serde(skip)]
    pub description: String,
    #[serde(skip)]
    pub activation: Option<String>,
    #[serde(skip)]
    pub visible: Option<bool>,
    #[serde(skip)]
    pub invisible_reason: Option<String>,
    #[serde(skip)]
    pub in_checkout: bool,
}

/// What a detector fills in per source; [`SourceMeta::finish`] adds what the items determine.
pub struct SourceMeta {
    pub id: String,
    pub origin: Origin,
    pub detector: &'static str,
    pub root: Option<String>,
    pub owner: &'static str,
    pub writable: bool,
    pub managed_by: Option<String>,
    pub state: &'static str,
    pub detail: String,
    pub freshness: Option<Freshness>,
    pub warnings: Vec<String>,
    pub enabled: Option<bool>,
}

impl SourceMeta {
    pub fn finish(self, items: &[Item], checked_at: &str) -> Source {
        let mut counts = Counts::default();
        let mut visible = Visible::default();
        let mut tokens = 0;
        for item in items.iter().filter(|i| i.source_id == self.id) {
            tokens += item.tokens.value;
            match item.kind {
                "skill" => {
                    counts.skill += 1;
                    visible.skill_total += 1;
                    if item.visible != Some(false) {
                        visible.skill += 1;
                    }
                }
                "command" => counts.command += 1,
                "agent" => counts.agent += 1,
                "rule" => counts.rule += 1,
                _ => counts.memory += 1,
            }
        }
        Source {
            id: self.id,
            origin: self.origin,
            detector: self.detector,
            root: self.root,
            owner: self.owner,
            writable: self.writable,
            managed_by: self.managed_by,
            status: Status {
                state: self.state,
                detail: self.detail,
                checked_at: checked_at.to_string(),
            },
            freshness: self.freshness,
            counts,
            tokens: Tokens::estimate(tokens),
            visible,
            warnings: self.warnings,
            enabled: self.enabled,
        }
    }
}

pub fn kind_rank(kind: &str) -> usize {
    KINDS.iter().position(|k| *k == kind).unwrap_or(KINDS.len())
}
