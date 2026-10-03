//! Seeded randomized and edge-case tests for the settings merge (`deep_merge_union`,
//! `ensure_policy`, `looks_secret_bearing`) and the context config file.

use super::compact::{parse_window, CheckpointSpec};
use super::config::{
    load_config, save_config, ContextConfig, DedupePolicy, ProfileSpec, SettingsPolicy,
};
use super::settings::{deep_merge_union, ensure_policy, looks_secret_bearing, PolicyOutcome};
use super::Roots;
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use crate::plus::skills::json::{parse, J};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

fn scalar(rng: &mut Rng) -> J {
    match rng.below(6) {
        0 => J::Null,
        1 => J::Bool(rng.chance(50)),
        2 => J::int(rng.below(40) as i64 - 20),
        3 => J::Num("1.5e3".into()),
        4 => J::str(*rng.pick(&["a", "b", "é", "", "x y", "\u{1f600}"])),
        _ => J::str(rng.garbage(6)),
    }
}

fn gen_j(rng: &mut Rng, depth: usize) -> J {
    match rng.below(if depth == 0 { 4 } else { 8 }) {
        0..=2 => scalar(rng),
        3 if depth == 0 => scalar(rng),
        3 | 4 => {
            let mut items = Vec::new();
            for _ in 0..rng.range(0, 4) {
                items.push(gen_j(rng, depth - 1));
            }
            J::Arr(items)
        }
        _ => gen_obj(rng, depth - 1),
    }
}

fn gen_obj(rng: &mut Rng, depth: usize) -> J {
    let mut items: Vec<(String, J)> = Vec::new();
    for _ in 0..rng.range(0, 5) {
        let key = rng.pick(&["a", "b", "allow", "ask"]).to_string();
        if items.iter().any(|(k, _)| *k == key) {
            continue;
        }
        let value = if key.starts_with('a') && rng.chance(60) {
            let mut list = Vec::new();
            for _ in 0..rng.range(0, 4) {
                list.push(J::str(*rng.pick(&["x", "y", "z"])));
            }
            J::Arr(list)
        } else {
            gen_j(rng, depth)
        };
        items.push((key, value));
    }
    J::Obj(items)
}

fn keys(j: &J) -> Vec<String> {
    match j {
        J::Obj(items) => items.iter().map(|(k, _)| k.clone()).collect(),
        _ => Vec::new(),
    }
}

#[test]
fn merge_is_idempotent_keeps_base_first_and_round_trips_through_json() {
    let (mut arrays, mut nested) = (0, 0);
    run_cases("settings-merge", 3000, |_, rng| {
        let base = gen_obj(rng, 3);
        let over = gen_obj(rng, 3);
        let merged = deep_merge_union(&base, &over);
        assert_eq!(deep_merge_union(&base, &base), base, "self merge");
        assert_eq!(deep_merge_union(&merged, &over), merged, "idempotent");
        assert_eq!(deep_merge_union(&base, &over), merged, "deterministic");
        assert_eq!(parse(&merged.dumps()).unwrap(), merged, "json round trip");

        let mut want_keys = keys(&base);
        for k in keys(&over) {
            if !want_keys.contains(&k) {
                want_keys.push(k);
            }
        }
        assert_eq!(keys(&merged), want_keys);

        let (J::Obj(base_items), J::Obj(over_items)) = (&base, &over) else {
            unreachable!()
        };
        for (key, over_value) in over_items {
            let got = merged.get(key).unwrap();
            let base_value = base_items.iter().find(|(k, _)| k == key).map(|(_, v)| v);
            match (base_value, over_value) {
                (Some(J::Arr(have)), J::Arr(add)) => {
                    arrays += 1;
                    let J::Arr(out) = got else {
                        panic!("array expected for {key}")
                    };
                    assert_eq!(&out[..have.len()], have.as_slice(), "base first");
                    let mut fresh: Vec<&J> = Vec::new();
                    for item in add {
                        if !have.contains(item) && !fresh.contains(&item) {
                            fresh.push(item);
                        }
                    }
                    assert_eq!(out.len(), have.len() + fresh.len());
                    assert!(add.iter().all(|i| out.contains(i)));
                }
                (Some(J::Obj(_)), J::Obj(_)) => {
                    nested += 1;
                    assert_eq!(got, &deep_merge_union(base_value.unwrap(), over_value));
                }
                (Some(_), _) | (None, _) => assert_eq!(got, over_value, "over wins for {key}"),
            }
        }
        for (key, base_value) in base_items {
            if !over_items.iter().any(|(k, _)| k == key) {
                assert_eq!(merged.get(key).unwrap(), base_value, "untouched {key}");
            }
        }
    });
    assert!(arrays > 300 && nested > 100, "{arrays} {nested}");
}

