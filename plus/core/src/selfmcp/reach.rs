//! The `clients_reach` tool: what `toolportctl client reach` does, for agents.

use serde_json::Value;
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::str_arg;
use crate::reach::{self, Filter};

pub(super) fn run(host: &dyn Host, args: &Value) -> Result<Value, ToolError> {
    let report = reach::assess(
        host,
        Filter {
            server: str_arg(args, "server").or_else(|| str_arg(args, "name")),
            client: str_arg(args, "client"),
        },
    )?;
    Ok(report.to_value())
}
