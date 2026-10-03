//! Seeded randomized and edge-case tests for the context layers: rule slugs, scaffolding,
//! frontmatter extraction and the `CLAUDE.local.md` deploy into client repos.

use super::layers::{
    body_of, deploy_client_locals, frontmatter, list_layers, scaffold_client_rule,
    scaffold_personal_rule, slug, MANAGED_LOCAL_HEADER,
};
use super::{Report, Roots};
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use regex::Regex;
use serde_yaml::Value as Yaml;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

type Tree = BTreeMap<String, Option<Vec<u8>>>;

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

fn slug_model(name: &str) -> String {
    let lower = name.to_lowercase();
    static RUNS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let runs = RUNS.get_or_init(|| Regex::new("[^a-z0-9]+").unwrap());
    let replaced = runs.replace_all(&lower, "-");
    let trimmed = replaced.trim_matches('-');
    if trimmed.is_empty() {
        "client".into()
    } else {
        trimmed.into()
    }
}

#[test]
fn slug_follows_the_regex_model_and_is_a_stable_name() {
    let valid = Regex::new("^[a-z0-9]+(-[a-z0-9]+)*$").unwrap();
    run_cases("layers-slug", 4000, |_, rng| {
        let name = match rng.below(3) {
            0 => rng.garbage(40),
            1 => rng.tokens(
                &[
                    "Acme", "_", "-", " ", "é", "日本", "CLIENT", "v18_arp", "..", "/", "1", "İ",
                    "ß", "\u{212a}", "--",
                ],
                8,
            ),
            _ => rng.string("abcXYZ019_-. ", 30),
        };
        let got = slug(&name);
        assert_eq!(got, slug_model(&name), "{name:?}");
        assert!(valid.is_match(&got) || got == "client", "{got:?}");
        assert_eq!(slug(&got), got, "idempotent");
    });
    assert_eq!(slug(""), "client");
    assert_eq!(slug("---"), "client");
    assert_eq!(slug("日本"), "client");
    assert_eq!(slug("v18_arp"), "v18-arp");
    assert_eq!(slug("A B"), slug("a-b"));
}

#[test]
fn long_client_names_are_refused_before_anything_is_written() {
    let home = ScratchDir::new("layers-client-long");
    let roots = Roots::from_home(home.path());
    let longest = "x".repeat(57);
    let created = scaffold_client_rule(&roots, &longest, None)
        .unwrap()
        .unwrap();
    let layers = list_layers(&roots);
    assert_eq!(layers[0].name.len(), 64);
    assert!(crate::plus::skills::parser::valid_name(&layers[0].name).is_ok());
    fs::remove_file(created).unwrap();
    for name in ["x".repeat(58), "x".repeat(70), "ab-".repeat(30)] {
        let before = tree(home.path());
        let err = scaffold_client_rule(&roots, &name, None).unwrap_err();
        assert!(err.contains("1-64 characters"), "{err}");
        assert_eq!(tree(home.path()), before, "nothing written for {name:?}");
    }
}

#[test]
fn personal_rule_is_created_once_and_never_overwritten() {
    let home = ScratchDir::new("layers-personal");
    run_cases("layers-personal", 80, |_, rng| {
        home.reset();
        let roots = Roots::from_home(home.path());
        let target = roots.rules_dir().join("personal").join("SKILL.md");
        if rng.chance(50) {
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, rng.bytes(60)).unwrap();
            let before = fs::read(&target).unwrap();
            assert_eq!(scaffold_personal_rule(&roots).unwrap(), None);
            assert_eq!(fs::read(&target).unwrap(), before);
            return;
        }
        assert_eq!(
            scaffold_personal_rule(&roots).unwrap(),
            Some(target.clone())
        );
        let layers = list_layers(&roots);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].name, "personal");
        assert!(layers[0].globs.is_empty());
        fs::write(&target, "edited").unwrap();
        assert_eq!(scaffold_personal_rule(&roots).unwrap(), None);
        assert_eq!(fs::read_to_string(&target).unwrap(), "edited");
    });
}

