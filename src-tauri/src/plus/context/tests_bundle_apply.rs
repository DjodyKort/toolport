use super::bundle_apply::{self, World};
use super::bundle_json as json;
use super::bundle_ledger as ledger;
use super::Roots;
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use crate::plus::sources::fsx;
use crate::plus::testutil::tree_snapshot;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const ACME: &str = include_str!("../../../tests/fixtures/bundles/acme-dev.yaml");
const CANARY: &str = "sk-canary-0123456789abcdef";

struct Fx {
    scratch: ScratchDir,
    roots: Roots,
    data: PathBuf,
    cwd: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let scratch = ScratchDir::new(tag);
        let base = fsx::canonical(scratch.path());
        let roots = Roots::from_home(&base.join("home"));
        let repo = roots.skills_repo_path();
        for skill in ["notes-helper", "scratch-1", "scratch-2", "long-guide", "erp-core", "erp-reports"] {
            put(&repo.join("skills").join(skill).join("SKILL.md"), &format!("---\nname: {skill}\ndescription: d\n---\nbody\n"));
        }
        put(
            &repo.join("rules/acme-knowledge/SKILL.md"),
            "---\nname: acme-knowledge\ndescription: d\nactivation: always\n---\n\nUse the ERP naming rules.\n",
        );
        put(&repo.join("profiles/acme-dev.yaml"), ACME);
        let cwd = base.join("proj");
        fs::create_dir_all(cwd.join(".git/info")).unwrap();
        put(&cwd.join(".git/info/exclude"), "# local\n");
        Self { data: base.join("data"), roots, cwd, scratch }
    }

    fn world(&self) -> World<'_> {
        World { roots: &self.roots, data_dir: &self.data }
    }

    fn settings(&self) -> PathBuf {
        self.cwd.join(".claude/settings.local.json")
    }

    fn apply(&self, name: &str) -> Value {
        bundle_apply::apply(&self.world(), name, &self.cwd, false).unwrap()
    }

    fn undo(&self) -> Value {
        bundle_apply::undo(&self.world(), &self.cwd, false).unwrap()
    }

    fn status(&self) -> Value {
        bundle_apply::status(&self.world(), &self.cwd).unwrap()
    }

    fn text(&self, rel: &str) -> Option<String> {
        fs::read_to_string(self.cwd.join(rel)).ok()
    }
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const FOREIGN: &str = "{\n  \"permissions\": {\n    \"allow\": [\n      \"Bash(ls:*)\"\n    ],\n    \"deny\": [\n      \"Read(./secrets/**)\"\n    ]\n  },\n  \"enabledPlugins\": {\n    \"mine@own\": true,\n    \"tools-pack@tools-market\": true\n  },\n  \"env\": {\n    \"API_TOKEN\": \"sk-canary-0123456789abcdef\"\n  },\n  \"model\": \"opus\"\n}\n";

