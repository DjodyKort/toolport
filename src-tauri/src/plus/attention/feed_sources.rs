//! What the sources scan says: skills Claude cannot see, a source behind its remote and a second
//! clone of the library. The scan is cached and local; the library's own ahead and behind
//! counts are `feed_library`.

use super::{Ctx, Item, Level};
use crate::plus::sources::model::Freshness;
use crate::plus::sources::{scan_host, ScanOptions};

/// Commits a source is behind that carry items the checkout lacks. A checkout whose remote only
/// moved files Claude never loads has nothing to ask the user.
fn only_on_remote(freshness: &Freshness) -> u64 {
    if freshness.in_checkout {
        0
    } else {
        freshness.behind
    }
}

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some(report) = scan_host(&ScanOptions::default()) else { return Vec::new() };
    let mut items = Vec::new();
    let hidden: usize = report.sources.iter().map(|s| s.visible.skill_total.saturating_sub(s.visible.skill)).sum();
    if hidden > 0 {
        items.push(
            Item::new(
                ctx,
                "skills:invisible",
                Level::Look,
                "skills",
                format!("{hidden} {} not visible to Claude", if hidden == 1 { "skill is" } else { "skills are" }),
                "Claude Code does not list them, so it cannot pick them by itself. The slash command still works.",
            )
            .target("library", &[("tab", "skills"), ("filter", "invisible")]),
        );
    }
    for source in &report.sources {
        let name = &source.origin.name;
        if source.id == "library" {
            if source.status.state == "duplicate" {
                items.push(
                    Item::new(
                        ctx,
                        "source:library:duplicate",
                        Level::Look,
                        "sources",
                        "There is more than one clone of your skills library",
                        "Two copies of the same repository can drift apart. Keep one and remove the other.",
                    )
                    .target("library", &[("tab", "sources"), ("source", "library")]),
                );
            }
            continue;
        }
        let behind = source.freshness.as_ref().map_or(0, only_on_remote);
        if behind > 0 {
            items.push(
                Item::new(
                    ctx,
                    format!("source:{}:behind", source.id),
                    Level::Look,
                    "sources",
                    format!("{name} is {behind} {} behind its remote", if behind == 1 { "commit" } else { "commits" }),
                    "Newer files exist on the remote that Claude does not load yet.",
                )
                .target("library", &[("tab", "sources"), ("source", &source.id)]),
            );
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh(behind: u64, in_checkout: bool) -> Freshness {
        Freshness {
            reference: "origin/main".into(),
            behind,
            ahead: 0,
            in_checkout,
            last_sync: None,
        }
    }

    #[test]
    fn a_remote_that_only_moved_files_claude_never_loads_asks_nothing() {
        assert_eq!(only_on_remote(&fresh(449, true)), 0);
    }

    #[test]
    fn a_remote_with_items_the_checkout_lacks_counts_its_commits() {
        assert_eq!(only_on_remote(&fresh(3, false)), 3);
        assert_eq!(only_on_remote(&fresh(0, false)), 0);
    }
}
