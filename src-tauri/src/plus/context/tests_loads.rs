//! The rows `context loads` adds to the memory, rule, MCP and skill walk (MIG-SRC-2): plugins,
//! commands, agents, the memory index, `@imports`, files that load on demand, and who a row
//! belongs to.

use super::config::ContextConfig;
use super::layers::MANAGED_LOCAL_HEADER;
use super::loads::{what_loads, what_loads_with, LoadItem, LoadsOptions, WhatLoads};
use super::roots::Roots;
use crate::savings::estimated_tokens;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub(super) struct Home(PathBuf);

impl Home {
    pub(super) fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = base.join(format!(
            "loads-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(super) fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    pub(super) fn repo(&self, rel: &str) -> PathBuf {
        let dir = self.0.join(rel);
        fs::create_dir_all(dir.join(".git")).unwrap();
        dir
    }

    pub(super) fn roots(&self) -> Roots {
        Roots::from_home(&self.0)
    }

    pub(super) fn at(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn config(extra: Value) -> ContextConfig {
    let mut base = json!({"wrap_default_claude": false, "dedupe": {"enabled": false}});
    for (key, value) in extra.as_object().unwrap() {
        base[key] = value.clone();
    }
    ContextConfig::from_value(base).unwrap()
}

pub(super) fn loads(h: &Home, cwd: &Path) -> WhatLoads {
    what_loads(&h.roots(), &config(json!({})), None, cwd).unwrap()
}

pub(super) fn row<'a>(r: &'a WhatLoads, kind: &str, name: &str) -> &'a LoadItem {
    r.items
        .iter()
        .find(|i| i.kind == kind && i.name == name)
        .unwrap_or_else(|| {
            let have: Vec<String> = r
                .items
                .iter()
                .map(|i| format!("{}:{}", i.kind, i.name))
                .collect();
            panic!("no {kind} row named {name}; rows: {have:?}")
        })
}

pub(super) fn tokens(text: &str) -> u64 {
    estimated_tokens(text.len() as u64)
}

pub(super) fn listing(name: &str, description: &str) -> u64 {
    estimated_tokens((name.len() + description.len()) as u64)
}

pub(super) fn skill(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\nBody of {name}.\n")
}

fn slug(path: &Path) -> String {
    path.display()
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub(super) fn install_plugin(h: &Home, id: &str, version: &str, parts: &[(&str, &str)]) -> PathBuf {
    let dir = h.at(&format!(".claude/plugins/cache/market/{}/{version}", id.split('@').next().unwrap()));
    for (rel, text) in parts {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let file = h.at(".claude/plugins/installed_plugins.json");
    let mut doc: Value = fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({"version": 2, "plugins": {}}));
    doc["plugins"][id] = json!([{
        "scope": "user",
        "installPath": dir.display().to_string(),
        "version": version,
    }]);
    h.put(
        ".claude/plugins/installed_plugins.json",
        &serde_json::to_string(&doc).unwrap(),
    );
    dir
}

#[test]
fn a_plugin_is_one_row_with_its_parts_and_follows_the_enabled_switch() {
    let h = Home::new();
    let kit_skill = skill("kit-skill", "Does the kit thing");
    install_plugin(
        &h,
        "kit@market",
        "1.0.0",
        &[
            ("skills/kit-skill/SKILL.md", &kit_skill),
            ("agents/kit-agent.md", "---\nname: kit-agent\ndescription: Kit agent\n---\nAgent.\n"),
            ("commands/kit-up.md", "# Bring the kit up\nRun it.\n"),
            ("commands/kit-down.md", "# Take the kit down\nRun it.\n"),
        ],
    );
    install_plugin(&h, "idle@market", "0.3.0", &[("skills/idle/SKILL.md", &skill("idle", "Never on"))]);
    h.put(
        ".claude/settings.json",
        r#"{"enabledPlugins":{"kit@market":true,"idle@market":false}}"#,
    );
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = loads(&h, &cwd);

    let kit = row(&r, "plugin", "kit@market");
    assert!(kit.loaded && !kit.lazy);
    assert_eq!(kit.reason, "1 skills, 1 agents, 2 commands");
    assert_eq!((kit.origin.kind, kit.origin.name.as_str()), ("plugin", "kit@market"));
    assert!(!kit.writable);
    let parts = listing("kit-skill", "Does the kit thing")
        + listing("kit-agent", "Kit agent")
        + listing("kit-up", "Bring the kit up")
        + listing("kit-down", "Take the kit down");
    assert_eq!(kit.tokens, parts);
    let idle = row(&r, "plugin", "idle@market");
    assert!(!idle.loaded && !idle.lazy);
    assert!(idle.reason.starts_with("turned off in "), "{}", idle.reason);
    assert_eq!(r.tokens_by_kind["plugin"], kit.tokens);
    assert_eq!(r.total_tokens, kit.tokens);
}

#[test]
fn a_project_settings_file_decides_a_plugin_over_the_user_settings() {
    let h = Home::new();
    install_plugin(&h, "kit@market", "1.0.0", &[("skills/k/SKILL.md", &skill("k", "Kit"))]);
    h.put(".claude/settings.json", r#"{"enabledPlugins":{"kit@market":true}}"#);
    let app = h.repo("work/app");
    h.put("work/app/.claude/settings.local.json", r#"{"enabledPlugins":{"kit@market":false}}"#);
    let r = loads(&h, &app);
    let kit = row(&r, "plugin", "kit@market");
    assert!(!kit.loaded);
    assert!(kit.reason.contains("settings.local.json"), "{}", kit.reason);
}

#[test]
fn an_installed_plugin_nobody_enabled_is_listed_but_not_counted() {
    let h = Home::new();
    install_plugin(&h, "kit@market", "1.0.0", &[("skills/k/SKILL.md", &skill("k", "Kit"))]);
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = loads(&h, &cwd);
    let kit = row(&r, "plugin", "kit@market");
    assert!(!kit.loaded);
    assert_eq!(kit.reason, "not enabled in any settings file");
    assert_eq!(r.total_tokens, 0);
}

#[test]
fn commands_and_agents_belong_to_whoever_manages_them_and_the_rest_is_loose() {
    let h = Home::new();
    h.put(".local/share/corp-tools/claude/CLAUDE.md", "# Corp\n");
    h.put(".local/share/corp-tools/claude/commands/ship.md", "# Ship it\nSteps.\n");
    h.put(".claude/commands/ship.md", "# Ship it\nSteps.\n");
    h.put(".claude/commands/mine.md", "# My own\nSteps.\n");
    h.put(
        ".claude/commands/ctx.md",
        &format!("{MANAGED_LOCAL_HEADER}\n# Context helper\nSteps.\n"),
    );
    h.put(
        ".config/mcpm/skills_repo/agents/reviewer/AGENT.md",
        "---\nname: reviewer\ndescription: Reviews a change\n---\nBody.\n",
    );
    h.put(
        ".claude/agents/reviewer.md",
        "---\nname: reviewer\ndescription: Reviews a change\n---\nBody.\n",
    );
    h.put(
        ".claude/agents/scratch.md",
        "---\nname: scratch\ndescription: Notes\n---\nBody.\n",
    );
    let app = h.repo("work/app");
    h.put("work/app/.claude/commands/local-only.md", "# Only here\nSteps.\n");
    h.put(
        "work/app/.claude/agents/guard.md",
        "---\nname: guard\ndescription: Guards the app\n---\nBody.\n",
    );
    let cfg = config(json!({"corp_tools_dir": "~/.local/share/corp-tools"}));
    let r = what_loads(&h.roots(), &cfg, None, &app).unwrap();
    let origin = |kind: &str, name: &str| {
        let item = row(&r, kind, name);
        (item.origin.kind, item.origin.name.clone(), item.writable)
    };
    assert_eq!(origin("command", "ship"), ("org", "corp-tools".to_string(), false));
    assert_eq!(origin("command", "mine").0, "loose");
    assert!(origin("command", "mine").2);
    assert_eq!(origin("command", "ctx"), ("managed", "toolportctl".to_string(), false));
    assert_eq!(origin("agent", "reviewer").0, "library");
    assert_eq!(origin("agent", "scratch").0, "loose");
    assert_eq!(origin("command", "local-only"), ("repo", "app".to_string(), false));
    assert_eq!(origin("agent", "guard"), ("repo", "app".to_string(), false));
    for item in r.items.iter().filter(|i| matches!(i.kind, "command" | "agent")) {
        assert!(item.loaded && item.tokens > 0, "{}", item.name);
    }
    assert_eq!(row(&r, "command", "ship").tokens, listing("ship", "Ship it"));
    assert_eq!(row(&r, "agent", "guard").tokens, listing("guard", "Guards the app"));
    let counted: u64 = r
        .items
        .iter()
        .filter(|i| matches!(i.kind, "command" | "agent"))
        .map(|i| i.tokens)
        .sum();
    assert_eq!(r.tokens_by_kind["command"] + r.tokens_by_kind["agent"], counted);
}

#[test]
fn a_user_rule_or_skill_with_no_manager_is_loose_never_org() {
    let h = Home::new();
    h.put(".claude/rules/notes.md", "keep notes short");
    h.put(".claude/skills/solo/SKILL.md", &skill("solo", "Standalone"));
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = loads(&h, &cwd);
    for (kind, name) in [("rule", "notes"), ("skill", "solo")] {
        let item = row(&r, kind, name);
        assert_eq!(item.source, "loose", "{name}");
        assert_eq!(item.origin.kind, "loose", "{name}");
        assert!(item.writable, "{name}");
    }
}

#[test]
fn the_user_memory_file_is_org_only_when_the_corp_clone_ships_one() {
    let h = Home::new();
    h.put(".claude/CLAUDE.md", "# Mine\n");
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let alone = loads(&h, &cwd);
    assert_eq!(row(&alone, "memory", "CLAUDE.md").origin.kind, "loose");
    h.put(".local/share/corp-dev-tools/claude/CLAUDE.md", "# Corp\n");
    let with_clone = loads(&h, &cwd);
    let item = row(&with_clone, "memory", "CLAUDE.md");
    assert_eq!(
        (item.origin.kind, item.origin.name.as_str(), item.writable),
        ("org", "corp-dev-tools", false)
    );
}

#[test]
fn the_memory_index_counts_its_first_200_lines() {
    let h = Home::new();
    let app = h.repo("work/app");
    let long: String = (1..=300).map(|n| format!("- note {n}\n")).collect();
    let dir = format!(".claude/projects/{}/memory/MEMORY.md", slug(&app));
    h.put(&dir, &long);
    let r = loads(&h, &app);
    let index = row(&r, "memory-index", "MEMORY.md");
    let kept: String = long.lines().take(200).map(|l| format!("{l}\n")).collect();
    assert_eq!(index.tokens, tokens(&kept));
    assert!(index.loaded);
    assert!(index.reason.contains("first 200 of 300 lines"), "{}", index.reason);
    assert_eq!(index.origin.kind, "user");
    assert_eq!(r.tokens_by_kind["memory-index"], index.tokens);
}

#[test]
fn the_memory_index_stops_at_25_kb() {
    let h = Home::new();
    let app = h.repo("work/app");
    let wide: String = (0..100).map(|_| format!("{}\n", "w".repeat(499))).collect();
    h.put(&format!(".claude/projects/{}/memory/MEMORY.md", slug(&app)), &wide);
    let index = loads(&h, &app).items.into_iter().find(|i| i.kind == "memory-index").unwrap();
    assert!(index.tokens <= estimated_tokens(25 * 1024), "{}", index.tokens);
    assert!(index.tokens > estimated_tokens(24 * 1024), "{}", index.tokens);
}

#[test]
fn no_memory_index_row_without_a_memory_file() {
    let h = Home::new();
    let app = h.repo("work/app");
    assert!(loads(&h, &app).items.iter().all(|i| i.kind != "memory-index"));
}

#[test]
fn imports_are_followed_four_hops_deep_and_carry_the_chain_that_led_to_them() {
    let h = Home::new();
    let app = h.repo("work/app");
    h.put(
        "work/app/CLAUDE.md",
        "# App\nSee @docs/a.md for more. Mail someone@example.org about it.\n\
         `@docs/in-code.md` is code, and so is\n```\n@docs/in-fence.md\n```\n",
    );
    h.put("work/app/docs/a.md", "a body @b.md\n");
    h.put("work/app/docs/b.md", "b body @c.md and @../loop.md\n");
    h.put("work/app/docs/c.md", "c body @d.md\n");
    h.put("work/app/docs/d.md", "d body @e.md\n");
    h.put("work/app/docs/e.md", "e is the fifth hop\n");
    h.put("work/app/loop.md", "loops back @CLAUDE.md\n");
    h.put("work/app/docs/in-code.md", "never listed\n");
    h.put("work/app/docs/in-fence.md", "never listed\n");
    let r = loads(&h, &app);
    let imports: Vec<&LoadItem> = r.items.iter().filter(|i| i.kind == "import").collect();
    let names: Vec<&str> = imports.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["docs/a.md", "b.md", "c.md", "d.md", "../loop.md"]);
    let root = h.at("work/app/CLAUDE.md").display().to_string();
    let doc = |n: &str| h.at(&format!("work/app/docs/{n}.md")).display().to_string();
    let via = |name: &str| row(&r, "import", name).via.clone();
    assert_eq!(via("docs/a.md"), [root.clone()]);
    assert_eq!(via("b.md"), [root.clone(), doc("a")]);
    assert_eq!(via("d.md"), [root.clone(), doc("a"), doc("b"), doc("c")]);
    assert_eq!(via("../loop.md"), [root.clone(), doc("a"), doc("b")]);
    assert!(imports.iter().all(|i| i.loaded && !i.lazy && i.source == "project"));
    assert!(imports.iter().all(|i| i.origin.kind == "repo" && i.origin.name == "app"));
    assert_eq!(row(&r, "import", "d.md").tokens, tokens("d body @e.md\n"));
    assert!(r.tokens_by_kind["import"] > 0);
}

#[test]
fn imports_of_the_user_memory_resolve_from_home_and_stay_org() {
    let h = Home::new();
    h.put(".local/share/corp-dev-tools/claude/CLAUDE.md", "# Corp\n");
    h.put(".claude/CLAUDE.md", "# Corp\n@~/shared/style.md\n@missing.md\n");
    h.put("shared/style.md", "style text\n");
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = loads(&h, &cwd);
    let style = row(&r, "import", "~/shared/style.md");
    assert_eq!(style.origin.kind, "org");
    assert_eq!(style.via, [h.at(".claude/CLAUDE.md").display().to_string()]);
    assert_eq!(r.items.iter().filter(|i| i.kind == "import").count(), 1);
}

#[test]
fn files_below_the_folder_load_on_demand_and_stay_out_of_the_total() {
    let h = Home::new();
    let app = h.repo("work/app");
    h.put("work/app/CLAUDE.md", "top\n");
    let nested = "# Billing notes\nUse cents.\n";
    h.put("work/app/billing/CLAUDE.md", nested);
    h.put(
        "work/app/billing/.claude/skills/refund/SKILL.md",
        &skill("refund", "Refund an order"),
    );
    h.put("work/app/node_modules/dep/CLAUDE.md", "skipped\n");
    h.put("work/app/_claude/CLAUDE.md", "parked\n");
    let r = loads(&h, &app);

    let md = row(&r, "memory", "billing/CLAUDE.md");
    assert!(!md.loaded && md.lazy);
    assert_eq!(md.reason, "loads when Claude reads a file in billing");
    assert_eq!(md.tokens, tokens(nested));
    assert_eq!(md.origin.kind, "repo");
    let sk = row(&r, "skill", "refund");
    assert!(!sk.loaded && sk.lazy);
    assert_eq!(sk.reason, "loads when Claude reads a file in billing");
    assert_eq!(sk.tokens, listing("refund", "Refund an order"));
    assert_eq!(r.tokens_lazy, md.tokens + sk.tokens);
    assert_eq!(r.total_tokens, tokens("top\n"));
    assert!(r.items.iter().all(|i| !i.path.as_deref().unwrap_or("").contains("node_modules")));
    assert!(r.items.iter().all(|i| !i.path.as_deref().unwrap_or("").contains("_claude")));
}

#[test]
fn no_lazy_leaves_the_on_demand_rows_out() {
    let h = Home::new();
    let app = h.repo("work/app");
    h.put("work/app/billing/CLAUDE.md", "# Billing\n");
    h.put(
        "work/app/.claude/rules/api.md",
        "---\npaths: [\"src/**\"]\n---\nrule body\n",
    );
    let with = loads(&h, &app);
    assert!(with.items.iter().filter(|i| i.lazy).count() >= 2);
    assert!(with.tokens_lazy > 0);
    let without = what_loads_with(
        &h.roots(),
        &config(json!({})),
        None,
        &app,
        &LoadsOptions {
            no_lazy: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(without.items.iter().all(|i| !i.lazy));
    assert_eq!(without.tokens_lazy, 0);
    assert_eq!(without.total_tokens, with.total_tokens);
}

#[test]
fn a_path_scoped_rule_is_a_lazy_row_with_scope_paths() {
    let h = Home::new();
    let app = h.repo("work/app");
    h.put(
        "work/app/.claude/rules/api.md",
        "---\npaths: [\"src/**\"]\n---\nrule body\n",
    );
    h.put("work/app/.claude/rules/always.md", "always body\n");
    let r = loads(&h, &app);
    let scoped = row(&r, "rule", "api");
    assert_eq!((scoped.scope, scoped.lazy, scoped.loaded), ("paths", true, false));
    assert_eq!(scoped.reason, "path-scoped: loads when a matching file is read");
    let always = row(&r, "rule", "always");
    assert_eq!((always.scope, always.lazy, always.loaded), ("always", false, true));
}

#[test]
fn a_skill_claude_code_cannot_parse_is_listed_but_not_counted() {
    let h = Home::new();
    h.put(".claude/skills/fine/SKILL.md", &skill("fine", "Works"));
    h.put(
        ".claude/skills/broken/SKILL.md",
        "---\nname: broken\ndescription: \"first line\ncontinues here\"\n---\nBody.\n",
    );
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = loads(&h, &cwd);
    assert_eq!(row(&r, "skill", "fine").visible, Some(true));
    let broken = row(&r, "skill", "broken");
    assert_eq!(broken.visible, Some(false));
    assert!(!broken.loaded);
    assert!(broken.reason.starts_with("Claude Code does not list this skill"), "{}", broken.reason);
    assert_eq!(r.tokens_by_kind["skill"], row(&r, "skill", "fine").tokens);
}

#[test]
fn the_managed_policy_file_loads_first_and_cannot_be_excluded() {
    let h = Home::new();
    let policy = h.put("managed/CLAUDE.md", "# Policy\n");
    h.put(".claude/CLAUDE.md", "# Org\n");
    let mut roots = h.roots();
    roots.managed_claude_md = Some(policy.clone());
    let cfg = config(json!({}));
    let r = what_loads(&roots, &cfg, None, &h.0).unwrap();
    let first = r.items.iter().find(|i| i.kind == "memory").unwrap();
    assert_eq!(first.path.as_deref(), Some(policy.to_str().unwrap()));
    assert_eq!((first.source, first.origin.kind), ("policy", "policy"));
    assert!(first.loaded);
    h.put(
        ".claude/settings.json",
        &json!({"claudeMdExcludes": [policy.to_str().unwrap()]}).to_string(),
    );
    let r = what_loads(&roots, &cfg, None, &h.0).unwrap();
    assert!(r.items.iter().any(|i| i.source == "policy" && i.loaded));
}
