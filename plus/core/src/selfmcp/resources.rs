//! The read-only resources that are built from other tools' data or fixed text: the inventories
//! of the library, the clients and the gateway, and the two reference documents.

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::{clients, content, skills, ToolError};
use crate::status;

const ARCHITECTURE: &str = "# Toolport+ architecture\n\n\
Toolport keeps one registry of MCP servers and puts one gateway in front of them. Clients do not\n\
get a copy of every server: each client holds a single entry named `toolport` that starts\n\
`toolport-gateway`, and the gateway serves the servers the client's access set reaches.\n\n\
## Servers and access sets\n\n\
- A server is a stdio command or a remote URL, switched on or off. Secret values live in the\n\
  vault and never appear in a tool result or a resource.\n\
- An access set lists servers (and their tool scope). A client is bound to one set; a folder\n\
  mapping can outrank the client's set inside a project folder. `clients_reach` shows who reaches\n\
  what.\n\
- A direct client entry (`client_direct_add`) bypasses the gateway for one server. It is the\n\
  exception: the gateway entry is the rule.\n\n\
## The library\n\n\
- The skills repository holds skills, rules, agents and styles. Transpilers write them into each\n\
  client's own folders when you sync; `skills_status` shows the drift.\n\
- Taps and plugin marketplaces feed the library through `library_select_plan` and\n\
  `library_select_apply`.\n\n\
## Safety model\n\n\
- Tier 1 tools only read. A write tool previews first (`dry_run` is on) and applies only with\n\
  `dry_run=false` and `confirm=true`; plans that carry a `plan_hash` must be applied with that hash.\n\
- Secret values are never returned, and a person approves every `secret_set`. Task definitions,
  plugin switches and `force` are not reachable from this server.\n\n\
Recipes for common multi-step jobs are in `mcpm://workflows`.\n";

const WORKFLOWS: &str = "# Toolport+ workflows\n\n\
Every write below previews first: call it once with the default `dry_run`, read the plan, then\n\
repeat with `dry_run=false` and `confirm=true` (and the `plan_hash` when the plan carries one).\n\n\
## Add a server from the catalog\n\n\
1. `catalog_search` with a word of its name; pick an entry with `installable=true`.\n\
2. `servers_install_catalog` with that name.\n\
3. `secret_set` once per key in `envKeys` (a person approves each call).\n\
4. `servers_set_enabled` to switch it on, then `servers_add_profile_tag` to list it in the access set
   the clients you want use.\n\n\
## Repair a client that lost the gateway\n\n\
1. `clients_list`: look for `gatewayInstalled=false` or a drifted entry.\n\
2. `clients_sync` with the client id, apply with the returned `plan_hash`.\n\n\
## Publish a new skill\n\n\
1. `skills_scaffold`, then edit the body with `skills_edit_body`.\n\
2. `skills_status` for the drift, `skills_sync` to write the client outputs.\n\n\
## Move to a new machine\n\n\
1. `setup_check` for what is not on a remote yet, `setup_parts` for what an export holds.\n\
2. A person exports with a passphrase; `setup_undo` reverses a restore run.\n\n\
## When something is wrong\n\n\
`doctor` first, then `attention_list` for what needs the person right now.\n";

fn lines(data: &Value, key: &str) -> String {
    data[key]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            format!(
                "{} \u{2014} {}",
                row["name"].as_str().unwrap_or(""),
                row["description"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn inventory(
    host: &dyn Host,
    key: &str,
    list: fn(&dyn Host, &Value) -> Result<Value, ToolError>,
) -> Result<String, ToolError> {
    match list(host, &json!({})) {
        Ok(data) => Ok(lines(&data, key)),
        Err(error) if error.kind == "not_found" => Ok(String::new()),
        Err(error) => Err(error),
    }
}

pub(super) fn inventory_skills(host: &dyn Host) -> Result<String, ToolError> {
    inventory(host, "skills", skills::list)
}

pub(super) fn inventory_agents(host: &dyn Host) -> Result<String, ToolError> {
    inventory(host, "agents", content::agents_list)
}

pub(super) fn inventory_styles(host: &dyn Host) -> Result<String, ToolError> {
    inventory(host, "styles", content::styles_list)
}

pub(super) fn clients_json(host: &dyn Host) -> Result<Value, ToolError> {
    clients::list(host, &json!({}))
}

/// The gateway is started by each client, so there is no daemon to report: what exists is the
/// binary and the clients whose entry points at it.
pub(super) fn gateway_status(host: &dyn Host) -> Result<Value, ToolError> {
    let snapshot = status::status(host, &status::snapshot(host));
    let clients = clients::list(host, &json!({}))?;
    let with_entry: Vec<Value> = clients["clients"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|client| client["gatewayInstalled"] == json!(true))
        .map(|client| client["id"].clone())
        .collect();
    Ok(json!({
        "gateway": snapshot["gateway"],
        "clientsWithGatewayEntry": with_entry,
        "note": "each client starts its own gateway process; there is no separate router daemon to run or restart",
    }))
}

pub(super) fn architecture() -> &'static str {
    ARCHITECTURE
}

pub(super) fn workflows() -> &'static str {
    WORKFLOWS
}
