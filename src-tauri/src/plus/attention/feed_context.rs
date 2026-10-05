//! A bundle whose `bind` pattern matches a folder where no bundle is applied yet.

use super::{home_short, Ctx, Item, Level};
use crate::plus::context::bundle_apply::{folder_key, World};
use crate::plus::context::bundle_use;
use crate::plus::hashing::sha256_hex;

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some((roots, _)) = crate::plus::sources::host_world() else { return Vec::new() };
    let Some(data_dir) = crate::registry::conduit_dir() else { return Vec::new() };
    let world = World { roots: &roots, data_dir: &data_dir };
    bundle_use::unapplied_matches(&world)
        .into_iter()
        .map(|(name, folder)| {
            let key = folder_key(&folder);
            let short: String = sha256_hex(key.as_bytes()).chars().take(8).collect();
            let shown = home_short(&key);
            Item::new(
                ctx,
                format!("bundle:{name}:{short}"),
                Level::Look,
                "context",
                format!("Bundle {name} matches {shown}"),
                format!("The bundle is bound to this folder but is not applied there yet. Applying it is previewed first."),
            )
            .target("context", &[("tab", "profiles"), ("bundle", &name), ("folder", &key)])
            .action("Apply bundle", &["toolportctl", "context", "bundle", "apply", &name, "--cwd", &key])
        })
        .collect()
}
