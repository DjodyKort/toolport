mod common;
use common::parity::*;

#[test]
fn vendored_goldens_match_parity_lock() {
    let root = fixtures_root();
    let n = verify_lock(&root).unwrap_or_else(|e| panic!("{e}"));
    assert!(n > 700, "expected the full golden set, got {n} locked files");
}

#[test]
fn every_case_has_a_loadable_manifest() {
    let cases = case_dirs(&fixtures_root()).unwrap();
    assert_eq!(cases.len(), 83);
    for dir in cases {
        let case = GoldenCase::load(&dir).unwrap_or_else(|e| panic!("{e}"));
        assert!(case.tree().is_dir() || case.classes.is_empty(), "{}", dir.display());
    }
}

#[test]
fn bless_is_not_requested_in_ci() {
    bless_requested().unwrap_or_else(|e| panic!("{e}"));
}
