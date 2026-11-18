//! `catalog_status` (tier 1): the same read as `toolportctl catalog status`.

use super::ToolError;
use crate::catalog_index;
use serde_json::{json, Value};
use toolport_plus_api::Host;

pub(super) fn status(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(json!(catalog_index::status(host)))
}
