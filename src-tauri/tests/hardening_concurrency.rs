//! Simultaneous writers on one data dir serialize or fail cleanly; nothing torn, nothing lost.

#![cfg(unix)]

mod hardening_support;

use hardening_support::{stray_temp_files, Canary, Pending, Run, Sandbox};
use serde_json::{json, Value};

const WRITERS: usize = 8;

fn seed_alpha(sb: &Sandbox) {
    sb.write_registry(&json!({
        "version": 1,
        "servers": [{"id": "alpha", "name": "alpha", "transport": "stdio",
                     "command": "alpha-server", "args": [], "env": []}],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
    }));
}

fn reveal(sb: &Sandbox, key: &str) -> Option<String> {
    let run = sb.ctl(&["--json", "secret", "get", "alpha", key, "--reveal"]);
    run.assert_orderly();
    (run.code == Some(0)).then(|| run.data()["value"].as_str().unwrap_or_default().to_string())
}

fn assert_registry_intact(sb: &Sandbox) {
    let registry = sb.registry();
    assert!(registry["servers"].is_array(), "registry lost its servers");
    let tree = sb.tree();
    assert!(
        stray_temp_files(&tree).is_empty(),
        "a writer left a temp file behind: {:?}",
        stray_temp_files(&tree)
    );
}

fn assert_clean_failure(run: &Run) {
    run.assert_failed();
    let message = run.error_message();
    assert!(
        message.contains("locked") || message.contains("try again"),
        "a refused writer must say why: {}",
        run.describe()
    );
}

#[test]
fn parallel_server_writers_lose_no_update_while_readers_stay_valid() {
    let sb = Sandbox::new("conc-registry");
    seed_alpha(&sb);
    let quiet = sb.script("quiet.sh", "exit 0");
    let quiet = quiet.to_string_lossy().into_owned();

    let names: Vec<String> = (0..WRITERS).map(|i| format!("srv{i}")).collect();
    let mut writers: Vec<Pending> = Vec::new();
    let mut readers: Vec<Pending> = Vec::new();
    for name in &names {
        writers.push(sb.spawn_ctl(
            &["--json", "server", "new", name, "--command", &quiet],
            None,
            &[],
        ));
        readers.push(sb.spawn_ctl(&["--json", "server", "ls"], None, &[]));
        readers.push(sb.spawn_ctl(&["--json", "status"], None, &[]));
    }
    for (name, writer) in names.iter().zip(writers) {
        let run = writer.wait();
        run.assert_ok();
        assert_eq!(run.envelope()["ok"], true, "{name}: {}", run.describe());
    }
    for reader in readers {
        let run = reader.wait();
        run.assert_ok();
        let envelope = run.envelope();
        assert_ne!(
            envelope["ok"],
            false,
            "a reader saw a broken registry: {}",
            run.describe()
        );
    }

    let present = sb.server_names();
    for name in &names {
        assert!(present.contains(name), "{name} was lost: {present:?}");
    }
    assert!(present.contains(&"alpha".to_string()));
    assert_registry_intact(&sb);
}

#[test]
fn parallel_vault_writers_keep_every_secret() {
    let sb = Sandbox::new("conc-vault");
    seed_alpha(&sb);
    let canary = Canary::new();
    let keys: Vec<String> = (0..WRITERS).map(|i| format!("ALPHA_KEY_{i}")).collect();

    let pending: Vec<Pending> = keys
        .iter()
        .map(|key| {
            sb.spawn_ctl(
                &["--json", "secret", "set", "alpha", key],
                Some(&canary.val(key)),
                &[],
            )
        })
        .collect();
    for run in pending.into_iter().map(Pending::wait) {
        run.assert_ok();
    }
    for key in &keys {
        assert_eq!(
            reveal(&sb, key).as_deref(),
            Some(canary.val(key).as_str()),
            "{key} was lost or damaged"
        );
    }

    let removals: Vec<Pending> = keys
        .iter()
        .step_by(2)
        .map(|key| sb.spawn_ctl(&["--json", "secret", "rm", "alpha", key], None, &[]))
        .chain(keys.iter().skip(1).step_by(2).map(|key| {
            sb.spawn_ctl(
                &["--json", "secret", "set", "alpha", key],
                Some("FAKE-rotated"),
                &[],
            )
        }))
        .collect();
    for run in removals.into_iter().map(Pending::wait) {
        run.assert_ok();
    }
    for (i, key) in keys.iter().enumerate() {
        let got = reveal(&sb, key);
        if i % 2 == 0 {
            assert_eq!(got, None, "{key} should be gone");
        } else {
            assert_eq!(got.as_deref(), Some("FAKE-rotated"), "{key}");
        }
    }
    let tree = sb.tree();
    assert!(
        stray_temp_files(&tree).is_empty(),
        "{:?}",
        stray_temp_files(&tree)
    );
}

