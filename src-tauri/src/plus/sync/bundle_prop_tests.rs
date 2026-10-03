//! Seeded randomized and edge-case tests for the Fernet codec, the key derivation and the sync
//! bundle: round trips, truncated or tampered data, unsafe keys and portable path tokens.

use super::bundle::{destination_for, safe_relative};
use super::fernet::{self, FernetError, FernetKey};
use super::kdf;
use super::*;
use crate::plus::randutil::{run_cases, Rng, ScratchDir};
use base64::{engine::general_purpose::URL_SAFE, Engine};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

fn key_text() -> String {
    kdf::derive_key_with("synthetic passphrase", b"synthetic salt 16", 1)
}

fn key() -> FernetKey {
    FernetKey::parse(&key_text()).unwrap()
}

fn payload(rng: &mut Rng) -> Vec<u8> {
    match rng.below(6) {
        0 => Vec::new(),
        1 => vec![0u8; *rng.pick(&[15, 16, 17, 31, 32, 33])],
        2 => rng.garbage(200).into_bytes(),
        _ => rng.bytes(300),
    }
}

#[test]
fn fernet_round_trips_random_payloads_deterministically() {
    let k = key();
    run_cases("fernet-roundtrip", 800, |_, rng| {
        let plain = payload(rng);
        let mut iv = [0u8; 16];
        iv.iter_mut().for_each(|b| *b = rng.next_u64() as u8);
        let ts = rng.next_u64();
        let token = fernet::encrypt_at(&k, &plain, ts, iv);
        assert_eq!(token, fernet::encrypt_at(&k, &plain, ts, iv));
        assert!(token.iter().all(|b| b.is_ascii()));
        let raw = URL_SAFE.decode(&token).unwrap();
        assert_eq!(raw.len(), 1 + 8 + 16 + (plain.len() / 16 + 1) * 16 + 32);
        assert_eq!(fernet::decrypt(&k, &token).unwrap(), plain);
        let mut padded = token.clone();
        padded.extend_from_slice(b"\n  ");
        assert_eq!(
            fernet::decrypt(&k, &padded).unwrap(),
            plain,
            "trailing whitespace"
        );
        let fresh = fernet::encrypt(&k, &plain).unwrap();
        assert_eq!(fernet::decrypt(&k, &fresh).unwrap(), plain);
    });
}

#[test]
fn fernet_rejects_tampered_truncated_and_garbage_tokens() {
    let k = key();
    let other = FernetKey::parse(&kdf::derive_key_with("other", b"salt", 1)).unwrap();
    run_cases("fernet-tamper", 600, |_, rng| {
        let plain = payload(rng);
        let token = fernet::encrypt(&k, &plain).unwrap();
        assert_eq!(
            fernet::decrypt(&other, &token),
            Err(FernetError::InvalidToken)
        );

        let mut raw = URL_SAFE.decode(&token).unwrap();
        let at = rng.below(raw.len());
        raw[at] ^= 1 << rng.below(8);
        assert_eq!(
            fernet::decrypt(&k, URL_SAFE.encode(&raw).as_bytes()),
            Err(FernetError::InvalidToken),
            "bit flip at {at}"
        );

        let cut = rng.below(token.len());
        assert!(fernet::decrypt(&k, &token[..cut]).is_err(), "prefix {cut}");
        let mut longer = token.clone();
        longer.extend_from_slice(rng.garbage(8).as_bytes());
        let extended = fernet::decrypt(&k, &longer);
        assert!(extended.is_err() || extended == Ok(plain.clone()));

        let mut text = token.clone();
        let at = rng.below(text.len());
        text[at] = *rng.pick(b"AZaz09-_=+/ \n\0");
        let mutated = fernet::decrypt(&k, &text);
        assert!(mutated.is_err() || mutated == Ok(plain.clone()));

        let garbage = rng.bytes(120);
        assert!(fernet::decrypt(&k, &garbage).is_err());
        assert!(fernet::decrypt(&k, rng.garbage(60).as_bytes()).is_err());
    });
    for token in [&b""[..], b" ", b"\n", b"=", b"gAAAAA", b"\xff\xfe", b"AAAA"] {
        assert!(fernet::decrypt(&k, token).is_err());
    }
}

#[test]
fn fernet_rejects_a_wrong_version_byte_even_with_a_valid_shape() {
    let k = key();
    let token = fernet::encrypt(&k, b"x").unwrap();
    let mut raw = URL_SAFE.decode(&token).unwrap();
    raw[0] = 0x81;
    assert_eq!(
        fernet::decrypt(&k, URL_SAFE.encode(&raw).as_bytes()),
        Err(FernetError::InvalidToken)
    );
}

