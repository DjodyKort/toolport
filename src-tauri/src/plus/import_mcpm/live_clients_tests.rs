//! A real mcpm root holds no `<client>.json` snapshots, so the clients are read from their own
//! config files under the home.

use super::run::{run, Action, RunOptions};
use super::run_tests::{
    client_files, desktop_config, live_opts, read_json, seed_clients, with_world, World,
};
use serde_json::{json, Value};

fn without_snapshots(w: &World) {
    seed_clients(w);
    for (id, _) in client_files(w) {
        std::fs::remove_file(w.root.join(format!("{id}.json"))).unwrap();
    }
    let left: Vec<String> = std::fs::read_dir(&w.root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            client_files(w)
                .iter()
                .any(|(id, _)| n == &format!("{id}.json"))
        })
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

fn bytes(w: &World) -> Vec<String> {
    client_files(w)
        .iter()
        .map(|(_, p)| std::fs::read_to_string(p).unwrap())
        .collect()
}

fn registry(w: &World) -> Value {
    serde_json::from_str(&w.registry_text().unwrap()).unwrap()
}

#[test]
fn every_client_with_mcpm_entries_is_switched_from_its_own_config() {
    with_world("live-switch", |w| {
        without_snapshots(w);

        let plan = run(&live_opts(w, false)).unwrap();

        assert_eq!(plan.clients.len(), 4, "{}", plan.summary());
        assert!(plan.clients.iter().all(|c| c.action == Action::Created));
        let reg = registry(w);
        for (id, path) in client_files(w) {
            let doc = read_json(&path);
            let servers = doc["mcpServers"].as_object().unwrap();
            assert!(servers.contains_key("toolport"), "{id}");
            assert_eq!(servers["toolport"]["env"]["TOOLPORT_CLIENT_ID"], id);
            assert!(servers.keys().all(|k| !k.starts_with("mcpm_")), "{id}");
            assert_eq!(doc["theme"], "FAKE-theme");
            assert_eq!(reg["clientScopes"][id], id);
            let change = plan.clients.iter().find(|c| c.id == id).unwrap();
            assert_eq!(
                change
                    .path
                    .as_deref()
                    .map(|p| std::fs::canonicalize(p).unwrap()),
                Some(std::fs::canonicalize(&path).unwrap())
            );
        }
        let profile = |id: &str| -> usize {
            reg["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == id)
                .unwrap()["enabledServerIds"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| *s != "toolport-plus-self")
                .count()
        };
        assert_eq!(profile("claude-code"), 18);
        assert_eq!(profile("claude-desktop"), 16);
        assert_eq!(reg["clientDiscovery"]["claude-code"], "full");
        assert_eq!(reg["clientDiscovery"]["claude-desktop"], "lazy");
        let desktop = read_json(&desktop_config(w));
        assert!(desktop["mcpServers"]["context7"].is_object());
    });
}

#[test]
fn live_clients_give_the_same_registry_as_snapshots() {
    let snapshot = {
        let mut out = Value::Null;
        with_world("live-vs", |w| {
            seed_clients(w);
            run(&live_opts(w, false)).unwrap();
            out = registry(w);
        });
        out
    };
    let live = {
        let mut out = Value::Null;
        with_world("live-vs", |w| {
            without_snapshots(w);
            run(&live_opts(w, false)).unwrap();
            out = registry(w);
        });
        out
    };
    let scrub = |mut v: Value| {
        v.as_object_mut().unwrap().remove("clientManagedEntries");
        v
    };
    assert_eq!(scrub(snapshot), scrub(live));
}

#[test]
fn a_dry_run_names_the_clients_it_would_switch_and_writes_nothing() {
    with_world("live-dry", |w| {
        without_snapshots(w);
        let before = bytes(w);

        let plan = run(&live_opts(w, true)).unwrap();

        assert_eq!(plan.clients.len(), 4);
        assert!(plan.clients.iter().all(|c| c.action == Action::Created));
        assert!(plan.clients.iter().all(|c| !c.removed.is_empty()));
        assert_eq!(bytes(w), before);
        assert!(w.registry_text().is_none());
    });
}

#[test]
fn a_second_run_after_a_live_switch_changes_nothing() {
    with_world("live-twice", |w| {
        without_snapshots(w);
        run(&live_opts(w, false)).unwrap();
        let written = bytes(w);
        let reg = w.registry_text().unwrap();

        let again = run(&live_opts(w, false)).unwrap();

        assert!(!again.changed(), "{}", again.summary());
        assert_eq!(bytes(w), written);
        assert_eq!(w.registry_text().unwrap(), reg);
    });
}

#[test]
fn a_client_without_mcpm_entries_is_left_alone() {
    with_world("live-bystander", |w| {
        without_snapshots(w);
        let cursor = w.home.join(".cursor/mcp.json");
        let own = json!({"mcpServers": {"fs": {"command": "npx", "args": ["-y", "FAKE-fs"]}}});
        std::fs::write(&cursor, own.to_string()).unwrap();
        let before = std::fs::read_to_string(&cursor).unwrap();

        let plan = run(&live_opts(w, false)).unwrap();

        assert!(plan.clients.iter().all(|c| c.id != "cursor"));
        assert_eq!(plan.clients.len(), 3);
        assert_eq!(std::fs::read_to_string(&cursor).unwrap(), before);
        let reg = registry(w);
        assert!(reg["clientScopes"].get("cursor").is_none_or(Value::is_null));
        assert!(plan
            .skipped_clients
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["clientId"] != "cursor"));
    });
}

