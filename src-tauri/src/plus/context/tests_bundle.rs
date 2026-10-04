use super::bundle::{self, Edit};
use super::bundle_store;
use super::Roots;
use crate::plus::randutil::ScratchDir;
use std::fs;

const ACME: &str = include_str!("../../../tests/fixtures/bundles/acme-dev.yaml");
const LEGACY: &str = include_str!("../../../tests/fixtures/bundles/default.yaml");
const UNKNOWN: &str = include_str!("../../../tests/fixtures/bundles/unknown-keys.yaml");

fn list(items: &[&str]) -> Option<Vec<String>> {
    Some(items.iter().map(|s| s.to_string()).collect())
}

fn world(tag: &str) -> (ScratchDir, Roots) {
    let scratch = ScratchDir::new(tag);
    let roots = Roots::from_home(scratch.path());
    fs::create_dir_all(bundle_store::dir(&roots)).unwrap();
    (scratch, roots)
}

#[test]
fn the_format_of_the_contract_parses_into_every_field() {
    let b = bundle::parse("acme-dev", ACME).unwrap();
    assert_eq!(b.description, "ERP work: no marketplace plugins, only the ERP skills");
    assert_eq!(b.servers.as_deref(), Some("acme-dev"));
    assert_eq!(b.skills_off, ["notes-helper", "scratch-*"]);
    assert_eq!(b.skills_name_only, ["long-guide"]);
    assert!(b.skills_allow.is_empty());
    assert_eq!(b.plugins_off, ["tools-pack@tools-market", "loop-runner@official"]);
    assert_eq!(b.layers_add, ["acme-knowledge"]);
    assert_eq!(b.layers_exclude, ["**/acme-erp/CLAUDE.md"]);
    assert_eq!(b.agents_off, ["reviewer-bot"]);
    assert_eq!(b.bind, ["~/work/acme-erp/clients/*"]);
    assert!(bundle::lint("acme-dev", ACME).is_empty());
}

#[test]
fn a_plain_skills_list_is_read_as_the_allow_list() {
    let b = bundle::parse("default", LEGACY).unwrap();
    assert!(b.legacy_list);
    assert_eq!(b.skills_allow, ["erp-core", "erp-reports"]);
    assert!(b.skills_off.is_empty());
    let issues = bundle::lint("default", LEGACY);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].level, "warning");
    assert_eq!(issues[0].key, "skills");
}

#[test]
fn lint_reports_unknown_and_reserved_keys_and_edit_keeps_them() {
    let issues = bundle::lint("unknown-keys", UNKNOWN);
    let keys: Vec<&str> = issues.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["extra", "plugins.config", "mcp.deny", "skills.future"]);
    assert!(issues.iter().all(|i| i.level == "warning"));
    assert!(issues[1].message.contains("reserved"));
    let edit = Edit {
        plugins_off: list(&["other@market"]),
        ..Edit::default()
    };
    let text = bundle::edited("unknown-keys", UNKNOWN, &edit).unwrap();
    let doc: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
    assert_eq!(doc["extra"]["note"], "kept");
    assert_eq!(doc["skills"]["future"][0], "x");
    assert_eq!(doc["mcp"]["deny"][0], "plugin:tools-pack:browser");
    assert_eq!(doc["plugins"]["config"]["tools-pack@tools-market"]["profile"], "minimal");
    assert_eq!(doc["plugins"]["off"][0], "other@market");
    assert_eq!(doc["plugins"]["off"].as_sequence().unwrap().len(), 1);
}

#[test]
fn lint_finds_wrong_types_bad_formats_repeats_and_a_name_mismatch() {
    let broken = bundle::lint("x", "skills:\n  off: not-a-list\n");
    assert_eq!(broken.len(), 1);
    assert_eq!(broken[0].level, "error");
    assert!(broken[0].message.contains("skills.off"));
    assert!(bundle::lint("x", "format: 2\n")[0].message.contains("format must be 1"));
    assert_eq!(bundle::lint("x", "a: [")[0].level, "error");
    let text = "format: 1\nname: other\nplugins:\n  off: [a@b, a@b, plain]\nskills:\n  off: [s]\n  name_only: [s]\n";
    let messages: Vec<String> = bundle::lint("x", text).into_iter().map(|i| i.message).collect();
    assert!(messages.iter().any(|m| m.contains("a@b is listed twice")));
    assert!(messages.iter().any(|m| m.contains("plain is not a plugin id")));
    assert!(messages.iter().any(|m| m.contains("the file name wins")));
    assert!(messages.iter().any(|m| m.contains("s is also in skills.off")));
}