#[test]
fn fernet_key_parsing_follows_the_base64_model() {
    run_cases("fernet-key-parse", 3000, |_, rng| {
        let text = match rng.below(5) {
            0 => URL_SAFE.encode(rng.bytes(40)),
            1 => URL_SAFE.encode(rng.bytes(32)),
            2 => format!(" {}\n", URL_SAFE.encode(rng.bytes(32))),
            3 => rng.garbage(50),
            _ => rng.string("AZaz09-_=+/", 50),
        };
        let model = URL_SAFE
            .decode(text.trim())
            .is_ok_and(|raw| raw.len() == 32);
        assert_eq!(FernetKey::parse(&text).is_ok(), model, "{text:?}");
    });
    assert!(FernetKey::parse("").is_err());
}

#[test]
fn key_derivation_is_deterministic_and_input_sensitive() {
    run_cases("kdf-derive", 300, |_, rng| {
        let pass = rng.garbage(20);
        let salt = rng.bytes(24);
        let iterations = rng.range(0, 5) as u32;
        let derived = kdf::derive_key_with(&pass, &salt, iterations);
        assert_eq!(derived, kdf::derive_key_with(&pass, &salt, iterations));
        assert!(FernetKey::parse(&derived).is_ok());
        assert_eq!(derived.len(), 44);
        let mut other_salt = salt.clone();
        other_salt.push(1);
        assert_ne!(
            derived,
            kdf::derive_key_with(&pass, &other_salt, iterations)
        );
        assert_ne!(
            derived,
            kdf::derive_key_with(&format!("{pass}x"), &salt, iterations)
        );
        if iterations > 0 {
            assert_ne!(derived, kdf::derive_key_with(&pass, &salt, iterations + 1));
        }
    });
    assert_eq!(kdf::derive_key_with("", b"", 1).len(), 44);
}

fn roots() -> PortableRoots {
    PortableRoots {
        home: "/home/synthetic".into(),
        mcpm_home: "/home/synthetic/.config/mcpm".into(),
    }
}

#[test]
fn portable_roots_round_trip_and_never_panic() {
    run_cases("portable-roots", 3000, |_, rng| {
        let home = format!("/home/{}", rng.slug(8));
        let r = PortableRoots {
            mcpm_home: if rng.chance(70) {
                format!("{home}/.config/mcpm")
            } else {
                format!("/opt/{}", rng.slug(6))
            },
            home,
        };
        let mut text = String::new();
        for _ in 0..rng.range(0, 8) {
            match rng.below(5) {
                0 => text.push_str(&r.home),
                1 => text.push_str(&r.mcpm_home),
                2 => text.push_str(&format!("{}/bin/{}", r.home, rng.slug(5))),
                _ => text.push_str(&rng.garbage(10).replace(['\\', '$'], "")),
            }
        }
        let portable = r.make_portable(&text);
        assert_eq!(r.resolve(&portable), text, "{portable:?}");
        assert_eq!(r.make_portable(&portable), portable, "idempotent");
        if !r.home.is_empty() {
            assert!(!portable.contains(&r.home) && !portable.contains(&r.mcpm_home));
        }
    });
    run_cases("portable-roots-hostile", 2000, |_, rng| {
        let r = PortableRoots {
            home: rng.garbage(8),
            mcpm_home: rng.garbage(8),
        };
        let text = rng.garbage(60);
        let _ = r.make_portable(&text);
        let _ = r.resolve(&text);
    });
    let windows = PortableRoots {
        home: "C:\\Users\\synthetic".into(),
        mcpm_home: "C:\\Users\\synthetic\\.config\\mcpm".into(),
    };
    for text in [
        "C:\\Users\\synthetic\\x",
        "C:/Users/synthetic/x",
        "C:\\\\Users\\\\synthetic\\\\x",
    ] {
        let portable = windows.make_portable(text);
        assert!(portable.starts_with("${HOME}"), "{portable:?}");
        assert!(!portable.contains("Users"), "{portable:?}");
        assert!(windows.resolve(&portable).starts_with("C:/Users/synthetic"));
        assert_eq!(windows.make_portable(&portable), portable);
    }
    let empty = PortableRoots {
        home: String::new(),
        mcpm_home: String::new(),
    };
    assert_eq!(empty.make_portable("/a/b"), "/a/b");
}