#[test]
fn apply_then_undo_leaves_a_foreign_file_byte_identical() {
    let fx = Fx::new("bundle-apply-undo");
    put(&fx.settings(), FOREIGN);
    let before = tree_snapshot(&fx.cwd);
    let applied = fx.apply("acme-dev");
    assert_eq!(applied["dryRun"], false);
    let text = fx.text(".claude/settings.local.json").unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["skillOverrides"], json!({"notes-helper": "off", "scratch-1": "off", "scratch-2": "off", "long-guide": "name-only"}));
    assert_eq!(v["enabledPlugins"], json!({"mine@own": true, "tools-pack@tools-market": false, "loop-runner@official": false}));
    assert_eq!(v["claudeMdExcludes"], json!(["**/acme-erp/CLAUDE.md"]));
    assert_eq!(v["permissions"]["deny"], json!(["Read(./secrets/**)", "Agent(reviewer-bot)"]));
    assert_eq!(v["permissions"]["allow"], json!(["Bash(ls:*)"]));
    assert_eq!(v["env"]["API_TOKEN"], CANARY);
    let md = fx.text("CLAUDE.local.md").unwrap();
    assert!(md.contains("toolport:bundle:begin acme-dev") && md.contains("Use the ERP naming rules."));
    let exclude = fx.text(".git/info/exclude").unwrap();
    assert_eq!(exclude, "# local\n.claude/settings.local.json\nCLAUDE.local.md\n");

    let status = fx.status();
    assert_eq!(status["applied"]["bundle"], "acme-dev");
    assert_eq!(status["applied"]["drift"], false);
    assert_eq!(status["applied"]["ownedKeys"]["enabledPlugins"], json!(["loop-runner@official", "tools-pack@tools-market"]));
    assert_eq!(status["applied"]["ownedKeys"]["permissionsDeny"], json!(["Agent(reviewer-bot)"]));

    let undone = fx.undo();
    assert_eq!(undone["conflicts"], json!([]));
    assert_eq!(tree_snapshot(&fx.cwd), before);
    assert_eq!(fx.text(".claude/settings.local.json").unwrap(), FOREIGN);
    assert!(fx.status()["applied"].is_null());
    assert!(ledger::load(&fx.data).folders.is_empty());
}

