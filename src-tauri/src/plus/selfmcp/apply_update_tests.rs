use super::state_tests::{call, kind};
use super::tests::{Fixture, FAKE_SECRET};
use super::wired_tests::{git, git_identity};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Lab {
    dir: PathBuf,
    origin: PathBuf,
    dev: PathBuf,
}

impl Lab {
    fn new(fixture: &Fixture) -> Self {
        let dir = fixture.dir.clone();
        let origin = dir.join("origin.git");
        let dev = dir.join("dev");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        std::fs::create_dir_all(&dev).unwrap();
        git(&dev, &["init", "-q", "-b", "main"]);
        git_identity(&dev);
        std::fs::write(dev.join("base.txt"), "base\n").unwrap();
        git(&dev, &["add", "-A"]);
        git(&dev, &["commit", "-q", "-m", "base"]);
        git(&dev, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&dev, &["push", "-q", "-u", "origin", "main"]);
        Self { dir, origin, dev }
    }

    fn checkout(&self, name: &str) -> PathBuf {
        let path = self.dir.join(name);
        git(
            &self.dir,
            &[
                "clone",
                "-q",
                self.origin.to_str().unwrap(),
                path.to_str().unwrap(),
            ],
        );
        git_identity(&path);
        path
    }

    fn publish(&self, file: &str, message: &str) -> String {
        std::fs::write(self.dev.join(file), format!("{message}\n")).unwrap();
        git(&self.dev, &["add", "-A"]);
        git(&self.dev, &["commit", "-q", "-m", message]);
        git(&self.dev, &["push", "-q", "origin", "main"]);
        head(&self.dev)
    }
}

fn head(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"])
}

fn registry_path(fixture: &Fixture) -> PathBuf {
    fixture.dir.join("registry.json")
}

fn registry(fixture: &Fixture) -> Value {
    serde_json::from_slice(&std::fs::read(registry_path(fixture)).unwrap()).unwrap()
}

fn register(fixture: &Fixture, id: &str, source: Option<Value>) {
    let mut reg = registry(fixture);
    let mut entry = json!({
        "id": id, "name": id.trim_start_matches("srv-"), "transport": "stdio",
        "command": format!("{id}-mcp"), "args": [],
        "env": [{"key": "TOKEN", "value": FAKE_SECRET, "secret": true}],
    });
    if let Some(source) = source {
        entry["mcpmSource"] = source;
    }
    reg["servers"].as_array_mut().unwrap().push(entry);
    std::fs::write(registry_path(fixture), reg.to_string()).unwrap();
}

fn git_source(path: &Path, post_update: Option<&str>) -> Value {
    let mut source = json!({"type": "git", "path": path.to_string_lossy(), "branch": "main"});
    if let Some(command) = post_update {
        source["post_update"] = json!(command);
    }
    source
}

fn server<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("{id} missing from {report}"))
}