#[test]
fn merging_non_objects_returns_the_overlay() {
    run_cases("settings-merge-scalar", 800, |_, rng| {
        let a = gen_j(rng, 2);
        let b = gen_j(rng, 2);
        if !matches!((&a, &b), (J::Obj(_), J::Obj(_))) {
            assert_eq!(deep_merge_union(&a, &b), b);
        }
    });
    let one = J::Arr(vec![J::int(1), J::int(1), J::int(2)]);
    let two = J::Arr(vec![J::int(2), J::int(3)]);
    assert_eq!(deep_merge_union(&one, &two), two);
    let wrap = |v: &J| J::Obj(vec![("allow".into(), v.clone())]);
    assert_eq!(
        deep_merge_union(&wrap(&one), &wrap(&two)),
        wrap(&J::Arr(vec![J::int(1), J::int(1), J::int(2), J::int(3)]))
    );
    assert_eq!(
        deep_merge_union(&J::Obj(vec![]), &J::Obj(vec![])),
        J::Obj(vec![])
    );
}

#[test]
fn merge_with_duplicate_keys_in_the_overlay_is_total() {
    let dup = J::Obj(vec![
        ("a".into(), J::int(1)),
        ("a".into(), J::int(2)),
        ("b".into(), J::Arr(vec![J::int(1)])),
        ("b".into(), J::Arr(vec![J::int(2)])),
    ]);
    let merged = deep_merge_union(&J::Obj(vec![]), &dup);
    assert_eq!(merged.get("a"), Some(&J::int(2)));
    assert_eq!(merged.get("b"), Some(&J::Arr(vec![J::int(1), J::int(2)])));
    assert_eq!(keys(&merged), vec!["a", "b"]);
}

#[test]
fn secret_guard_matches_a_regex_model_and_never_panics() {
    let model = Regex::new("(?i)(secret|token|password|api_?key|bearer)").unwrap();
    run_cases("settings-secret-guard", 4000, |_, rng| {
        let text = rng.tokens(
            &[
                "Bash(", ")", "=", "TOKEN", "token", "Secret", "api_key", "APIKEY", "apikey",
                "pass", "word", "password", " ", "export ", "X=1", "bearer", "mcp__x", ":*",
            ],
            7,
        );
        let want = text.contains('=') && model.is_match(&text);
        assert_eq!(looks_secret_bearing(&text), want, "{text:?}");
        let _ = looks_secret_bearing(&rng.garbage(40));
    });
    assert!(looks_secret_bearing("Bash(export API_TOKEN=abc)"));
    assert!(!looks_secret_bearing("Bash(echo token)"));
    assert!(!looks_secret_bearing("Bash(FOO=1 ls)"));
    assert!(!looks_secret_bearing(""));
}

fn json_value(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(if depth == 0 { 5 } else { 7 }) {
        0 => Value::Null,
        1 => Value::Bool(rng.chance(50)),
        2 => json!(rng.below(1000) as i64 - 500),
        3 => Value::String(rng.garbage(8)),
        4 => Value::String(rng.pick(&["a", "b", "Bash(ls:*)", "mcp__x"]).to_string()),
        5 => {
            let mut items = Vec::new();
            for _ in 0..rng.range(0, 3) {
                items.push(json_value(rng, depth - 1));
            }
            Value::Array(items)
        }
        _ => {
            let mut map = Map::new();
            for _ in 0..rng.range(0, 3) {
                let key = rng.pick(&["a", "b", "c"]).to_string();
                let value = json_value(rng, depth - 1);
                map.insert(key, value);
            }
            Value::Object(map)
        }
    }
}

