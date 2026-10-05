//! A server that is on in the active profile but has no value for a secret it declares. Only the
//! presence is looked at; no value is kept or shown, and a keychain that cannot answer counts as
//! not known rather than as missing.

use super::{Ctx, Item, Level};
use crate::plus::registry_ro;

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some(registry) = registry_ro::read_opt() else { return Vec::new() };
    let active = registry.active_profile_id();
    let mut items = Vec::new();
    for server in registry.servers.iter().filter(|s| registry.is_enabled(&active, &s.id)) {
        let mut keys: Vec<&str> = server.env.iter().filter(|e| e.secret).map(|e| e.key.as_str()).collect();
        if let Some(launch) = &server.launch {
            keys.extend(launch.inputs.iter().filter(|i| i.secret).map(|i| i.key.as_str()));
        }
        keys.sort_unstable();
        keys.dedup();
        let missing: Vec<&str> = keys.into_iter().filter(|key| matches!(crate::secrets::get_secret_result(&server.id, key), Ok(None))).collect();
        if missing.is_empty() {
            continue;
        }
        let many = missing.len() > 1;
        items.push(
            Item::new(
                ctx,
                format!("secrets:{}:missing", server.id),
                Level::NeedsYou,
                "doctor",
                format!("{} is missing {}", server.name, if many { "secrets" } else { "a secret" }),
                format!("Not set yet: {}. The server cannot start without {}.", missing.join(", "), if many { "them" } else { "it" }),
            )
            .target("servers", &[("tab", "secrets"), ("server", &server.id)]),
        );
    }
    items
}
