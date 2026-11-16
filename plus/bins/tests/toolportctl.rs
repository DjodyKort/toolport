use std::path::Path;
use std::process::{Command, Output};

const SERVER_ENTRY: &str = include_str!("../../fixtures/server_entry.json");

fn toolportctl(data_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_toolportctl"))
        .args(args)
        .env("TOOLPORT_DATA_DIR", data_dir)
        .env("TOOLPORT_REGISTRY", data_dir.join("registry.json"))
        .output()
        .expect("toolportctl starts")
}

fn seed_registry(data_dir: &Path) {
    let server: serde_json::Value = serde_json::from_str(SERVER_ENTRY).unwrap();
    let registry = serde_json::json!({
        "version": 3,
        "servers": [server],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
        "activeProfileId": "default",
    });
    std::fs::write(data_dir.join("registry.json"), registry.to_string()).unwrap();
}

#[test]
fn status_and_set_server_enabled_run_against_a_scratch_registry() {
    let data_dir = std::env::temp_dir().join(format!("toolportctl-test-{}", std::process::id()));
    std::fs::create_dir_all(&data_dir).unwrap();
    seed_registry(&data_dir);

    let output = toolportctl(&data_dir, &["status"]);
    assert!(output.status.success(), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["registry"]["servers"], 1);
    assert_eq!(report["registry"]["enabledServers"], 0);

    let output = toolportctl(&data_dir, &["set-server-enabled", "fixture-files", "true"]);
    assert!(output.status.success(), "{output:?}");
    let output = toolportctl(&data_dir, &["status"]);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["registry"]["enabledServers"], 1);

    let output = toolportctl(&data_dir, &["set-server-enabled", "missing", "true"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown server missing"));

    std::fs::remove_dir_all(&data_dir).unwrap();
}