#[test]
fn drift_is_reported_and_a_changed_key_is_a_conflict_that_undo_leaves_alone() {
    let fx = Fx::new("bundle-apply-drift");
    put(&fx.settings(), FOREIGN);
    fx.apply("acme-dev");
    let applied = fs::read_to_string(fx.settings()).unwrap();
    let mut v: Value = serde_json::from_str(&applied).unwrap();
    v["skillOverrides"]["notes-helper"] = json!("on");
    v["enabledPlugins"].as_object_mut().unwrap().remove("loop-runner@official");
    v["permissions"]["allow"].as_array_mut().unwrap().push(json!("Bash(git status)"));
    v["theme"] = json!("dark");
    fs::write(fx.settings(), serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();

    let status = fx.status();
    assert_eq!(status["applied"]["drift"], true);
    let changed: Vec<&str> = status["applied"]["changedKeys"].as_array().unwrap().iter().map(|k| k.as_str().unwrap()).collect();
    assert_eq!(changed, ["skillOverrides.notes-helper", "enabledPlugins.loop-runner@official"]);
    assert_eq!(status["conflicts"], json!(["skillOverrides.notes-helper"]));

    let undone = fx.undo();
    assert_eq!(undone["conflicts"], json!(["skillOverrides.notes-helper"]));
    let after: Value = serde_json::from_str(&fs::read_to_string(fx.settings()).unwrap()).unwrap();
    assert_eq!(after["skillOverrides"], json!({"notes-helper": "on"}));
    assert_eq!(after["enabledPlugins"], json!({"mine@own": true, "tools-pack@tools-market": true}));
    assert_eq!(after["permissions"]["allow"], json!(["Bash(ls:*)", "Bash(git status)"]));
    assert_eq!(after["permissions"]["deny"], json!(["Read(./secrets/**)"]));
    assert_eq!(after["theme"], "dark");
    assert!(after.get("claudeMdExcludes").is_none());
    assert!(fx.text("CLAUDE.local.md").is_none());
}

#[test]
fn a_block_edited_by_hand_is_a_conflict_and_the_file_around_it_is_untouched() {
    let fx = Fx::new("bundle-apply-block");
    put(&fx.cwd.join("CLAUDE.local.md"), "my notes\nno newline at the end");
    fx.apply("acme-dev");
    let md = fx.text("CLAUDE.local.md").unwrap();
    assert!(md.starts_with("my notes\nno newline at the end\n\n<!-- toolport:bundle:begin acme-dev -->"));
    fx.undo();
    assert_eq!(fx.text("CLAUDE.local.md").unwrap(), "my notes\nno newline at the end");

    fx.apply("acme-dev");
    let edited = fx.text("CLAUDE.local.md").unwrap().replace("ERP naming rules", "ERP naming conventions");
    fs::write(fx.cwd.join("CLAUDE.local.md"), &edited).unwrap();
    assert_eq!(fx.status()["conflicts"], json!(["CLAUDE.local.md"]));
    let undone = fx.undo();
    assert_eq!(undone["conflicts"], json!(["CLAUDE.local.md"]));
    assert_eq!(fx.text("CLAUDE.local.md").unwrap(), edited);
}

#[test]
fn a_dry_run_writes_nothing_and_the_plan_lists_every_step() {
    let fx = Fx::new("bundle-apply-dry");
    put(&fx.settings(), FOREIGN);
    let before = tree_snapshot(&fx.cwd);
    let out = bundle_apply::apply(&fx.world(), "acme-dev", &fx.cwd, true).unwrap();
    assert_eq!(out["dryRun"], true);
    assert!(out["result"].is_null());
    assert_eq!(tree_snapshot(&fx.cwd), before);
    assert!(!fx.data.exists());
    let plan = &out["plan"];
    assert!(plan["summary"].as_str().unwrap().starts_with("Apply bundle acme-dev in "));
    assert!(plan["undo"].as_str().unwrap().starts_with("toolportctl context bundle undo --cwd "));
    let ops: Vec<&str> = plan["steps"].as_array().unwrap().iter().map(|s| s["op"].as_str().unwrap()).collect();
    assert_eq!(ops, ["merge", "create", "update", "update", "note", "note"]);
    assert!(plan["steps"][0]["keys"].as_array().unwrap().contains(&json!("skillOverrides")));
    assert!(plan["steps"].as_array().unwrap().iter().any(|s| s["detail"].as_str().unwrap().contains("hygiene, not savings")));
}

#[test]
fn nothing_is_written_above_the_start_folder_and_a_missing_file_is_created_and_removed() {
    let fx = Fx::new("bundle-apply-nested");
    let parent = fx.cwd.parent().unwrap().to_path_buf();
    put(&parent.join(".claude/settings.local.json"), "{\"enabledPlugins\": {\"keep@me\": true}}\n");
    put(&parent.join("CLAUDE.local.md"), "parent notes\n");
    let outside = |_: &Fx| {
        let mut snap = tree_snapshot(&parent);
        snap.retain(|k, _| !k.starts_with("proj") && !k.starts_with("home") && !k.starts_with("data"));
        snap
    };
    let before = outside(&fx);
    fx.apply("acme-dev");
    assert_eq!(outside(&fx), before);
    assert!(fx.settings().exists());
    let created = fs::read_to_string(fx.settings()).unwrap();
    assert!(created.starts_with("{\n  \"skillOverrides\": {\n    \"long-guide\": \"name-only\""));
    assert!(created.ends_with("}\n"));
    fx.undo();
    assert_eq!(outside(&fx), before);
    assert!(!fx.settings().exists());
    assert!(!fx.cwd.join(".claude").exists());
    assert!(!fx.cwd.join("CLAUDE.local.md").exists());
    assert_eq!(fx.text(".git/info/exclude").unwrap(), "# local\n");
}

#[test]
fn a_folder_that_is_not_a_repo_root_is_written_but_not_ignored_and_says_so() {
    let fx = Fx::new("bundle-apply-norepo");
    let plain = fx.cwd.parent().unwrap().join("plain");
    fs::create_dir_all(&plain).unwrap();
    let out = bundle_apply::apply(&fx.world(), "acme-dev", &plain, false).unwrap();
    let warnings = out["plan"]["warnings"].as_array().unwrap();
    assert!(warnings.iter().any(|w| w.as_str().unwrap().contains("not a git repository root")));
    assert!(plain.join(".claude/settings.local.json").exists());
    assert!(!plain.join(".git").exists());
}

#[test]
fn applying_another_bundle_puts_the_first_one_back_before_writing_the_second() {
    let fx = Fx::new("bundle-apply-replace");
    put(&fx.settings(), FOREIGN);
    put(
        &fx.roots.skills_repo_path().join("profiles/lean.yaml"),
        "format: 1\nname: lean\nskills:\n  allow: [erp-core]\nagents:\n  off: [other-bot]\n",
    );
    fx.apply("acme-dev");
    let out = fx.apply("lean");
    assert!(out["plan"]["steps"][0]["detail"].as_str().unwrap().starts_with("replaces bundle acme-dev"));
    let v: Value = serde_json::from_str(&fx.text(".claude/settings.local.json").unwrap()).unwrap();
    assert_eq!(v["enabledPlugins"], json!({"mine@own": true, "tools-pack@tools-market": true}));
    assert_eq!(v["skillOverrides"].as_object().unwrap().len(), 5);
    assert!(v["skillOverrides"].get("erp-core").is_none());
    assert_eq!(v["permissions"]["deny"], json!(["Read(./secrets/**)", "Agent(other-bot)"]));
    assert!(fx.text("CLAUDE.local.md").is_none());
    assert_eq!(fx.status()["applied"]["bundle"], "lean");
    fx.undo();
    assert_eq!(fx.text(".claude/settings.local.json").unwrap(), FOREIGN);
}

#[test]
fn no_secret_reaches_the_ledger_the_plan_or_the_result() {
    let fx = Fx::new("bundle-apply-canary");
    put(&fx.settings(), FOREIGN);
    let dry = bundle_apply::apply(&fx.world(), "acme-dev", &fx.cwd, true).unwrap();
    let applied = fx.apply("acme-dev");
    let undone = fx.undo();
    fx.apply("acme-dev");
    let ledger_text = fs::read_to_string(ledger::path(&fx.data)).unwrap();
    for text in [dry.to_string(), applied.to_string(), undone.to_string(), ledger_text, fx.status().to_string()] {
        assert!(!text.contains(CANARY), "{text}");
        assert!(!text.contains("API_TOKEN"), "{text}");
    }
}

#[test]
fn a_file_that_is_not_json_is_left_alone_and_the_apply_fails() {
    let fx = Fx::new("bundle-apply-badjson");
    put(&fx.settings(), "{\"a\": 1,}\n");
    let err = bundle_apply::apply(&fx.world(), "acme-dev", &fx.cwd, false).unwrap_err();
    assert_eq!(err.code, "failed");
    assert!(err.message.contains("left alone"));
    assert_eq!(fx.text(".claude/settings.local.json").unwrap(), "{\"a\": 1,}\n");
    assert!(ledger::load(&fx.data).folders.is_empty());
    assert_eq!(bundle_apply::undo(&fx.world(), &fx.cwd, false).unwrap_err().code, "not_applied");
    assert_eq!(bundle_apply::apply(&fx.world(), "nope", &fx.cwd, false).unwrap_err().code, "not_found");
    assert_eq!(bundle_apply::apply(&fx.world(), "acme-dev", &fx.cwd.join("missing"), false).unwrap_err().code, "usage");
}

#[test]
fn applied_to_lists_the_folders_with_their_drift() {
    let fx = Fx::new("bundle-apply-applied-to");
    fx.apply("acme-dev");
    let rows = bundle_apply::applied_to(&fx.data, "acme-dev");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["folder"], fsx::display(&fx.cwd));
    assert_eq!(rows[0]["drift"], false);
    assert!(bundle_apply::applied_to(&fx.data, "other").is_empty());
    let _ = &fx.scratch;
}

