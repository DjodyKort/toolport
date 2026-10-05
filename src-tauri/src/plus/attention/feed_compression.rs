//! Compression presets that were taken from another engine build than the pinned one. Reads
//! `compression.json` only; the engine is not asked for its version.

use super::{Ctx, Item, Level};
use crate::plus::compression::model::ProviderName;
use crate::plus::compression::store::{self, Paths};

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some(paths) = Paths::from_data_dir() else { return Vec::new() };
    let Ok(loaded) = store::read(&paths) else { return Vec::new() };
    let config = &loaded.config;
    if !loaded.existed || config.provider != ProviderName::Headroom {
        return Vec::new();
    }
    let pin = &config.provider_version.pin;
    let stale: Vec<&str> = config
        .presets
        .iter()
        .filter(|(_, p)| p.savings_profile.is_some() && p.snapshot_version.as_deref() != Some(pin.as_str()))
        .map(|(name, _)| name.as_str())
        .collect();
    if stale.is_empty() {
        return Vec::new();
    }
    vec![Item::new(
        ctx,
        "compression:drift",
        Level::Look,
        "compression",
        "Compression presets come from another engine build",
        format!("The presets {} were copied from a build other than the pinned {pin}. Refreshing them is previewed first.", stale.join(", ")),
    )
    .target("tokens", &[("tab", "compression")])
    .action("Refresh presets", &["toolportctl", "compression", "presets", "--refresh"])]
}