#[test]
fn add_creates_the_canonical_order_and_edit_replaces_only_the_given_lists() {
    let created = bundle::create(
        "ops",
        &Edit {
            description: Some("Ops".into()),
            skills_off: list(&["a", "b"]),
            plugins_off: list(&["p@m"]),
            bind: list(&["~/ops/*"]),
            ..Edit::default()
        },
    );
    assert!(created.starts_with("format: 1\nname: ops\ndescription: Ops\nskills:\n"));
    let b = bundle::parse("ops", &created).unwrap();
    assert_eq!(b.skills_off, ["a", "b"]);
    let edited = bundle::edited(
        "ops",
        &created,
        &Edit {
            skills_off: list(&["c"]),
            plugins_off: Some(Vec::new()),
            ..Edit::default()
        },
    )
    .unwrap();
    let b = bundle::parse("ops", &edited).unwrap();
    assert_eq!(b.skills_off, ["c"]);
    assert!(b.plugins_off.is_empty());
    assert!(!edited.contains("plugins"));
    assert_eq!(b.bind, ["~/ops/*"]);
    let legacy = bundle::edited("default", LEGACY, &Edit { skills_off: list(&["z"]), ..Edit::default() }).unwrap();
    let b = bundle::parse("default", &legacy).unwrap();
    assert_eq!((b.skills_allow.len(), b.skills_off.as_slice()), (2, &["z".to_string()][..]));
    assert!(!b.legacy_list);
}

#[test]
fn bind_patterns_expand_the_home_directory_and_stop_at_a_slash() {
    let home = "/home/u";
    assert!(bundle::bind_matches("~/work/acme-erp/clients/*", "/home/u/work/acme-erp/clients/v18", home));
    assert!(!bundle::bind_matches("~/work/acme-erp/clients/*", "/home/u/work/acme-erp/clients/v18/sub", home));
    assert!(bundle::bind_matches("/srv/app/", "/srv/app", home));
    assert!(!bundle::bind_matches("~/work/*", "/elsewhere/work/x", home));
}

#[test]
fn the_store_adds_edits_lists_and_removes_files_in_the_profiles_directory() {
    let (_scratch, roots) = world("bundle-store");
    let edit = Edit { skills_off: list(&["a"]), ..Edit::default() };
    let dry = bundle_store::save(&roots, "ops", &edit, true, true).unwrap();
    assert!(!dry.path.exists());
    let written = bundle_store::save(&roots, "ops", &edit, true, false).unwrap();
    assert_eq!(fs::read_to_string(&written.path).unwrap(), written.after);
    assert_eq!(bundle_store::save(&roots, "ops", &edit, true, false).unwrap_err().code, "exists");
    assert_eq!(bundle_store::save(&roots, "nope", &edit, false, false).unwrap_err().code, "not_found");
    fs::write(bundle_store::dir(&roots).join("default.yaml"), LEGACY).unwrap();
    fs::write(bundle_store::dir(&roots).join("broken.yaml"), "skills: [").unwrap();
    fs::write(bundle_store::dir(&roots).join("notes.txt"), "x").unwrap();
    let listed = bundle_store::list(&roots);
    let names: Vec<&str> = listed.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["broken", "default", "ops"]);
    assert!(listed[0].parsed.is_err());
    bundle_store::save(&roots, "ops", &Edit { description: Some("d".into()), ..Edit::default() }, false, false).unwrap();
    assert_eq!(bundle_store::load(&roots, "ops").unwrap().bundle.description, "d");
    assert_eq!(bundle_store::load(&roots, "../x").unwrap_err().code, "usage");
    bundle_store::remove(&roots, "ops", true).unwrap();
    assert!(written.path.exists());
    bundle_store::remove(&roots, "ops", false).unwrap();
    assert!(!written.path.exists());
    assert_eq!(bundle_store::remove(&roots, "ops", false).unwrap_err().code, "not_found");
}

#[test]
fn add_from_a_folder_reads_only_the_keys_a_bundle_owns() {
    let scratch = ScratchDir::new("bundle-from-folder");
    let dir = scratch.path().join(".claude");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("settings.local.json"),
        r#"{"permissions":{"allow":["Bash(ls)"],"deny":["Agent(bot)","Read(x)"]},"skillOverrides":{"a":"off","b":"name-only","c":"on"},"enabledPlugins":{"p@m":false,"q@m":true},"claudeMdExcludes":["**/x.md"],"env":{"K":"v"}}"#,
    )
    .unwrap();
    let edit = bundle_store::edit_from_folder(scratch.path()).unwrap();
    assert_eq!(edit.skills_off, list(&["a"]));
    assert_eq!(edit.skills_name_only, list(&["b"]));
    assert_eq!(edit.plugins_off, list(&["p@m"]));
    assert_eq!(edit.layers_exclude, list(&["**/x.md"]));
    assert_eq!(edit.agents_off, list(&["bot"]));
    assert!(edit.description.is_none() && edit.bind.is_none());
}
