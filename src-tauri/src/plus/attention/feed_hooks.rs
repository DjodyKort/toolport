//! How many hook processes one Bash call starts, counted from the settings and plugin files.

use super::{Ctx, Item, Level};
use crate::plus::plugins::{api, hooks};

const THRESHOLD: u64 = 10;

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Ok(data) = api::hooks_ls(None, &hooks::Filter::default()) else { return Vec::new() };
    let total = data["counts"]["perTool"]["Bash"]["total"].as_u64().unwrap_or(0);
    if total < THRESHOLD {
        return Vec::new();
    }
    vec![Item::new(
        ctx,
        "hooks:bash",
        Level::Fyi,
        "context",
        format!("Every Bash call starts {total} hook processes"),
        "Counted from your settings and your plugins. Each one is a program that starts before or after the command.",
    )
    .target("context", &[("tab", "hooks"), ("tool", "Bash")])]
}
