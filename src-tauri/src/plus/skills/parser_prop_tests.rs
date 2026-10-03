//! Seeded randomized and edge-case tests for the SKILL.md parser, the frontmatter schema
//! coercions and the order-preserving JSON value shared by the transpilers.

use super::json::{self, J};
use super::parser::{
    build_frontmatter, discover_skills_report, find_skill, parse_frontmatter, parse_skill_file,
    split_frontmatter, valid_name, Activation,
};
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use serde_yaml::{Mapping, Value};
use std::fs;

const NAME_CHARS: &str = "abcdefghijklmnopqrstuvwxyz0123456789-";

fn text_without_dashes(rng: &mut Rng, max: usize) -> String {
    let mut text = rng.garbage(max).replace("---", "-_-");
    while text.contains("---") {
        text = text.replace("---", "-_-");
    }
    text
}

fn yaml_value(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(if depth >= 3 { 6 } else { 8 }) {
        0 => Value::Null,
        1 => Value::Bool(rng.chance(50)),
        2 => Value::from(rng.next_u64() as i64 >> rng.below(60)),
        3 => Value::from(rng.below(2000) as f64 / 8.0),
        4 | 5 => Value::String(match rng.below(6) {
            0 => rng
                .pick(&["always", "auto", "agent", "manual", "5", "true", ""])
                .to_string(),
            1 => rng.slug(40),
            _ => rng.garbage(24),
        }),
        6 => Value::Sequence(
            (0..rng.below(4))
                .map(|_| yaml_value(rng, depth + 1))
                .collect(),
        ),
        _ => {
            let mut map = Mapping::new();
            for _ in 0..rng.below(4) {
                let key = match rng.below(6) {
                    0 => Value::from(rng.below(9) as i64),
                    1 => Value::Null,
                    _ => Value::String(
                        rng.pick(&[
                            "command", "matcher", "type", "servers", "skills", "version", "x",
                        ])
                        .to_string(),
                    ),
                };
                map.insert(key, yaml_value(rng, depth + 1));
            }
            Value::Mapping(map)
        }
    }
}

fn hook_map(rng: &mut Rng, chaos: u64) -> Value {
    let mut hook = Mapping::new();
    if rng.chance(100 - chaos) {
        let command = match if chaos > 10 { rng.below(5) } else { 4 } {
            0 => "/abs/run.sh".to_string(),
            1 => "~/run.sh".to_string(),
            2 => "  ".to_string(),
            _ => format!("scripts/{}.sh", rng.slug(40)),
        };
        hook.insert("command".into(), Value::String(command));
    }
    if rng.chance(40) {
        let matcher = if rng.chance(100 - chaos) {
            Value::String(rng.garbage(6))
        } else {
            yaml_value(rng, 2)
        };
        hook.insert("matcher".into(), matcher);
    }
    if rng.chance(30) {
        let kind = if rng.chance(100 - chaos) {
            Value::String("command".into())
        } else {
            yaml_value(rng, 2)
        };
        hook.insert("type".into(), kind);
    }
    Value::Mapping(hook)
}