fn entry_list(rng: &mut Rng) -> Vec<String> {
    let mut out = Vec::new();
    for _ in 0..rng.range(0, 4) {
        out.push(match rng.below(4) {
            0 => rng
                .pick(&["Bash(git status:*)", "mcp__a__b", "Read(~/x)"])
                .to_string(),
            1 => rng.garbage(12),
            2 => "dup".to_string(),
            _ => rng.pick(&["Bash(ls)", "WebFetch"]).to_string(),
        });
    }
    out
}

fn settings_value(rng: &mut Rng) -> Value {
    let mut root = Map::new();
    for key in ["theme", "env", "hooks", "model"] {
        if rng.chance(40) {
            let value = json_value(rng, 2);
            root.insert(key.to_string(), value);
        }
    }
    if rng.chance(85) {
        let mut perms = Map::new();
        for key in ["allow", "ask", "deny", "defaultMode"] {
            if !rng.chance(55) {
                continue;
            }
            let value = if (key == "allow" || key == "ask") && rng.chance(85) {
                let mut items: Vec<Value> =
                    entry_list(rng).into_iter().map(Value::String).collect();
                if rng.chance(15) {
                    items.push(json!(7));
                }
                Value::Array(items)
            } else {
                json_value(rng, 1)
            };
            perms.insert(key.to_string(), value);
        }
        let value = if rng.chance(92) {
            Value::Object(perms)
        } else {
            json_value(rng, 1)
        };
        root.insert("permissions".into(), value);
    }
    Value::Object(root)
}

fn settings_text(rng: &mut Rng) -> Option<String> {
    match rng.below(20) {
        0 => None,
        1 => Some(String::new()),
        2 => Some(
            rng.pick(&["[]", "null", "5", "\"x\"", "true", "[{\"permissions\":{}}]"])
                .to_string(),
        ),
        3 => Some(rng.garbage(60)),
        4 => {
            let text = serde_json::to_string_pretty(&settings_value(rng)).unwrap();
            let cut = text
                .char_indices()
                .nth(rng.below(text.len().max(1)))
                .map_or(0, |(i, _)| i);
            Some(text[..cut].to_string())
        }
        _ => Some(serde_json::to_string_pretty(&settings_value(rng)).unwrap()),
    }
}

enum Want {
    Unchanged,
    Changed(Value),
    Skipped(String),
}

fn model(text: Option<&str>, policy: &SettingsPolicy) -> Want {
    if policy.ensure_allow.is_empty() && policy.ensure_ask.is_empty() {
        return Want::Unchanged;
    }
    let mut value = match text {
        Some(t) => match serde_json::from_str::<Value>(t) {
            Ok(v) => v,
            Err(_) => return Want::Unchanged,
        },
        None => json!({}),
    };
    let Value::Object(root) = &mut value else {
        return Want::Skipped("settings.json is not a JSON object".into());
    };
    match root.get("permissions") {
        None => {
            root.insert("permissions".into(), json!({}));
        }
        Some(Value::Object(_)) => {}
        Some(_) => return Want::Skipped("permissions is not an object".into()),
    }
    let perms = root
        .get_mut("permissions")
        .unwrap()
        .as_object_mut()
        .unwrap();
    let wanted = [("allow", &policy.ensure_allow), ("ask", &policy.ensure_ask)];
    for (key, entries) in wanted {
        if !entries.is_empty() && perms.get(key).is_some_and(|v| !v.is_array()) {
            return Want::Skipped(format!("permissions.{key} is not a list; left alone"));
        }
    }
    let mut changed = false;
    for (key, entries) in wanted {
        if entries.is_empty() {
            continue;
        }
        let list = perms
            .entry(key.to_string())
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap();
        for entry in entries {
            let item = Value::String(entry.clone());
            if !list.contains(&item) {
                list.push(item);
                changed = true;
            }
        }
    }
    if changed {
        Want::Changed(value)
    } else {
        Want::Unchanged
    }
}