#[test]
fn personal_rule_scaffold_fails_cleanly_when_the_rules_dir_is_a_file() {
    let home = ScratchDir::new("layers-personal-blocked");
    let roots = Roots::from_home(home.path());
    fs::create_dir_all(roots.rules_dir().parent().unwrap()).unwrap();
    fs::write(roots.rules_dir(), "not a dir").unwrap();
    assert!(scaffold_personal_rule(&roots).is_err());
    assert!(scaffold_client_rule(&roots, "acme", None).is_err());
    assert!(list_layers(&roots).is_empty());
}

fn safe_client_name(rng: &mut Rng) -> String {
    loop {
        let name = rng.string("abcXYZ019 _.-:#[]{}&*!'%@\"\\", 14);
        if !name.is_empty() && !name.contains("---") {
            return name;
        }
    }
}

#[test]
fn scaffolded_client_rules_parse_back_and_are_never_overwritten() {
    let home = ScratchDir::new("layers-client");
    run_cases("layers-client-scaffold", 400, |_, rng| {
        home.reset();
        let roots = Roots::from_home(home.path());
        let name = safe_client_name(rng);
        let glob = rng
            .chance(40)
            .then(|| rng.string("*/abc._-, \"\\", 16))
            .filter(|g| !g.trim().is_empty() && !g.contains("---"));
        let created = scaffold_client_rule(&roots, &name, glob.as_deref())
            .unwrap()
            .expect("fresh dir");
        let expected_dir = roots.rules_dir().join(format!("client-{}", slug(&name)));
        assert_eq!(created, expected_dir.join("SKILL.md"));
        let layers = list_layers(&roots);
        assert_eq!(layers.len(), 1, "{name:?}");
        let layer = &layers[0];
        assert_eq!(layer.name, format!("client-{}", slug(&name)));
        assert_eq!(layer.description, format!("Client context: {name}"));
        let raw = glob
            .clone()
            .unwrap_or_else(|| format!("**/clients/{name}/**"));
        let want: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(String::from)
            .collect();
        assert_eq!(layer.globs, want, "{name:?} {glob:?}");
        assert!(body_of(&layer.path).contains(&format!("## {name} — client context")));
        let before = fs::read(&created).unwrap();
        assert_eq!(scaffold_client_rule(&roots, &name, None).unwrap(), None);
        assert_eq!(fs::read(&created).unwrap(), before);
    });
}

#[test]
fn client_names_with_quotes_and_backslashes_are_escaped_and_round_trip() {
    let home = ScratchDir::new("layers-client-quote");
    for name in ["ac\"me", "ac\\me", "\"", "\\", "a\\\"b\\", "ac\"\"me\\\\"] {
        home.reset();
        let roots = Roots::from_home(home.path());
        scaffold_client_rule(&roots, name, None).unwrap();
        let layers = list_layers(&roots);
        assert_eq!(layers.len(), 1, "{name:?}");
        assert_eq!(layers[0].name, format!("client-{}", slug(name)), "{name:?}");
        assert_eq!(
            layers[0].description,
            format!("Client context: {name}"),
            "{name:?}"
        );
        assert_eq!(layers[0].globs, vec![format!("**/clients/{name}/**")]);
        assert!(body_of(&layers[0].path).contains(&format!("## {name} \u{2014} client context")));
    }
    home.reset();
    let roots = Roots::from_home(home.path());
    scaffold_client_rule(&roots, "acme", Some("a\"b/**, c\\d/**")).unwrap();
    assert_eq!(list_layers(&roots)[0].globs, vec!["a\"b/**", "c\\d/**"]);
}

