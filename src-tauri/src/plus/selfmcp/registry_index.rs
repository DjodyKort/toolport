use super::ToolError;
use crate::catalog_index::{self, SyncMode};
use crate::plus::args::flag;
use crate::plus::op::OpError;
use serde_json::{json, Value};

pub(super) fn status(_args: &Value) -> Result<Value, ToolError> {
    Ok(catalog_index::status())
}

pub(super) fn sync(args: &Value) -> Result<Value, ToolError> {
    let mode = if flag(args, "full") {
        SyncMode::Full
    } else {
        SyncMode::Incremental
    };
    catalog_index::sync(mode, &mut |_| {})
        .map(|report| json!(report))
        .map_err(|error| ToolError::from(OpError::from(error)))
}
