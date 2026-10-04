//! The client-folder world of MIG-SRC-2 (`tests/common/loads_world.rs`): the numbers the first
//! `context loads` gave still hold, now labelled as estimates, and the rows it missed are there.

use super::layers::MANAGED_LOCAL_HEADER;
use super::loads::{what_loads, WhatLoads};
use super::roots::Roots;
use super::tests_loads::listing;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "../../../tests/common/loads_world.rs"]
mod world;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fx {
    w: world::LoadsWorld,
}

impl Fx {
    fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap().join(format!(
            "loads-baseline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&base);
        Self {
            w: world::build_in(&base, MANAGED_LOCAL_HEADER),
        }
    }

    fn loads(&self) -> WhatLoads {
        let roots = Roots::from_home(&self.w.home);
        let cfg = super::config::load_config(&roots.context_config_path());
        what_loads(&roots, &cfg, None, &self.w.client).unwrap()
    }

    fn at(&self, rel: &str) -> String {
        self.w.home.join(rel).display().to_string()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.w.base);
    }
}

fn by_path<'a>(r: &'a WhatLoads, path: &str) -> &'a super::loads::LoadItem {
    r.items
        .iter()
        .find(|i| i.path.as_deref() == Some(path))
        .unwrap_or_else(|| panic!("no row for {path}"))
}

fn kind_sum(r: &WhatLoads, kinds: &[&str]) -> u64 {
    r.items
        .iter()
        .filter(|i| i.loaded && kinds.contains(&i.kind))
        .map(|i| i.tokens)
        .sum()
}

#[test]
fn the_rows_that_existed_before_keep_their_numbers() {
    let fx = Fx::new();
    let r = fx.loads();
    let user = by_path(&r, &fx.at(".claude/CLAUDE.md"));
    assert_eq!((user.source, user.tokens, user.loaded), ("org", 10_070, true));
    assert_eq!(user.origin.kind, "org");
    let workspace = by_path(&r, &fx.at("work/erp/CLAUDE.md"));
    assert_eq!((workspace.source, workspace.tokens, workspace.loaded), ("project", 5_610, true));
    let managed = by_path(&r, &fx.at("work/erp/clients/acme-erp/CLAUDE.local.md"));
    assert_eq!((managed.source, managed.tokens), ("client-layer", 83));
    assert_eq!(managed.origin.kind, "managed");
    let client_rule = by_path(&r, &fx.at(".claude/rules/client-acme-erp.md"));
    assert_eq!((client_rule.source, client_rule.tokens), ("client-layer", 56));
    assert!(!client_rule.loaded && client_rule.lazy && client_rule.scope == "paths");
    let personal = by_path(&r, &fx.at(".claude/rules/personal.md"));
    assert_eq!((personal.source, personal.tokens, personal.loaded), ("personal", 47, true));
    let skills: u64 = r.items.iter().filter(|i| i.kind == "skill" && i.loaded && i.source != "project").map(|i| i.tokens).sum();
    assert_eq!(skills, 3_657);
    assert_eq!(r.items.iter().filter(|i| i.kind == "skill" && i.path.as_deref().is_some_and(|p| p.contains("/.claude/skills/skill-"))).count(), world::SKILL_COUNT);
    let mcp = r.items.iter().find(|i| i.kind == "mcp").unwrap();
    assert_eq!((mcp.name.as_str(), mcp.tokens, mcp.source), ("docs-search", 40, "user"));

    assert_eq!(
        kind_sum(&r, &["memory", "rule", "mcp", "skill"]),
        world::BASELINE_TOKENS,
        "the four kinds that were counted before still add up to the old total"
    );
}

#[test]
fn every_row_and_the_report_say_their_numbers_are_estimates() {
    let r = Fx::new().loads();
    assert_eq!(r.basis, "estimate");
    assert!(r.items.iter().all(|i| i.basis == "estimate"));
}

