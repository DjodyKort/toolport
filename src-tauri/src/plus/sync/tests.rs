use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const GOLDEN_KEY: &str = "WrGla9wzWbq8AcLDRsyivn44CMMRn7UnEAsT65gp7Pw=";
const GOLDEN_PASSPHRASE: &str = "synthetic-passphrase";

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/plus/sync/fixtures/old-bundle")
}

fn roots() -> PortableRoots {
    PortableRoots {
        home: "/home/new-machine".into(),
        mcpm_home: "/home/new-machine/.config/mcpm".into(),
    }
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("toolport-sync-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn targets(base: &Path) -> ImportTargets {
    ImportTargets {
        config_dir: base.join("cfg"),
        skills_repo_dir: base.join("cfg/skills_repo"),
        projects: BTreeMap::new(),
    }
}

#[test]
fn golden_python_bundle_imports_with_derived_key() {
    let base = scratch("golden-key");
    let report = import_bundle(
        &golden_dir(),
        Credential::Key(GOLDEN_KEY),
        &roots(),
        &targets(&base),
        false,
    )
    .unwrap();
    assert_eq!(report.written.len(), 6, "{:?}", report.written);

    let servers = fs::read_to_string(base.join("cfg/servers.json")).unwrap();
    assert!(
        servers.contains("/home/new-machine/srv/index.js"),
        "{servers}"
    );
    assert!(servers.contains("/home/new-machine/.config/mcpm/x"));
    assert!(!servers.contains("${HOME}"));
    assert_eq!(
        fs::read_to_string(base.join("cfg/skills_repo/skills/demo/SKILL.md")).unwrap(),
        "---\nname: demo\n---\nSynthetic skill \u{e9}\n"
    );
    assert_eq!(
        fs::read(base.join("cfg/skills_repo/skills/demo/blob.bin")).unwrap(),
        vec![0, 159, 146, 150, 255, 1, 2, 3]
    );
    assert_eq!(
        fs::read_to_string(base.join("cfg/skills_repo/agents/helper.md")).unwrap(),
        "synthetic agent\n"
    );
    assert!(base.join("cfg/skills_repo/mcpm-skills.yaml").exists());
    assert_eq!(
        report.server_origins.unwrap()["servers"]["demo"]["server_name"],
        "demo"
    );
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn golden_python_bundle_imports_with_passphrase() {
    let files = read_bundle(
        &golden_dir(),
        Credential::Passphrase(GOLDEN_PASSPHRASE),
        &roots(),
    )
    .unwrap();
    assert!(files.iter().any(|f| f.key == "global/sources.json"));
    let err = read_bundle(&golden_dir(), Credential::Passphrase("wrong"), &roots()).unwrap_err();
    assert!(matches!(err, SyncError::Crypto(_)), "{err}");
}

#[test]
fn passphrase_derivation_matches_python_key() {
    let salt: Vec<u8> = (0u8..16).collect();
    assert_eq!(kdf::derive_key(GOLDEN_PASSPHRASE, &salt), GOLDEN_KEY);
}

#[test]
fn rust_written_bundle_round_trips() {
    let base = scratch("roundtrip");
    let source_roots = PortableRoots {
        home: "/home/old".into(),
        mcpm_home: "/home/old/.config/mcpm".into(),
    };
    let sources = vec![
        SourceFile {
            key: "global/servers.json".into(),
            category: "global".into(),
            project_name: None,
            bytes: br#"{"a":"/home/old/x","b":"/home/old/.config/mcpm/y"}"#.to_vec(),
        },
        SourceFile {
            key: "skills_repo/lib/helper.py".into(),
            category: "global".into(),
            project_name: None,
            bytes: b"print('hi')\n".to_vec(),
        },
        SourceFile {
            key: "bin/tool.bin".into(),
            category: "global".into(),
            project_name: None,
            bytes: vec![0xff, 0x00, 0xfe, 0x10],
        },
        SourceFile {
            key: "projects/app/CLAUDE.md".into(),
            category: "project".into(),
            project_name: Some("app".into()),
            bytes: b"# project\n".to_vec(),
        },
    ];
    let bundle = base.join("bundle");
    let salt = [7u8; 16];
    let manifest = write_bundle(
        &bundle,
        Credential::Key(GOLDEN_KEY),
        Some(&salt),
        "m1",
        "2026-01-01T00:00:00+00:00",
        &sources,
        &source_roots,
    )
    .unwrap();
    assert_eq!(manifest.entries["bin/tool.bin"].encoding, "base64");
    let stored = fs::read_to_string(bundle.join("blobs/global__servers.json.enc")).unwrap();
    assert!(!stored.contains("home"));

    let mut t = targets(&base);
    t.projects.insert("app".into(), base.join("app"));
    let report = import_bundle(&bundle, Credential::Key(GOLDEN_KEY), &roots(), &t, true).unwrap();
    assert_eq!(report.written.len(), 4);
    assert_eq!(
        fs::read_to_string(base.join("cfg/servers.json")).unwrap(),
        r#"{"a":"/home/new-machine/x","b":"/home/new-machine/.config/mcpm/y"}"#
    );
    assert_eq!(
        fs::read(base.join("cfg/bin/tool.bin")).unwrap(),
        vec![0xff, 0x00, 0xfe, 0x10]
    );
    assert_eq!(
        fs::read_to_string(base.join("cfg/skills_repo/lib/helper.py")).unwrap(),
        "print('hi')\n"
    );
    assert_eq!(
        fs::read_to_string(base.join("app/CLAUDE.md")).unwrap(),
        "# project\n"
    );

    let skipped = import_bundle(
        &bundle,
        Credential::Key(GOLDEN_KEY),
        &roots(),
        &targets(&base),
        false,
    )
    .unwrap();
    assert_eq!(skipped.skipped, vec!["projects/app/CLAUDE.md".to_string()]);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn passphrase_bundle_round_trips_through_salt_file() {
    let base = scratch("passphrase");
    let salt = kdf::random_salt().unwrap();
    let key = kdf::derive_key_with("pw", &salt, kdf::PBKDF2_ITERATIONS);
    let sources = vec![SourceFile {
        key: "global/sources.json".into(),
        category: "global".into(),
        project_name: None,
        bytes: b"{}".to_vec(),
    }];
    let bundle = base.join("bundle");
    write_bundle(
        &bundle,
        Credential::Passphrase("pw"),
        Some(&salt),
        "m",
        "t",
        &sources,
        &roots(),
    )
    .unwrap();
    let files = read_bundle(&bundle, Credential::Key(&key), &roots()).unwrap();
    assert_eq!(files[0].bytes, b"{}");
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn keys_entries_and_path_traversal_are_refused() {
    let base = scratch("unsafe");
    for key in [
        "keys/id",
        "global/keys/x",
        "global/sync_keyfile",
        "global/../evil",
        "/abs",
    ] {
        let sources = vec![SourceFile {
            key: key.into(),
            category: "global".into(),
            project_name: None,
            bytes: b"x".to_vec(),
        }];
        let err = write_bundle(
            &base.join("b"),
            Credential::Key(GOLDEN_KEY),
            None,
            "m",
            "t",
            &sources,
            &roots(),
        )
        .unwrap_err();
        assert!(matches!(err, SyncError::UnsafePath(_)), "{key}: {err}");
    }

    let bundle = base.join("hostile");
    let sources = vec![SourceFile {
        key: "global/ok.json".into(),
        category: "global".into(),
        project_name: None,
        bytes: b"{}".to_vec(),
    }];
    write_bundle(
        &bundle,
        Credential::Key(GOLDEN_KEY),
        None,
        "m",
        "t",
        &sources,
        &roots(),
    )
    .unwrap();
    let manifest_path = bundle.join("sync_manifest.json");
    let text = fs::read_to_string(&manifest_path)
        .unwrap()
        .replace("global/ok.json", "global/keys/ok.json");
    fs::write(&manifest_path, text).unwrap();
    let err = read_bundle(&bundle, Credential::Key(GOLDEN_KEY), &roots()).unwrap_err();
    assert!(matches!(err, SyncError::UnsafePath(_)), "{err}");
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn tampered_blob_fails_authentication() {
    let base = scratch("tamper");
    let copy = base.join("bundle");
    fs::create_dir_all(copy.join("blobs")).unwrap();
    for entry in fs::read_dir(golden_dir().join("blobs")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), copy.join("blobs").join(entry.file_name())).unwrap();
    }
    for name in ["salt.txt", "sync_manifest.json"] {
        fs::copy(golden_dir().join(name), copy.join(name)).unwrap();
    }
    let blob = copy.join("blobs/global__sources.json.enc");
    let mut bytes = fs::read(&blob).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] = if bytes[mid] == b'A' { b'B' } else { b'A' };
    fs::write(&blob, bytes).unwrap();
    let err = read_bundle(&copy, Credential::Key(GOLDEN_KEY), &roots()).unwrap_err();
    assert!(matches!(err, SyncError::Crypto(_)), "{err}");
    let _ = fs::remove_dir_all(&base);
}
