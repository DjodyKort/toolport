use super::run_with;
use crate::plus::skills::tap_fixtures::Remotes;
use crate::plus::skills::tap_handlers::TEST_GIT;
use crate::plus::testutil::{tree_snapshot, DataDirFx};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

const REMOTES: [&str; 3] = ["acme/skills", "acme/audit", "acme/extras"];
const NEW_ONE: &str = "---\nname: new-one\ndescription: Newly added\n---\nbody\n";

struct Fx {
    _base: DataDirFx,
    root: PathBuf,
    remotes: Remotes,
}

impl Fx {
    /// The GitHub URLs a `user/repo` tap expands to are served from local bare repositories.
    fn new(tag: &str) -> Self {
        let fx = Self::real(tag);
        TEST_GIT.with(|g| *g.borrow_mut() = Some(Rc::new(fx.remotes.rewrite())));
        fx
    }

    /// Leaves git alone, so `SystemGit` runs against `file://` remotes.
    fn real(tag: &str) -> Self {
        let base = DataDirFx::with_data_subdir("ctl-taps", tag, "data");
        let root = base.dir.canonicalize().unwrap();
        let remotes = Remotes::new(&root.join("_infra"));
        for repo in REMOTES {
            remotes.publish(repo);
        }
        Self {
            _base: base,
            root,
            remotes,
        }
    }

    fn arg(&self, rel: &str) -> String {
        self.root.join(rel).to_string_lossy().into_owned()
    }

    fn snapshot(&self) -> BTreeMap<String, Option<Vec<u8>>> {
        tree_snapshot(&self.root)
    }

    fn ok(&self, list: &[&str]) -> String {
        let (code, out, err) = run(list);
        assert_eq!(code, 0, "{list:?}: {out}{err}");
        out
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        TEST_GIT.with(|g| *g.borrow_mut() = None);
    }
}

