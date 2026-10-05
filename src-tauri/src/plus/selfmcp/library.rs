//! `library_status` (tier 1) and `library_pull` (tier 2): the same operations as
//! `toolportctl library status|pull`, over `plus::sources::{library_remote, library_sync}`.
//! `skills_git_push` reaches the same push core through [`push`].

use super::ToolError;
use crate::plus::args::{flag, flag_or};
use crate::plus::op::{ErrorKind, OpError};
use crate::plus::sources::library_remote as remote;
use crate::plus::sources::library_sync as sync;
use serde_json::Value;
use std::path::Path;

pub(super) fn tool_error(error: OpError) -> ToolError {
    match error.kind {
        ErrorKind::Failed("refused") => ToolError::new("refused", error.message),
        _ => ToolError::from(error),
    }
}

pub(super) fn status(args: &Value) -> Result<Value, ToolError> {
    remote::status_host(flag(args, "fetch")).map_err(tool_error)
}

pub(super) fn pull(args: &Value) -> Result<Value, ToolError> {
    let repo = remote::host_repo().map_err(tool_error)?;
    sync::pull(&repo, flag_or(args, "dry_run", true)).map_err(tool_error)
}

pub(super) fn push(repo: &Path, message: &str) -> Result<Value, ToolError> {
    sync::push(repo, Some(message), false).map_err(tool_error)
}