#[test]
fn the_json_editor_inserts_and_removes_by_exact_inverses() {
    for text in ["{}", "{\n}", "{\"a\": 1}", "{\n  \"a\": 1,\n  \"b\": [\n    \"x\"\n  ]\n}\n", "{\n\t\"a\": {\"b\": []}\n}"] {
        let mut t = text.to_string();
        let created = json::set(&mut t, &["k", "n"], "\"v\"").unwrap();
        assert_eq!(created, ["k"]);
        json::push(&mut t, &["l", "m"], "\"i\"").unwrap();
        json::push(&mut t, &["l", "m"], "\"j\"").unwrap();
        assert_eq!(json::value(&t, &["l", "m"]), Some(json!(["i", "j"])));
        assert!(json::pull(&mut t, &["l", "m"], &json!("j")).unwrap());
        assert!(json::pull(&mut t, &["l", "m"], &json!("i")).unwrap());
        assert!(json::prune_empty(&mut t, &["l", "m"]).unwrap());
        assert!(json::prune_empty(&mut t, &["l"]).unwrap());
        assert!(json::remove(&mut t, &["k", "n"]).unwrap());
        assert!(json::prune_empty(&mut t, &["k"]).unwrap());
        let squash = |s: &str| s.split_whitespace().collect::<String>();
        assert_eq!(squash(&t), squash(text), "{text:?} -> {t:?}");
        if !matches!(text, "{\n}") {
            assert_eq!(t, text);
        }
    }
    let mut t = "{\"a\": [1]}".to_string();
    assert!(json::set(&mut t, &["a", "b"], "1").is_err());
    assert!(json::push(&mut t, &["a", "b"], "1").is_err());
    assert!(!json::remove(&mut t, &["x", "y"]).unwrap());
    assert_eq!(json::get(&t, &["a"]), Some(json::Found::Container));
}