fn top_keys(text: &str) -> Vec<String> {
    match parse(text) {
        Ok(j) => keys(&j),
        Err(_) => Vec::new(),
    }
}

#[test]
fn ensure_policy_matches_the_model_is_additive_and_idempotent() {
    let home = ScratchDir::new("settings-policy");
    let (mut changed, mut skipped, mut unchanged) = (0, 0, 0);
    run_cases("settings-policy", 700, |_, rng| {
        home.reset();
        let roots = Roots::from_home(home.path());
        let nested = rng.chance(30);
        let settings = if nested {
            home.path().join("new/dir/settings.json")
        } else {
            roots.claude_home.join("settings.json")
        };
        let text = settings_text(rng);
        if let Some(text) = &text {
            fs::create_dir_all(settings.parent().unwrap()).unwrap();
            fs::write(&settings, text).unwrap();
        }
        let policy = SettingsPolicy {
            ensure_allow: if rng.chance(70) {
                entry_list(rng)
            } else {
                Vec::new()
            },
            ensure_ask: if rng.chance(40) {
                entry_list(rng)
            } else {
                Vec::new()
            },
        };
        let backup = rng.chance(60);
        let start = tree(home.path());
        let want = model(text.as_deref(), &policy);

        let dry = ensure_policy(&roots, &settings, &policy, backup, true).unwrap();
        assert_eq!(tree(home.path()), start, "dry run wrote something");

        let got = ensure_policy(&roots, &settings, &policy, backup, false).unwrap();
        assert_eq!(got, dry, "dry run and real run agree");
        let end = tree(home.path());
        match want {
            Want::Unchanged => {
                unchanged += 1;
                assert_eq!(got, PolicyOutcome::Unchanged, "{text:?} {policy:?}");
                assert_eq!(end, start);
            }
            Want::Skipped(reason) => {
                skipped += 1;
                assert_eq!(got, PolicyOutcome::Skipped(reason), "{text:?}");
                assert_eq!(end, start);
            }
            Want::Changed(expected) => {
                changed += 1;
                assert_eq!(got, PolicyOutcome::Changed, "{text:?} {policy:?}");
                let after = fs::read_to_string(&settings).unwrap();
                assert!(after.ends_with('\n'));
                assert_eq!(serde_json::from_str::<Value>(&after).unwrap(), expected);
                let mut want_keys = text.as_deref().map(top_keys).unwrap_or_default();
                if !want_keys.iter().any(|k| k == "permissions") {
                    want_keys.push("permissions".into());
                }
                assert_eq!(top_keys(&after), want_keys, "top-level order");
                let backups = tree(&roots.backups_dir());
                let saved: Vec<&Vec<u8>> = backups.values().flatten().collect();
                if backup && text.is_some() {
                    assert_eq!(saved, vec![&text.clone().unwrap().into_bytes()]);
                } else {
                    assert!(saved.is_empty(), "unexpected backup");
                }
                for (path, content) in &end {
                    let rel = settings
                        .strip_prefix(home.path())
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    let allowed = path == &rel
                        || path.starts_with(".cache/mcpm/context/backups")
                        || (path.ends_with('/') && rel.starts_with(path.as_str()))
                        || (path.ends_with('/')
                            && ".cache/mcpm/context/backups/".starts_with(path.as_str()))
                        || start.get(path) == Some(content);
                    assert!(allowed, "unexpected path {path:?}");
                }
                let again = ensure_policy(&roots, &settings, &policy, false, false).unwrap();
                assert_eq!(again, PolicyOutcome::Unchanged, "idempotent");
                assert_eq!(fs::read_to_string(&settings).unwrap(), after);
            }
        }
    });
    assert!(
        changed > 150 && skipped > 20 && unchanged > 100,
        "{changed} {skipped} {unchanged}"
    );
}

type Tree = std::collections::BTreeMap<String, Option<Vec<u8>>>;

