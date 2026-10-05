//! The skills library against the remote refs git already has. Nothing is fetched.

use super::{Ctx, Item, Level};
use crate::plus::sources::library_remote;

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Ok(data) = library_remote::status_host(false) else { return Vec::new() };
    let (ahead, behind) = (data["ahead"].as_u64().unwrap_or(0), data["behind"].as_u64().unwrap_or(0));
    let plural = |n: u64| if n == 1 { "commit" } else { "commits" };
    let mut items = Vec::new();
    if behind > 0 {
        items.push(Item::new(
            ctx,
            "library:behind",
            Level::Fyi,
            "sources",
            format!("Your skills library is {behind} {} behind its remote", plural(behind)),
            "Newer skills are waiting on the remote. Pulling is previewed first.",
        ));
    }
    if ahead > 0 {
        items.push(Item::new(
            ctx,
            "library:ahead",
            Level::Fyi,
            "sources",
            format!("Your skills library has {ahead} {} not pushed", plural(ahead)),
            "The changes exist only on this computer until you push them.",
        ));
    }
    items.into_iter().map(|i| i.target("library", &[("tab", "sources"), ("source", "library")])).collect()
}
