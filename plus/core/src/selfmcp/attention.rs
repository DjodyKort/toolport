//! `attention_list` (tier 1): the same list as `toolportctl attention ls`, over `crate::attention`.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::attention;

pub(super) fn list(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(attention::snapshot(host).to_json())
}