fn frontmatter_fields(rng: &mut Rng, chaos: u64) -> Vec<(String, Value)> {
    let mut fields = Vec::new();
    let mut put = |key: &str, rng: &mut Rng, valid: Value| {
        if rng.chance(90) {
            let value = if rng.chance(100 - chaos) {
                valid
            } else {
                yaml_value(rng, 0)
            };
            fields.push((key.to_string(), value));
        }
    };
    let name = Value::String(rng.slug(40));
    put("name", rng, name);
    let long = if chaos > 10 {
        *rng.pick(&[0usize, 1, 1023, 1024, 1025])
    } else {
        *rng.pick(&[0usize, 1, 1023, 1024])
    };
    let description = if long == 0 {
        format!("{}d", rng.garbage(30))
    } else {
        "d".repeat(long)
    };
    put("description", rng, Value::String(description));
    let license = Value::String(rng.garbage(8));
    put("license", rng, license);
    let compat = "c".repeat(*rng.pick(&[0usize, 499, 500, if chaos > 10 { 501 } else { 7 }]));
    put("compatibility", rng, Value::String(compat));
    let tools = Value::String(rng.garbage(12));
    put("allowed_tools", rng, tools);
    let mut metadata = Mapping::new();
    metadata.insert("version".into(), yaml_value(rng, 2));
    put("metadata", rng, Value::Mapping(metadata));
    let mut hooks = Mapping::new();
    for event in ["PreToolUse", "Stop"] {
        if rng.chance(50) {
            hooks.insert(event.into(), hook_map(rng, chaos));
        }
    }
    put("hooks", rng, Value::Mapping(hooks));
    let globs = Value::String(rng.garbage(12));
    put("globs", rng, globs);
    let activation = if chaos > 10 {
        *rng.pick(&["always", "auto", "agent", "manual", "sometimes"])
    } else {
        *rng.pick(&["always", "auto", "agent", "manual"])
    };
    put("activation", rng, Value::String(activation.into()));
    let priority = Value::from(rng.below(100) as i64 - 20);
    put("priority", rng, priority);
    let mut deps = Mapping::new();
    deps.insert(
        "servers".into(),
        Value::Sequence(vec![Value::String("srv".into())]),
    );
    put("dependencies", rng, Value::Mapping(deps));
    fields
}

fn line_model(doc: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = doc.split('\n').collect();
    let bare = |l: &str| l.starts_with("---") && l.trim() == "---";
    if lines.len() < 2 || !bare(lines[0]) {
        return None;
    }
    let close = (1..lines.len()).find(|&i| bare(lines[i]))?;
    let yaml: String = lines[1..close].iter().map(|l| format!("{l}\n")).collect();
    Some((yaml, lines[close + 1..].join("\n").trim().to_string()))
}

#[test]
fn splitting_garbage_documents_never_panics_and_matches_a_line_model() {
    const VOCAB: &[&str] = &[
        "---",
        "---\n",
        "--- \n",
        "---\r\n",
        "--- x\n",
        "----\n",
        "  ---\n",
        "a---b\n",
        "\n",
        "\r\n",
        "name: a\n",
        "description: d\n",
        ": ",
        "- ",
        "\u{feff}",
        " ",
        "\t",
        "#",
        "\"",
        "{",
        "}",
        "[",
        "]",
        "|",
        ">",
        "\0",
        "é",
        "日本",
    ];
    run_cases("skills-split-garbage", 4000, |_, rng| {
        let mut doc = String::new();
        if rng.chance(70) {
            doc.push_str(rng.pick(&["---\n", "---\r\n", "--- \n", "---"]));
        }
        for _ in 0..rng.below(8) {
            doc.push_str(&rng.tokens(VOCAB, 3));
            doc.push_str(&rng.garbage(10));
        }
        let split = split_frontmatter(&doc);
        assert_eq!(
            split.map(|(y, b)| (y.to_string(), b.to_string())),
            line_model(&doc),
            "{doc:?}"
        );
        if let Some((yaml, body)) = split {
            assert!(doc.starts_with("---"), "{doc:?}");
            assert!(
                yaml.split('\n')
                    .all(|l| !(l.starts_with("---") && l.trim() == "---")),
                "a fence line must close the frontmatter: {doc:?}"
            );
            assert_eq!(body, body.trim());
            assert!(doc.contains(yaml) && doc.contains(body));
        }
        let first = parse_frontmatter(&doc);
        assert_eq!(first, parse_frontmatter(&doc), "deterministic");
        if split.is_none() {
            assert_eq!(first, Ok((Vec::new(), doc.clone())));
        }
        if let Ok((fields, _)) = &first {
            assert!(fields.iter().all(|(k, _)| !k.contains('-')));
            let mut keys: Vec<&String> = fields.iter().map(|(k, _)| k).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(
                keys.len(),
                fields.len(),
                "keys are unique after normalising"
            );
        }
    });
}

