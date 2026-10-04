use super::state_tests::{call, kind};
use crate::plus::testutil::tree_snapshot;
use super::tests::Fixture;
use super::wired_tests::git;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const PASSPHRASE: &str = "synthetic passphrase for the sync tool";

fn claude_file(fixture: &Fixture) -> PathBuf {
    fixture.home.join(".claude.json")
}

fn claude_doc(fixture: &Fixture) -> Value {
    serde_json::from_slice(&std::fs::read(claude_file(fixture)).unwrap()).unwrap()
}

fn claude_servers(fixture: &Fixture) -> Vec<String> {
    let mut names: Vec<String> = claude_doc(fixture)["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

fn seed_claude(fixture: &Fixture, servers: Value) {
    let doc = json!({"theme": "FAKE-theme", "mcpServers": servers});
    std::fs::write(claude_file(fixture), serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
}

fn names(row: &Value, key: &str) -> Vec<String> {
    row[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().or(v.as_str()).unwrap().to_string())
        .collect()
}

#[test]
fn clients_sync_writes_the_gateway_entry_and_prunes_orphans_only_when_applied() {
    let fixture = Fixture::new("effect-clients-sync");
    let _config_dir = crate::clients::EnvRestore::set("CLAUDE_CONFIG_DIR", Path::new(""));
    seed_claude(&fixture, json!({"stray": {"command": "stray-mcp"}}));
    let seeded = std::fs::read(claude_file(&fixture)).unwrap();
    let backups = fixture.dir.join("backups");

    assert_eq!(
        kind(call("clients_sync", json!({"client": "claude-code"}))),
        "refused"
    );
    assert_eq!(
        kind(call("clients_sync", json!({"client": "--all", "dry_run": true}))),
        "invalid_arguments"
    );
    assert_eq!(std::fs::read(claude_file(&fixture)).unwrap(), seeded);

    let planned = call(
        "clients_sync",
        json!({"client": "claude-code", "dry_run": true}),
    )
    .unwrap();
    assert_eq!(planned["dryRun"], true);
    let row = &planned["clients"][0];
    assert_eq!(row["client"], "claude-code");
    assert_eq!(row["gateway"], "would-install");
    assert_eq!(names(row, "removed"), ["stray"]);
    assert_eq!(row["removed"][0]["reason"], "orphan");
    assert_eq!(std::fs::read(claude_file(&fixture)).unwrap(), seeded);
    assert!(!backups.exists(), "a dry run writes no backup");

    let applied = call(
        "clients_sync",
        json!({"client": "claude-code", "confirm": true}),
    )
    .unwrap();
    assert_eq!(applied["dryRun"], false);
    let row = &applied["clients"][0];
    assert_eq!(row["gateway"], "installed");
    assert_eq!(names(row, "removed"), ["stray"]);
    assert_eq!(claude_servers(&fixture), ["toolport"]);
    assert_eq!(claude_doc(&fixture)["theme"], "FAKE-theme");
    let written = names(row, "backups");
    assert!(!written.is_empty());
    assert!(written.iter().all(|path| Path::new(path).is_file()), "{written:?}");
    assert!(backups.is_dir());

    let mut doc = claude_doc(&fixture);
    doc["mcpServers"]["second-stray"] = json!({"command": "stray-mcp"});
    std::fs::write(claude_file(&fixture), serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    let kept = call(
        "clients_sync",
        json!({"client": "claude-code", "keep_orphans": true, "confirm": true}),
    )
    .unwrap();
    assert_eq!(names(&kept["clients"][0], "kept"), ["second-stray"]);
    assert_eq!(claude_servers(&fixture), ["second-stray", "toolport"]);
    call(
        "clients_sync",
        json!({"client": "claude-code", "confirm": true}),
    )
    .unwrap();
    assert_eq!(claude_servers(&fixture), ["toolport"]);

    let again = std::fs::read(claude_file(&fixture)).unwrap();
    call("clients_sync", json!({"client": "claude-code", "confirm": true})).unwrap();
    assert_eq!(
        std::fs::read(claude_file(&fixture)).unwrap(),
        again,
        "a second sync changes nothing"
    );
}

fn remote_commits(remote: &Path) -> String {
    git(remote, &["rev-list", "--all", "--count"])
}

#[test]
fn sync_push_commits_an_encrypted_bundle_only_when_confirmed() {
    let fixture = Fixture::new("effect-sync-push");
    let remote = fixture.dir.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    let skill = fixture.dir.join("skills_repo/skills/synthetic/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(&skill, "---\nname: synthetic\n---\nSYNTHETIC-PLAINTEXT-BODY\n").unwrap();
    crate::plus::sync::handlers::init_handler(json!({
        "repo": remote.to_string_lossy(),
        "passphrase": PASSPHRASE,
        "machineId": "tool-test",
    }))
    .unwrap();
    assert_eq!(remote_commits(&remote), "0");

    assert_eq!(kind(call("sync_push", json!({}))), "refused");
    assert_eq!(kind(call("sync_push", json!({"dry_run": false}))), "refused");
    assert_eq!(remote_commits(&remote), "0");

    let planned = call("sync_push", json!({"dry_run": true})).unwrap();
    assert_eq!(planned["dryRun"], true);
    assert_eq!(planned["pushed"], false);
    assert_eq!(planned["machineId"], "tool-test");
    assert!(
        planned["entries"]
            .as_array()
            .unwrap()
            .contains(&json!("skills_repo/skills/synthetic/SKILL.md")),
        "{planned}"
    );
    assert_eq!(remote_commits(&remote), "0", "a dry run pushes nothing");

    let pushed = call("sync_push", json!({"confirm": true})).unwrap();
    assert_eq!(pushed["pushed"], true);
    assert_eq!(pushed["committed"], true);
    assert_eq!(remote_commits(&remote), "1");
    let subject = git(&remote, &["log", "-1", "--format=%s", "main"]);
    assert!(subject.starts_with("sync push from tool-test"), "{subject}");
    let files = git(&remote, &["ls-tree", "-r", "--name-only", "main"]);
    assert!(files.contains("sync_manifest.json") && files.contains("salt.txt"), "{files}");

    let clone = fixture.dir.join("verify-clone");
    git(
        &fixture.dir,
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let mut stored = String::new();
    for rel in git(&clone, &["ls-files"]).lines() {
        stored.push_str(&String::from_utf8_lossy(&std::fs::read(clone.join(rel)).unwrap()));
    }
    for secret in ["SYNTHETIC-PLAINTEXT-BODY", PASSPHRASE] {
        assert!(!stored.contains(secret), "the remote holds {secret}");
    }
    assert!(!pushed.to_string().contains(PASSPHRASE));

    let again = call("sync_push", json!({"confirm": true})).unwrap();
    assert_eq!(again["committed"], false, "nothing changed, nothing committed");
    assert_eq!(remote_commits(&remote), "1");

    std::fs::write(&skill, "---\nname: synthetic\n---\nCHANGED-BODY\n").unwrap();
    let changed = call("sync_push", json!({"confirm": true})).unwrap();
    assert_eq!(changed["committed"], true);
    assert_eq!(remote_commits(&remote), "2");
}

#[test]
fn agents_and_styles_sync_write_nothing_on_a_dry_run_and_stay_inside_the_named_client() {
    let fixture = Fixture::new("effect-sync-dry");
    let before = tree_snapshot(&fixture.dir);
    let tools = [
        ("agents_sync", "agentCount"),
        ("styles_sync_tier1", "styleCount"),
    ];
    for (tool, count) in tools {
        let planned = call(tool, json!({"client_keys": ["claude-code"], "dry_run": true})).unwrap();
        assert_eq!(planned["dryRun"], true, "{tool}");
        assert_eq!(planned[count], 1, "{tool}");
        assert_eq!(tree_snapshot(&fixture.dir), before, "{tool} dry run wrote");
    }
    for (tool, count) in tools {
        let done = call(tool, json!({"client_keys": ["claude-code"]})).unwrap();
        assert_eq!(done["dryRun"], false, "{tool}");
        assert_eq!(done[count], 1, "{tool}");
    }
    let after = tree_snapshot(&fixture.dir);
    let created: Vec<String> = after
        .keys()
        .filter(|path| !before.contains_key(*path) && !path.ends_with(['/', '\\']))
        .map(|path| path.replace('\\', "/"))
        .collect();
    assert_eq!(
        created,
        [
            "home/.claude/agents/helper.md",
            "home/.claude/output-styles/plain.md",
            "mcpm-skills.lock"
        ]
    );
}
