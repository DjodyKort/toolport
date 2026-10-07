use super::*;
use crate::catalog_index::testkit::{entry, hold_sync, index_of, write_index};
use crate::plus::testutil::DataDirFx;
use serde_json::json;

#[test]
fn the_registry_index_tools_read_the_local_file_and_a_sync_refuses_while_one_runs() {
    let fx = DataDirFx::new("toolport-selfmcp", "registry-index");
    for (name, tier) in [("catalog_status", 1), ("catalog_sync", 2)] {
        let tool = find_tool(name).unwrap();
        assert_eq!((tool.tier, tool.gate), (tier, Gate::None), "{name}");
    }
    let path = fx.dir.join("plus").join("registry-index.json");
    write_index(
        &path,
        &index_of(
            &[entry("Alpha", "x"), entry("Beta", "y")],
            Some("2026-10-07T09:00:00Z"),
            true,
        ),
    );

    let status = call_tool("catalog_status", &json!({})).unwrap();
    assert_eq!(status["count"], 2);
    assert_eq!(status["lastSyncedAt"], "2026-10-07T09:00:00Z");
    assert_eq!(status["complete"], true);
    assert_eq!(status["running"], false);

    let wrong = call_tool("catalog_sync", &json!({"full": "yes"})).unwrap_err();
    assert_eq!(wrong.kind, "invalid_arguments");

    let _running = hold_sync(&path);
    assert_eq!(call_tool("catalog_status", &json!({})).unwrap()["running"], true);
    let refused = call_tool("catalog_sync", &json!({})).unwrap_err();
    assert_eq!(refused.kind, "conflict");
    assert!(refused.message.contains("already running"), "{}", refused.message);
}