#[test]
fn an_mcpm_launcher_given_by_absolute_path_is_still_an_mcpm_entry() {
    with_world("live-abs", |w| {
        without_snapshots(w);
        let cursor = w.home.join(".cursor/mcp.json");
        let doc = json!({"mcpServers": {"mcpm_context7": {
            "command": "/opt/FAKE/bin/mcpm", "args": ["run", "context7"]}}});
        std::fs::write(&cursor, doc.to_string()).unwrap();

        let plan = run(&live_opts(w, false)).unwrap();

        let change = plan.clients.iter().find(|c| c.id == "cursor").unwrap();
        assert_eq!(change.action, Action::Created);
        assert_eq!(change.removed, ["mcpm_context7"]);
        let after = read_json(&cursor);
        assert!(after["mcpServers"].get("mcpm_context7").is_none());
        assert!(after["mcpServers"]["toolport"].is_object());
    });
}

#[test]
fn a_snapshot_still_wins_over_the_live_config() {
    with_world("live-mixed", |w| {
        seed_clients(w);
        std::fs::remove_file(w.root.join("claude-desktop.json")).unwrap();
        std::fs::remove_file(w.root.join("gemini-cli.json")).unwrap();
        let snapshot_only = w.root.join("cursor.json");
        std::fs::write(
            &snapshot_only,
            json!({"mcpServers": {"mcpm_drawio": {"command": "mcpm", "args": ["run", "drawio"]}}})
                .to_string(),
        )
        .unwrap();
        let cursor = w.home.join(".cursor/mcp.json");
        std::fs::write(&cursor, json!({"mcpServers": {}}).to_string()).unwrap();

        let plan = run(&live_opts(w, false)).unwrap();

        assert_eq!(plan.clients.len(), 4, "{}", plan.summary());
        let reg = registry(w);
        let cursor_profile = reg["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "cursor")
            .unwrap();
        assert!(cursor_profile["enabledServerIds"]
            .as_array()
            .unwrap()
            .contains(&json!("drawio")));
    });
}

#[test]
fn an_unreadable_client_config_is_reported_and_the_others_still_switch() {
    with_world("live-broken", |w| {
        without_snapshots(w);
        std::fs::write(w.home.join(".cursor/mcp.json"), "{ not json").unwrap();

        let plan = run(&live_opts(w, false)).unwrap();

        assert_eq!(plan.clients.len(), 3);
        let notes = plan.warnings.as_array().unwrap();
        let note = notes
            .iter()
            .find(|n| n["kind"] == "client-unreadable")
            .unwrap();
        assert!(note["detail"].as_str().unwrap().starts_with("cursor:"));
    });
}

#[test]
fn skip_clients_reads_no_client_config() {
    with_world("live-skip", |w| {
        without_snapshots(w);
        let before = bytes(w);
        let opts = RunOptions {
            write_clients: false,
            ..live_opts(w, false)
        };

        let plan = run(&opts).unwrap();

        assert!(plan.clients.is_empty());
        assert_eq!(bytes(w), before);
        let reg = registry(w);
        assert!(reg["clientScopes"].as_object().is_none_or(|m| m.is_empty()));
    });
}

#[test]
fn the_ctl_switches_clients_of_a_root_without_snapshots() {
    with_world("live-ctl", |w| {
        without_snapshots(w);
        let root = w.root.to_string_lossy().into_owned();
        let args: Vec<String> = [
            "--json",
            "import",
            "mcpm",
            &root,
            "--home",
            &w.home.to_string_lossy(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());

        let code = crate::plus::ctl::run_with(&args, &mut out, &mut err);

        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
        let envelope: Value =
            serde_json::from_str(String::from_utf8(out).unwrap().lines().next().unwrap()).unwrap();
        let clients = envelope["data"]["clients"].as_array().unwrap();
        assert_eq!(clients.len(), 4);
        assert!(clients.iter().all(|c| c["action"] == "created"));
        for (_, path) in client_files(w) {
            assert!(read_json(&path)["mcpServers"]["toolport"].is_object());
        }
    });
}