#[test]
fn normalised_keys_keep_first_position_and_last_value() {
    const KEYS: &[&str] = &[
        "name",
        "allowed-tools",
        "allowed_tools",
        "a-b-c",
        "a_b-c",
        "x",
        "y-z",
    ];
    run_cases("skills-key-normalisation", 2000, |_, rng| {
        let mut text = String::from("---\n");
        let mut model: Vec<(String, i64)> = Vec::new();
        let mut seen_raw = std::collections::HashSet::new();
        for _ in 0..rng.range(1, 8) {
            let raw = *rng.pick(KEYS);
            if !seen_raw.insert(raw) {
                continue;
            }
            let value = rng.below(1000) as i64;
            text.push_str(&format!("{raw}: {value}\n"));
            let key = raw.replace('-', "_");
            match model.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => model.push((key, value)),
            }
        }
        text.push_str("---\nbody\n");
        let (fields, body) = parse_frontmatter(&text).unwrap();
        let got: Vec<(String, i64)> = fields
            .into_iter()
            .map(|(k, v)| (k, v.as_i64().unwrap()))
            .collect();
        assert_eq!(got, model);
        assert_eq!(body, "body");
    });
}

#[test]
fn serialised_frontmatter_round_trips_through_the_parser() {
    run_cases("skills-frontmatter-roundtrip", 2000, |_, rng| {
        let name = rng.slug(40);
        let description = {
            let mut text = text_without_dashes(rng, 60);
            text.push('x');
            text
        };
        let globs = text_without_dashes(rng, 20);
        let priority = rng.below(50) as i64 - 10;
        let activation = *rng.pick(&["always", "auto", "agent", "manual"]);
        let mut map = Mapping::new();
        map.insert("name".into(), Value::String(name.clone()));
        map.insert("description".into(), Value::String(description.clone()));
        map.insert("globs".into(), Value::String(globs.clone()));
        map.insert("priority".into(), Value::from(priority));
        map.insert("activation".into(), Value::String(activation.into()));
        let yaml = serde_yaml::to_string(&map).unwrap();
        let body = text_without_dashes(rng, 80);
        let doc = format!("---\n{yaml}---\n{body}");
        let (fields, parsed_body) = parse_frontmatter(&doc).unwrap();
        assert_eq!(parsed_body, body.trim());
        let fm = build_frontmatter(&fields).unwrap();
        assert_eq!(fm.name, name);
        assert_eq!(fm.description, description);
        assert_eq!(fm.globs.as_deref(), Some(globs.as_str()));
        assert_eq!(fm.priority, priority);
        assert_eq!(fm.activation.as_str(), activation);
        assert_eq!(parse_frontmatter(&doc), parse_frontmatter(&doc));
    });
}

fn with_dashes(rng: &mut Rng, max: usize) -> String {
    let mut chars: Vec<char> = rng.garbage(max).chars().collect();
    for _ in 0..rng.range(1, 4) {
        let at = rng.range(0, chars.len());
        for (i, c) in "---".chars().enumerate() {
            chars.insert(at + i, c);
        }
    }
    let mut text: String = chars.into_iter().collect();
    text.push('x');
    text
}