const SEGMENTS: &[&str] = &[
    "a",
    "b",
    "a__b",
    "x_y",
    "skills",
    "SKILL.md",
    "notes.md",
    "é",
    "日本",
    "with space",
    "a.json",
    "servers.json",
    "CLAUDE.md",
    "tool.bin",
];

fn source(rng: &mut Rng) -> SourceFile {
    let mut segment = |rng: &mut Rng| rng.pick(SEGMENTS).to_string();
    let (key, category, project) = match rng.below(4) {
        0 => (format!("global/{}", segment(rng)), "global", None),
        1 => (
            format!("skills_repo/{}/{}", segment(rng), segment(rng)),
            "global",
            None,
        ),
        2 => (format!("bin/{}", segment(rng)), "global", None),
        _ => {
            let name = rng.slug(5);
            (
                format!("projects/{name}/{}", segment(rng)),
                "project",
                Some(name),
            )
        }
    };
    let bytes = match rng.below(5) {
        0 => Vec::new(),
        1 => rng.bytes(80),
        2 => format!(
            "{{\"path\": \"{}/x\", \"mcpm\": \"{}/y\", \"note\": {:?}}}",
            roots().home,
            roots().mcpm_home,
            rng.garbage(20).replace('$', "")
        )
        .into_bytes(),
        3 => rng.garbage(100).into_bytes(),
        _ => vec![0xff; rng.range(1, 64)],
    };
    SourceFile {
        key,
        category: category.into(),
        project_name: project,
        bytes,
    }
}

fn colliding_sibling(rng: &mut Rng, of: &SourceFile) -> Option<SourceFile> {
    let slashes: Vec<usize> = of.key.match_indices('/').map(|(i, _)| i).collect();
    let at = *slashes.get(rng.below(slashes.len().max(1)))?;
    let mut key = of.key.clone();
    key.replace_range(at..=at, "__");
    if key == of.key {
        return None;
    }
    Some(SourceFile {
        key,
        bytes: rng.bytes(20),
        ..of.clone_source()
    })
}

fn blob_collides(sources: &[SourceFile]) -> bool {
    let mut seen: HashMap<String, &str> = HashMap::new();
    sources.iter().any(|s| {
        let blob = s.key.replace('/', "__");
        seen.insert(blob, &s.key).is_some_and(|prev| prev != s.key)
    })
}

trait CloneSource {
    fn clone_source(&self) -> SourceFile;
}

impl CloneSource for SourceFile {
    fn clone_source(&self) -> SourceFile {
        SourceFile {
            key: self.key.clone(),
            category: self.category.clone(),
            project_name: self.project_name.clone(),
            bytes: self.bytes.clone(),
        }
    }
}

fn write(dir: &Path, sources: &[SourceFile]) -> Result<Manifest, SyncError> {
    write_bundle(
        dir,
        Credential::Key(&key_text()),
        Some(&[3u8; 16]),
        "machine",
        "2026-01-01T00:00:00+00:00",
        sources,
        &roots(),
    )
}

fn read(dir: &Path) -> Result<Vec<BundleFile>, SyncError> {
    read_bundle(dir, Credential::Key(&key_text()), &roots())
}

#[test]
fn bundles_round_trip_random_files() {
    let tmp = ScratchDir::new("bundle-roundtrip");
    let (mut collisions, mut clean) = (0, 0);
    run_cases("bundle-roundtrip", 300, |_, rng| {
        tmp.reset();
        let mut sources: Vec<SourceFile> = Vec::new();
        for _ in 0..rng.range(0, 6) {
            let s = source(rng);
            let sibling = if rng.chance(25) {
                colliding_sibling(rng, &s)
            } else {
                None
            };
            sources.retain(|o| o.key != s.key);
            sources.push(s);
            if let Some(sibling) = sibling {
                sources.retain(|o| o.key != sibling.key);
                sources.push(sibling);
            }
        }
        let result = write(tmp.path(), &sources);
        if blob_collides(&sources) {
            collisions += 1;
            assert!(
                matches!(result, Err(SyncError::Format(_))),
                "colliding blob names must be refused: {result:?}"
            );
            assert!(!tmp.path().join("blobs").exists(), "nothing written");
            return;
        }
        clean += 1;
        let manifest = result.unwrap();
        assert_eq!(manifest.entries.len(), sources.len());
        let files = read(tmp.path()).unwrap();
        assert_eq!(files.len(), sources.len());
        let got: BTreeMap<&str, &[u8]> = files
            .iter()
            .map(|f| (f.key.as_str(), f.bytes.as_slice()))
            .collect();
        for s in &sources {
            assert_eq!(got[s.key.as_str()], s.bytes.as_slice(), "{}", s.key);
        }
        let again = write(tmp.path(), &sources).unwrap();
        assert_eq!(again, manifest, "deterministic manifest");
        let listed: Vec<&String> = manifest.entries.keys().collect();
        let mut sorted = listed.clone();
        sorted.sort();
        assert_eq!(listed, sorted);
    });
    assert!(clean > 40 && collisions > 15, "{clean} {collisions}");
}

