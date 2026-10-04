//! Replays the mcpm findings entry (lint, audit, collisions) over the golden inputs and
//! compares the emitted JSON byte-for-byte with the goldens, including mcpm's audit blind spots.

mod common;

use common::replay::*;
use conduit_lib::plus::skills::audit::audit_skills;
use conduit_lib::plus::skills::collisions::detect_collisions;
use conduit_lib::plus::skills::lint::lint_skills;
use conduit_lib::plus::skills::transpiler::Transpiler;
use conduit_lib::plus::skills::transpilers::register_all_with_home;
use conduit_lib::plus::skills::{discover_skills, FixedClock, TranspilerRegistry};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const CASES: &[&str] = &["clean", "lint-audit-collisions-project"];

/// Python `json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"`; serde_json's
/// default map is a BTreeMap, so keys come out sorted.
fn dump(rel: &Path, home: &Path, value: &Value) {
    let path = home.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value).unwrap())).unwrap();
}

fn findings_entry(home: &Path, args: &Value, _clock: &FixedClock) -> Extras {
    let repo = home.join(args["repo"].as_str().unwrap());
    let out = Path::new(args["out"].as_str().unwrap_or("findings"));
    let skills = discover_skills(&repo);

    let lint = lint_skills(&skills);
    dump(
        &out.join("lint.json"),
        home,
        &json!({
            "has_errors": lint.has_errors(),
            "messages": lint.messages.iter().map(|m| json!({
                "level": m.level, "skill": m.name, "message": m.message,
            })).collect::<Vec<_>>(),
        }),
    );

    let audit = audit_skills(&skills);
    dump(
        &out.join("audit.json"),
        home,
        &json!({
            "has_high_severity": audit.has_high_severity(),
            "findings": audit.findings.iter().map(|f| json!({
                "severity": f.severity, "skill": f.skill_name, "message": f.message, "line": f.line,
            })).collect::<Vec<_>>(),
        }),
    );

    let mut registry = TranspilerRegistry::new();
    register_all_with_home(&mut registry, Some(home.to_path_buf()));
    let wanted: Vec<&str> = args["clients"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let transpilers: Vec<&dyn Transpiler> = registry
        .all()
        .filter(|t| !t.capabilities().append_mode)
        .filter(|t| wanted.is_empty() || wanted.contains(&t.client_key()))
        .collect();
    let collisions = detect_collisions(&skills, &transpilers, &repo);
    dump(
        &out.join("collisions.json"),
        home,
        &Value::Array(
            collisions
                .iter()
                .map(|c| {
                    json!({
                        "skill": c.skill_name,
                        "client": c.client_key,
                        "synced_path": c.synced_path.to_string_lossy(),
                        "collision_path": c.collision_path.to_string_lossy(),
                        "synced_content_len": c.synced_content.chars().count(),
                    })
                })
                .collect(),
        ),
    );
    Vec::new()
}

#[test]
fn every_declared_case_has_vendored_inputs_and_a_golden() {
    assert_vendored("findings", CASES);
}

#[test]
fn findings_match_golden() {
    for case in CASES {
        replay("findings", case, &findings_entry);
    }
}
