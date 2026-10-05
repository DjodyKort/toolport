//! `attention_ls` (tier 1): the same list as `toolportctl attention ls`, over
//! `plus::attention`. Read only; dismissing a row is not exposed to a model.

use super::ToolError;
use crate::plus::args::str_nonempty;
use crate::plus::attention::{self, Level};
use serde_json::Value;

pub(super) fn ls(args: &Value) -> Result<Value, ToolError> {
    let level = match str_nonempty(args, "level") {
        Some(text) => Some(Level::parse(text).ok_or_else(|| {
            ToolError::new("invalid_arguments", format!("level must be one of {}", Level::NAMES.join(", ")))
        })?),
        None => None,
    };
    Ok(attention::ls_host(level)?)
}
