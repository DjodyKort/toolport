//! `context_compose` (tier 1): the same read-only preview as `toolportctl context compose`.

use super::ToolError;
use crate::plus::args::str_nonempty;
use crate::plus::context::compose::compose_here;
use serde_json::Value;
use std::path::Path;

pub(super) fn compose(args: &Value) -> Result<Value, ToolError> {
    let cwd = str_nonempty(args, "cwd")
        .ok_or_else(|| ToolError::new("invalid_arguments", "cwd is required"))?;
    Ok(compose_here(Path::new(cwd))?)
}