fn without_update_stamp(mut reg: Value, id: &str) -> Value {
    let entry = reg["servers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["id"] == id)
        .unwrap();
    entry["mcpmSource"].as_object_mut().unwrap().remove("last_updated");
    reg
}

fn apply(name: &str) -> Result<Value, super::ToolError> {
    call("servers_apply_update", json!({"name": name, "confirm": true}))
}

#[test]
fn applying_an_update_fast_forwards_the_named_checkout_and_writes_only_its_entry() {
    let fixture = Fixture::new("apply-update");
    let lab = Lab::new(&fixture);
    let (up, other) = (lab.checkout("up"), lab.checkout("other"));
    register(&fixture, "srv-up", Some(git_source(&up, None)));
    register(&fixture, "srv-other", Some(git_source(&other, None)));
    let (base_up, base_other) = (head(&up), head(&other));
    let tip = lab.publish("feature.txt", "add feature");
    assert_ne!(tip, base_up);
    crate::registry::update(|_| Ok(())).unwrap();
    let before = registry(&fixture);

    let preview = call("servers_check_updates", json!({"name": "up"})).unwrap();
    assert_eq!(preview["mode"], "check");
    let planned = server(&preview, "srv-up");
    assert_eq!(planned["status"], "update-available");
    assert_eq!(planned["behind"], 1);
    assert_eq!(planned["plan"], json!(["git merge --ff-only origin/main"]));
    assert_eq!(head(&up), base_up, "a check never moves the checkout");
    assert_eq!(registry(&fixture), before, "a check never writes the registry");

    let refused = call("servers_apply_update", json!({"name": "up"}));
    assert_eq!(kind(refused), "refused");
    assert_eq!(head(&up), base_up);
    assert_eq!(registry(&fixture), before);

    let done = apply("up").unwrap();
    assert_eq!(done["mode"], "apply");
    assert_eq!(done["servers"].as_array().unwrap().len(), 1);
    let report = server(&done, "srv-up");
    assert_eq!(report["status"], "updated");
    assert_eq!(report["message"], "updated (1 new commit(s))");
    assert_eq!(
        report["steps"],
        json!([{"name": "git merge --ff-only", "ok": true, "detail": "1 new commit(s)"}])
    );
    assert_eq!(head(&up), tip);
    assert!(up.join("feature.txt").is_file());
    assert_eq!(head(&other), base_other, "no other checkout is touched");
    assert!(!other.join("feature.txt").exists());

    let after = registry(&fixture);
    let stamped = after["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "srv-up")
        .unwrap()["mcpmSource"]["last_updated"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!stamped.is_empty());
    assert_eq!(
        without_update_stamp(after, "srv-up"),
        before,
        "only the applied server's source stamp changes"
    );

    let written = std::fs::read(registry_path(&fixture)).unwrap();
    let again = apply("up").unwrap();
    assert_eq!(server(&again, "srv-up")["status"], "up-to-date");
    assert_eq!(
        std::fs::read(registry_path(&fixture)).unwrap(),
        written,
        "nothing to apply writes nothing"
    );

    let all = call("servers_check_updates", json!({})).unwrap();
    assert_eq!(server(&all, "srv-up")["status"], "up-to-date");
    assert_eq!(server(&all, "srv-other")["status"], "update-available");
    for text in [&preview, &done, &again, &all].map(Value::to_string) {
        assert!(!text.contains(FAKE_SECRET), "{text}");
    }
    assert!(registry(&fixture).to_string().contains(FAKE_SECRET));
}

#[test]
fn a_dirty_or_diverged_checkout_is_skipped_and_left_exactly_as_it_was() {
    let fixture = Fixture::new("apply-update-skip");
    let lab = Lab::new(&fixture);
    let (dirty, diverged) = (lab.checkout("dirty"), lab.checkout("diverged"));
    register(&fixture, "srv-dirty", Some(git_source(&dirty, None)));
    register(&fixture, "srv-diverged", Some(git_source(&diverged, None)));
    std::fs::write(dirty.join("wip.txt"), "unsaved work\n").unwrap();
    std::fs::write(diverged.join("local.txt"), "local\n").unwrap();
    git(&diverged, &["add", "-A"]);
    git(&diverged, &["commit", "-q", "-m", "local change"]);
    lab.publish("upstream.txt", "upstream change");
    let heads = (head(&dirty), head(&diverged));
    let before = std::fs::read(registry_path(&fixture)).unwrap();

    let skipped = apply("dirty").unwrap();
    let report = server(&skipped, "srv-dirty");
    assert_eq!(report["status"], "skipped");
    assert_eq!(report["message"], "uncommitted changes");
    let skipped = apply("diverged").unwrap();
    let report = server(&skipped, "srv-diverged");
    assert_eq!(report["status"], "skipped");
    assert_eq!(
        report["message"],
        "cannot fast-forward: local and remote have diverged"
    );

    assert_eq!((head(&dirty), head(&diverged)), heads);
    assert_eq!(
        std::fs::read_to_string(dirty.join("wip.txt")).unwrap(),
        "unsaved work\n"
    );
    assert!(!diverged.join("upstream.txt").exists());
    assert_eq!(std::fs::read(registry_path(&fixture)).unwrap(), before);
}

#[test]
fn bad_names_and_broken_sources_are_refused_or_reported_without_writing() {
    let fixture = Fixture::new("apply-update-refuse");
    let missing = fixture.dir.join("missing-checkout");
    let not_git = fixture.dir.join("plain-dir");
    std::fs::create_dir_all(&not_git).unwrap();
    register(&fixture, "srv-gone", Some(git_source(&missing, None)));
    register(&fixture, "srv-plain", Some(git_source(&not_git, None)));
    let mut reg = registry(&fixture);
    reg["servers"].as_array_mut().unwrap().push(json!({
        "id": "srv-pin", "name": "pin", "transport": "stdio", "command": "npx",
        "args": ["-y", "synthetic-unpinned-package"]
    }));
    std::fs::write(registry_path(&fixture), reg.to_string()).unwrap();
    let before = std::fs::read(registry_path(&fixture)).unwrap();

    for bad in ["", "   ", "-x", "--force"] {
        assert_eq!(kind(apply(bad)), "invalid_arguments", "{bad:?}");
    }
    assert_eq!(
        kind(call("servers_apply_update", json!({"confirm": true}))),
        "invalid_arguments"
    );
    assert_eq!(
        kind(call("servers_apply_update", json!({"name": 3, "confirm": true}))),
        "invalid_arguments"
    );
    assert_eq!(
        kind(call(
            "servers_apply_update",
            json!({"name": "gone", "confirm": true, "unknown_flag": true})
        )),
        "invalid_arguments"
    );
    for ghost in ["ghost", "gone/../plain", "SRV-GONE"] {
        assert_eq!(kind(apply(ghost)), "not_found", "{ghost}");
    }

    let gone = apply("gone").unwrap();
    let report = server(&gone, "srv-gone");
    assert_eq!(report["status"], "error");
    assert!(
        report["message"].as_str().unwrap().starts_with("path not found: "),
        "{report}"
    );
    let plain = apply("plain").unwrap();
    assert_eq!(server(&plain, "srv-plain")["status"], "error");
    assert_eq!(server(&plain, "srv-plain")["message"], "not a git repository");
    let pinned = apply("pin").unwrap();
    assert_eq!(server(&pinned, "srv-pin")["status"], "auto");
    assert_eq!(
        server(&pinned, "srv-pin")["message"],
        "unpinned: synthetic-unpinned-package resolves at runtime [no stored source; run update --init]"
    );
    let remote = apply("beta").unwrap();
    assert_eq!(server(&remote, "srv-beta")["status"], "skipped");
    let unknown = apply("alpha").unwrap();
    assert_eq!(server(&unknown, "srv-alpha")["status"], "skipped");
    assert!(server(&unknown, "srv-alpha")["message"]
        .as_str()
        .unwrap()
        .starts_with("unknown source"));

    assert_eq!(std::fs::read(registry_path(&fixture)).unwrap(), before);
    for text in [&gone, &plain, &pinned, &remote, &unknown].map(Value::to_string) {
        assert!(!text.contains(FAKE_SECRET), "{text}");
    }
}

#[test]
fn the_post_update_command_runs_on_apply_only_and_a_failing_one_is_reported() {
    let fixture = Fixture::new("apply-update-hook");
    let lab = Lab::new(&fixture);
    let (hooked, broken) = (lab.checkout("hooked"), lab.checkout("broken"));
    register(
        &fixture,
        "srv-hooked",
        Some(git_source(&hooked, Some("git rev-parse HEAD > post-update.txt"))),
    );
    register(
        &fixture,
        "srv-broken",
        Some(git_source(&broken, Some("exit 3"))),
    );
    let tip = lab.publish("feature.txt", "add feature");

    let preview = call("servers_check_updates", json!({"name": "hooked"})).unwrap();
    assert_eq!(
        server(&preview, "srv-hooked")["plan"],
        json!([
            "git merge --ff-only origin/main",
            "post_update: git rev-parse HEAD > post-update.txt (not run without --allow-commands)"
        ])
    );
    assert!(!hooked.join("post-update.txt").exists());
    assert_eq!(head(&hooked), head(&broken));
    assert_ne!(head(&hooked), tip);

    let done = apply("hooked").unwrap();
    let report = server(&done, "srv-hooked");
    assert_eq!(report["status"], "updated");
    let steps = report["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[1]["name"], "post_update");
    assert_eq!(steps[1]["ok"], true);
    assert_eq!(head(&hooked), tip);
    assert_eq!(
        std::fs::read_to_string(hooked.join("post-update.txt"))
            .unwrap()
            .trim(),
        tip,
        "the hook runs in the checkout after the fast-forward"
    );

    let failed = apply("broken").unwrap();
    let report = server(&failed, "srv-broken");
    assert_eq!(report["status"], "error");
    assert!(
        report["message"]
            .as_str()
            .unwrap()
            .starts_with("git updated but post_update failed; run manually: cd "),
        "{report}"
    );
    let steps = report["steps"].as_array().unwrap();
    assert_eq!(steps[0]["ok"], true);
    assert_eq!(steps[1]["name"], "post_update");
    assert_eq!(steps[1]["ok"], false);
    assert!(steps[1]["detail"].as_str().unwrap().contains("failed (exit 3)"));
    assert_eq!(head(&broken), tip, "the fast-forward itself is kept");
}