#[test]
fn client_names_and_globs_that_cannot_sit_in_the_frontmatter_are_refused() {
    let home = ScratchDir::new("layers-client-refused");
    let roots = Roots::from_home(home.path());
    let before = tree(home.path());
    for name in [
        "ac\nme",
        "ac\rme",
        "ac\u{1}me",
        "ac\tme",
        "ac\u{7f}me",
        "a---b",
    ] {
        let err = scaffold_client_rule(&roots, name, None).unwrap_err();
        assert!(err.starts_with("client name must not contain"), "{err}");
        assert_eq!(tree(home.path()), before, "{name:?}");
    }
    for glob in ["a\nb", "a\u{0}b", "a---b"] {
        let err = scaffold_client_rule(&roots, "acme", Some(glob)).unwrap_err();
        assert!(err.starts_with("glob must not contain"), "{err}");
        assert_eq!(tree(home.path()), before, "{glob:?}");
    }
    scaffold_client_rule(&roots, "a--b", Some("x--y")).unwrap();
}

#[test]
fn scaffolded_rules_stay_under_the_rules_dir_for_any_name() {
    let home = ScratchDir::new("layers-client-garbage");
    run_cases("layers-client-garbage", 600, |_, rng| {
        home.reset();
        let roots = Roots::from_home(home.path());
        let name = rng.garbage(30);
        let glob = rng.chance(30).then(|| rng.garbage(20));
        let _ = scaffold_client_rule(&roots, &name, glob.as_deref());
        for (path, _) in tree(home.path()) {
            let under = path.starts_with(".config/mcpm/skills_repo/rules/client-")
                || ".config/mcpm/skills_repo/rules/".starts_with(&path)
                || ".config/mcpm/skills_repo/".starts_with(&path)
                || ".config/mcpm/".starts_with(&path)
                || ".config/".starts_with(&path);
            assert!(under, "{path:?} from {name:?}");
        }
        let layers = list_layers(&roots);
        assert!(layers.len() <= 1);
        if let Some(layer) = layers.first() {
            assert_eq!(layer.name, format!("client-{}", slug(&name)), "{name:?}");
        }
    });
}

fn model_split(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---")?;
    let end = rest.find("---")?;
    Some((&rest[..end], &rest[end + 3..]))
}

fn layer_text(rng: &mut Rng) -> Vec<u8> {
    const LINES: &[&str] = &[
        "name: alpha",
        "name: 12",
        "name: true",
        "name: ~",
        "name: [a, b]",
        "description: \"desc\"",
        "description: plain text",
        "description: 5",
        "globs: \"a, b ,,c\"",
        "globs: [x, 1, true]",
        "globs: 5",
        "globs: \"\"",
        "activation: always",
        "bad: [",
        "- item",
        "\tname: tabbed",
        "# comment",
        "",
    ];
    match rng.below(7) {
        0 => rng.bytes(80),
        1 => rng.garbage(80).into_bytes(),
        2 => {
            let mut lines = Vec::new();
            for _ in 0..rng.range(0, 5) {
                lines.push(*rng.pick(LINES));
            }
            format!("---\n{}\n---\n{}", lines.join("\n"), rng.garbage(40)).into_bytes()
        }
        3 => format!("---\n{}", rng.pick(LINES)).into_bytes(),
        4 => format!("{}\n---\nname: x\n---\nbody", rng.garbage(10)).into_bytes(),
        5 => b"---".to_vec(),
        _ => format!(
            "---{}---{}---{}",
            rng.garbage(10),
            rng.garbage(10),
            rng.garbage(10)
        )
        .into_bytes(),
    }
}