#[test]
fn values_containing_dashes_round_trip_through_the_parser() {
    run_cases("skills-frontmatter-dashes-roundtrip", 3000, |_, rng| {
        let name = rng.slug(40);
        let description = with_dashes(rng, 60);
        let globs = with_dashes(rng, 20);
        let mut map = Mapping::new();
        map.insert("name".into(), Value::String(name.clone()));
        map.insert("description".into(), Value::String(description.clone()));
        map.insert("globs".into(), Value::String(globs.clone()));
        let body = with_dashes(rng, 80);
        let doc = format!("---\n{}---\n{body}", serde_yaml::to_string(&map).unwrap());
        let (fields, parsed_body) = parse_frontmatter(&doc).unwrap();
        assert_eq!(parsed_body, body.trim(), "{doc:?}");
        let fm = build_frontmatter(&fields).unwrap();
        assert_eq!(fm.name, name);
        assert_eq!(fm.description, description, "{doc:?}");
        assert_eq!(fm.globs.as_deref(), Some(globs.as_str()));
    });
}

#[test]
fn plain_scalars_with_dashes_inside_survive_unquoted() {
    run_cases("skills-frontmatter-dashes-plain", 1000, |_, rng| {
        let head = rng.string("abc xyz019", 12);
        let tail = rng.string("abc xyz019", 12);
        let value = format!("w{head}---{tail}w");
        let doc = format!("---\nname: a\ndescription: {value}\n---\nbody");
        let (fields, body) = parse_frontmatter(&doc).unwrap();
        assert_eq!(fields[1].1, Value::String(value), "{doc:?}");
        assert_eq!(body, "body");
    });
}

#[test]
fn building_from_arbitrary_values_never_panics_and_only_returns_valid_models() {
    let mut valid = 0;
    run_cases("skills-build-frontmatter", 8000, |_, rng| {
        let chaos = *rng.pick(&[2, 2, 30]);
        let fields = frontmatter_fields(rng, chaos);
        let first = build_frontmatter(&fields).map(|fm| (fm.name, fm.description, fm.priority));
        let second = build_frontmatter(&fields).map(|fm| (fm.name, fm.description, fm.priority));
        assert_eq!(first, second, "deterministic");
        if let Ok(fm) = build_frontmatter(&fields) {
            valid += 1;
            assert!(valid_name(&fm.name).is_ok());
            assert!((1..=1024).contains(&fm.description.chars().count()));
            assert!(fm.compatibility.is_none_or(|c| c.chars().count() <= 500));
            for (_, hook) in fm.hooks.unwrap_or_default() {
                assert!(!hook.command.trim().is_empty());
                assert!(!hook.command.starts_with('/') && !hook.command.starts_with('~'));
            }
        }
    });
    assert!(valid > 1500, "{valid}");
}

#[test]
fn name_validation_matches_a_reference_model() {
    run_cases("skills-valid-name", 10000, |_, rng| {
        let alphabet = *rng.pick(&["ab-", "ab09-", "abAB09-_ ", "a-é日\n", NAME_CHARS]);
        let name = rng.string(alphabet, 70);
        let chars: Vec<char> = name.chars().collect();
        let expected = (1..=64).contains(&chars.len())
            && chars
                .iter()
                .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--");
        assert_eq!(valid_name(&name).is_ok(), expected, "{name:?}");
    });
}

fn fields_of(doc: &str) -> Vec<(String, Value)> {
    parse_frontmatter(doc).unwrap().0
}

fn priority_of(literal: &str) -> Result<i64, String> {
    let doc = format!("---\nname: a\ndescription: d\npriority: {literal}\n---\n");
    parse_frontmatter(&doc).and_then(|(fields, _)| build_frontmatter(&fields).map(|fm| fm.priority))
}

