//! `spend_status`, `spend_summary` and `spend_setup_plan` (tier 1): the same operations as
//! `toolportctl spend`, over `crate::spend::api`. Turning collection on or off stays with the
//! command line and the Spend page, which show the plan first.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::str_trimmed;
use crate::spend::api;

pub(super) fn status(host: &dyn Host, _args: &Value) -> Result<Value, ToolError> {
    Ok(api::status(host)?)
}

pub(super) fn summary(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::summary(
        host,
        str_trimmed(args, "range"),
        str_trimmed(args, "project"),
    )?)
}

pub(super) fn setup_plan(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    Ok(api::setup_plan(host, str_trimmed(args, "port"), &[])?)
}