#[test]
fn list_layers_and_body_match_a_find_based_model() {
    let home = ScratchDir::new("layers-list");
    run_cases("layers-list", 400, |_, rng| {
        home.reset();
        let roots = Roots::from_home(home.path());
        let rules = roots.rules_dir();
        fs::create_dir_all(&rules).unwrap();
        let mut dirs: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        for i in 0..rng.range(0, 6) {
            let name = format!("{}-{i}", rng.slug(6));
            let dir = rules.join(&name);
            match rng.below(6) {
                0 => {
                    fs::write(&dir, "stray file").unwrap();
                }
                1 => fs::create_dir_all(&dir).unwrap(),
                2 => fs::create_dir_all(dir.join("SKILL.md")).unwrap(),
                _ => {
                    let bytes = layer_text(rng);
                    fs::create_dir_all(&dir).unwrap();
                    fs::write(dir.join("SKILL.md"), &bytes).unwrap();
                    dirs.push((name, Some(bytes)));
                    continue;
                }
            }
            if dir.join("SKILL.md").exists() {
                dirs.push((name, None));
            }
        }
        dirs.sort();
        let layers = list_layers(&roots);
        assert_eq!(layers.len(), dirs.len());
        assert_eq!(format!("{layers:?}"), format!("{:?}", list_layers(&roots)));
        for (layer, (dir, bytes)) in layers.iter().zip(&dirs) {
            assert_eq!(layer.path, rules.join(dir).join("SKILL.md"));
            let text = bytes
                .as_ref()
                .and_then(|b| String::from_utf8(b.clone()).ok());
            let mapping = text
                .as_deref()
                .and_then(model_split)
                .and_then(|(yaml, _)| serde_yaml::from_str::<Yaml>(yaml).ok())
                .and_then(|v| match v {
                    Yaml::Mapping(m) => Some(m),
                    _ => None,
                })
                .unwrap_or_default();
            assert_eq!(frontmatter(&layer.path), mapping, "{dir}");
            let field = |key: &str| mapping.get(Yaml::String(key.into()));
            match field("name") {
                Some(Yaml::String(s)) => assert_eq!(&layer.name, s),
                None => assert_eq!(&layer.name, dir),
                Some(_) => {}
            }
            match field("description") {
                Some(Yaml::String(s)) => assert_eq!(&layer.description, s),
                None => assert!(layer.description.is_empty()),
                Some(_) => {}
            }
            match field("globs") {
                Some(Yaml::String(s)) => {
                    let want: Vec<&str> = s
                        .split(',')
                        .map(str::trim)
                        .filter(|g| !g.is_empty())
                        .collect();
                    assert_eq!(layer.globs, want);
                }
                None => assert!(layer.globs.is_empty()),
                Some(_) => {}
            }
            let body = match text.as_deref() {
                Some(t) => model_split(t).map(|(_, b)| b).unwrap_or(t).to_string(),
                None => String::new(),
            };
            assert_eq!(body_of(&layer.path), body, "{dir}");
        }
    });
}

#[test]
fn frontmatter_and_body_edge_cases() {
    let home = ScratchDir::new("layers-edges");
    let file = |text: &str| {
        let path = home.path().join("SKILL.md");
        fs::write(&path, text).unwrap();
        path
    };
    assert!(frontmatter(&file("")).is_empty());
    assert!(frontmatter(&file("---\n---\n")).is_empty());
    assert!(frontmatter(&file("---\nname: a\n")).is_empty());
    assert!(frontmatter(&file("\n---\nname: a\n---\n")).is_empty());
    assert!(frontmatter(&file("---\n- a\n- b\n---\n")).is_empty());
    assert!(frontmatter(&file("---\nname: a: b\n---\n")).is_empty());
    assert!(frontmatter(&home.path().join("missing")).is_empty());
    assert_eq!(frontmatter(&file("---\nname: a\n---\n")).len(), 1);
    assert_eq!(
        body_of(&file("---\nname: a\n---\nBody\n--- x ---\n")),
        "\nBody\n--- x ---\n"
    );
    assert_eq!(body_of(&file("---\nname: a\n")), "---\nname: a\n");
    assert_eq!(body_of(&file("plain --- text")), "plain --- text");
    assert_eq!(body_of(&file("")), "");
    assert_eq!(body_of(&home.path().join("missing")), "");
    fs::write(home.path().join("SKILL.md"), [0xff, 0xfe, b'-']).unwrap();
    assert_eq!(body_of(&home.path().join("SKILL.md")), "");
    assert!(frontmatter(&home.path().join("SKILL.md")).is_empty());
}

