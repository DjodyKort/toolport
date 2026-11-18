//! What sync is set up to do (`config.json`) and what it remembers between runs (`state.json`):
//! the baseline both machines last agreed on, the last revision seen on the remote and the last
//! apply, which `sync undo` reverses. Neither file holds the passphrase.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use crate::setup::threeway::Baseline;
use crate::setup::{Ctx, PartId};

/// The one file the remote holds.
pub const FILE_NAME: &str = "toolport-sync.tpsync";
pub const DEFAULT_BRANCH: &str = "main";

/// Parts a sync can carry. Repositories (per-machine paths), the skills library (its own remote)
/// and sessions never travel.
pub const SYNC_PARTS: [PartId; 7] = [
    PartId::Servers,
    PartId::Access,
    PartId::Tasks,
    PartId::Settings,
    PartId::ClaudeUser,
    PartId::Memory,
    PartId::Secrets,
];

pub const DEFAULT_PARTS: [PartId; 4] = [
    PartId::Servers,
    PartId::Access,
    PartId::Tasks,
    PartId::Settings,
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TransportCfg {
    Git { url: String, branch: String },
    Folder { path: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub transport: TransportCfg,
    pub parts: BTreeSet<String>,
    pub enabled: bool,
}

impl Config {
    pub fn new(transport: TransportCfg) -> Self {
        Self {
            version: 1,
            transport,
            parts: DEFAULT_PARTS.iter().map(|p| p.id().to_string()).collect(),
            enabled: true,
        }
    }

    pub fn parts(&self) -> BTreeSet<PartId> {
        self.parts
            .iter()
            .filter_map(|p| PartId::parse(p))
            .filter(|p| SYNC_PARTS.contains(p))
            .collect()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LastRun {
    pub direction: String,
    pub at: String,
    pub revision: String,
    pub machine: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LastApply {
    pub run: String,
    /// The baseline before the apply, put back by `sync undo` so the next pull offers the same
    /// changes again.
    pub before: BTreeMap<String, String>,
    #[serde(default)]
    pub seen_before: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub baseline: BTreeMap<String, String>,
    /// The remote revision this machine last pushed or pulled; a push over any other is refused.
    #[serde(default)]
    pub remote_seen: Option<String>,
    #[serde(default)]
    pub last_sync: Option<LastRun>,
    #[serde(default)]
    pub last_check: Option<Value>,
    #[serde(default)]
    pub last_apply: Option<LastApply>,
    /// Rows the last pull left as they were, with both machines' changes unsettled.
    #[serde(default)]
    pub pending: Vec<String>,
    /// Machine name to the time it was last seen on the remote.
    #[serde(default)]
    pub machines: BTreeMap<String, String>,
}

impl State {
    pub fn baseline(&self) -> Baseline {
        Baseline {
            rows: self.baseline.clone(),
        }
    }
}

pub fn dir(ctx: &Ctx<'_>) -> PathBuf {
    ctx.data_dir.join("plus").join("sync")
}

fn config_path(ctx: &Ctx<'_>) -> PathBuf {
    dir(ctx).join("config.json")
}

fn state_path(ctx: &Ctx<'_>) -> PathBuf {
    dir(ctx).join("state.json")
}

fn failed(why: impl std::fmt::Display) -> OpError {
    OpError::failed("sync", why.to_string())
}

pub fn read_config(ctx: &Ctx<'_>) -> Result<Option<Config>, OpError> {
    match std::fs::read_to_string(config_path(ctx)) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| failed(format!("{}: {e}", config_path(ctx).display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(failed(format!("{}: {e}", config_path(ctx).display()))),
    }
}

pub fn write_config(ctx: &Ctx<'_>, config: &Config) -> Result<(), OpError> {
    let text = serde_json::to_string_pretty(config).unwrap_or_default() + "\n";
    atomic_write(&config_path(ctx), &text, Mode::Private)
        .map_err(|e| failed(format!("{}: {e}", config_path(ctx).display())))
}

pub fn read_state(ctx: &Ctx<'_>) -> Result<State, OpError> {
    match std::fs::read_to_string(state_path(ctx)) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| failed(format!("{}: {e}", state_path(ctx).display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(failed(format!("{}: {e}", state_path(ctx).display()))),
    }
}

pub fn write_state(ctx: &Ctx<'_>, state: &State) -> Result<(), OpError> {
    let text = serde_json::to_string_pretty(state).unwrap_or_default() + "\n";
    atomic_write(&state_path(ctx), &text, Mode::Private)
        .map_err(|e| failed(format!("{}: {e}", state_path(ctx).display())))
}

pub fn transport_json(transport: &TransportCfg) -> Value {
    match transport {
        TransportCfg::Git { url, branch } => json!({
            "kind": "git",
            "url": crate::skills::taps::redact(url),
            "branch": branch,
        }),
        TransportCfg::Folder { path } => json!({"kind": "folder", "path": path}),
    }
}
