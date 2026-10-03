//! Damaged state files give an orderly, explained failure and the damaged bytes stay recoverable.

#![cfg(unix)]

mod hardening_support;

use hardening_support::{truncate_half, Sandbox, Tree};
use serde_json::{json, Value};
use std::path::Path;

fn alpha_registry(sb: &Sandbox) {
    sb.write_registry(&json!({
        "version": 1,
        "servers": [{"id": "alpha", "name": "alpha", "transport": "stdio",
                     "command": "alpha-server", "args": [], "env": []}],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
    }));
}

fn survives(tree: &Tree, bytes: &[u8]) -> bool {
    tree.values().any(|content| content == bytes)
}

fn files_named(tree: &Tree, needle: &str) -> Vec<String> {
    tree.keys()
        .filter(|path| path.contains(needle))
        .cloned()
        .collect()
}

const DAMAGE: &[(&str, &[u8])] = &[
    ("prose", b"this is not json at all\n"),
    ("json array", b"[1, 2, 3]"),
    ("wrong schema", b"{\"version\": 1, \"servers\": \"oops\"}"),
    ("nul bytes", b"\0\0\0\0\0\0\0\0"),
    (
        "cut mid-token",
        b"{\"version\": 1, \"servers\": [{\"id\": \"al",
    ),
    ("invalid utf-8", &[0xff, 0xfe, 0xfd, 0x00, 0x80]),
];