#[test]
fn blob_name_collisions_are_refused_at_write_time() {
    let tmp = ScratchDir::new("bundle-collision");
    let file = |key: &str, bytes: &[u8]| SourceFile {
        key: key.into(),
        category: "global".into(),
        project_name: None,
        bytes: bytes.to_vec(),
    };
    let result = write(
        tmp.path(),
        &[
            file("skills_repo/a/b.md", b"one"),
            file("skills_repo/a__b.md", b"two"),
        ],
    );
    assert!(matches!(result, Err(SyncError::Format(_))), "{result:?}");
    tmp.reset();
    write(
        tmp.path(),
        &[
            file("skills_repo/a/b.md", b"one"),
            file("skills_repo/a/b.md", b"two"),
        ],
    )
    .unwrap();
    assert_eq!(read(tmp.path()).unwrap()[0].bytes, b"two");
}

fn manifest_value(dir: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(dir.join("sync_manifest.json")).unwrap()).unwrap()
}

fn put_manifest(dir: &Path, value: &Value) {
    fs::write(dir.join("sync_manifest.json"), value.to_string()).unwrap();
}

fn blob_paths(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir.join("blobs"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths
}

fn assert_rejected_or_unchanged(dir: &Path, sources: &[SourceFile]) {
    if let Ok(files) = read(dir) {
        for f in files {
            let original = sources.iter().find(|s| s.key == f.key);
            if let Some(original) = original {
                assert_eq!(f.bytes, original.bytes, "{}", f.key);
            }
        }
    }
}

#[test]
fn tampered_bundles_are_rejected_or_return_hash_verified_files() {
    let tmp = ScratchDir::new("bundle-tamper");
    run_cases("bundle-tamper", 500, |_, rng| {
        tmp.reset();
        let mut sources: Vec<SourceFile> = Vec::new();
        while sources.len() < 3 {
            let s = source(rng);
            if sources
                .iter()
                .all(|o| o.key.replace('/', "__") != s.key.replace('/', "__"))
            {
                sources.push(s);
            }
        }
        write(tmp.path(), &sources).unwrap();
        let blobs = blob_paths(tmp.path());
        let victim = rng.pick(&blobs).clone();
        match rng.below(9) {
            0 => {
                let mut bytes = fs::read(&victim).unwrap();
                let at = rng.below(bytes.len());
                bytes[at] = bytes[at].wrapping_add(1 + rng.below(200) as u8);
                fs::write(&victim, bytes).unwrap();
                assert_rejected_or_unchanged(tmp.path(), &sources);
            }
            1 => {
                let bytes = fs::read(&victim).unwrap();
                fs::write(&victim, &bytes[..rng.below(bytes.len())]).unwrap();
                assert_rejected_or_unchanged(tmp.path(), &sources);
            }
            2 => {
                fs::remove_file(&victim).unwrap();
                assert!(matches!(read(tmp.path()), Err(SyncError::Io(_))));
            }
            3 => {
                let mut m = manifest_value(tmp.path());
                let entry = m["entries"]
                    .as_object_mut()
                    .unwrap()
                    .values_mut()
                    .next()
                    .unwrap();
                entry["hash"] = json!("sha256:0000000000000000");
                put_manifest(tmp.path(), &m);
                assert!(matches!(read(tmp.path()), Err(SyncError::HashMismatch(_))));
            }
            4 => {
                let other = blobs.iter().find(|b| **b != victim).unwrap();
                let (a, b) = (fs::read(&victim).unwrap(), fs::read(other).unwrap());
                fs::write(&victim, b).unwrap();
                fs::write(other, a).unwrap();
                let content = |blob: &Path| {
                    let name = blob.file_name().unwrap().to_string_lossy().into_owned();
                    sources
                        .iter()
                        .find(|s| format!("{}.enc", s.key.replace('/', "__")) == name)
                        .unwrap()
                        .bytes
                        .clone()
                };
                if content(&victim) == content(other) {
                    assert_rejected_or_unchanged(tmp.path(), &sources);
                } else {
                    assert!(read(tmp.path()).is_err());
                }
            }
            5 => {
                let wrong = kdf::derive_key_with("wrong", b"salt", 1);
                let result = read_bundle(tmp.path(), Credential::Key(&wrong), &roots());
                assert!(matches!(result, Err(SyncError::Crypto(_))));
                assert!(matches!(
                    read_bundle(tmp.path(), Credential::Key("short"), &roots()),
                    Err(SyncError::Crypto(FernetError::InvalidKey))
                ));
            }
            6 => {
                let mut m = manifest_value(tmp.path());
                let (first_key, entry) = {
                    let entries = m["entries"].as_object_mut().unwrap();
                    let key = entries.keys().next().unwrap().clone();
                    (key.clone(), entries.get_mut(&key).unwrap())
                };
                let already_binary = entry["encoding"] == "base64";
                entry["encoding"] = json!("base64");
                put_manifest(tmp.path(), &m);
                let was_empty = sources
                    .iter()
                    .any(|s| s.key == first_key && s.bytes.is_empty());
                assert_eq!(
                    read(tmp.path()).is_ok(),
                    was_empty || already_binary,
                    "{first_key}"
                );
            }
            7 => {
                let mut bytes = fs::read(&victim).unwrap();
                bytes.extend_from_slice(rng.garbage(10).as_bytes());
                fs::write(&victim, bytes).unwrap();
                assert_rejected_or_unchanged(tmp.path(), &sources);
            }
            _ => {
                let text = fs::read_to_string(tmp.path().join("sync_manifest.json")).unwrap();
                let mut chars: Vec<char> = text.chars().collect();
                for _ in 0..rng.range(1, 4) {
                    let at = rng.below(chars.len());
                    chars[at] = rng.garbage(1).chars().next().unwrap_or('x');
                }
                fs::write(
                    tmp.path().join("sync_manifest.json"),
                    chars.iter().collect::<String>(),
                )
                .unwrap();
                assert_rejected_or_unchanged(tmp.path(), &sources);
            }
        }
    });
}

#[test]
fn manifest_files_that_are_not_bundles_are_format_errors() {
    let tmp = ScratchDir::new("bundle-manifest");
    for (text, ok) in [
        ("{}", true),
        (r#"{"entries": {}}"#, true),
        (r#"{"entries": []}"#, false),
        (r#"{"entries": {"a": 1}}"#, false),
        (r#"{"entries": {"a": {"hash": "h"}}}"#, false),
        ("[]", true),
        ("[\"x\"]", false),
        ("null", false),
        ("", false),
        ("{\"entries\": {", false),
    ] {
        fs::write(tmp.path().join("sync_manifest.json"), text).unwrap();
        assert_eq!(read(tmp.path()).is_ok(), ok, "{text}");
    }
    fs::write(tmp.path().join("sync_manifest.json"), [0xff, 0xfe]).unwrap();
    assert!(read(tmp.path()).is_err());
    assert!(read(&tmp.path().join("missing")).is_err());
    assert!(read_bundle(tmp.path(), Credential::Passphrase("pw"), &roots()).is_err());
}

const UNSAFE_KEYS: &[&str] = &[
    "",
    "/",
    "/abs/path",
    "../escape",
    "a/../../escape",
    "./dot",
    "a\\b",
    "C:\\x",
    "keys/secret",
    "global/keys/secret",
    "global/sync_keyfile",
    "projects/p/sync_keyfile",
    "a/..",
];

#[test]
fn unsafe_keys_are_refused_on_write_and_on_read() {
    let tmp = ScratchDir::new("bundle-unsafe");
    for key in UNSAFE_KEYS {
        tmp.reset();
        let file = SourceFile {
            key: (*key).into(),
            category: "global".into(),
            project_name: None,
            bytes: b"x".to_vec(),
        };
        assert!(
            matches!(write(tmp.path(), &[file]), Err(SyncError::UnsafePath(_))),
            "write {key:?}"
        );
        tmp.reset();
        write(tmp.path(), &[source_with("global/ok.json", "global")]).unwrap();
        let mut m = manifest_value(tmp.path());
        let entry = m["entries"]["global/ok.json"].clone();
        m["entries"] = json!({ *key: entry });
        put_manifest(tmp.path(), &m);
        assert!(
            matches!(read(tmp.path()), Err(SyncError::UnsafePath(_))),
            "read {key:?}"
        );
    }
    for blob in ["../x.enc", "/etc/passwd", "blobs/../../x", ""] {
        tmp.reset();
        write(tmp.path(), &[source_with("global/ok.json", "global")]).unwrap();
        let mut m = manifest_value(tmp.path());
        m["entries"]["global/ok.json"]["encrypted_file"] = json!(blob);
        put_manifest(tmp.path(), &m);
        assert!(
            matches!(read(tmp.path()), Err(SyncError::UnsafePath(_))),
            "blob {blob:?}"
        );
    }
}

fn source_with(key: &str, category: &str) -> SourceFile {
    SourceFile {
        key: key.into(),
        category: category.into(),
        project_name: None,
        bytes: b"{}".to_vec(),
    }
}

#[test]
fn safe_relative_accepts_only_plain_relative_paths() {
    run_cases("safe-relative", 4000, |_, rng| {
        let raw = rng.tokens(
            &["a", "b", "/", "//", "..", ".", "\\", "é", " ", "-", "_"],
            8,
        );
        let model = !raw.contains('\\')
            && !raw.is_empty()
            && raw.split('/').all(|c| c != "..")
            && raw.split('/').any(|c| !c.is_empty() && c != ".")
            && !raw.starts_with('/')
            && !raw.starts_with("./");
        let got = safe_relative(&raw);
        assert_eq!(got.is_ok(), model, "{raw:?}");
        if let Ok(path) = got {
            assert!(path.is_relative());
            assert!(path
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_))));
        }
    });
}

#[test]
fn destinations_stay_under_their_targets() {
    let base = Path::new("/target-root");
    let targets = ImportTargets {
        config_dir: base.join("cfg"),
        skills_repo_dir: base.join("cfg/skills_repo"),
        projects: BTreeMap::from([("p".to_string(), base.join("project"))]),
    };
    run_cases("destinations", 4000, |_, rng| {
        let key = rng.tokens(
            &[
                "global",
                "skills_repo",
                "bin",
                "projects",
                "p",
                "q",
                "/",
                "a",
                "b.json",
                "..",
                ".",
                "\\",
                "",
            ],
            8,
        );
        let category = *rng.pick(&["global", "project", "other", ""]);
        if let Ok(Some(dest)) = destination_for(&key, category, &targets) {
            assert!(dest.starts_with(base), "{key:?} -> {dest:?}");
            assert!(
                dest.components()
                    .all(|c| !matches!(c, std::path::Component::ParentDir)),
                "{dest:?}"
            );
        }
    });
}

#[test]
fn import_writes_only_inside_the_targets_and_skips_unknown_projects() {
    let tmp = ScratchDir::new("bundle-import");
    run_cases("bundle-import", 150, |_, rng| {
        tmp.reset();
        let bundle = tmp.path().join("bundle");
        let mut sources: Vec<SourceFile> = Vec::new();
        while sources.len() < rng.range(1, 5) {
            let s = source(rng);
            if sources
                .iter()
                .all(|o| o.key.replace('/', "__") != s.key.replace('/', "__"))
            {
                sources.push(s);
            }
        }
        write(&bundle, &sources).unwrap();
        let targets = ImportTargets {
            config_dir: tmp.path().join("cfg"),
            skills_repo_dir: tmp.path().join("cfg/skills_repo"),
            projects: BTreeMap::from([("known".to_string(), tmp.path().join("proj"))]),
        };
        let include_projects = rng.chance(50);
        let report = import_bundle(
            &bundle,
            Credential::Key(&key_text()),
            &roots(),
            &targets,
            include_projects,
        )
        .unwrap();
        assert_eq!(report.written.len() + report.skipped.len(), sources.len());
        for entry in fs::read_dir(tmp.path()).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            assert!(
                ["bundle", "cfg", "proj"].contains(&name.as_str()),
                "stray {name}"
            );
        }
        for s in &sources {
            if s.category == "project" {
                assert!(report.skipped.contains(&s.key), "{}", s.key);
            }
        }
        let second = import_bundle(
            &bundle,
            Credential::Key(&key_text()),
            &roots(),
            &targets,
            include_projects,
        )
        .unwrap();
        assert_eq!(second.written, report.written, "idempotent");
    });
}