#[test]
fn name_length_and_shape_edges() {
    for ok in ["a", "0", "a-b", "0-9", &"a".repeat(64)] {
        assert!(valid_name(ok).is_ok(), "{ok:?}");
    }
    let too_long = "a".repeat(65);
    for bad in [
        "", "-a", "a-", "a--b", "A", "a_b", "a b", "abc\n", "\nabc", "é", "日本", "a.b", &too_long,
    ] {
        assert!(valid_name(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn description_and_compatibility_limits_count_characters_not_bytes() {
    let build = |description: &str, compat: &str| {
        let mut map = Mapping::new();
        map.insert("name".into(), "ok".into());
        map.insert("description".into(), Value::String(description.into()));
        map.insert("compatibility".into(), Value::String(compat.into()));
        let doc = format!("---\n{}---\n", serde_yaml::to_string(&map).unwrap());
        build_frontmatter(&fields_of(&doc))
    };
    assert!(build(&"é".repeat(1024), &"日".repeat(500)).is_ok());
    assert!(build(&"é".repeat(1025), "").is_err());
    assert!(build(&"🙂".repeat(1024), "").is_ok());
    assert!(build("", "").is_err());
    assert!(build("d", &"日".repeat(501)).is_err());
}

#[test]
fn frontmatter_shape_edges() {
    assert_eq!(
        parse_frontmatter("---\n---\nbody"),
        Ok((Vec::new(), "body".into()))
    );
    assert_eq!(
        parse_frontmatter("---\n# only a comment\n---\n"),
        Ok((Vec::new(), String::new()))
    );
    assert_eq!(
        parse_frontmatter("---\nname: x"),
        Ok((Vec::new(), "---\nname: x".into()))
    );
    assert_eq!(
        parse_frontmatter("no fence\n---\na: 1\n---\n").unwrap().0,
        Vec::new()
    );
    assert_eq!(parse_frontmatter(""), Ok((Vec::new(), String::new())));
    let (fields, body) = parse_frontmatter("---\r\nname: crlf\r\n---\r\nbody\r\n").unwrap();
    assert_eq!(fields[0].1, Value::String("crlf".into()));
    assert_eq!(body, "body");
    let (fields, body) = parse_frontmatter("---\nname: x\n---").unwrap();
    assert_eq!((fields.len(), body.as_str()), (1, ""));
    assert!(parse_frontmatter("---\n- a\n- b\n---\n").is_err());
    assert!(parse_frontmatter("---\njust text\n---\n").is_err());
    assert!(parse_frontmatter("---\n? [a, b]\n: c\n---\n").is_err());
    assert!(parse_frontmatter("---\nname: [unclosed\n---\n").is_err());
    assert!(parse_frontmatter("---\na: 1\na: 2\n---\n").is_err());
    let (fields, _) = parse_frontmatter("---\n1: x\ntrue: y\n~: z\n---\n").unwrap();
    let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["1", "True", "None"]);
}

#[test]
fn bom_prefixed_document_has_no_frontmatter() {
    let doc = "\u{feff}---\nname: a\ndescription: d\n---\nbody";
    assert_eq!(parse_frontmatter(doc), Ok((Vec::new(), doc.into())));
}

#[test]
fn dashes_inside_a_value_do_not_end_the_frontmatter() {
    let doc = "---\nname: a\ndescription: use --- as a separator\n---\nbody";
    let (fields, body) = parse_frontmatter(doc).unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[1].1, Value::String("use --- as a separator".into()));
    assert_eq!(body, "body");
}

#[test]
fn only_a_bare_dashes_line_closes_the_frontmatter() {
    let body_of = |doc: &str| parse_frontmatter(doc).unwrap().1;
    assert_eq!(body_of("---\nname: a\n--- \t\nbody"), "body");
    assert_eq!(body_of("---\r\nname: a\r\n---\r\nbody"), "body");
    assert_eq!(body_of("---\nname: a\n---"), "");
    assert_eq!(body_of("---\nname: a\n---\n---\nlater"), "---\nlater");
    for open in [
        "---\nname: a\n--- body",
        "---\nname: a\n----\nbody",
        "---\nname: a\n  ---\nbody",
        "---\nname: a\nk: v ---\nbody",
        "---\nname: a\nk: ---\nbody",
    ] {
        assert_eq!(
            parse_frontmatter(open),
            Ok((Vec::new(), open.into())),
            "{open:?}"
        );
    }
    let (fields, body) = parse_frontmatter("---\nname: a\nk: v ---\n---\nbody").unwrap();
    assert_eq!(fields[1].1, Value::String("v ---".into()));
    assert_eq!(body, "body");
}

#[test]
fn priority_coercions() {
    let priority = priority_of;
    assert_eq!(priority("5"), Ok(5));
    assert_eq!(priority("-3"), Ok(-3));
    assert_eq!(priority("\"7\""), Ok(7));
    assert_eq!(priority("\" 7 \""), Ok(7));
    assert_eq!(priority("2.0"), Ok(2));
    assert!(priority("2.5").is_err());
    assert!(priority("true").is_err());
    assert!(priority("[1]").is_err());
    assert!(priority("\"x\"").is_err());
    assert!(priority("99999999999999999999").is_err());
    assert!(priority(".inf").is_err());
    assert!(priority(".nan").is_err());
}

#[test]
fn activation_hook_and_dependency_edges() {
    let build = |extra: &str| {
        let doc = format!("---\nname: a\ndescription: d\n{extra}\n---\n");
        build_frontmatter(&fields_of(&doc))
    };
    assert_eq!(
        build("activation: always").unwrap().activation,
        Activation::Always
    );
    assert!(build("activation: Always").is_err());
    assert!(build("activation: 1").is_err());
    assert!(build("activation: ~").is_err());
    assert!(build("hooks:\n  Stop:\n    command: /abs").is_err());
    assert!(build("hooks:\n  Stop:\n    command: ~/x").is_err());
    assert!(build("hooks:\n  Stop:\n    command: '  '").is_err());
    assert!(build("hooks:\n  Stop:\n    matcher: '*'").is_err());
    assert!(build("hooks:\n  Stop: nope").is_err());
    assert!(build("hooks: []").is_err());
    assert_eq!(build("hooks:").unwrap().hooks, None);
    assert_eq!(build("hooks: {}").unwrap().hooks, Some(Vec::new()));
    assert!(build("dependencies:\n  servers: x").is_err());
    assert!(build("dependencies:\n  servers: [1]").is_err());
    assert_eq!(
        build("dependencies:\n  servers: [a, b]\n  skills: ~")
            .unwrap()
            .dependencies
            .unwrap()
            .servers,
        ["a", "b"]
    );
    assert!(build("metadata: [x]").is_err());
    assert!(build("license: 5").is_err());
    assert!(build("allowed-tools: [a]").is_err());
}

#[test]
fn skill_files_on_disk_fail_cleanly() {
    let tmp = ScratchDir::new("skill-files");
    let write = |dir: &str, bytes: &[u8]| {
        let path = tmp.path().join(dir).join("SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    };
    assert!(parse_skill_file(&tmp.path().join("missing/SKILL.md")).is_err());
    assert!(parse_skill_file(&write("bad-utf8", b"---\nname: a\n---\n\xff\xfe")).is_err());
    assert!(parse_skill_file(&write("empty", b"")).is_err());
    assert!(parse_skill_file(&write("no-fm", b"# just a heading\n")).is_err());
    let hooked = |command: &str| {
        let doc = format!(
            "---\nname: hooked\ndescription: d\nhooks:\n  Stop:\n    command: {command}\n---\nbody\n"
        );
        parse_skill_file(&write("hooked", doc.as_bytes()))
    };
    assert!(hooked("../outside.sh").is_err());
    assert!(hooked("scripts/missing.sh").is_err());
    fs::create_dir_all(tmp.path().join("hooked/scripts")).unwrap();
    fs::write(tmp.path().join("hooked/scripts/run.sh"), "#!/bin/sh\n").unwrap();
    assert!(hooked("scripts/run.sh").is_ok());
    assert!(hooked("scripts/../scripts/run.sh").is_ok());
    assert!(hooked("scripts/../../hooked/../outside.sh").is_err());
}

#[test]
fn discovery_survives_garbage_skill_files() {
    let tmp = ScratchDir::new("skill-discovery");
    run_cases("skills-discovery", 60, |_, rng| {
        tmp.reset();
        let mut good = 0;
        let count = rng.range(1, 6);
        for i in 0..count {
            let dir = tmp
                .path()
                .join(*rng.pick(&["skills", "rules"]))
                .join(format!("s{i}"));
            fs::create_dir_all(&dir).unwrap();
            let doc = match rng.below(4) {
                0 => {
                    good += 1;
                    format!("---\nname: s{i}\ndescription: d\n---\nbody\n").into_bytes()
                }
                1 => rng.bytes(120),
                2 => format!("---\n{}\n---\n{}", rng.garbage(60), rng.garbage(40)).into_bytes(),
                _ => Vec::new(),
            };
            fs::write(dir.join("SKILL.md"), doc).unwrap();
        }
        let report = discover_skills_report(tmp.path());
        assert!(report.skills.len() >= good);
        assert_eq!(report.skills.len() + report.warnings.len(), count);
        let again = discover_skills_report(tmp.path());
        let names = |r: &super::parser::Discovery| -> Vec<String> {
            r.skills.iter().map(|s| s.name().to_string()).collect()
        };
        assert_eq!(names(&report), names(&again));
    });
}

#[test]
fn finding_a_skill_by_name_matches_the_first_discovered_one() {
    let tmp = ScratchDir::new("skill-find");
    let names = ["alpha", "beta", "gamma", "delta"];
    let (mut hits, mut duplicates) = (0, 0);
    run_cases("skills-find", 120, |_, rng| {
        tmp.reset();
        for i in 0..rng.range(0, 8) {
            let base = tmp.path().join(*rng.pick(&["skills", "rules"]));
            let dir = base.join(format!("d{}", rng.below(6)));
            let _ = fs::remove_file(&dir);
            fs::create_dir_all(&dir).unwrap();
            match rng.below(6) {
                0 => {
                    fs::remove_dir_all(&dir).unwrap();
                    fs::write(&dir, "not a directory").unwrap();
                }
                1 => {}
                2 => fs::write(dir.join("SKILL.md"), rng.bytes(60)).unwrap(),
                _ => {
                    let name = *rng.pick(&names);
                    let doc = format!("---\nname: {name}\ndescription: d{i}\n---\nbody {i}\n");
                    fs::write(dir.join("SKILL.md"), doc).unwrap();
                }
            }
        }
        let all = discover_skills_report(tmp.path()).skills;
        for name in names.iter().copied().chain(["", "missing"]) {
            let expected = all.iter().find(|s| s.name() == name);
            let got = find_skill(tmp.path(), name);
            assert_eq!(
                got.as_ref()
                    .map(|s| (&s.source_path, &s.body, s.skill_type)),
                expected.map(|s| (&s.source_path, &s.body, s.skill_type)),
                "{name}"
            );
            hits += usize::from(got.is_some());
            duplicates += usize::from(all.iter().filter(|s| s.name() == name).count() > 1);
        }
    });
    assert!(hits > 100 && duplicates > 20, "{hits} {duplicates}");
    assert!(find_skill(&tmp.path().join("no-repo"), "alpha").is_none());
}

fn json_value(rng: &mut Rng, depth: usize) -> J {
    match rng.below(if depth >= 4 { 5 } else { 7 }) {
        0 => J::Null,
        1 => J::Bool(rng.chance(50)),
        2 => J::Num(
            rng.pick(&[
                "0",
                "-1",
                "42",
                "3.5",
                "-0.25",
                "1e5",
                "2E-3",
                "123456789012345678901234567890",
            ])
            .to_string(),
        ),
        3 | 4 => J::Str(rng.garbage(16)),
        5 => J::Arr(
            (0..rng.below(4))
                .map(|_| json_value(rng, depth + 1))
                .collect(),
        ),
        _ => {
            let mut items: Vec<(String, J)> = Vec::new();
            for _ in 0..rng.below(4) {
                let key = rng.garbage(6);
                if !items.iter().any(|(k, _)| *k == key) {
                    items.push((key, json_value(rng, depth + 1)));
                }
            }
            J::Obj(items)
        }
    }
}

#[test]
fn json_dumps_then_parse_is_the_identity() {
    run_cases("skills-json-roundtrip", 3000, |_, rng| {
        let value = json_value(rng, 0);
        let text = value.dumps();
        assert!(text.is_ascii(), "ensure_ascii output");
        assert_eq!(json::parse(&text), Ok(value.clone()), "{text}");
        assert_eq!(value.dumps(), text, "deterministic");
        assert_eq!(json::parse(&text).unwrap().dumps(), text, "idempotent");
    });
}

#[test]
fn json_parse_never_panics_on_garbage_or_truncation() {
    const VOCAB: &[&str] = &[
        "{",
        "}",
        "[",
        "]",
        ",",
        ":",
        "\"",
        "\\",
        "\\u",
        "\\ud800",
        "\\udbff",
        "\\udbff\\u0000",
        "\\ud800\\u0041",
        "\\udc00",
        "\\u12",
        "\\u+123",
        "null",
        "true",
        "false",
        "-",
        "1e",
        "1.",
        ".5",
        "+1",
        "é",
        "日",
        "🙂",
        " ",
        "\n",
        "\0",
    ];
    run_cases("skills-json-garbage", 20000, |_, rng| {
        let text = if rng.chance(50) {
            rng.tokens(VOCAB, 14)
        } else {
            rng.garbage(40)
        };
        let _ = json::parse(&text);
        let _ = json::parse(&format!("{{\"k\": \"{text}\"}}"));
        let _ = json::parse(&format!("[{text}]"));
    });
    run_cases("skills-json-truncation", 400, |_, rng| {
        let mut value = json_value(rng, 0);
        if !matches!(value, J::Obj(_) | J::Arr(_)) {
            value = J::Arr(vec![value]);
        }
        let text = value.dumps();
        for (i, _) in text.char_indices().skip(1) {
            assert!(json::parse(&text[..i]).is_err(), "prefix {i} of {text}");
        }
        assert!(json::parse(&format!("{text} x")).is_err());
        assert!(json::parse(&format!("{text}{text}")).is_err());
    });
}

#[test]
fn json_escapes_and_nesting_edges() {
    assert_eq!(json::parse(r#""😀""#), Ok(J::str("\u{1f600}")));
    assert_eq!(json::parse(r#""\ud800""#), Ok(J::str("\u{fffd}")));
    assert_eq!(json::parse(r#""\udc00""#), Ok(J::str("\u{fffd}")));
    assert!(json::parse(r#""\ud800A""#).is_ok());
    assert!(json::parse(r#""\udbff\u0000""#).is_ok());
    assert!(json::parse(r#""\ud800\udbff""#).is_ok());
    assert!(json::parse(r#""\u12""#).is_err());
    assert!(json::parse("\"a\u{1}\"").is_err());
    assert!(json::parse("\"unterminated").is_err());
    assert!(json::parse("\"\\").is_err());
    assert!(json::parse("").is_err());
    assert!(json::parse("   ").is_err());
    assert_eq!(
        json::parse(r#"{"a": 1, "b": 2, "a": 3}"#),
        Ok(J::Obj(vec![
            ("a".into(), J::Num("3".into())),
            ("b".into(), J::Num("2".into()))
        ]))
    );
    let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    assert!(json::parse(&nested(100)).is_ok());
    assert!(json::parse(&nested(129)).is_ok());
    assert!(json::parse(&nested(130)).is_err());
    assert!(json::parse(&"[".repeat(200_000)).is_err());
    assert!(json::parse(&"{\"a\":".repeat(50_000)).is_err());
}