fn tree(root: &Path) -> Tree {
    fn walk(base: &Path, dir: &Path, out: &mut Tree) {
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let rel = path
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                out.insert(format!("{rel}/"), None);
                walk(base, &path, out);
            } else if kind.is_file() {
                out.insert(rel, Some(fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Tree::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn ensure_policy_adds_each_entry_once_in_first_seen_order() {
    let home = ScratchDir::new("settings-policy-order");
    let roots = Roots::from_home(home.path());
    let settings = home.path().join("settings.json");
    fs::write(
        &settings,
        r#"{"permissions": {"allow": ["b"], "deny": ["z"]}, "theme": "dark"}"#,
    )
    .unwrap();
    let policy = SettingsPolicy {
        ensure_allow: vec!["a".into(), "b".into(), "a".into(), "c".into()],
        ensure_ask: vec!["q".into(), "q".into()],
    };
    assert_eq!(
        ensure_policy(&roots, &settings, &policy, false, false).unwrap(),
        PolicyOutcome::Changed
    );
    let after: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(after["permissions"]["allow"], json!(["b", "a", "c"]));
    assert_eq!(after["permissions"]["ask"], json!(["q"]));
    assert_eq!(after["permissions"]["deny"], json!(["z"]));
    assert_eq!(after["theme"], "dark");
}

#[test]
fn ensure_policy_io_edges() {
    let home = ScratchDir::new("settings-policy-io");
    let roots = Roots::from_home(home.path());
    let policy = SettingsPolicy {
        ensure_allow: vec!["a".into()],
        ensure_ask: vec![],
    };
    let empty = SettingsPolicy::default();

    let dir = home.path().join("is-a-dir.json");
    fs::create_dir_all(&dir).unwrap();
    assert!(ensure_policy(&roots, &dir, &policy, true, false).is_err());
    assert_eq!(
        ensure_policy(&roots, &dir, &empty, true, false).unwrap(),
        PolicyOutcome::Unchanged
    );

    let binary = home.path().join("binary.json");
    fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
    assert!(ensure_policy(&roots, &binary, &policy, true, false).is_err());
    assert_eq!(fs::read(&binary).unwrap(), [0xff, 0xfe, 0x00]);

    let blocked = home.path().join("file-as-parent");
    fs::write(&blocked, "x").unwrap();
    assert!(ensure_policy(
        &roots,
        &blocked.join("settings.json"),
        &policy,
        false,
        false
    )
    .is_err());

    let missing = home.path().join("fresh/settings.json");
    assert_eq!(
        ensure_policy(&roots, &missing, &policy, true, false).unwrap(),
        PolicyOutcome::Changed
    );
    assert_eq!(
        fs::read_to_string(&missing).unwrap(),
        "{\n  \"permissions\": {\n    \"allow\": [\n      \"a\"\n    ]\n  }\n}\n"
    );
    assert!(
        !roots.backups_dir().exists(),
        "nothing to back up for a new file"
    );
}

#[test]
fn ensure_policy_collapses_duplicate_keys_like_python_does() {
    let home = ScratchDir::new("settings-policy-dup");
    let roots = Roots::from_home(home.path());
    let settings = home.path().join("settings.json");
    fs::write(
        &settings,
        r#"{"permissions": {"allow": ["x"]}, "theme": 1, "permissions": {"allow": ["y"]}}"#,
    )
    .unwrap();
    let policy = SettingsPolicy {
        ensure_allow: vec!["x".into(), "n".into()],
        ensure_ask: vec![],
    };
    assert_eq!(
        ensure_policy(&roots, &settings, &policy, false, false).unwrap(),
        PolicyOutcome::Changed
    );
    let after = fs::read_to_string(&settings).unwrap();
    assert_eq!(top_keys(&after), vec!["permissions", "theme"]);
    let value: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(value["permissions"]["allow"], json!(["y", "x", "n"]));
}

fn valid_profile_name_model(name: &str) -> bool {
    Regex::new("^[a-z0-9][a-z0-9-]{0,63}$")
        .unwrap()
        .is_match(name)
}

#[test]
fn profile_names_follow_the_shell_function_rule() {
    run_cases("config-profile-names", 3000, |_, rng| {
        let name = match rng.below(4) {
            0 => rng.garbage(10),
            1 => rng.string("abz09-_ A", 70),
            2 => format!("a{}", "b".repeat(rng.range(60, 68))),
            _ => rng.tokens(&["a", "-", "0", "x", "é", "_", " ", "Z"], 6),
        };
        let mut config = ContextConfig::default();
        config.profiles.insert(name.clone(), ProfileSpec::default());
        assert_eq!(
            config.validate().is_ok(),
            valid_profile_name_model(&name),
            "{name:?}"
        );
    });
    for ok in ["a", "0", "a-b", &"a".repeat(64)] {
        assert!(valid_profile_name_model(ok), "{ok}");
    }
    for bad in ["", "-a", "A", "a_b", &"a".repeat(65)] {
        assert!(!valid_profile_name_model(bad), "{bad}");
    }
}

#[test]
fn compact_window_validation_follows_the_range_model() {
    run_cases("config-windows", 3000, |_, rng| {
        let value = match rng.below(9) {
            0 => json!("auto"),
            1 => json!("AUTO"),
            2 => json!(*rng.pick(&[99_999u64, 100_000, 1_000_000, 1_000_001, 0, 150_000])),
            3 => json!(rng.next_u64()),
            4 => json!(-(rng.below(500_000) as i64)),
            5 => json!(150_000.5),
            6 => json!(150000.0),
            7 => json!(rng.garbage(6)),
            _ => json_value(rng, 1),
        };
        let ok = match &value {
            Value::String(s) => s == "auto",
            Value::Number(n) => n
                .as_u64()
                .is_some_and(|t| (100_000..=1_000_000).contains(&t)),
            _ => false,
        };
        assert_eq!(parse_window(&value, "w").is_ok(), ok, "{value}");
    });
}

fn random_profile(rng: &mut Rng) -> ProfileSpec {
    let mut spec = ProfileSpec {
        org: rng.chance(50),
        org_mode: rng.pick(&["import", "inherit", "none"]).to_string(),
        commands: rng.chance(50),
        skills: rng.chance(50),
        agents: rng.chance(50),
        copy_auth: rng.chance(50),
        link_rules: rng.chance(50),
        autocompact_flag: rng.chance(30),
        auto_compact_enabled: rng.chance(30).then(|| rng.chance(50)),
        ..ProfileSpec::default()
    };
    spec.rules = match rng.below(3) {
        0 => json!("inherit"),
        1 => json!([]),
        _ => json!(["a", "b"]),
    };
    spec.servers = if rng.chance(50) {
        json!("inherit")
    } else {
        json!(["s1"])
    };
    for _ in 0..rng.range(0, 3) {
        let key = rng.pick(&["env", "model", "theme"]).to_string();
        let value = json_value(rng, 1);
        if !value.is_null() {
            spec.settings_overrides.insert(key, value);
        }
    }
    if rng.chance(40) {
        spec.auto_compact_window = Some(if rng.chance(50) {
            json!("auto")
        } else {
            json!(200_000)
        });
    }
    if rng.chance(30) {
        spec.model_windows.insert("model-x".into(), json!(300_000));
    }
    if rng.chance(30) {
        spec.compact_instructions = Some(format!("keep {}", rng.garbage(10)));
    }
    if rng.chance(30) {
        spec.checkpoint = Some(CheckpointSpec {
            command: rng.chance(50).then(|| "run".to_string()),
            checkpoint_at: rng.chance(50).then(|| rng.range(1, 900_000) as u64),
        });
    }
    spec
}

fn random_config(rng: &mut Rng) -> ContextConfig {
    let mut config = ContextConfig::default();
    for _ in 0..rng.range(0, 3) {
        config.profiles.insert(rng.slug(8), random_profile(rng));
    }
    config.profiles.remove("");
    config.wrap_default_claude = rng.chance(50);
    config.dedupe = DedupePolicy {
        enabled: rng.chance(50),
        legacy_names: entry_list(rng),
        require_mcpm_twin: rng.chance(50),
    };
    config.settings = SettingsPolicy {
        ensure_allow: entry_list(rng),
        ensure_ask: entry_list(rng),
    };
    config.clients_root = rng.garbage(20);
    config.corp_tools_dir = rng.chance(40).then(|| rng.garbage(10));
    config.cf_wrapper_hash = rng.chance(40).then(|| rng.garbage(10));
    config
}

#[test]
fn config_files_round_trip_and_serialise_deterministically() {
    let home = ScratchDir::new("config-roundtrip");
    run_cases("config-roundtrip", 400, |_, rng| {
        let config = random_config(rng);
        config.validate().unwrap();
        let path = home.path().join("nested/context.json");
        save_config(&path, &config).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        assert!(first.ends_with('\n'));
        assert_eq!(first, config.to_json_text());
        assert_eq!(load_config(&path), config, "{first}");
        save_config(&path, &load_config(&path)).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            first,
            "stable on re-save"
        );
        assert_eq!(parse(&first).unwrap().dumps() + "\n", first);
    });
}

fn config_json(rng: &mut Rng, depth: usize) -> Value {
    if depth == 0 || rng.chance(30) {
        return json_value(rng, 0);
    }
    let mut map = Map::new();
    for _ in 0..rng.range(0, 6) {
        let key = rng
            .pick(&[
                "profiles",
                "wrap_default_claude",
                "dedupe",
                "settings",
                "clients_root",
                "corp_tools_dir",
                "cf_wrapper_hash",
                "org",
                "org_mode",
                "rules",
                "servers",
                "auto_compact_window",
                "model_windows",
                "checkpoint",
                "ensure_allow",
                "legacy_names",
                "enabled",
                "x",
            ])
            .to_string();
        let value = config_json(rng, depth - 1);
        map.insert(key, value);
    }
    Value::Object(map)
}

#[test]
fn loading_arbitrary_config_text_never_panics_and_falls_back_to_defaults() {
    let home = ScratchDir::new("config-garbage");
    let path = home.path().join("context.json");
    run_cases("config-garbage", 1500, |_, rng| {
        let value = config_json(rng, 4);
        let _ = ContextConfig::from_value(value.clone());
        let text = match rng.below(4) {
            0 => rng.garbage(60),
            1 => serde_json::to_string(&value).unwrap(),
            2 => {
                let text = serde_json::to_string(&value).unwrap();
                text.chars()
                    .take(rng.below(text.chars().count() + 1))
                    .collect()
            }
            _ => String::new(),
        };
        fs::write(&path, &text).unwrap();
        let loaded = load_config(&path);
        let parsed = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| ContextConfig::from_value(v).ok());
        assert_eq!(loaded, parsed.unwrap_or_default());
    });
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert_eq!(load_config(&path), ContextConfig::default());
    assert_eq!(
        load_config(&home.path().join("missing")),
        ContextConfig::default()
    );
}

#[test]
fn corp_tools_and_clients_root_resolution_prefers_env_then_config() {
    let home = Path::new("/home/synthetic");
    run_cases("config-resolution", 800, |_, rng| {
        let mut roots = Roots::from_home(home);
        let mut config = ContextConfig::default();
        let pick = |rng: &mut Rng| -> Option<String> {
            match rng.below(4) {
                0 => None,
                1 => Some(String::new()),
                2 => Some("~/x".into()),
                _ => Some("/abs/y".into()),
            }
        };
        roots.env_corp_tools_dir = pick(rng);
        roots.env_clients_root = pick(rng);
        config.corp_tools_dir = pick(rng);
        config.clients_root = pick(rng).unwrap_or_default();
        let corp = roots.resolve_corp_tools_dir(&config);
        let nonempty = |v: &Option<String>| v.clone().filter(|s| !s.is_empty());
        let want = nonempty(&roots.env_corp_tools_dir)
            .or_else(|| nonempty(&config.corp_tools_dir))
            .map(|raw| roots.expand_user(&raw))
            .unwrap_or_else(|| roots.cf_dir.clone());
        assert_eq!(corp, want);
        assert_eq!(roots.resolved(&config).cf_dir, want);
        let clients = roots.resolve_clients_root(&config);
        let want = match nonempty(&roots.env_clients_root) {
            Some(raw) => roots.expand_user(&raw),
            None => roots.expand_user(&config.clients_root),
        };
        assert_eq!(clients, want);
    });
}
