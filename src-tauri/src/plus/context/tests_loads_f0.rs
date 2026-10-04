//! The three assumptions proof F0 showed wrong (MIG-SRC-2): `claudeMdExcludes` entries are
//! globs, settings of folders above the git root are not merged, and the skill list has a budget
//! of 1% of the context window.

use super::loads::{what_loads, what_loads_with, LoadsOptions};
use super::tests_loads::{config, install_plugin, loads, row, skill, tokens, Home};
use serde_json::json;
use std::fs;

#[test]
fn claude_md_excludes_are_globs_and_a_plain_path_still_matches_itself() {
    let h = Home::new();
    h.put(".claude/settings.json", r#"{"claudeMdExcludes":["**/parent/CLAUDE.md"]}"#);
    h.put("work/parent/CLAUDE.md", "parent rules\n");
    h.put("work/parent/app/CLAUDE.md", "app rules\n");
    let app = h.repo("work/parent/app");
    let r = loads(&h, &app);
    let parent = r.items.iter().find(|i| i.path.as_deref() == Some(h.at("work/parent/CLAUDE.md").to_str().unwrap())).unwrap();
    assert!(!parent.loaded);
    assert_eq!(parent.reason, "excluded by claudeMdExcludes");
    assert!(r.items.iter().any(|i| i.name == "CLAUDE.md" && i.loaded && i.source == "project"));
    assert_eq!(r.total_tokens, tokens("app rules\n"));

    let exact = h.at("work/parent/app/CLAUDE.md").display().to_string();
    h.put(
        ".claude/settings.json",
        &json!({"claudeMdExcludes": [exact]}).to_string(),
    );
    let r = loads(&h, &app);
    assert_eq!(r.total_tokens, tokens("parent rules\n"));
}

#[test]
fn exclude_patterns_of_every_settings_layer_apply_together() {
    let h = Home::new();
    h.put(".claude/settings.json", r#"{"claudeMdExcludes":["**/one/CLAUDE.md"]}"#);
    h.put("work/one/CLAUDE.md", "one\n");
    h.put("work/one/two/CLAUDE.md", "two\n");
    h.put("work/one/two/app/CLAUDE.md", "app\n");
    let app = h.repo("work/one/two/app");
    h.put(
        "work/one/two/app/.claude/settings.local.json",
        r#"{"claudeMdExcludes":["**/two/CLAUDE.md"]}"#,
    );
    let r = loads(&h, &app);
    assert_eq!(r.total_tokens, tokens("app\n"));
    assert_eq!(r.items.iter().filter(|i| i.reason == "excluded by claudeMdExcludes").count(), 2);
}

#[test]
fn a_glob_exclude_also_unloads_a_nested_file_that_would_load_on_demand() {
    let h = Home::new();
    let app = h.repo("work/app");
    h.put("work/app/.claude/settings.json", r#"{"claudeMdExcludes":["**/vendor-notes/CLAUDE.md"]}"#);
    h.put("work/app/vendor-notes/CLAUDE.md", "skip me\n");
    h.put("work/app/billing/CLAUDE.md", "keep me\n");
    let r = loads(&h, &app);
    let skipped = row(&r, "memory", "vendor-notes/CLAUDE.md");
    assert!(!skipped.loaded && !skipped.lazy);
    assert_eq!(skipped.reason, "excluded by claudeMdExcludes");
    assert!(row(&r, "memory", "billing/CLAUDE.md").lazy);
    assert_eq!(r.tokens_lazy, tokens("keep me\n"));
}

fn settings_rows(h: &Home, r: &super::loads::WhatLoads) -> Vec<String> {
    let user = h.at(".claude/settings.json").display().to_string();
    r.items
        .iter()
        .filter(|i| i.kind == "settings")
        .map(|i| i.path.clone().unwrap())
        .filter(|p| *p != user)
        .collect()
}

#[test]
fn settings_of_a_folder_above_the_git_root_are_not_merged() {
    let h = Home::new();
    h.put(".claude/settings.json", r#"{"enabledPlugins":{"kit@market":true}}"#);
    install_plugin(&h, "kit@market", "1.0.0", &[("skills/k/SKILL.md", &skill("k", "Kit"))]);
    h.put("work/parent/.claude/settings.local.json", r#"{"enabledPlugins":{"kit@market":false},"model":"parent"}"#);
    h.put("work/parent/.claude/settings.json", r#"{"model":"parent-shared"}"#);
    h.put("work/parent/CLAUDE.md", "parent memory\n");
    let app = h.repo("work/parent/app");
    h.put("work/parent/app/.claude/settings.json", r#"{"model":"app"}"#);
    let r = loads(&h, &app);

    assert_eq!(
        settings_rows(&h, &r),
        [h.at("work/parent/app/.claude/settings.json").display().to_string()]
    );
    assert!(row(&r, "plugin", "kit@market").loaded);
    assert!(r.clobbers.iter().all(|c| !c.winner.contains("work/parent/.claude") && !c.overridden.iter().any(|o| o.contains("work/parent/.claude"))));
    let parent_memory = r.items.iter().find(|i| i.path.as_deref() == Some(h.at("work/parent/CLAUDE.md").to_str().unwrap())).unwrap();
    assert!(parent_memory.loaded, "memory files still come from every folder above");
}

#[test]
fn settings_are_read_from_the_folder_and_from_its_git_root_only() {
    let h = Home::new();
    let app = h.repo("work/app");
    h.put("work/app/.claude/settings.json", r#"{"model":"root"}"#);
    h.put("work/app/packages/.claude/settings.json", r#"{"model":"middle"}"#);
    h.put("work/app/packages/web/.claude/settings.local.json", r#"{"model":"leaf"}"#);
    let leaf = app.join("packages/web");
    let r = loads(&h, &leaf);
    assert_eq!(
        settings_rows(&h, &r),
        [
            h.at("work/app/.claude/settings.json").display().to_string(),
            h.at("work/app/packages/web/.claude/settings.local.json").display().to_string(),
        ]
    );
}

#[test]
fn outside_a_repository_only_the_folder_itself_counts() {
    let h = Home::new();
    h.put("work/.claude/settings.json", r#"{"model":"above"}"#);
    h.put("work/loose/.claude/settings.json", r#"{"model":"here"}"#);
    let r = loads(&h, &h.at("work/loose"));
    assert_eq!(
        settings_rows(&h, &r),
        [h.at("work/loose/.claude/settings.json").display().to_string()]
    );
}

#[test]
fn a_nested_repository_ignores_the_settings_of_the_repository_around_it() {
    let h = Home::new();
    h.repo("work/outer");
    h.put("work/outer/.claude/settings.local.json", r#"{"model":"outer"}"#);
    let inner = h.repo("work/outer/clients/inner");
    h.put("work/outer/clients/inner/.claude/settings.json", r#"{"model":"inner"}"#);
    let r = loads(&h, &inner);
    assert_eq!(
        settings_rows(&h, &r),
        [h.at("work/outer/clients/inner/.claude/settings.json").display().to_string()]
    );
}

fn many_skills(h: &Home, count: usize) {
    for n in 0..count {
        let name = format!("skill-{n:02}");
        h.put(
            &format!(".claude/skills/{name}/SKILL.md"),
            &skill(&name, &"d".repeat(200)),
        );
    }
}

#[test]
fn the_skill_list_budget_is_one_percent_of_the_window_and_names_what_it_caps() {
    let h = Home::new();
    many_skills(&h, 40);
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = loads(&h, &cwd);
    let per = tokens(&format!("skill-00: {}", "d".repeat(200)));
    let b = &r.skill_budget;
    assert_eq!((b.fraction, b.context_window, b.limit_tokens), (0.01, 200_000, 2_000));
    let fit = (2_000 / per) as usize;
    assert_eq!(b.used_tokens, per * fit as u64);
    assert!(b.used_tokens <= b.limit_tokens);
    let want: Vec<String> = (fit..40).map(|n| format!("skill-{n:02}")).collect();
    assert_eq!(b.capped, want);
    assert!(r.notes.iter().any(|n| n.contains("over its budget")), "{:?}", r.notes);
    assert_eq!(r.tokens_by_kind["skill"], per * 40, "the estimate stays the uncapped sum");
}

#[test]
fn a_larger_window_raises_the_budget_and_nothing_is_capped_when_it_fits() {
    let h = Home::new();
    many_skills(&h, 40);
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = what_loads_with(
        &h.roots(),
        &config(json!({})),
        None,
        &cwd,
        &LoadsOptions {
            context_window: Some(1_000_000),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.skill_budget.limit_tokens, 10_000);
    assert!(r.skill_budget.capped.is_empty());
    assert!(!r.notes.iter().any(|n| n.contains("over its budget")));
}

#[test]
fn enabled_plugin_skills_share_the_budget_under_their_plugin_name() {
    let h = Home::new();
    h.put(".claude/settings.json", r#"{"enabledPlugins":{"kit@market":true,"off@market":false}}"#);
    let files: Vec<(String, String)> = (0..30)
        .map(|n| {
            let name = format!("s-{n:02}");
            (format!("skills/{name}/SKILL.md"), skill(&name, &"p".repeat(380)))
        })
        .collect();
    let parts: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    install_plugin(&h, "kit@market", "1.0.0", &parts);
    install_plugin(&h, "off@market", "1.0.0", &[("skills/hidden/SKILL.md", &skill("hidden", "never"))]);
    h.put(".claude/skills/mine/SKILL.md", &skill("mine", "Own skill"));
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    let r = what_loads(&h.roots(), &config(json!({})), None, &cwd).unwrap();
    let capped = &r.skill_budget.capped;
    assert!(capped.iter().any(|n| n == "kit:s-29"), "{capped:?}");
    assert!(!capped.iter().any(|n| n == "kit:s-00"), "{capped:?}");
    assert!(capped.iter().all(|n| n.starts_with("kit:")), "{capped:?}");
    assert!(row(&r, "skill", "mine").loaded);
    assert!(r.skill_budget.used_tokens <= r.skill_budget.limit_tokens);
}

#[test]
fn a_skill_claude_code_does_not_list_uses_no_budget() {
    let h = Home::new();
    h.put(
        ".claude/skills/broken/SKILL.md",
        "---\nname: broken\ndescription: \"first line\ncontinues here\"\n---\nBody.\n",
    );
    let cwd = h.at("work");
    fs::create_dir_all(&cwd).unwrap();
    assert_eq!(loads(&h, &cwd).skill_budget.used_tokens, 0);
}