fn client_layer_text(slug: &str, body: &str) -> String {
    format!(
        "---\nname: client-{slug}\ndescription: \"Client context: {slug}\"\nactivation: always\n---\n{body}"
    )
}

struct World {
    roots: Roots,
    clients: PathBuf,
    bodies: HashMap<String, String>,
    children: Vec<String>,
}

const CHILD_POOL: &[&str] = &[
    "acme", "Acme", "ACME", "v18_arp", "beta-co", "Zed One", "é-corp", "日本", "a.b", "x__y", "1",
];

fn build_world(rng: &mut Rng, home: &Path) -> World {
    let roots = Roots::from_home(home);
    let clients = home.join("clients");
    fs::create_dir_all(&clients).unwrap();
    let mut bodies = HashMap::new();
    for _ in 0..rng.range(0, 4) {
        let name = rng.pick(CHILD_POOL).to_string();
        let s = slug(&name);
        if bodies.contains_key(&s) {
            continue;
        }
        let body = format!("\n{}\n", rng.garbage(60));
        let dir = roots.rules_dir().join(format!("client-{s}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), client_layer_text(&s, &body)).unwrap();
        bodies.insert(s, body);
    }
    let mut children = Vec::new();
    for _ in 0..rng.range(0, 6) {
        let name = format!("{}{}", rng.pick(CHILD_POOL), rng.string("0-", 1));
        let dir = clients.join(&name);
        if dir.exists() {
            continue;
        }
        fs::create_dir_all(&dir).unwrap();
        match rng.below(6) {
            0 => {}
            1 => fs::write(
                dir.join("CLAUDE.local.md"),
                format!("{MANAGED_LOCAL_HEADER}\n\nstale\n"),
            )
            .unwrap(),
            2 => fs::write(dir.join("CLAUDE.local.md"), "my own notes").unwrap(),
            3 => {
                let mut bytes = vec![0xff, 0xfe];
                if rng.chance(50) {
                    bytes.extend_from_slice(MANAGED_LOCAL_HEADER.as_bytes());
                }
                fs::write(dir.join("CLAUDE.local.md"), bytes).unwrap();
            }
            _ => {}
        }
        match rng.below(6) {
            0 | 1 => {}
            2 => fs::create_dir_all(dir.join(".git")).unwrap(),
            3 => {
                let text = *rng.pick(&[
                    "",
                    "target\n",
                    "target",
                    "CLAUDE.local.md\n",
                    "a\r\nb\r\n",
                    "x\n\n",
                ]);
                fs::create_dir_all(dir.join(".git/info")).unwrap();
                fs::write(dir.join(".git/info/exclude"), text).unwrap();
            }
            4 => fs::write(dir.join(".git"), "gitdir: ../elsewhere").unwrap(),
            _ => fs::create_dir_all(dir.join(".git/info")).unwrap(),
        }
        children.push(name);
    }
    fs::write(clients.join("stray-file"), "x").unwrap();
    World {
        roots,
        clients,
        bodies,
        children,
    }
}

fn expected_exclude(before: Option<&str>) -> Option<String> {
    let text = before.unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.contains(&"CLAUDE.local.md") {
        return None;
    }
    lines.push("CLAUDE.local.md");
    Some(format!("{}\n", lines.join("\n")))
}

#[test]
fn deploying_client_locals_matches_the_model_and_converges() {
    let home = ScratchDir::new("layers-deploy");
    let (mut deployed, mut left_alone) = (0, 0);
    run_cases("layers-deploy", 250, |_, rng| {
        home.reset();
        let world = build_world(rng, home.path());
        let start = tree(home.path());

        let mut dry = Report::default();
        deploy_client_locals(&world.roots, &world.clients, &mut dry, true).unwrap();
        assert_eq!(tree(home.path()), start, "dry run wrote something");

        let mut report = Report::default();
        deploy_client_locals(&world.roots, &world.clients, &mut report, false).unwrap();
        assert_eq!(
            report.actions, dry.actions,
            "dry run reports what a run does"
        );
        assert_eq!(report.warnings, dry.warnings);
        let end = tree(home.path());

        let mut touched: Vec<String> = Vec::new();
        for child in &world.children {
            let local = format!("clients/{child}/CLAUDE.local.md");
            let exclude = format!("clients/{child}/.git/info/exclude");
            let before_local = start.get(&local).cloned().flatten();
            let before_exclude = start
                .get(&exclude)
                .cloned()
                .flatten()
                .map(|b| String::from_utf8(b).unwrap());
            let Some(body) = world.bodies.get(&slug(child)) else {
                left_alone += 1;
                assert_eq!(end.get(&local).cloned().flatten(), before_local, "{child}");
                assert_eq!(
                    end.get(&exclude)
                        .cloned()
                        .flatten()
                        .map(|b| String::from_utf8(b).unwrap()),
                    before_exclude,
                    "{child}"
                );
                continue;
            };
            let unmanaged = before_local
                .as_ref()
                .is_some_and(|b| !String::from_utf8_lossy(b).contains(MANAGED_LOCAL_HEADER));
            if unmanaged {
                left_alone += 1;
                assert_eq!(end.get(&local).cloned().flatten(), before_local, "{child}");
                assert_eq!(
                    end.get(&exclude)
                        .cloned()
                        .flatten()
                        .map(|b| String::from_utf8(b).unwrap()),
                    before_exclude,
                    "{child}"
                );
                assert!(report
                    .warnings
                    .iter()
                    .any(|w| w.contains(&format!("{child}/CLAUDE.local.md"))));
                continue;
            }
            deployed += 1;
            let want = format!("{MANAGED_LOCAL_HEADER}\n\n{}\n", body.trim());
            assert_eq!(end[&local].as_deref(), Some(want.as_bytes()), "{child}");
            touched.push(local);
            let git = home.path().join("clients").join(child).join(".git");
            if git.is_dir() {
                let after = expected_exclude(before_exclude.as_deref());
                let got = end
                    .get(&exclude)
                    .cloned()
                    .flatten()
                    .map(|b| String::from_utf8(b).unwrap());
                match after {
                    Some(text) => {
                        assert_eq!(got.as_deref(), Some(text.as_str()), "{child}");
                        touched.push(exclude);
                    }
                    None => assert_eq!(got, before_exclude),
                }
            }
        }
        for (path, content) in &end {
            if start.get(path) != Some(content) {
                let parent_dir = path.ends_with('/') && path.contains("/.git/");
                assert!(
                    touched.contains(path) || parent_dir,
                    "unexpected change {path:?}"
                );
            }
        }

        let mut again = Report::default();
        deploy_client_locals(&world.roots, &world.clients, &mut again, false).unwrap();
        assert!(again.actions.is_empty(), "{:?}", again.actions);
        assert_eq!(tree(home.path()), end, "second run changed files");
    });
    assert!(deployed > 50 && left_alone > 100, "{deployed} {left_alone}");
}

#[test]
fn deploy_is_a_no_op_without_a_clients_directory() {
    let home = ScratchDir::new("layers-deploy-missing");
    let roots = Roots::from_home(home.path());
    let mut report = Report::default();
    deploy_client_locals(&roots, &home.path().join("missing"), &mut report, false).unwrap();
    let file = home.path().join("file");
    fs::write(&file, "x").unwrap();
    deploy_client_locals(&roots, &file, &mut report, false).unwrap();
    assert_eq!(report, Report::default());
}

#[test]
fn deploy_edge_cases_duplicate_layers_invalid_yaml_and_layer_names() {
    let home = ScratchDir::new("layers-deploy-edges");
    let roots = Roots::from_home(home.path());
    let rules = roots.rules_dir();
    let clients = home.path().join("clients");
    for child in ["alpha", "broken", "renamed"] {
        fs::create_dir_all(clients.join(child)).unwrap();
    }
    let layer = |dir: &str, text: &str| {
        fs::create_dir_all(rules.join(dir)).unwrap();
        fs::write(rules.join(dir).join("SKILL.md"), text).unwrap();
    };
    layer("a-first", &client_layer_text("alpha", "FIRST"));
    layer("b-second", &client_layer_text("alpha", "SECOND"));
    layer("client-broken", "---\nname: [unclosed\n---\nBROKEN BODY");
    layer("elsewhere", "---\nname: client-renamed\n---\nRENAMED BODY");
    layer(
        "client-renamed",
        "---\nname: not-a-client-name\n---\nIGNORED",
    );
    let mut report = Report::default();
    deploy_client_locals(&roots, &clients, &mut report, false).unwrap();
    let read =
        |child: &str| fs::read_to_string(clients.join(child).join("CLAUDE.local.md")).unwrap();
    assert!(read("alpha").ends_with("\n\nFIRST\n"));
    assert!(read("broken").ends_with("\n\nBROKEN BODY\n"));
    assert!(read("renamed").ends_with("\n\nRENAMED BODY\n"));
    assert!(report.warnings.is_empty());
}

#[test]
fn exclude_file_keeps_existing_lines_and_normalises_line_endings() {
    let home = ScratchDir::new("layers-exclude");
    let roots = Roots::from_home(home.path());
    let clients = home.path().join("clients");
    let repo = clients.join("acme");
    fs::create_dir_all(repo.join(".git/info")).unwrap();
    fs::write(repo.join(".git/info/exclude"), "a\r\nb").unwrap();
    fs::create_dir_all(roots.rules_dir().join("client-acme")).unwrap();
    fs::write(
        roots.rules_dir().join("client-acme/SKILL.md"),
        client_layer_text("acme", "body"),
    )
    .unwrap();
    let mut report = Report::default();
    deploy_client_locals(&roots, &clients, &mut report, false).unwrap();
    assert_eq!(
        fs::read_to_string(repo.join(".git/info/exclude")).unwrap(),
        "a\nb\nCLAUDE.local.md\n"
    );
    assert_eq!(report.actions.len(), 2);
}

#[test]
fn expand_user_and_skills_repo_resolution_never_panic() {
    let home = ScratchDir::new("layers-roots");
    run_cases("layers-roots", 400, |_, rng| {
        home.reset();
        let roots = Roots::from_home(home.path());
        let raw = rng.tokens(&["~", "/", "~/", "a", "..", "é", " ", "~x", ""], 5);
        let expanded = roots.expand_user(&raw);
        if raw == "~" {
            assert_eq!(expanded, home.path());
        } else if let Some(rest) = raw.strip_prefix("~/") {
            assert_eq!(expanded, home.path().join(rest));
        } else {
            assert_eq!(expanded, PathBuf::from(&raw));
        }

        let config_dir = roots.config_dir.clone();
        fs::create_dir_all(&config_dir).unwrap();
        let local = rng.garbage(12);
        let text = match rng.below(6) {
            0 => rng.garbage(40),
            1 => format!(
                "{{\"local_path\": {}}}",
                serde_json::to_string(&local).unwrap()
            ),
            2 => "{\"local_path\": 5}".into(),
            3 => "{\"local_path\": \"\"}".into(),
            4 => "[]".into(),
            _ => String::new(),
        };
        fs::write(config_dir.join("skills_sync.json"), &text).unwrap();
        let repo = roots.skills_repo_path();
        let from_config = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("local_path")
                    .and_then(|p| p.as_str().map(String::from))
            })
            .filter(|p| !p.is_empty());
        match from_config {
            Some(p) => assert_eq!(repo, roots.expand_user(&p)),
            None => assert_eq!(repo, config_dir.join("skills_repo")),
        }
        assert_eq!(roots.rules_dir(), repo.join("rules"));
    });
}
