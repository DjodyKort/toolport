use super::tests::Fixture;
use super::*;
use crate::plus::skills::tap_fixtures::Remotes;
use crate::plus::skills::tap_handlers::TEST_GIT;
use crate::plus::testutil::tree_snapshot;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

const WRITERS: &[&str] = &[
    "skills_tap_add",
    "skills_tap_remove",
    "skills_tap_update",
    "skills_install",
];
const READERS: &[&str] = &["skills_tap_list", "skills_search"];

struct Taps {
    fx: Fixture,
    remotes: Remotes,
}

impl Taps {
    /// The GitHub URLs a `user/repo` tap expands to are served from local bare repositories.
    fn new(tag: &str) -> Self {
        let fx = Fixture::new(tag);
        let remotes = Remotes::new(&fx.dir.join("_infra"));
        for repo in ["acme/skills", "acme/audit"] {
            remotes.publish(repo);
        }
        TEST_GIT.with(|g| *g.borrow_mut() = Some(Rc::new(remotes.rewrite())));
        Self { fx, remotes }
    }

    fn snapshot(&self) -> BTreeMap<String, Option<Vec<u8>>> {
        tree_snapshot(&self.fx.dir)
    }
}

impl Drop for Taps {
    fn drop(&mut self) {
        TEST_GIT.with(|g| *g.borrow_mut() = None);
    }
}

fn tool(name: &str) -> &'static ToolDef {
    find_tool(name).unwrap_or_else(|| panic!("{name}"))
}

fn kind(result: Result<Value, ToolError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(e) => e.kind,
    }
}

#[test]
fn the_writing_tap_tools_declare_dry_run_with_a_true_default() {
    for name in WRITERS {
        let tool = tool(name);
        assert_eq!(tool.tier, 2, "{name}");
        assert_eq!(tool.gate, Gate::None, "{name}");
        let schema = catalog::input_schema(tool);
        assert_eq!(schema["properties"]["dry_run"]["type"], "boolean", "{name}");
        assert_eq!(schema["properties"]["dry_run"]["default"], true, "{name}");
        let required = schema["required"].as_array().cloned().unwrap_or_default();
        assert!(!required.contains(&json!("dry_run")), "{name}");
        assert!(
            !schema["properties"]
                .as_object()
                .unwrap()
                .contains_key("no_audit"),
            "{name}: the audit cannot be skipped from a tool"
        );
    }
    for name in READERS {
        let tool = tool(name);
        assert_eq!(tool.tier, 1, "{name}");
        assert!(
            !catalog::input_schema(tool)["properties"]
                .as_object()
                .unwrap()
                .contains_key("dry_run"),
            "{name}"
        );
    }
}

#[test]
fn a_tool_that_writes_does_nothing_unless_dry_run_is_false() {
    let taps = Taps::new("tap-dry");
    let before = taps.snapshot();
    let defaults = [
        ("skills_tap_add", json!({"repo": "acme/skills"})),
        ("skills_install", json!({"spec": "@acme/skills"})),
        ("skills_tap_update", json!({})),
        ("skills_tap_remove", json!({"name": "acme-skills"})),
    ];
    for explicit in [false, true] {
        for (name, args) in &defaults {
            let mut args = args.clone();
            if explicit {
                args["dry_run"] = json!(true);
            }
            let result = call_tool(name, &args);
            if *name == "skills_tap_remove" {
                assert_eq!(kind(result), "not_found", "{name}");
            } else {
                assert_eq!(result.unwrap()["dryRun"], true, "{name} {explicit}");
            }
            assert_eq!(taps.snapshot(), before, "{name} explicit={explicit}");
        }
    }

    call_tool(
        "skills_tap_add",
        &json!({"repo": "acme/skills", "dry_run": false}),
    )
    .unwrap();
    let registered = taps.snapshot();
    for (name, args) in &defaults {
        let mut explicit = args.clone();
        explicit["dry_run"] = json!(true);
        for call in [&explicit, args] {
            let result = call_tool(name, call);
            if *name == "skills_tap_add" {
                assert_eq!(kind(result), "conflict", "{name}");
            } else {
                assert_eq!(result.unwrap()["dryRun"], true, "{name} {call}");
            }
            assert_eq!(taps.snapshot(), registered, "{name} {call}");
        }
    }
    assert!(!taps.fx.repo.join("rules").exists());
    assert!(!taps.fx.repo.join("skills/code-review").exists());
}