fn word(rng: &mut Rng) -> String {
    rng.pick(&["alpha", "beta", "g\u{e9}", "\u{65e5}\u{672c}", "q\"uote", "back\\slash", "tab\there", "x y", "\u{1f600}"]).to_string()
}

fn gen_value(rng: &mut Rng, depth: usize) -> Value {
    match rng.below(if depth == 0 { 4 } else { 6 }) {
        0 => json!(rng.below(100) as i64 - 50),
        1 => json!(rng.chance(50)),
        2 => Value::Null,
        3 => json!(word(rng)),
        4 => Value::Array((0..rng.range(0, 3)).map(|_| gen_value(rng, depth - 1)).collect()),
        _ => Value::Object((0..rng.range(0, 3)).map(|i| (format!("{}{i}", rng.pick(&["k", "m", "z"])), gen_value(rng, depth - 1))).collect()),
    }
}

fn sublist(rng: &mut Rng, items: &[&str]) -> Vec<Value> {
    items.iter().filter(|_| rng.chance(50)).map(|s| json!(s)).collect()
}

fn gen_foreign(rng: &mut Rng) -> String {
    let mut root = serde_json::Map::new();
    for i in 0..rng.range(0, 4) {
        root.insert(format!("other{i}"), gen_value(rng, 2));
    }
    if rng.chance(70) {
        let mut perms = serde_json::Map::new();
        perms.insert("allow".into(), Value::Array(sublist(rng, &["Bash(ls:*)", "Read(x)"])));
        if rng.chance(50) {
            perms.insert("deny".into(), Value::Array(sublist(rng, &["Read(./s/**)", "Agent(reviewer-bot)", "Agent(mine)"])));
        }
        if rng.chance(30) {
            perms.insert("ask".into(), Value::Array(vec![]));
        }
        root.insert("permissions".into(), Value::Object(perms));
    }
    if rng.chance(60) {
        let mut plugins = serde_json::Map::new();
        for id in ["mine@own", "tools-pack@tools-market", "loop-runner@official"] {
            if rng.chance(50) {
                plugins.insert(id.into(), json!(rng.chance(50)));
            }
        }
        root.insert("enabledPlugins".into(), Value::Object(plugins));
    }
    if rng.chance(40) {
        let mut over = serde_json::Map::new();
        for name in ["notes-helper", "scratch-1", "mine-skill"] {
            if rng.chance(50) {
                over.insert(name.into(), json!(*rng.pick(&["off", "name-only", "on"])));
            }
        }
        root.insert("skillOverrides".into(), Value::Object(over));
    }
    if rng.chance(40) {
        root.insert("claudeMdExcludes".into(), Value::Array(sublist(rng, &["**/acme-erp/CLAUDE.md", "**/mine.md"])));
    }
    let value = Value::Object(root);
    let mut text = match rng.below(4) {
        0 => serde_json::to_string_pretty(&value).unwrap(),
        1 | 2 => {
            let indent: &[u8] = if rng.chance(50) { b"    " } else { b"\t" };
            let mut buf = Vec::new();
            let fmt = serde_json::ser::PrettyFormatter::with_indent(indent);
            serde::Serialize::serialize(&value, &mut serde_json::Serializer::with_formatter(&mut buf, fmt)).unwrap();
            String::from_utf8(buf).unwrap()
        }
        _ => serde_json::to_string(&value).unwrap(),
    };
    if rng.chance(70) {
        text.push('\n');
    }
    text
}

