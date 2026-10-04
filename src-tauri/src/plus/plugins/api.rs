//! The operations behind `plugins ls|show`, `hooks ls` and their self-MCP tools: this machine's
//! folders, the real `claude` binary when it exists, and the contract's error kinds.

use super::report::{self, Failure, Opts};
use super::{absolute_cwd, hooks, installed, Env, SystemClaude};
use crate::plus::op::OpError;
use serde_json::Value;
use std::path::PathBuf;

fn env() -> Result<Env, OpError> {
    Env::host().ok_or_else(|| OpError::failed("no_home", "home directory could not be resolved"))
}

fn folder(raw: Option<&str>) -> Result<Option<PathBuf>, OpError> {
    let cwd = absolute_cwd(raw).map_err(OpError::usage)?;
    match cwd {
        Some(dir) if !dir.is_dir() => Err(OpError::usage(format!(
            "cwd is not a folder: {}",
            dir.display()
        ))),
        other => Ok(other),
    }
}

pub fn ls(cwd: Option<&str>, refresh: bool) -> Result<Value, OpError> {
    let cwd = folder(cwd)?;
    let runner = SystemClaude::from_env();
    Ok(report::ls(
        &env()?,
        Some(&runner),
        &Opts {
            cwd: cwd.as_deref(),
            refresh,
        },
    ))
}

pub fn show(id: &str, cwd: Option<&str>) -> Result<Value, OpError> {
    if !installed::valid_ident(id) {
        return Err(OpError::usage(format!("invalid plugin id: {id}")));
    }
    let cwd = folder(cwd)?;
    let runner = SystemClaude::from_env();
    report::show(
        &env()?,
        Some(&runner),
        id,
        &Opts {
            cwd: cwd.as_deref(),
            refresh: false,
        },
    )
    .map_err(|failure| match failure {
        Failure::Missing(message) => OpError::not_found(message),
        Failure::Failed(message) => OpError::conflict(message),
    })
}

pub fn hooks_ls(cwd: Option<&str>, filter: &hooks::Filter) -> Result<Value, OpError> {
    filter.validate().map_err(OpError::usage)?;
    let cwd = folder(cwd)?;
    let env = env()?;
    let layers = env.layers(cwd.as_deref());
    let inventory = hooks::collect(&env, cwd.as_deref(), &layers);
    Ok(hooks::to_value(&inventory, filter))
}