#[test]
fn damaged_registry_without_a_backup_is_a_clear_error_and_stays_put() {
    for (label, damage) in DAMAGE {
        let sb = Sandbox::new("corrupt-registry-solo");
        std::fs::write(sb.registry_path(), damage).unwrap();
        let quiet = sb.script("quiet.sh", "exit 0");
        let quiet = quiet.to_string_lossy().into_owned();

        for args in [
            vec!["--json", "server", "ls"],
            vec!["--json", "status"],
            vec!["--json", "client", "sync"],
            vec!["--json", "server", "new", "added", "--command", &quiet],
        ] {
            let run = sb.ctl(&args);
            run.assert_orderly();
            if run.code == Some(0) {
                continue;
            }
            assert!(
                !run.error_message().is_empty(),
                "{label}: {} gave no reason",
                args.join(" ")
            );
        }
        let tree = sb.tree();
        assert!(
            survives(&tree, damage),
            "{label}: the damaged registry.json was lost; data dir holds {:?}",
            tree.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn truncated_registry_is_refused_by_the_ctl_and_recovered_by_the_loader() {
    let sb = Sandbox::new("corrupt-registry-bak");
    alpha_registry(&sb);
    let quiet = sb.script("quiet.sh", "exit 0");
    let quiet = quiet.to_string_lossy().into_owned();
    sb.ctl(&["--json", "server", "new", "beta", "--command", &quiet])
        .assert_ok();
    sb.ctl(&["--json", "server", "new", "gamma", "--command", &quiet])
        .assert_ok();
    let backup = std::fs::read(sb.data.join("registry.json.bak")).unwrap();
    let backed_up: Value = serde_json::from_slice(&backup).unwrap();
    let kept: Vec<&str> = backed_up["servers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(
        kept.contains(&"alpha") && kept.contains(&"beta"),
        "{kept:?}"
    );

    truncate_half(&sb.registry_path());
    let damaged = std::fs::read(sb.registry_path()).unwrap();

    let before = sb.tree();
    let listing = sb.ctl(&["--json", "server", "ls"]);
    listing.assert_failed();
    assert!(
        listing.error_message().contains("registry is not readable"),
        "{}",
        listing.describe()
    );
    assert_eq!(
        sb.tree(),
        before,
        "an inspection command must not change the data directory"
    );

    let refused = sb.ctl(&["--json", "server", "new", "delta", "--command", &quiet]);
    refused.assert_failed();
    assert_eq!(sb.tree(), before, "{}", refused.describe());

    let (recovered, source) = {
        let _lock = conduit_lib::registry::data_dir_test_lock();
        let _data = conduit_lib::registry::DataDirOverride::set(&sb.data);
        conduit_lib::registry::load_resolved_with_source().expect("the app-side loader recovers")
    };
    assert_eq!(source, conduit_lib::registry::LoadSource::Backup);
    let names = sb.server_names();
    for name in &kept {
        assert!(
            names.contains(&name.to_string()),
            "{name} was not recovered: {names:?}"
        );
        assert!(recovered.servers.iter().any(|s| s.name == *name));
    }
    let tree = sb.tree();
    assert!(
        files_named(&tree, "registry.json.unreadable-").len() == 1 && survives(&tree, &damaged),
        "the damaged file must be kept next to the registry: {:?}",
        tree.keys().collect::<Vec<_>>()
    );

    sb.ctl(&["--json", "server", "new", "delta", "--command", &quiet])
        .assert_ok();
    assert!(sb.server_names().contains(&"delta".to_string()));
    sb.ctl(&["--json", "server", "ls"]).assert_ok();
}

#[test]
fn damaged_secrets_vault_is_refused_clearly_and_never_rewritten() {
    let sb = Sandbox::new("corrupt-vault");
    alpha_registry(&sb);
    sb.ctl_in(
        &["--json", "secret", "set", "alpha", "ALPHA_API_KEY"],
        "FAKE-vault-value",
    )
    .assert_ok();
    let vault = sb.data.join("secrets.enc");
    let good = std::fs::read(&vault).unwrap();

    let mut variants: Vec<(&str, Vec<u8>)> = vec![
        ("truncated", good[..good.len() / 2].to_vec()),
        ("shorter than a nonce", b"AAAA".to_vec()),
        ("empty", Vec::new()),
        ("not base64", b"@@@ not base64 @@@".to_vec()),
        ("invalid utf-8", vec![0xff, 0xfe, 0x00]),
    ];
    let mut flipped = good.clone();
    let middle = flipped.len() / 2;
    flipped[middle] = if flipped[middle] == b'A' { b'B' } else { b'A' };
    variants.push(("one flipped character", flipped));

    for (label, bytes) in variants {
        std::fs::write(&vault, &bytes).unwrap();
        let attempts: Vec<Vec<&str>> = vec![
            vec!["--json", "secret", "get", "alpha", "ALPHA_API_KEY"],
            vec!["--json", "inspect", "alpha"],
            vec!["--json", "doctor"],
        ];
        for args in attempts {
            let run = sb.ctl(&args);
            run.assert_orderly();
            assert!(
                !run.combined().contains("FAKE-vault-value"),
                "{label}: {}",
                run.describe()
            );
        }
        let read = sb.ctl(&["--json", "secret", "get", "alpha", "ALPHA_API_KEY"]);
        read.assert_failed();
        let message = read.error_message();
        assert!(
            message.contains("secrets.enc")
                || message.contains("base64")
                || message.contains("decrypt"),
            "{label}: the failure must name the vault: {}",
            read.describe()
        );
        let write = sb.ctl_in(
            &["--json", "secret", "set", "alpha", "OTHER_KEY"],
            "FAKE-other-value",
        );
        write.assert_failed();
        assert_eq!(
            std::fs::read(&vault).unwrap(),
            bytes,
            "{label}: a refused write must leave secrets.enc as it was"
        );
    }

    std::fs::write(&vault, &good).unwrap();
    let back = sb.ctl(&[
        "--json",
        "secret",
        "get",
        "alpha",
        "ALPHA_API_KEY",
        "--reveal",
    ]);
    back.assert_ok();
    assert_eq!(back.data()["value"], json!("FAKE-vault-value"));
}

#[test]
fn a_wrong_vault_key_is_refused_without_touching_the_vault() {
    let sb = Sandbox::new("corrupt-vault-key");
    alpha_registry(&sb);
    sb.ctl_in(
        &["--json", "secret", "set", "alpha", "ALPHA_API_KEY"],
        "FAKE-vault-value",
    )
    .assert_ok();
    let vault = sb.data.join("secrets.enc");
    let good = std::fs::read(&vault).unwrap();
    let wrong = [("TOOLPORT_SECRET_KEY", "cd".repeat(32))];
    let wrong: Vec<(&str, &str)> = wrong.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let read = sb.ctl_env(
        &["--json", "secret", "get", "alpha", "ALPHA_API_KEY"],
        &wrong,
    );
    read.assert_failed();
    assert!(
        read.error_message().contains("decrypt"),
        "{}",
        read.describe()
    );
    let write = sb.spawn_ctl(
        &["--json", "secret", "set", "alpha", "OTHER_KEY"],
        Some("FAKE-other-value"),
        &wrong,
    );
    write.wait().assert_failed();
    assert_eq!(std::fs::read(&vault).unwrap(), good);
}

#[test]
fn import_fails_cleanly_on_a_damaged_vault_and_loses_nothing() {
    let sb = Sandbox::new("corrupt-import");
    alpha_registry(&sb);
    sb.ctl_in(
        &["--json", "secret", "set", "alpha", "ALPHA_API_KEY"],
        "FAKE-vault-value",
    )
    .assert_ok();
    let vault = sb.data.join("secrets.enc");
    truncate_half(&vault);
    let damaged = std::fs::read(&vault).unwrap();
    let registry_before = std::fs::read(sb.registry_path()).unwrap();

    let root = sb.mcpm_root(
        "mcpm",
        &json!({"imp": {"name": "imp", "command": "imported-server", "args": [],
                        "env": {"IMP_API_KEY": "FAKE-imported-value"}}}),
        None,
    );
    let home = sb.home.to_string_lossy().into_owned();
    let run = sb.ctl(&[
        "--json",
        "import",
        "mcpm",
        &root.to_string_lossy(),
        "--home",
        &home,
    ]);
    run.assert_failed();
    assert_eq!(
        std::fs::read(&vault).unwrap(),
        damaged,
        "{}",
        run.describe()
    );
    let names = sb.server_names();
    assert!(names.contains(&"alpha".to_string()), "{names:?}");
    let registry_after = std::fs::read(sb.registry_path()).unwrap();
    assert!(
        registry_after == registry_before || sb.server_names().contains(&"imp".to_string()),
        "the registry must stay as it was or hold the whole import"
    );
}

fn lockfile_in(sb: &Sandbox) -> std::path::PathBuf {
    sb.data.join("mcpm-skills.lock")
}

#[test]
fn damaged_skills_lockfile_is_replaced_but_its_bytes_are_kept() {
    let sb = Sandbox::new("corrupt-lock");
    let repo = sb.skills_repo().to_string_lossy().into_owned();
    let sync = [
        "--json",
        "skills",
        "sync",
        "--repo",
        &repo,
        "--client",
        "claude-code",
    ];
    sb.ctl(&sync).assert_ok();
    let lock = lockfile_in(&sb);
    let good = std::fs::read(&lock).unwrap();
    assert!(!good.is_empty());

    let variants: [(&str, Vec<u8>); 4] = [
        ("truncated", good[..good.len() / 2].to_vec()),
        ("prose", b"not a lockfile".to_vec()),
        ("empty", Vec::new()),
        ("invalid utf-8", vec![0xff, 0xfe, 0x00, 0x80]),
    ];
    for (label, damage) in variants {
        std::fs::write(&lock, &damage).unwrap();
        for args in [
            vec!["--json", "skills", "diff", "--repo", &repo],
            vec!["--json", "skills", "ls", "--repo", &repo],
        ] {
            sb.ctl(&args).assert_orderly();
        }
        assert_eq!(
            std::fs::read(&lock).unwrap(),
            damage,
            "{label}: read-only commands must not rewrite the lockfile"
        );

        let run = sb.ctl(&sync);
        run.assert_ok();
        let rewritten = std::fs::read(&lock).unwrap();
        assert!(
            String::from_utf8_lossy(&rewritten).contains("\"skills\""),
            "{label}: the sync must write a valid lockfile"
        );
        if !damage.is_empty() {
            assert!(
                survives(&sb.tree(), &damage),
                "{label}: the damaged lockfile was destroyed"
            );
        }
    }
    assert!(sb.home.join(".claude/skills/demo/SKILL.md").is_file());
}

fn context_path(sb: &Sandbox) -> std::path::PathBuf {
    sb.home.join(".config/mcpm/context.json")
}

#[test]
fn damaged_context_config_is_kept_when_a_sync_replaces_it() {
    let valid = json!({
        "profiles": {"research": {}},
        "settings": {"ensure_allow": ["Bash(ls:*)"], "ensure_ask": []},
        "wrap_default_claude": true,
        "clients_root": "/nonexistent-clients-root",
    })
    .to_string();
    let variants: Vec<(&str, Vec<u8>)> = vec![
        ("truncated", valid.as_bytes()[..valid.len() / 2].to_vec()),
        ("prose", b"profiles: research\n".to_vec()),
        (
            "invalid profile name",
            json!({"profiles": {"Bad Name": {}}})
                .to_string()
                .into_bytes(),
        ),
        (
            "wrong types",
            b"{\"profiles\": 7, \"wrap_default_claude\": \"yes\"}".to_vec(),
        ),
        ("invalid utf-8", vec![0xff, 0xfe, 0x00]),
    ];
    for (label, damage) in variants {
        let sb = Sandbox::new("corrupt-context");
        let path = context_path(&sb);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &damage).unwrap();
        let home = sb.home.to_string_lossy().into_owned();

        for args in [
            vec!["--json", "context", "plan", "--home", &home],
            vec!["--json", "context", "sync", "--home", &home, "--dry-run"],
            vec!["--json", "context", "loads", "--cwd", "."],
        ] {
            sb.ctl(&args).assert_orderly();
        }
        assert_eq!(
            std::fs::read(&path).unwrap(),
            damage,
            "{label}: planning must not touch context.json"
        );
        assert!(
            files_named(&sb.tree(), "context.json.unreadable-").is_empty(),
            "{label}: planning must not leave copies"
        );

        let run = sb.ctl(&["--json", "context", "sync", "--home", &home]);
        run.assert_orderly();
        assert_eq!(run.code, Some(0), "{label}: {}", run.describe());
        assert!(
            survives(&sb.tree(), &damage),
            "{label}: the damaged context.json was destroyed: {:?}",
            sb.tree().keys().collect::<Vec<_>>()
        );
        assert!(
            run.stdout.contains("unreadable"),
            "{label}: the sync must say what it did: {}",
            run.describe()
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            serde_json::from_str::<Value>(&text).is_ok(),
            "{label}: the replacement must be valid JSON"
        );
    }
}

#[test]
fn a_valid_context_config_is_never_copied_aside() {
    let sb = Sandbox::new("corrupt-context-valid");
    let path = context_path(&sb);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, json!({"profiles": {"research": {}}}).to_string()).unwrap();
    let home = sb.home.to_string_lossy().into_owned();
    sb.ctl(&["--json", "context", "sync", "--home", &home])
        .assert_ok();
    sb.ctl(&["--json", "context", "sync", "--home", &home])
        .assert_ok();
    assert!(
        files_named(&sb.tree(), "context.json.unreadable-").is_empty(),
        "{:?}",
        sb.tree().keys().collect::<Vec<_>>()
    );
}

fn monitor_db(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE monitor_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT, server_id TEXT,
            resource_id TEXT, session_id TEXT, client_id TEXT, timestamp DATETIME,
            duration_ms INTEGER, request_size INTEGER, response_size INTEGER,
            success BOOLEAN, error_message TEXT, metadata TEXT, raw_request TEXT,
            raw_response TEXT);
         INSERT INTO monitor_events (event_type,server_id,resource_id,session_id,client_id,timestamp,duration_ms,request_size,response_size,success)
          VALUES ('TOOL_INVOCATION','github','list','s1','claude-code','2026-09-01T10:00:00',100,10,200,1);",
    )
    .unwrap();
}