fn gen_bundle(rng: &mut Rng) -> String {
    let pick = |rng: &mut Rng, items: &[&str]| -> String {
        let chosen: Vec<&&str> = items.iter().filter(|_| rng.chance(50)).collect();
        if chosen.is_empty() { "[]".into() } else { format!("[{}]", chosen.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(", ")) }
    };
    let (a, b, c, d, e, f) = (
        pick(rng, &["notes-helper", "scratch-*", "mine-skill"]),
        pick(rng, &["long-guide", "erp-core"]),
        pick(rng, &["tools-pack@tools-market", "loop-runner@official", "mine@own"]),
        pick(rng, &["**/acme-erp/CLAUDE.md", "**/other.md"]),
        pick(rng, &["reviewer-bot", "other-bot", "mine"]),
        pick(rng, &["acme-knowledge", "missing-layer"]),
    );
    format!("format: 1\nname: gen\nskills:\n  off: {a}\n  name_only: {b}\nplugins:\n  off: {c}\nlayers:\n  add: {f}\n  exclude: {d}\nagents:\n  off: {e}\n")
}

#[test]
fn apply_then_undo_is_byte_identical_for_any_foreign_settings_file() {
    run_cases("bundle-apply-undo-property", 80, |_, rng| {
        let fx = Fx::new("bundle-prop");
        put(&fx.roots.skills_repo_path().join("profiles/gen.yaml"), &gen_bundle(rng));
        let foreign = gen_foreign(rng);
        let md = rng.chance(40).then(|| format!("{}{}", word(rng), if rng.chance(50) { "\n" } else { "" }));
        if rng.chance(85) {
            put(&fx.settings(), &foreign);
        }
        if let Some(md) = &md {
            put(&fx.cwd.join("CLAUDE.local.md"), md);
        }
        let before = tree_snapshot(&fx.cwd);
        let original: Option<Value> = fx.text(".claude/settings.local.json").map(|t| serde_json::from_str(&t).unwrap());

        fx.apply("gen");
        let applied: Value = serde_json::from_str(&fx.text(".claude/settings.local.json").unwrap_or_else(|| "{}".into())).unwrap();
        for key in original.iter().flat_map(|o| o.as_object().unwrap().keys()) {
            if !["skillOverrides", "enabledPlugins", "claudeMdExcludes", "permissions"].contains(&key.as_str()) {
                assert_eq!(applied[key], original.as_ref().unwrap()[key], "foreign key {key}");
            }
        }
        for (path, items) in [("/permissions/allow", true), ("/permissions/ask", true), ("/permissions/deny", true)] {
            if let Some(Value::Array(old)) = original.as_ref().and_then(|o| o.pointer(path)) {
                let now = applied.pointer(path).and_then(Value::as_array).unwrap();
                assert!(items && old.iter().all(|i| now.contains(i)), "{path}");
                assert_eq!(&now[..old.len()], &old[..], "{path} order");
            }
        }
        let status = fx.status();
        assert_eq!(status["applied"]["drift"], false, "{status}");

        let tail = rng.chance(50);
        let mut added = None;
        if tail && fx.settings().exists() {
            let mut t = fx.text(".claude/settings.local.json").unwrap();
            if json::set(&mut t, &["zzForeign"], "1").is_ok() {
                fs::write(fx.settings(), &t).unwrap();
                added = Some(t);
            }
        }
        let undone = fx.undo();
        assert_eq!(undone["conflicts"], json!([]), "{undone}");
        match added {
            None => assert_eq!(tree_snapshot(&fx.cwd), before),
            Some(_) => {
                let now: Value = serde_json::from_str(&fx.text(".claude/settings.local.json").unwrap()).unwrap();
                let mut expect = original.clone().unwrap_or_else(|| json!({}));
                expect["zzForeign"] = json!(1);
                assert_eq!(now, expect);
                if let Some(text) = fx.text(".claude/settings.local.json") {
                    let mut reference = before.get(".claude/settings.local.json").cloned().flatten().map(|b| String::from_utf8(b).unwrap()).unwrap_or_else(|| "{}\n".into());
                    json::set(&mut reference, &["zzForeign"], "1").unwrap();
                    if reference.ends_with('\n') || original.is_some() {
                        assert_eq!(text, reference);
                    }
                }
            }
        }
        assert_eq!(fx.text("CLAUDE.local.md"), md);
        assert_eq!(fx.text(".git/info/exclude").unwrap(), "# local\n");
    });
}

