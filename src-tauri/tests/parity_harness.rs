mod common;
use common::parity::*;
use std::fs;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("parity-harness-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn golden(name: &str) -> (PathBuf, PathBuf) {
    let root = tmp(name);
    let case = root.join("skills/demo");
    write(&case, "tree/a.md", "alpha\nbeta\n");
    write(&case, "tree/cfg.json", "{\"b\":1,\"a\":[1,2]}\n");
    write(
        &case,
        "manifest.json",
        r#"{"comparison":"A","files":{"a.md":{"class":"A"},"cfg.json":{"class":"B"}}}"#,
    );
    let mut lock = String::from("# mcpm-sha test\n");
    for rel in ["skills/demo/tree/a.md", "skills/demo/tree/cfg.json", "skills/demo/manifest.json"] {
        lock.push_str(&format!("{}  {rel}\n", sha256_hex(&fs::read(root.join(rel)).unwrap())));
    }
    fs::write(root.join("PARITY.lock"), lock).unwrap();
    (root, case)
}

fn actual(name: &str) -> PathBuf {
    let a = tmp(name);
    write(&a, "a.md", "alpha\nbeta\n");
    write(&a, "cfg.json", "{\n \"a\": [1, 2],\n \"b\": 1\n}");
    a
}

#[test]
fn identical_tree_passes_and_json_is_canonical() {
    let (_, case) = golden("ok");
    let case = GoldenCase::load(&case).unwrap();
    compare_case(&case, &actual("ok-act"), &Normalizers::default()).unwrap();
}

#[test]
fn crlf_and_root_placeholders_are_normalized() {
    let (_, case) = golden("norm");
    let case = GoldenCase::load(&case).unwrap();
    let a = actual("norm-act");
    write(&a, "a.md", "alpha\r\nbeta\r\n");
    compare_case(&case, &a, &Normalizers::default()).unwrap();
    let n = Normalizers { root: Some("/x/root".into()), ..Default::default() };
    write(&a, "a.md", "alpha\nbeta\n");
    assert_eq!(normalize_text(b"at /x/root/f", &n), b"at <ROOT>/f");
}

#[test]
fn mutated_byte_fails_with_diff_naming_file() {
    let (_, case) = golden("mut");
    let case = GoldenCase::load(&case).unwrap();
    let a = actual("mut-act");
    write(&a, "a.md", "alpha\nbetx\n");
    let err = compare_case(&case, &a, &Normalizers::default()).unwrap_err();
    assert!(err.contains("a.md"), "{err}");
    assert!(err.contains("-beta") && err.contains("+betx"), "{err}");
}

#[test]
fn json_semantic_difference_fails() {
    let (_, case) = golden("json");
    let case = GoldenCase::load(&case).unwrap();
    let a = actual("json-act");
    write(&a, "cfg.json", "{\"a\":[2,1],\"b\":1}");
    let err = compare_case(&case, &a, &Normalizers::default()).unwrap_err();
    assert!(err.contains("cfg.json"), "{err}");
}

#[test]
fn missing_and_extra_files_fail() {
    let (_, case) = golden("files");
    let case = GoldenCase::load(&case).unwrap();
    let a = actual("files-act");
    fs::remove_file(a.join("a.md")).unwrap();
    write(&a, "extra.txt", "x");
    let err = compare_case(&case, &a, &Normalizers::default()).unwrap_err();
    assert!(err.contains("missing file: a.md"), "{err}");
    assert!(err.contains("unexpected file: extra.txt"), "{err}");
}

#[test]
fn lock_verifies_then_detects_mismatch_and_unlisted_files() {
    let (root, case) = golden("lock");
    assert_eq!(verify_lock(&root).unwrap(), 3);
    write(&case, "tree/a.md", "alphA\nbeta\n");
    let err = verify_lock(&root).unwrap_err();
    assert!(err.contains("sha256 mismatch: skills/demo/tree/a.md"), "{err}");
    write(&case, "tree/new.md", "n");
    assert!(verify_lock(&root).unwrap_err().contains("file not in PARITY.lock"));
    fs::remove_file(case.join("tree/cfg.json")).unwrap();
    assert!(verify_lock(&root).unwrap_err().contains("locked file missing"));
}

#[test]
fn bless_is_rejected_in_ci() {
    assert!(bless_guard(Some("1"), Some("true")).is_err());
    assert_eq!(bless_guard(Some("1"), None), Ok(true));
    assert_eq!(bless_guard(None, Some("true")), Ok(false));
    assert_eq!(bless_guard(Some("0"), Some("true")), Ok(false));
}

#[test]
fn class2_skipped_without_private_dir() {
    if std::env::var_os("PARITY_PRIVATE_DIR").is_none() {
        assert!(skip_class2());
        assert!(private_dir().is_none());
    }
}
