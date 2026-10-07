use super::*;
use crate::catalog_index::testkit::{entry, index_of, write_index};
use crate::plus::testutil::DataDirFx;
use serde_json::Value;

fn cli(list: &[&str]) -> (i32, String) {
    let args: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args, &mut out, &mut err);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn status_of_a_data_dir_without_an_index_is_an_empty_one() {
    let _fx = DataDirFx::new("toolportctl", "catalog-status-empty");

    let (code, text) = cli(&["catalog", "status"]);
    assert_eq!(code, 0);
    assert_eq!(text.trim(), "Registry index: 0 servers, never fully synced");

    let (code, json) = cli(&["--json", "catalog", "status"]);
    let value: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(code, 0);
    assert_eq!(value["command"], "catalog status");
    assert_eq!(value["data"]["count"], 0);
    assert_eq!(value["data"]["complete"], false);
    assert_eq!(value["data"]["running"], false);
    assert!(value["data"]["lastSyncedAt"].is_null());
}

#[test]
fn status_reads_the_index_the_sync_left_in_the_data_dir() {
    let fx = DataDirFx::new("toolportctl", "catalog-status");
    let index = fx.dir.join("plus").join("registry-index.json");
    write_index(
        &index,
        &index_of(
            &[entry("Alpha", "x"), entry("Beta", "y")],
            Some("2026-10-07T09:00:00Z"),
            true,
        ),
    );

    let (code, text) = cli(&["catalog", "status"]);
    assert_eq!(code, 0);
    assert_eq!(
        text.trim(),
        "Registry index: 2 servers, synced 2026-10-07T09:00:00Z"
    );

    let (_, json) = cli(&["--json", "catalog", "status"]);
    let value: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(value["data"]["count"], 2);
    assert_eq!(value["data"]["lastSyncedAt"], "2026-10-07T09:00:00Z");
    assert_eq!(value["data"]["complete"], true);
}

#[test]
fn a_search_offline_finds_what_the_index_holds() {
    let fx = DataDirFx::new("toolportctl", "catalog-offline");
    let index = fx.dir.join("plus").join("registry-index.json");
    write_index(
        &index,
        &index_of(
            &[entry("Zqx Registry Widget", "Only in the local index")],
            Some("2026-10-07T09:00:00Z"),
            true,
        ),
    );

    let (code, json) = cli(&["--json", "server", "search", "zqx", "--offline"]);

    let value: Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(code, 0);
    assert_eq!(value["data"]["results"][0]["name"], "Zqx Registry Widget");
    assert_eq!(value["data"]["results"][0]["source"], "registry");
}