const CONTROLS: &str = "format: 1\nname: ctl\nplugins:\n  config:\n    ecc@ecc: {gateguard: off, hook_profile: minimal, gateguard_exempt_globs: [\"a/**\", \"b/**\"], hooks_enabled: false}\nmcp:\n  deny: [\"plugin:ecc:chrome-devtools\"]\n";

#[test]
fn a_bundle_applies_plugin_knobs_and_mcp_denials_and_undoes_them() {
    let fx = Fx::new("bundle-controls");
    put(&fx.roots.skills_repo_path().join("profiles/ctl.yaml"), CONTROLS);
    put(&fx.settings(), "{\n  \"deniedMcpServers\": [{\"serverName\": \"other\"}],\n  \"env\": {\"FOO\": \"1\", \"ECC_HOOK_PROFILE\": \"strict\"}\n}\n");
    let before = tree_snapshot(&fx.cwd);
    let plan = bundle_apply::apply(&fx.world(), "ctl", &fx.cwd, true).unwrap();
    assert_eq!(plan["plan"]["steps"][0]["keys"], json!(["env", "deniedMcpServers"]));
    assert_eq!(tree_snapshot(&fx.cwd), before);

    fx.apply("ctl");
    let v: Value = serde_json::from_str(&fx.text(".claude/settings.local.json").unwrap()).unwrap();
    assert_eq!(
        v["env"],
        json!({"FOO": "1", "ECC_HOOK_PROFILE": "minimal", "ECC_GATEGUARD": "off", "GATEGUARD_EXEMPT_GLOBS": "a/**,b/**", "ECC_HOOKS_ENABLED": "false"})
    );
    assert_eq!(v["deniedMcpServers"], json!([{"serverName": "other"}, {"serverName": "plugin:ecc:chrome-devtools"}]));
    let owned = &fx.status()["applied"]["ownedKeys"];
    assert_eq!(owned["env"], json!(["ECC_GATEGUARD", "ECC_HOOKS_ENABLED", "ECC_HOOK_PROFILE", "GATEGUARD_EXEMPT_GLOBS"]));
    assert_eq!(owned["deniedMcpServers"], json!(["plugin:ecc:chrome-devtools"]));
    assert_eq!(fx.status()["applied"]["drift"], false);

    let mut t = fx.text(".claude/settings.local.json").unwrap();
    json::set(&mut t, &["env", "ECC_GATEGUARD"], "\"on\"").unwrap();
    fs::write(fx.settings(), &t).unwrap();
    let drifted = fx.status();
    assert_eq!(drifted["applied"]["drift"], true);
    assert_eq!(drifted["applied"]["changedKeys"], json!(["env.ECC_GATEGUARD"]));
    let undone = fx.undo();
    assert_eq!(undone["conflicts"], json!(["env.ECC_GATEGUARD"]), "a changed owned key is a reported conflict");
    let left: Value = serde_json::from_str(&fx.text(".claude/settings.local.json").unwrap()).unwrap();
    assert_eq!(left["env"], json!({"FOO": "1", "ECC_HOOK_PROFILE": "strict", "ECC_GATEGUARD": "on"}));
    assert_eq!(left["deniedMcpServers"], json!([{"serverName": "other"}]));
}