#[test]
fn mixed_registry_and_vault_writers_serialize() {
    let sb = Sandbox::new("conc-mixed");
    seed_alpha(&sb);
    let canary = Canary::new();
    sb.set_env("COUNCIL_KEY_MIXED", &canary.val("council"));
    let quiet = sb.script("quiet.sh", "exit 0");
    let quiet = quiet.to_string_lossy().into_owned();
    let root = sb.mcpm_root(
        "mcpm",
        &json!({
            "imp1": {"name": "imp1", "command": "imported-server", "args": [],
                     "env": {"IMP_API_KEY": canary.val("imp1")}},
            "imp2": {"name": "imp2", "url": "https://imp2.example.invalid/mcp",
                     "headers": {"Authorization": format!("Bearer {}", canary.val("imp2"))}}
        }),
        None,
    );
    let root = root.to_string_lossy().into_owned();
    let home = sb.home.to_string_lossy().into_owned();
    let repo = sb.skills_repo().to_string_lossy().into_owned();

    let mut jobs: Vec<(String, Pending)> = Vec::new();
    let mut add = |label: &str, args: &[&str], stdin: Option<&str>| {
        jobs.push((label.to_string(), sb.spawn_ctl(args, stdin, &[])));
    };
    for round in 0..2 {
        add(
            "council install",
            &[
                "--json",
                "council",
                "install",
                "--api-key-env",
                "COUNCIL_KEY_MIXED",
            ],
            None,
        );
        add(
            "secret set",
            &[
                "--json",
                "secret",
                "set",
                "alpha",
                &format!("MIX_KEY_{round}"),
            ],
            Some(&canary.val(&format!("mix{round}"))),
        );
        add(
            "server new",
            &[
                "--json",
                "server",
                "new",
                &format!("mixsrv{round}"),
                "--command",
                &quiet,
            ],
            None,
        );
        add(
            "import mcpm",
            &["--json", "import", "mcpm", &root, "--home", &home],
            None,
        );
        add("client sync", &["--json", "client", "sync"], None);
        add(
            "skills sync",
            &[
                "--json",
                "skills",
                "sync",
                "--repo",
                &repo,
                "--client",
                "claude-code",
            ],
            None,
        );
        add(
            "context sync",
            &["--json", "context", "sync", "--home", &home],
            None,
        );
        add("server ls", &["--json", "server", "ls"], None);
    }

    let mut refused = Vec::new();
    for (label, pending) in jobs {
        let run = pending.wait();
        run.assert_orderly();
        if run.code == Some(0) {
            continue;
        }
        assert_clean_failure(&run);
        refused.push(label);
    }
    assert!(
        refused.is_empty(),
        "with a 30 s lock budget every writer must get its turn, refused: {refused:?}"
    );

    let names = sb.server_names();
    for expected in ["alpha", "imp1", "imp2", "mixsrv0", "mixsrv1"] {
        assert!(
            names.contains(&expected.to_string()),
            "{expected}: {names:?}"
        );
    }
    assert_eq!(
        names.iter().filter(|n| n.as_str() == "imp1").count(),
        1,
        "concurrent imports must not duplicate a server: {names:?}"
    );
    for (id, key, label) in [
        ("imp1", "IMP_API_KEY", "imp1"),
        ("alpha", "MIX_KEY_0", "mix0"),
        ("alpha", "MIX_KEY_1", "mix1"),
    ] {
        let run = sb.ctl(&["--json", "secret", "get", id, key, "--reveal"]);
        run.assert_ok();
        assert_eq!(run.data()["value"], json!(canary.val(label)), "{id}::{key}");
    }
    assert_registry_intact(&sb);
    let doctor = sb.ctl(&["--json", "doctor"]);
    doctor.assert_orderly();
    let client: Value = serde_json::from_str(
        &std::fs::read_to_string(sb.home.join(".claude.json")).unwrap_or_else(|_| "{}".into()),
    )
    .expect("a client file written concurrently must stay valid JSON");
    assert!(client.is_object());
}