#[test]
fn the_new_rows_add_to_the_total_without_touching_the_old_ones() {
    let fx = Fx::new();
    let r = fx.loads();
    let old = kind_sum(&r, &["memory", "rule", "mcp", "skill"]);
    let new = kind_sum(&r, &["plugin", "command", "agent", "memory-index", "import"]);
    assert!(new > 0);
    assert_eq!(r.total_tokens, old + new);
    for kind in ["plugin", "command", "agent", "memory-index", "import"] {
        assert!(r.tokens_by_kind[kind] > 0, "{kind}");
    }

    let kit = r.items.iter().find(|i| i.kind == "plugin" && i.name == "kit@market").unwrap();
    assert!(kit.loaded);
    assert_eq!(kit.reason, "2 skills, 1 agents, 2 commands");
    assert_eq!(
        kit.tokens,
        listing("kit-build", "Build the kit")
            + listing("kit-test", "Test the kit")
            + listing("kit-agent", "Works the kit")
            + listing("kit-up", "Bring the kit up")
            + listing("kit-down", "Take the kit down")
    );
    let idle = r.items.iter().find(|i| i.kind == "plugin" && i.name == "idle@market").unwrap();
    assert!(!idle.loaded);

    let origin = |kind: &str, name: &str| {
        let i = r.items.iter().find(|i| i.kind == kind && i.name == name).unwrap_or_else(|| panic!("{kind} {name}"));
        i.origin.kind
    };
    assert_eq!(origin("command", "ship"), "org");
    assert_eq!(origin("command", "local-note"), "loose");
    assert_eq!(origin("command", "local-only"), "client");
    assert_eq!(origin("agent", "reviewer"), "library");

    let index = r.items.iter().find(|i| i.kind == "memory-index").unwrap();
    assert!(index.reason.starts_with("first 200 of 300 lines"), "{}", index.reason);
}

#[test]
fn the_import_chain_of_the_workspace_memory_is_listed_with_its_via() {
    let fx = Fx::new();
    let r = fx.loads();
    let imports: Vec<_> = r.items.iter().filter(|i| i.kind == "import").collect();
    assert_eq!(
        imports.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        ["docs/conventions.md", "naming.md", "glossary.md"]
    );
    let root = fx.at("work/erp/CLAUDE.md");
    assert_eq!(imports[0].via, [root.clone()]);
    assert_eq!(imports[2].via, [root, fx.at("work/erp/docs/conventions.md"), fx.at("work/erp/docs/naming.md")]);
    assert_eq!(imports[2].tokens, 50);
    assert!(imports.iter().all(|i| i.origin.kind == "repo" && i.origin.name == "erp"));
}

#[test]
fn files_below_the_client_load_on_demand_and_an_excluded_one_is_unloaded() {
    let fx = Fx::new();
    let r = fx.loads();
    let billing = r.items.iter().find(|i| i.name == "addons/billing/CLAUDE.md").unwrap();
    assert!(billing.lazy && !billing.loaded);
    assert_eq!(billing.tokens, 100);
    assert_eq!(billing.origin.kind, "client");
    assert_eq!(billing.reason, "loads when Claude reads a file in addons/billing");
    let refund = r.items.iter().find(|i| i.kind == "skill" && i.name == "refund").unwrap();
    assert!(refund.lazy && !refund.loaded);
    assert_eq!(r.tokens_lazy, 56 + 100 + refund.tokens);
    let excluded = r.items.iter().find(|i| i.name == "excluded-notes/CLAUDE.md").unwrap();
    assert!(!excluded.lazy && !excluded.loaded);
    assert_eq!(excluded.reason, "excluded by claudeMdExcludes");
    assert_eq!(r.tokens_lazy, r.items.iter().filter(|i| i.lazy && !i.loaded).map(|i| i.tokens).sum::<u64>());
}

#[test]
fn the_settings_of_the_workspace_around_the_client_repo_are_not_merged() {
    let fx = Fx::new();
    let r = fx.loads();
    let settings: Vec<String> = r
        .items
        .iter()
        .filter(|i| i.kind == "settings")
        .map(|i| i.path.clone().unwrap())
        .collect();
    assert_eq!(
        settings,
        [fx.at(".claude/settings.json"), fx.at("work/erp/clients/acme-erp/.claude/settings.json")]
    );
    assert!(r.items.iter().find(|i| i.kind == "plugin" && i.name == "kit@market").unwrap().loaded);
}

#[test]
fn the_skill_list_of_33_library_skills_is_over_its_budget() {
    let r = Fx::new().loads();
    let b = &r.skill_budget;
    assert_eq!((b.context_window, b.limit_tokens), (200_000, 2_000));
    assert!(b.used_tokens <= 2_000 && b.used_tokens > 1_800, "{}", b.used_tokens);
    assert!(!b.capped.is_empty());
    assert!(b.capped.iter().any(|n| n == "skill-33"));
    assert!(!b.capped.iter().any(|n| n == "skill-01"));
}