#[test]
fn damaged_monitor_db_is_refused_and_the_imported_history_survives() {
    let sb = Sandbox::new("corrupt-monitor");
    let _lock = conduit_lib::registry::data_dir_test_lock();
    let _data = conduit_lib::registry::DataDirOverride::set(&sb.data);
    let db = sb.input.join("monitor.db");
    monitor_db(&db);
    let import = |path: &Path| {
        conduit_lib::plus::dispatch(
            "plus.obs.importMonitor",
            json!({"path": path.to_string_lossy()}),
        )
    };

    let first = import(&db).unwrap();
    assert_eq!(first["rows"], 1);
    let history = sb.data.join("obs").join("monitor-history.json");
    let kept = std::fs::read(&history).expect("the import must store a history");
    let pristine = std::fs::read(&db).unwrap();

    let variants: Vec<(&str, Vec<u8>)> = vec![
        ("truncated", pristine[..pristine.len() / 2].to_vec()),
        (
            "garbage",
            b"definitely not a sqlite database, just prose of some length".repeat(40),
        ),
        ("empty", Vec::new()),
        ("header only", pristine[..100.min(pristine.len())].to_vec()),
    ];
    for (label, bytes) in variants {
        std::fs::write(&db, &bytes).unwrap();
        let outcome = import(&db);
        let message = outcome.expect_err(&format!("{label}: a damaged monitor.db must be refused"));
        assert!(!message.is_empty(), "{label}");
        assert_eq!(
            std::fs::read(&history).unwrap(),
            kept,
            "{label}: a refused import must leave the stored history alone"
        );
        assert_eq!(
            std::fs::read(&db).unwrap(),
            bytes,
            "{label}: the source database is opened read-only"
        );
    }
    let missing = import(&sb.input.join("absent.db")).unwrap_err();
    assert!(missing.contains("not found"), "{missing}");
}

