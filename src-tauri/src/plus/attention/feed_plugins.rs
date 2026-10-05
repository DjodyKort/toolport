//! A plugin that starts an MCP server of its own, so Toolport cannot approve or record its calls,
//! and that no settings layer denies. Read from the plugin files; `claude` is never started.

use super::{Ctx, Item, Level};
use crate::plus::plugins::report::{self, Opts};
use crate::plus::plugins::Env;

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some(env) = Env::host() else { return Vec::new() };
    let data = report::ls(&env, None, &Opts { cwd: None, refresh: false });
    let mut items = Vec::new();
    for plugin in data["plugins"].as_array().into_iter().flatten() {
        if plugin["enabled"]["effective"] != true {
            continue;
        }
        let open: Vec<&str> = plugin["mcpOutsideGateway"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["denied"] != true)
            .filter_map(|m| m["name"].as_str())
            .collect();
        let (Some(id), false) = (plugin["id"].as_str(), open.is_empty()) else { continue };
        let name = plugin["name"].as_str().unwrap_or(id);
        items.push(
            Item::new(
                ctx,
                format!("plugins:{id}:mcp"),
                Level::Look,
                "sources",
                format!("{} MCP {} from the {name} plugin bypass the gateway", open.len(), if open.len() == 1 { "server" } else { "servers" }),
                format!("Claude Code starts {} itself, so Toolport cannot approve, record or share it. It can be denied per folder.", open.join(", ")),
            )
            .target("library", &[("tab", "plugins"), ("plugin", id)]),
        );
    }
    items
}