fn run(list: &[&str]) -> (i32, String, String) {
    let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&list, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn cli(list: &[&str]) -> (i32, Value) {
    let mut full = vec!["--json"];
    full.extend_from_slice(list);
    let (code, out, _) = run(&full);
    (
        code,
        serde_json::from_str(out.trim()).unwrap_or(Value::Null),
    )
}

#[test]
fn every_subcommand_reports_usage_errors_with_exit_two() {
    let _fx = Fx::new("usage");
    let cases: &[(&[&str], &str)] = &[
        (&["skills", "tap"], "usage: skills tap add|ls|remove|update"),
        (
            &["skills", "tap", "bogus"],
            "usage: skills tap add|ls|remove|update",
        ),
        (&["skills", "tap", "add"], "missing tap source"),
        (
            &["skills", "tap", "add", "a/b", "c/d"],
            "unexpected argument: c/d",
        ),
        (&["skills", "tap", "add", "a/b", "--name"], "--name"),
        (&["skills", "tap", "ls", "x"], "unexpected argument: x"),
        (&["skills", "tap", "remove"], "missing tap name"),
        (
            &["skills", "tap", "update", "a", "b"],
            "unexpected argument: b",
        ),
        (&["skills", "search"], "missing query"),
        (&["skills", "search", "a", "b"], "unexpected argument: b"),
        (&["skills", "install"], "missing install spec"),
        (&["skills", "install", "@a/b", "--bogus"], "--bogus"),
    ];
    for (list, needle) in cases {
        let (code, out, err) = run(list);
        assert_eq!(code, 2, "{list:?}: {out}{err}");
        assert!(err.contains(needle), "{list:?}: {err}");
        assert!(out.is_empty(), "{list:?}: {out}");
    }
}

#[test]
fn the_skills_group_usage_lists_the_new_commands() {
    let (code, _, err) = run(&["skills"]);
    assert_eq!(code, 2);
    assert!(err.contains("tap"), "{err}");
    assert!(err.contains("search"), "{err}");
    assert!(err.contains("install"), "{err}");
}

#[test]
fn json_envelopes_carry_the_handler_data() {
    let fx = Fx::new("json");
    let target = fx.arg("target");

    let (code, add) = cli(&["skills", "tap", "add", "acme/skills"]);
    assert_eq!(code, 0, "{add}");
    assert_eq!(add["ok"], json!(true));
    assert_eq!(add["command"], json!("skills tap add"));
    let data = &add["data"];
    assert_eq!(data["name"], json!("acme-skills"));
    assert_eq!(data["repo"], json!("acme/skills"));
    assert_eq!(data["cloned"], json!(true));
    assert_eq!(data["dryRun"], json!(false));
    assert_eq!(data["url"], json!("https://github.com/acme/skills.git"));
    assert!(data["head"].as_str().unwrap().len() >= 7, "{data}");

    let (_, ls) = cli(&["skills", "tap", "ls"]);
    assert_eq!(ls["data"]["taps"][0]["name"], json!("acme-skills"));
    assert_eq!(ls["data"]["taps"][0]["cloned"], json!(true));
    assert!(ls["data"]["tapsRoot"].as_str().unwrap().ends_with("taps"));

    let (_, search) = cli(&["skills", "search", "terraform"]);
    assert_eq!(search["data"]["tapCount"], json!(1));
    let hit = &search["data"]["results"][0];
    assert_eq!(hit["name"], json!("terraform-helper"));
    assert_eq!(hit["tap"], json!("acme-skills"));
    assert_eq!(hit["type"], json!("skill"));

    let (_, update) = cli(&["skills", "tap", "update"]);
    assert_eq!(update["data"]["failed"], json!(0));
    assert_eq!(update["data"]["results"][0]["ok"], json!(true));

    let (code, install) = cli(&["skills", "install", "@acme/skills", "--path", &target]);
    assert_eq!(code, 0, "{install}");
    let data = &install["data"];
    assert_eq!(data["installedCount"], json!(3));
    assert_eq!(data["skippedCount"], json!(0));
    assert_eq!(data["foundCount"], json!(3));
    assert_eq!(data["blocked"], json!(false));
    assert_eq!(data["tapAdded"], json!(false));
    assert_eq!(data["audit"]["ran"], json!(true));
    let names: Vec<&str> = data["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["code-review", "terraform-helper", "style-guide"]);

    let (code, remove) = cli(&["skills", "tap", "remove", "acme-skills"]);
    assert_eq!(code, 0);
    assert_eq!(remove["data"]["removed"], json!(true));
    assert_eq!(remove["data"]["hadClone"], json!(true));
}

#[test]
fn a_blocked_install_is_an_unhealthy_exit_with_the_findings() {
    let fx = Fx::new("blocked");
    let target = fx.arg("target");
    let (code, out, _) = run(&["skills", "install", "@acme/audit", "--path", &target]);
    assert_eq!(code, 1);
    assert!(
        out.contains("Security audit found high-severity issues"),
        "{out}"
    );
    assert!(out.contains("--no-audit"), "{out}");
    assert!(!fx.root.join("target").exists());

    let (code, json) = cli(&["skills", "install", "@acme/audit", "--path", &target]);
    assert_eq!(code, 1);
    assert_eq!(json["ok"], json!(false));
    assert_eq!(json["data"]["blocked"], json!(true));
    assert!(json["data"]["audit"]["high"].as_u64().unwrap() >= 1);
    assert_eq!(json["data"]["installedCount"], json!(0));
    assert!(!fx.root.join("target").exists());
}

#[test]
fn dry_runs_leave_the_tree_unchanged() {
    let fx = Fx::new("dry");
    let target = fx.arg("target");

    let before = fx.snapshot();
    let out = fx.ok(&["skills", "tap", "add", "acme/skills", "--dry-run"]);
    assert!(out.contains("Would clone acme/skills into"), "{out}");
    assert!(out.contains("Tap 'acme-skills' would be added."), "{out}");
    assert_eq!(fx.snapshot(), before);

    let out = fx.ok(&[
        "skills",
        "install",
        "@acme/skills",
        "--dry-run",
        "--path",
        &target,
    ]);
    assert!(
        out.contains("Tap 'acme-skills' is not registered; would clone https://github.com/acme/skills.git first."),
        "{out}"
    );
    assert!(out.contains("Nothing was written."), "{out}");
    assert_eq!(fx.snapshot(), before);

    fx.ok(&["skills", "tap", "add", "acme/skills"]);
    fx.remotes
        .commit_file("acme/skills", "skills/new-one/SKILL.md", NEW_ONE);
    let registered = fx.snapshot();

    let out = fx.ok(&["skills", "tap", "update", "--dry-run"]);
    assert!(out.contains("  Would update acme-skills"), "{out}");
    assert_eq!(fx.snapshot(), registered);

    let out = fx.ok(&["skills", "tap", "remove", "acme-skills", "--dry-run"]);
    assert!(
        out.contains("Would remove tap 'acme-skills' and delete "),
        "{out}"
    );
    assert_eq!(fx.snapshot(), registered);

    let out = fx.ok(&[
        "skills",
        "install",
        "@acme/skills",
        "--dry-run",
        "--path",
        &target,
    ]);
    assert!(out.contains("Would install code-review (skill)"), "{out}");
    assert!(out.contains("Would install 3 skill(s) into "), "{out}");
    assert!(!out.contains("new-one"), "the update was dry: {out}");
    assert_eq!(fx.snapshot(), registered);
    assert!(!fx.root.join("target").exists());

    let (code, json) = cli(&[
        "skills",
        "install",
        "@acme/skills",
        "--dry-run",
        "--path",
        &target,
    ]);
    assert_eq!(code, 0);
    assert_eq!(json["data"]["dryRun"], json!(true));
    assert_eq!(json["data"]["installedCount"], json!(3));
    assert_eq!(fx.snapshot(), registered);

    fx.ok(&["skills", "install", "@acme/skills", "--path", &target]);
    let installed = fx.snapshot();
    let out = fx.ok(&[
        "skills",
        "install",
        "@acme/skills",
        "--dry-run",
        "--path",
        &target,
    ]);
    assert!(
        out.contains("Would skip code-review (already exists)"),
        "{out}"
    );
    assert!(out.contains("No new skills would be installed."), "{out}");
    assert_eq!(fx.snapshot(), installed);
}

#[test]
fn a_malicious_tap_name_is_rejected_before_anything_is_written() {
    let fx = Fx::new("names");
    let before = fx.snapshot();
    let names = [
        "../x", "../../x", "..", "/tmp/x", "/", "a/b", "a\\b", "x\0y", ".hidden", "-rf", "a b",
        "C:\\x", "x/../y",
    ];
    for name in names {
        for extra in [&[][..], &["--dry-run"][..]] {
            let mut list = vec!["skills", "tap", "add", "acme/skills", "--name", name];
            list.extend_from_slice(extra);
            let (code, out, err) = run(&list);
            assert_eq!(code, 1, "add {name:?} {extra:?}: {out}{err}");
            assert!(err.starts_with("toolportctl: "), "{name:?}: {err}");
            assert!(out.is_empty(), "{name:?}: {out}");
        }
        for verb in ["remove", "update"] {
            let (code, out, err) = run(&["skills", "tap", verb, name]);
            assert!(code == 1 || code == 2, "{verb} {name:?}: {out}{err}");
            assert!(err.starts_with("toolportctl: "), "{verb} {name:?}: {err}");
        }
    }
    assert_eq!(fx.snapshot(), before);
    assert!(!fx.root.join("x").exists());
}

#[test]
fn a_hostile_tap_source_is_rejected() {
    let fx = Fx::new("sources");
    let before = fx.snapshot();
    let sources = [
        "http://example.com/a/b.git",
        "--upload-pack=touch /tmp/pwned",
        "ext::sh -c touch% /tmp/pwned",
        "https://user:secret@example.com/a/b.git",
        "relative/path/x.git",
        "../escape",
        "a/b\0c",
    ];
    for source in sources {
        let (code, out, err) = run(&["skills", "tap", "add", source]);
        assert_ne!(code, 0, "{source:?}: {out}{err}");
        assert!(out.is_empty(), "{source:?}: {out}");
        assert!(!err.contains("secret"), "{source:?}: {err}");
    }
    assert_eq!(fx.snapshot(), before);
}

#[test]
fn a_skill_named_with_a_traversal_is_never_installed() {
    let fx = Fx::new("escape");
    let target = fx.arg("target");
    fx.ok(&["skills", "tap", "add", "acme/skills"]);

    let out = fx.ok(&["skills", "install", "@acme/skills", "--path", &target]);
    assert!(out.contains("Installed 3 skill(s)"), "{out}");
    assert!(out.contains("Failed to parse"), "{out}");
    assert!(out.contains("escape/SKILL.md"), "{out}");
    assert!(!fx.root.join("target/escape").exists());
    assert!(!fx.root.join("escape").exists());
    let files: Vec<String> = fx
        .snapshot()
        .into_iter()
        .filter(|(p, c)| c.is_some() && p.starts_with("target/"))
        .map(|(p, _)| p)
        .collect();
    assert!(files.iter().all(|p| !p.contains("escape")), "{files:?}");

    let (code, _, err) = run(&[
        "skills",
        "install",
        "@acme/skills/escape",
        "--path",
        &target,
    ]);
    assert_eq!(code, 1);
    assert!(
        err.contains("No skills found for '@acme/skills/escape'"),
        "{err}"
    );

    for spec in [
        "@acme/skills/../escape",
        "@acme/skills/..",
        "@../skills/escape",
        "@acme/../skills",
        "@acme/skills/a/b",
        "@acme/skills/Bad_Name",
        "@/skills",
        "@acme",
    ] {
        let (code, out, err) = run(&["skills", "install", spec, "--dry-run", "--path", &target]);
        assert_eq!(code, 1, "{spec}: {out}{err}");
        assert!(err.contains("invalid"), "{spec}: {err}");
    }
    assert!(!fx.root.join("escape").exists());
}

#[test]
fn hints_name_toolportctl_not_mcpm() {
    let fx = Fx::new("hints");
    let target = fx.arg("target");
    let mut texts = vec![
        fx.ok(&["skills", "tap", "ls"]),
        fx.ok(&["skills", "tap", "update"]),
        fx.ok(&["skills", "search", "anything"]),
    ];
    fx.ok(&["skills", "tap", "add", "acme/skills"]);
    texts.push(fx.ok(&["skills", "search", "review"]));
    texts.push(fx.ok(&["skills", "search", "no-such-skill-anywhere"]));
    texts.push(fx.ok(&["skills", "install", "@acme/skills", "--path", &target]));
    texts.push(fx.ok(&["skills", "install", "@acme/skills", "--path", &target]));
    for text in &texts {
        assert!(!text.contains("mcpm"), "{text}");
    }
    assert!(
        texts[0].contains("toolportctl skills tap add user/repo"),
        "{}",
        texts[0]
    );
    assert!(
        texts[3].contains("toolportctl skills install @<repo>/<skill-name>"),
        "{}",
        texts[3]
    );
    assert!(texts[5].contains("toolportctl skills sync"), "{}", texts[5]);
}

#[test]
fn a_file_url_tap_runs_through_the_system_git() {
    let fx = Fx::real("system-git");
    let target = fx.arg("target");
    let url = fx.remotes.url("acme/extras");

    let out = fx.ok(&["skills", "tap", "add", &url, "--name", "acme-extras"]);
    assert!(
        out.contains("Tap 'acme-extras' added successfully."),
        "{out}"
    );
    let (_, ls) = cli(&["skills", "tap", "ls"]);
    assert_eq!(ls["data"]["taps"][0]["name"], json!("acme-extras"));
    assert_eq!(ls["data"]["taps"][0]["url"], json!(url));
    assert_eq!(ls["data"]["taps"][0]["cloned"], json!(true));
    let clone = PathBuf::from(ls["data"]["taps"][0]["path"].as_str().unwrap());
    assert!(clone.join(".git").exists());
    assert!(clone.join("skills/release-notes/SKILL.md").exists());

    let (_, first) = cli(&["skills", "tap", "update"]);
    let head = first["data"]["results"][0]["head"]
        .as_str()
        .unwrap()
        .to_string();
    fx.remotes
        .commit_file("acme/extras", "skills/new-one/SKILL.md", NEW_ONE);
    let (code, second) = cli(&["skills", "tap", "update", "acme-extras"]);
    assert_eq!(code, 0, "{second}");
    assert_ne!(second["data"]["results"][0]["head"].as_str().unwrap(), head);
    assert!(clone.join("skills/new-one/SKILL.md").exists());

    let out = fx.ok(&["skills", "search", "newly"]);
    assert!(out.contains("new-one"), "{out}");
    assert!(out.contains("1 result(s)."), "{out}");

    let out = fx.ok(&[
        "skills",
        "install",
        "@acme/extras/new-one",
        "--path",
        &target,
    ]);
    assert!(out.contains("Installed new-one (skill)"), "{out}");
    assert!(fx.root.join("target/skills/new-one/SKILL.md").exists());

    let out = fx.ok(&["skills", "tap", "remove", "acme-extras"]);
    assert!(out.contains("Tap 'acme-extras' removed."), "{out}");
    assert!(!clone.exists());
}

#[test]
fn a_clone_that_fails_leaves_no_tap_and_no_directory() {
    let fx = Fx::real("clone-fails");
    let before = fx.snapshot();
    let gone = format!("file://{}", fx.arg("_infra/remotes/acme/nothing.git"));
    let (code, out, err) = run(&["skills", "tap", "add", &gone, "--name", "gone"]);
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        err.starts_with("toolportctl: Failed to clone file://"),
        "{err}"
    );
    let after = fx.snapshot();
    let changed: Vec<&String> = after.keys().filter(|k| !before.contains_key(*k)).collect();
    assert_eq!(
        changed,
        ["data/taps.json.lock", "data/taps/"],
        "{changed:?}"
    );
    assert!(before.keys().all(|k| after.contains_key(k)));
    assert!(after
        .keys()
        .all(|k| !k.starts_with("data/taps/") || k == "data/taps/"));
}

#[test]
fn the_plus_skills_routes_take_snake_case_arguments_and_return_camel_case() {
    let _fx = Fx::new("ipc");
    let call =
        |name: &str, args: Value| crate::plus::dispatch(&format!("plus.skills.{name}"), args);

    let listed = call("tapList", json!({})).unwrap();
    assert_eq!(listed["taps"], json!([]));
    let dry = call("tapAdd", json!({"repo": "acme/skills", "dry_run": true})).unwrap();
    assert_eq!(dry["dryRun"], json!(true));
    assert_eq!(dry["cloned"], json!(false));
    assert_eq!(call("tapList", json!({})).unwrap()["taps"], json!([]));

    let added = call("tapAdd", json!({"repo": "acme/skills"})).unwrap();
    assert_eq!(added["name"], json!("acme-skills"));
    assert_eq!(added["cloned"], json!(true));
    assert_eq!(
        call("tapAdd", json!({"repo": "acme/skills"})).unwrap_err(),
        "Tap 'acme-skills' already exists"
    );
    assert_eq!(call("tapAdd", json!({})).unwrap_err(), "repo is required");
    assert_eq!(
        call("tapRemove", json!({"name": "nope"})).unwrap_err(),
        "Tap 'nope' not found."
    );
    assert!(call("tapRemove", json!({"name": "../x"})).is_err());

    let found = call("search", json!({"query": "REVIEW"})).unwrap();
    assert_eq!(found["results"][0]["name"], json!("code-review"));
    assert_eq!(found["discoveryWarnings"].as_array().unwrap().len(), 1);
    assert_eq!(call("search", json!({})).unwrap_err(), "query is required");

    let planned = call(
        "install",
        json!({"spec": "@acme/skills/code-review", "dry_run": true}),
    )
    .unwrap();
    assert_eq!(planned["installedCount"], json!(1));
    assert_eq!(planned["skills"][0]["status"], json!("installed"));

    let updated = call("tapUpdate", json!({})).unwrap();
    assert_eq!(updated["failed"], json!(0));
    let removed = call("tapRemove", json!({"name": "acme-skills"})).unwrap();
    assert_eq!(removed["removed"], json!(true));
}
