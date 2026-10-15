//! A client on the gateway whose own profile leaves out servers that are on in the active profile,
//! so it never sees their tools. One row per client; the id carries the left-out set, so a row
//! dismissed for a deliberate choice comes back when another server is left out.

use super::{Ctx, Item, Level};
use crate::plus::{reach, registry_ro};
use std::collections::BTreeMap;

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some(registry) = registry_ro::read_opt() else { return Vec::new() };
    rows(ctx, &reach::assess(&registry, &crate::clients::detect_clients()))
}

pub(super) fn rows(ctx: &Ctx, report: &reach::Report) -> Vec<Item> {
    let mut by_client: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for finding in report.findings.iter().filter(|f| f.kind == "left-out-by-profile") {
        if let (Some(client), Some(server)) = (&finding.client, &finding.server) {
            by_client.entry(client).or_default().push(server);
        }
    }
    let mut items = Vec::new();
    for (client_id, mut ids) in by_client {
        ids.sort_unstable();
        let Some(client) = report.clients.iter().find(|c| c.id == client_id) else { continue };
        let names: Vec<&str> = ids
            .iter()
            .map(|id| report.servers.iter().find(|s| s.id == *id).map_or(*id, |s| s.name.as_str()))
            .collect();
        let profile = client.profile.as_deref().unwrap_or("its own");
        let many = ids.len() > 1;
        let set = crate::registry::sha256_hex(&ids.join(","));
        items.push(
            Item::new(
                ctx,
                format!("profiles:{client_id}:left-out:{}", &set[..12]),
                Level::Look,
                "profiles",
                format!(
                    "{} does not get {} {} that {} on in the active profile",
                    client.name,
                    ids.len(),
                    if many { "servers" } else { "server" },
                    if many { "are" } else { "is" }
                ),
                format!(
                    "The profile {profile} leaves out {}. Add {} to that profile, or dismiss this \
                     row if it is on purpose.",
                    names.join(", "),
                    if many { "them" } else { "it" }
                ),
            )
            .target("servers", &[("server", ids[0])]),
        );
    }
    items
}