#[test]
fn cutover_stops_on_a_damaged_context_config_after_taking_its_backup() {
    let sb = Sandbox::new("corrupt-cutover");
    let root = sb.home.join(".config/mcpm");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("servers.json"),
        json!({"alpha": {"name": "alpha", "command": "alpha-server", "args": []}}).to_string(),
    )
    .unwrap();
    std::fs::write(root.join("context.json"), b"{\"settings\": {\"ensure_al").unwrap();
    let damaged = std::fs::read(root.join("context.json")).unwrap();
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/cutover/cutover.sh");
    let home = sb.home.to_string_lossy().into_owned();

    let out = sb
        .command("bash")
        .args([script, "--home", &home, "--ctl", hardening_support::CTL])
        .output()
        .expect("bash is required");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a damaged context.json must stop the cutover"
    );
    assert!(
        stderr.contains("not valid JSON") && !stderr.contains("Traceback"),
        "the failure must be explained: {stderr}"
    );
    assert_eq!(std::fs::read(root.join("context.json")).unwrap(), damaged);
    let tree = sb.tree();
    assert!(
        !files_named(&tree, ".toolport-cutover-backups/").is_empty(),
        "the backup is taken before anything is touched"
    );
    assert!(
        !tree.contains_key("data/registry.json"),
        "nothing was imported: {:?}",
        tree.keys().collect::<Vec<_>>()
    );
}