#[test]
fn the_tap_tools_apply_when_dry_run_is_false() {
    let taps = Taps::new("tap-apply");
    let added = call_tool(
        "skills_tap_add",
        &json!({"repo": "acme/skills", "name": "acme-skills", "dry_run": false}),
    )
    .unwrap();
    assert_eq!(added["name"], "acme-skills");
    assert_eq!(added["cloned"], true);
    let listed = call_tool("skills_tap_list", &json!({})).unwrap();
    assert_eq!(listed["taps"][0]["name"], "acme-skills");
    assert_eq!(listed["taps"][0]["cloned"], true);

    let found = call_tool("skills_search", &json!({"query": "terraform"})).unwrap();
    assert_eq!(found["results"][0]["name"], "terraform-helper");

    let installed = call_tool(
        "skills_install",
        &json!({"spec": "@acme/skills", "dry_run": false}),
    )
    .unwrap();
    assert_eq!(installed["installedCount"], 3, "{installed}");
    assert!(taps.fx.repo.join("skills/code-review/SKILL.md").exists());
    assert!(taps.fx.repo.join("rules/style-guide/SKILL.md").exists());
    assert!(!taps.fx.repo.join("skills/escape").exists());

    taps.remotes.commit_file(
        "acme/skills",
        "skills/new-one/SKILL.md",
        "---\nname: new-one\ndescription: Newly added\n---\nbody\n",
    );
    let updated = call_tool("skills_tap_update", &json!({"dry_run": false})).unwrap();
    assert_eq!(updated["failed"], 0);
    assert_eq!(updated["results"][0]["ok"], true);

    let removed = call_tool(
        "skills_tap_remove",
        &json!({"name": "acme-skills", "dry_run": false}),
    )
    .unwrap();
    assert_eq!(removed["removed"], true);
    assert_eq!(
        call_tool("skills_tap_list", &json!({})).unwrap()["taps"],
        json!([])
    );
}

#[test]
fn a_high_severity_audit_finding_blocks_an_applied_install() {
    let taps = Taps::new("tap-audit");
    let before = taps.snapshot();
    let blocked = call_tool(
        "skills_install",
        &json!({"spec": "@acme/audit", "dry_run": false}),
    )
    .unwrap();
    assert_eq!(blocked["blocked"], true);
    assert_eq!(blocked["installedCount"], 0);
    assert!(blocked["audit"]["high"].as_u64().unwrap() >= 1);
    assert!(!taps.fx.repo.join("skills/injector").exists());
    let after = taps.snapshot();
    let added: Vec<&String> = after.keys().filter(|k| !before.contains_key(*k)).collect();
    assert!(
        added
            .iter()
            .all(|k| k.starts_with("data/") || k.contains("taps")),
        "{added:?}"
    );
    assert_eq!(
        kind(call_tool(
            "skills_install",
            &json!({"spec": "@acme/audit", "no_audit": true, "dry_run": false})
        )),
        "invalid_arguments"
    );
}

#[test]
fn the_tap_tools_reject_bad_input_with_their_kind() {
    let taps = Taps::new("tap-bad");
    let before = taps.snapshot();
    let cases: &[(&str, Value, &str)] = &[
        ("skills_tap_add", json!({}), "invalid_arguments"),
        (
            "skills_tap_add",
            json!({"repo": "acme/skills", "dry_run": "no"}),
            "invalid_arguments",
        ),
        (
            "skills_tap_add",
            json!({"repo": "acme/skills", "confirm": true}),
            "invalid_arguments",
        ),
        (
            "skills_tap_add",
            json!({"repo": "http://example.com/a/b.git"}),
            "invalid_arguments",
        ),
        (
            "skills_tap_add",
            json!({"repo": "acme/skills", "name": "../x"}),
            "invalid_arguments",
        ),
        (
            "skills_tap_add",
            json!({"repo": "acme/skills", "name": "/abs"}),
            "invalid_arguments",
        ),
        (
            "skills_tap_add",
            json!({"repo": "acme/skills", "name": "a\\b"}),
            "invalid_arguments",
        ),
        (
            "skills_tap_remove",
            json!({"name": "../x"}),
            "invalid_arguments",
        ),
        ("skills_tap_remove", json!({"name": "nope"}), "not_found"),
        ("skills_tap_update", json!({"name": "nope"}), "not_found"),
        (
            "skills_tap_update",
            json!({"name": "../x"}),
            "invalid_arguments",
        ),
        ("skills_search", json!({}), "invalid_arguments"),
        (
            "skills_tap_list",
            json!({"dry_run": true}),
            "invalid_arguments",
        ),
        (
            "skills_install",
            json!({"spec": "@acme/skills/../escape"}),
            "invalid_arguments",
        ),
        (
            "skills_install",
            json!({"spec": "@acme"}),
            "invalid_arguments",
        ),
    ];
    for (name, args, want) in cases {
        let got = kind(call_tool(name, args));
        assert_eq!(got, *want, "{name} {args}");
    }
    let after = taps.snapshot();
    assert_eq!(after, before);
    assert_eq!(
        kind(call_tool(
            "skills_tap_add",
            &json!({"repo": "acme/skills", "dry_run": false})
        )),
        "ok"
    );
    assert_eq!(
        kind(call_tool(
            "skills_tap_add",
            &json!({"repo": "acme/skills", "dry_run": false})
        )),
        "conflict"
    );
    assert_eq!(
        kind(call_tool(
            "skills_install",
            &json!({"spec": "@acme/skills/escape"})
        )),
        "not_found"
    );
}
