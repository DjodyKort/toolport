//! Whether the unattended sync may run. A pause is a flag under `<data dir>/plus/skills-auto.json`
//! with when and why it was set: `undo` pauses so the next run does not redo what was undone,
//! `manual` is the user's own pause.

use super::clock::Clock;
use crate::fsx::{atomic_write, Mode};
use crate::op::OpError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const REASON_UNDO: &str = "undo";
pub const REASON_MANUAL: &str = "manual";
const FILE: &str = "skills-auto.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub paused: bool,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl State {
    pub fn to_json(&self) -> Value {
        json!({"paused": self.paused, "since": self.since, "reason": self.reason})
    }

    pub fn describe(&self) -> String {
        format!(
            "auto-sync is paused since {} (reason: {}); resume it with `skills auto resume`",
            self.since.as_deref().unwrap_or("an unknown time"),
            self.reason.as_deref().unwrap_or("unknown")
        )
    }
}

fn path(base: &Path) -> PathBuf {
    base.join("plus").join(FILE)
}

/// No file is a run that is not paused; a file that cannot be read is an error, so an unreadable
/// pause never lets an unattended run through.
pub fn read(base: &Path) -> Result<State, OpError> {
    let file = path(base);
    match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            OpError::failed(
                "skills",
                format!("{} is not valid: {e}; remove it to resume", file.display()),
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(OpError::failed(
            "skills",
            format!("{}: {e}", file.display()),
        )),
    }
}

fn write(base: &Path, state: &State) -> Result<(), OpError> {
    let text = serde_json::to_string_pretty(state).unwrap_or_default() + "\n";
    atomic_write(&path(base), &text, Mode::Private)
        .map_err(|e| OpError::failed("skills", format!("{}: {e}", path(base).display())))
}

/// A pause that is already set stays as it is, with its own since and reason; the flag says
/// whether this call changed anything.
pub fn pause(base: &Path, reason: &str, clock: &dyn Clock) -> Result<(State, bool), OpError> {
    let state = read(base)?;
    if state.paused {
        return Ok((state, false));
    }
    let state = State {
        paused: true,
        since: Some(clock.now().zulu()),
        reason: Some(reason.to_string()),
    };
    write(base, &state)?;
    Ok((state, true))
}

pub fn resume(base: &Path) -> Result<(State, bool), OpError> {
    let was = read(base)?;
    if !was.paused {
        return Ok((was, false));
    }
    let state = State::default();
    write(base, &state)?;
    Ok((state, true))
}

fn data_dir() -> Result<PathBuf, OpError> {
    crate::fsx::data_dir().ok_or_else(|| OpError::not_found("data directory unknown"))
}

fn result(state: &State, changed: bool) -> Value {
    let mut data = state.to_json();
    data["changed"] = json!(changed);
    data
}

pub fn status_op() -> Result<Value, OpError> {
    Ok(read(&data_dir()?)?.to_json())
}

pub fn pause_op(clock: &dyn Clock) -> Result<Value, OpError> {
    let (state, changed) = pause(&data_dir()?, REASON_MANUAL, clock)?;
    Ok(result(&state, changed))
}

pub fn resume_op() -> Result<Value, OpError> {
    let (state, changed) = resume(&data_dir()?)?;
    Ok(result(&state, changed))
}