#[test]
fn concurrent_imports_converge_on_the_single_run_result() {
    let fixture = |sb: &Sandbox| {
        sb.mcpm_root(
            "mcpm",
            &json!({
                "one": {"name": "one", "command": "imported-server", "args": [],
                        "env": {"ONE_API_KEY": "FAKE-CANARY-converge-one"}},
                "two": {"name": "two", "url": "https://two.example.invalid/mcp",
                        "headers": {"Authorization": "Bearer FAKE-CANARY-converge-two"}}
            }),
            Some(&json!({"mcpServers": {
                "mcpm_one": {"command": "mcpm", "args": ["run", "one"]}
            }})),
        )
        .to_string_lossy()
        .into_owned()
    };

    let solo = Sandbox::new("conc-import-solo");
    let solo_root = fixture(&solo);
    let solo_home = solo.home.to_string_lossy().into_owned();
    solo.ctl(&["--json", "import", "mcpm", &solo_root, "--home", &solo_home])
        .assert_ok();

    let sb = Sandbox::new("conc-import");
    let root = fixture(&sb);
    let home = sb.home.to_string_lossy().into_owned();
    let racers: Vec<Pending> = (0..4)
        .map(|_| {
            sb.spawn_ctl(
                &["--json", "import", "mcpm", &root, "--home", &home],
                None,
                &[],
            )
        })
        .collect();
    for run in racers.into_iter().map(Pending::wait) {
        run.assert_ok();
    }

    let pick = |registry: &Value, field: &str| {
        let mut rows: Vec<Value> = registry[field].as_array().cloned().unwrap_or_default();
        rows.sort_by_key(|row| row["id"].to_string());
        rows
    };
    assert_eq!(
        pick(&sb.registry(), "servers"),
        pick(&solo.registry(), "servers"),
        "racing imports must end where a single import ends"
    );
    assert_eq!(
        pick(&sb.registry(), "profiles"),
        pick(&solo.registry(), "profiles")
    );
    let reveal = sb.ctl(&["--json", "secret", "get", "one", "ONE_API_KEY", "--reveal"]);
    reveal.assert_ok();
    assert!(
        reveal.stdout.contains("FAKE-CANARY-converge-one"),
        "{}",
        reveal.describe()
    );
    let again = sb.ctl(&["--json", "import", "mcpm", &root, "--home", &home]);
    again.assert_ok();
    let counts = &again.data()["counts"];
    assert_eq!(
        counts.as_object().map(|c| c.len()),
        Some(1),
        "every secret, including the bearer, must already hold its imported value: {}",
        again.describe()
    );
    assert!(
        counts["unchanged"].as_u64().unwrap_or(0) >= 6,
        "{}",
        again.describe()
    );
    let client = std::fs::read_to_string(sb.home.join(".claude.json")).unwrap_or_default();
    assert_eq!(
        client.matches("\"toolport\"").count(),
        1,
        "exactly one gateway entry: {client}"
    );
    assert_registry_intact(&sb);
}

#[test]
fn a_held_registry_lock_makes_writers_fail_cleanly_and_then_recover() {
    let sb = Sandbox::new("conc-held-registry");
    seed_alpha(&sb);
    let quiet = sb.script("quiet.sh", "exit 0");
    let quiet = quiet.to_string_lossy().into_owned();
    let before = std::fs::read(sb.registry_path()).unwrap();
    let short = [("TOOLPORT_LOCK_TIMEOUT_MS", "400")];

    let guard = conduit_lib::registry::lock_at(&sb.registry_path()).unwrap();
    let started = std::time::Instant::now();
    let refused = sb.ctl_env(
        &["--json", "server", "new", "blocked", "--command", &quiet],
        &short,
    );
    assert_clean_failure(&refused);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "a refused writer must give up promptly"
    );
    let reader = sb.ctl_env(&["--json", "server", "ls"], &short);
    reader.assert_ok();
    assert_eq!(
        std::fs::read(sb.registry_path()).unwrap(),
        before,
        "a refused writer must not touch registry.json"
    );

    drop(guard);
    sb.ctl(&["--json", "server", "new", "unblocked", "--command", &quiet])
        .assert_ok();
    let names = sb.server_names();
    assert!(names.contains(&"unblocked".to_string()), "{names:?}");
    assert!(!names.contains(&"blocked".to_string()), "{names:?}");
    assert_registry_intact(&sb);
}

#[test]
fn a_held_vault_lock_makes_secret_writers_fail_cleanly_and_then_recover() {
    let sb = Sandbox::new("conc-held-vault");
    seed_alpha(&sb);
    sb.ctl_in(
        &["--json", "secret", "set", "alpha", "KEEP_KEY"],
        "FAKE-keep",
    )
    .assert_ok();
    let vault = sb.data.join("secrets.enc");
    let before = std::fs::read(&vault).unwrap();
    let short = [("TOOLPORT_LOCK_TIMEOUT_MS", "400")];

    let guard = conduit_lib::registry::lock_at(&vault).unwrap();
    let refused = sb.spawn_ctl(
        &["--json", "secret", "set", "alpha", "NEW_KEY"],
        Some("FAKE-new"),
        &short,
    );
    assert_clean_failure(&refused.wait());
    assert_eq!(
        std::fs::read(&vault).unwrap(),
        before,
        "a refused writer must not touch secrets.enc"
    );

    drop(guard);
    sb.ctl_in(&["--json", "secret", "set", "alpha", "NEW_KEY"], "FAKE-new")
        .assert_ok();
    assert_eq!(reveal(&sb, "KEEP_KEY").as_deref(), Some("FAKE-keep"));
    assert_eq!(reveal(&sb, "NEW_KEY").as_deref(), Some("FAKE-new"));
}