#[test]
fn bundles_and_plugin_controls_share_a_folder_and_leave_in_any_order() {
    use super::bundle_controls as controls;
    for first_bundle in [true, false] {
        let fx = Fx::new("bundle-with-controls");
        put(&fx.roots.skills_repo_path().join("profiles/ctl.yaml"), CONTROLS);
        let before = tree_snapshot(&fx.cwd);
        let want = bundle_apply::Desired { deny_servers: vec!["plugin:ecc:other".into()], env: vec![("ECC_DISABLED_HOOKS".into(), "pre:x".into())], ..Default::default() };
        controls::apply(&fx.data, &fx.cwd, "config:ecc@ecc", &want, false).unwrap();
        fx.apply("ctl");
        let v: Value = serde_json::from_str(&fx.text(".claude/settings.local.json").unwrap()).unwrap();
        assert_eq!(v["env"]["ECC_DISABLED_HOOKS"], "pre:x");
        assert_eq!(v["env"]["ECC_GATEGUARD"], "off");
        if first_bundle {
            fx.undo();
            assert!(fx.text(".claude/settings.local.json").unwrap().contains("ECC_DISABLED_HOOKS"));
            controls::undo(&fx.data, &fx.cwd, "config:ecc@ecc", false).unwrap();
        } else {
            controls::undo(&fx.data, &fx.cwd, "config:ecc@ecc", false).unwrap();
            assert!(fx.text(".claude/settings.local.json").unwrap().contains("ECC_GATEGUARD"));
            fx.undo();
        }
        assert_eq!(tree_snapshot(&fx.cwd), before, "first_bundle {first_bundle}");
        assert_eq!(ledger::load(&fx.data), ledger::Ledger::default());
    }
}

#[test]
fn launch_settings_carry_the_plugin_env_and_the_denied_servers() {
    let fx = Fx::new("bundle-launch-controls");
    put(&fx.roots.skills_repo_path().join("profiles/ctl.yaml"), CONTROLS);
    let out = super::bundle_use::launch(&fx.world(), "ctl", Some(&fx.cwd)).unwrap();
    let file: Value = serde_json::from_str(&fs::read_to_string(out["settingsFile"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(file["env"]["ECC_GATEGUARD"], "off");
    assert_eq!(file["env"]["GATEGUARD_EXEMPT_GLOBS"], "a/**,b/**");
    assert_eq!(file["deniedMcpServers"], json!([{"serverName": "plugin:ecc:chrome-devtools"}]));
}

#[test]
fn lint_reports_unknown_knobs_unknown_plugins_and_bare_server_names() {
    use crate::plus::plugins::adapters::Registry;
    let text = "plugins:\n  config:\n    ecc@ecc: {nope: 1, hook_profile: wild, gateguard: off}\n    ghost@market: {a: b}\nmcp:\n  deny: [chrome-devtools, \"plugin:ecc:chrome-devtools\"]\n";
    let issues = super::bundle::lint_with("x", text, &Registry::load(None));
    let got: Vec<(&str, &str)> = issues.iter().map(|i| (i.level, i.key.as_str())).collect();
    assert_eq!(
        got,
        [("error", "plugins.config.ecc@ecc.nope"), ("error", "plugins.config.ecc@ecc.hook_profile"), ("error", "plugins.config.ghost@market"), ("error", "mcp.deny")]
    );
    assert!(issues[0].message.contains("unknown knob") && issues[0].message.contains("gateguard"));
    assert!(issues[3].message.contains("bare name does not block"));
}
