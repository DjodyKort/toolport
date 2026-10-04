use super::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

pub(super) fn cli(list: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args(list), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

pub(super) fn cli_json(list: &[&str]) -> (i32, Value) {
    let mut full = vec!["--json"];
    full.extend_from_slice(list);
    let (code, out, err) = cli(&full);
    let text = if out.trim().is_empty() { err } else { out };
    (code, serde_json::from_str(text.trim()).expect(&text))
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Cursor {
    Scoped,
    Bare,
    Direct,
    Missing,
}

pub(super) struct World {
    pub home: PathBuf,
    pub data: PathBuf,
    _vars: Vec<crate::clients::EnvRestore>,
}

impl World {
    pub fn cursor_file(&self) -> PathBuf {
        self.home.join(".cursor").join("mcp.json")
    }

    pub fn registry_bytes(&self) -> Vec<u8> {
        std::fs::read(self.data.join("registry.json")).unwrap()
    }

    pub fn registry(&self) -> Value {
        serde_json::from_slice(&self.registry_bytes()).unwrap()
    }

    pub fn server_names(&self) -> Vec<String> {
        self.registry()["servers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap().to_string())
            .collect()
    }

    pub fn cursor_entries(&self) -> Vec<String> {
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(self.cursor_file()).unwrap()).unwrap();
        doc["mcpServers"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

pub(super) fn base_registry() -> Value {
    json!({
        "version": 1,
        "servers": [
            {"id": "alpha", "name": "alpha", "transport": "stdio", "command": "npx", "args": ["-y", "alpha-mcp"]},
            {"id": "beta", "name": "beta", "transport": "stdio", "command": "uvx", "args": ["beta-mcp", "--flag"]},
            {"id": "gamma", "name": "gamma", "transport": "http", "url": "https://gamma.example.com/mcp"}
        ],
        "profiles": [
            {"id": "work", "name": "work", "enabledServerIds": ["alpha", "beta"]},
            {"id": "play", "name": "play", "enabledServerIds": ["beta"]},
            {"id": "empty", "name": "empty", "enabledServerIds": []}
        ],
        "activeProfileId": "work"
    })
}

pub(super) fn world_with(registry: Value, cursor: Cursor, test: impl FnOnce(&World)) {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let data = crate::registry::conduit_dir().unwrap();
        let home = std::env::temp_dir().join(format!("ctl-profile-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        let empty = Path::new("");
        let mut vars = vec![
            crate::clients::EnvRestore::set("XDG_CONFIG_HOME", &home.join(".config")),
            crate::clients::EnvRestore::set("XDG_DATA_HOME", &home.join(".local/share")),
        ];
        for key in [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "GEMINI_CLI_HOME",
            "GOOSE_PATH_ROOT",
            "COPILOT_HOME",
            "KIMI_CODE_HOME",
        ] {
            vars.push(crate::clients::EnvRestore::set(key, empty));
        }
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        let w = World {
            home,
            data,
            _vars: vars,
        };
        std::fs::write(
            w.data.join("registry.json"),
            serde_json::to_string_pretty(&registry).unwrap(),
        )
        .unwrap();
        let direct = |name: &str, command: &str, args: Value| json!({name: {"command": command, "args": args}});
        let servers = match cursor {
            Cursor::Scoped | Cursor::Bare => direct("direct1", "npx", json!(["-y", "direct-mcp"])),
            Cursor::Direct => {
                let mut all = serde_json::Map::new();
                for entry in [
                    direct("direct1", "npx", json!(["-y", "direct-mcp"])),
                    direct(
                        "direct2",
                        "uvx",
                        json!(["tool-mcp", "--option", "value-that-is-long-enough-to-clip"]),
                    ),
                    direct("alpha", "npx", json!(["-y", "alpha-mcp"])),
                ] {
                    all.extend(entry.as_object().unwrap().clone());
                }
                Value::Object(all)
            }
            Cursor::Missing => Value::Null,
        };
        if cursor != Cursor::Missing {
            std::fs::write(
                w.cursor_file(),
                serde_json::to_string_pretty(
                    &json!({"theme": "FAKE-theme", "mcpServers": servers}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        if cursor == Cursor::Scoped {
            crate::registry_controller::connect_client_stdio("cursor", Some("work"), false)
                .unwrap();
        }
        test(&w);
    });
}

pub(super) fn world(cursor: Cursor, test: impl FnOnce(&World)) {
    world_with(base_registry(), cursor, test)
}

fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

fn untouched(w: &World, run: impl FnOnce()) {
    let (home, data) = (tree(&w.home), tree(&w.data));
    run();
    assert_eq!(tree(&w.home), home, "the home tree changed");
    assert_eq!(tree(&w.data), data, "the data dir changed");
}

#[test]
fn profile_ls_json_lists_servers_scopes_and_the_active_profile() {
    world(Cursor::Scoped, |_| {
        let (code, value) = cli_json(&["profile", "ls"]);
        assert_eq!(code, 0, "{value}");
        let data = &value["data"];
        assert_eq!(data["activeProfile"], "work");
        let rows = data["profiles"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["name"], "work");
        assert_eq!(rows[0]["active"], true);
        assert_eq!(rows[0]["clients"], json!(["cursor"]));
        assert_eq!(rows[0]["servers"][0]["target"], "npx -y alpha-mcp");
        assert_eq!(rows[0]["servers"][1]["target"], "uvx beta-mcp --flag");
        assert_eq!(rows[1]["clients"], json!([]));
    });
}

#[test]
fn profile_create_dry_run_writes_nothing_and_a_real_run_adds_the_profile() {
    world(Cursor::Scoped, |w| {
        untouched(w, || {
            let (code, text, _) = cli(&["profile", "create", "demo", "--dry-run"]);
            assert_eq!(code, 0);
            assert!(text.starts_with("Would create profile 'demo'."), "{text}");
            let (code, value) = cli_json(&["profile", "create", "demo", "--dry-run"]);
            assert_eq!(
                (
                    code,
                    value["data"]["created"].clone(),
                    value["data"]["dryRun"].clone()
                ),
                (0, json!(true), json!(true))
            );
        });
        let (code, value) = cli_json(&["profile", "create", "demo"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["created"], true);
        let names: Vec<String> = w.registry()["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["work", "play", "empty", "demo"]);
    });
}

#[test]
fn profile_create_on_an_existing_name_conflicts_and_force_changes_nothing() {
    world(Cursor::Scoped, |w| {
        let before = w.registry_bytes();
        let (code, value) = cli_json(&["profile", "create", "work"]);
        assert_eq!(code, 1);
        assert_eq!(value["error"]["code"], "conflict");
        let (code, value) = cli_json(&["profile", "create", "WORK", "--force"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["created"], false);
        assert_eq!(value["data"]["id"], "work");
        assert_eq!(w.registry_bytes(), before);
        let (code, _, err) = cli(&["profile", "create"]);
        assert_eq!(code, 2, "{err}");
        let (code, _, _) = cli(&["profile", "create", "   "]);
        assert_eq!(code, 2);
    });
}

#[test]
fn profile_edit_dry_run_writes_nothing_and_reports_the_plan() {
    world(Cursor::Scoped, |w| {
        untouched(w, || {
            let (code, text, _) = cli(&[
                "profile",
                "edit",
                "work",
                "--name",
                "job",
                "--add-server",
                "gamma",
                "--dry-run",
            ]);
            assert_eq!(code, 0);
            assert_eq!(
                text.trim(),
                "Updating profile 'work':\nName: work → job\nServers: 2 servers → 3 servers\n  + Added: gamma\n\nDry run: nothing was written."
            );
            let (code, value) = cli_json(&[
                "profile",
                "edit",
                "work",
                "--remove-server",
                "alpha",
                "--dry-run",
            ]);
            assert_eq!(code, 0);
            assert_eq!(value["data"]["servers"]["removed"], json!(["alpha"]));
            assert_eq!(value["data"]["dryRun"], true);
        });
    });
}

#[test]
fn profile_edit_changes_servers_and_renames_without_changing_the_id_or_scopes() {
    world(Cursor::Scoped, |w| {
        let (code, value) = cli_json(&[
            "profile",
            "edit",
            "work",
            "--name",
            "job",
            "--set-servers",
            "gamma,beta",
        ]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["servers"]["added"], json!(["gamma"]));
        assert_eq!(value["data"]["servers"]["removed"], json!(["alpha"]));
        let reg = w.registry();
        let job = reg["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "work")
            .unwrap();
        assert_eq!(job["name"], "job");
        let mut enabled: Vec<&str> = job["enabledServerIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        enabled.sort();
        assert_eq!(enabled, ["beta", "gamma"]);
        assert_eq!(reg["clientScopes"]["cursor"], "work");
        let (code, value) = cli_json(&["profile", "ls"]);
        assert_eq!(code, 0);
        assert_eq!(value["data"]["profiles"][0]["name"], "job");
        assert_eq!(value["data"]["profiles"][0]["clients"], json!(["cursor"]));
    });
}

#[test]
fn a_rejected_edit_leaves_the_registry_byte_identical() {
    world(Cursor::Scoped, |w| {
        let before = w.registry_bytes();
        for (list, code, kind) in [
            (
                vec!["profile", "edit", "work", "--add-server", "ghost"],
                1,
                "not_found",
            ),
            (
                vec![
                    "profile",
                    "edit",
                    "work",
                    "--name",
                    "fresh",
                    "--add-server",
                    "ghost",
                ],
                1,
                "not_found",
            ),
            (
                vec!["profile", "edit", "work", "--name", "play"],
                1,
                "conflict",
            ),
            (
                vec![
                    "profile",
                    "edit",
                    "work",
                    "--name",
                    "PLAY",
                    "--add-server",
                    "gamma",
                ],
                1,
                "conflict",
            ),
            (
                vec!["profile", "edit", "ghost", "--name", "x"],
                1,
                "not_found",
            ),
            (
                vec![
                    "profile",
                    "edit",
                    "work",
                    "--servers",
                    "alpha",
                    "--add-server",
                    "beta",
                ],
                2,
                "usage",
            ),
            (vec!["profile", "edit", "work", "--name", ""], 2, "usage"),
            (vec!["profile", "edit", "work", "--bogus"], 2, "usage"),
        ] {
            let (got, value) = cli_json(&list);
            assert_eq!(
                (got, value["error"]["code"].as_str()),
                (code, Some(kind)),
                "{list:?}: {value}"
            );
            assert_eq!(w.registry_bytes(), before, "{list:?}");
        }
    });
}

#[test]
fn a_no_op_edit_does_not_rewrite_the_registry() {
    world(Cursor::Scoped, |w| {
        let before = w.registry_bytes();
        let stamp = std::fs::metadata(w.data.join("registry.json"))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        for list in [
            vec!["profile", "edit", "work"],
            vec!["profile", "edit", "work", "--servers", "alpha,beta"],
            vec!["profile", "edit", "work", "--remove-server", "gamma"],
            vec!["profile", "edit", "work", "--name", "work"],
        ] {
            let (code, value) = cli_json(&list);
            assert_eq!(code, 0, "{list:?}: {value}");
            assert_eq!(value["data"]["changed"], false, "{list:?}");
        }
        assert_eq!(w.registry_bytes(), before);
        assert_eq!(
            std::fs::metadata(w.data.join("registry.json"))
                .unwrap()
                .modified()
                .unwrap(),
            stamp
        );
        let (_, text, _) = cli(&["profile", "edit", "work", "--remove-server", "gamma"]);
        assert!(text.starts_with("Warning: Server(s) not in profile: gamma\n\nUpdating profile 'work':\nNo changes specified"), "{text}");
    });
}

#[test]
fn profile_edit_on_a_registry_without_servers_names_the_next_step() {
    let mut empty = base_registry();
    empty["servers"] = json!([]);
    empty["profiles"] = json!([{"id": "work", "name": "work", "enabledServerIds": []}]);
    world_with(empty, Cursor::Bare, |w| {
        let before = w.registry_bytes();
        let (code, _, err) = cli(&["profile", "edit", "work", "--add-server", "alpha"]);
        assert_eq!(code, 1);
        assert_eq!(
            err.trim(),
            "toolportctl: No servers found in the registry\nInstall servers first with `toolportctl server install <name>`"
        );
        let (code, _, _) = cli(&["profile", "edit", "work", "--name", "job"]);
        assert_eq!(code, 0);
        assert_ne!(w.registry_bytes(), before);
    });
}

#[test]
fn profile_rm_dry_run_writes_nothing() {
    world(Cursor::Scoped, |w| {
        untouched(w, || {
            let (code, text, _) = cli(&["profile", "rm", "work", "--dry-run"]);
            assert_eq!(code, 0);
            assert_eq!(
                text.trim(),
                "Would remove profile 'work'.\n2 server(s) would remain in the registry\nWould remove 1 entry from 1 client(s):\n  • Cursor: gateway entry\nDry run: nothing was written."
            );
            let (code, value) = cli_json(&["profile", "rm", "work", "--dry-run"]);
            assert_eq!(code, 0);
            assert_eq!(value["data"]["dryRun"], true);
            assert_eq!(value["data"]["clients"][0]["client"], "cursor");
        });
    });
}

#[test]
fn profile_rm_removes_the_profile_and_cleans_the_clients_scoped_to_it() {
    world(Cursor::Scoped, |w| {
        assert!(w.cursor_entries().contains(&"toolport".to_string()));
        let (code, value) = cli_json(&["profile", "rm", "work"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["servers"], 2);
        assert_eq!(value["data"]["clients"][0]["client"], "cursor");
        assert_eq!(value["data"]["clients"][0]["error"], Value::Null);
        assert_eq!(w.cursor_entries(), ["direct1"]);
        let reg = w.registry();
        let ids: Vec<&str> = reg["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["play", "empty"]);
        assert_ne!(reg["activeProfileId"], "work");
        assert!(reg["clientScopes"].get("cursor").is_none(), "{reg}");
        assert_eq!(reg["servers"].as_array().unwrap().len(), 3);
    });
}

#[test]
fn profile_rm_no_clients_leaves_the_entry_and_says_so() {
    world(Cursor::Scoped, |w| {
        let (code, text, _) = cli(&["profile", "rm", "work", "--no-clients"]);
        assert_eq!(code, 0);
        assert!(text.contains("Left in place: cursor"), "{text}");
        assert!(w.cursor_entries().contains(&"toolport".to_string()));
        let (_, value) = cli_json(&["profile", "ls"]);
        assert_eq!(value["data"]["profiles"].as_array().unwrap().len(), 2);
    });
}

#[test]
fn profile_rm_refuses_the_last_profile_and_unknown_names_without_writing() {
    let mut single = base_registry();
    single["profiles"] = json!([{"id": "work", "name": "work", "enabledServerIds": ["alpha"]}]);
    world_with(single, Cursor::Scoped, |w| {
        let before = w.registry_bytes();
        let cursor = std::fs::read(w.cursor_file()).unwrap();
        let (code, value) = cli_json(&["profile", "rm", "work"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("conflict")),
            "{value}"
        );
        let (code, value) = cli_json(&["profile", "rm", "ghost"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found")),
            "{value}"
        );
        assert_eq!(cli(&["profile", "rm"]).0, 2);
        assert_eq!(w.registry_bytes(), before);
        assert_eq!(std::fs::read(w.cursor_file()).unwrap(), cursor);
    });
}

#[test]
fn profile_rm_with_a_failing_client_cleanup_exits_one_and_names_it() {
    world(Cursor::Scoped, |w| {
        std::fs::write(w.cursor_file(), "{ not json").unwrap();
        let (code, out, _) = cli(&["profile", "rm", "work"]);
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("Could not clean Cursor"), "{out}");
        let ids: Vec<String> = w.registry()["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_string())
            .collect();
        assert!(!ids.contains(&"work".to_string()));
    });
}

#[test]
fn client_edit_moves_a_client_to_another_profile() {
    world(Cursor::Scoped, |w| {
        untouched(w, || {
            let (code, value) = cli_json(&[
                "client",
                "edit",
                "cursor",
                "--set-profiles",
                "play",
                "--dry-run",
            ]);
            assert_eq!(code, 0, "{value}");
            assert_eq!(
                value["data"]["scope"],
                json!({"before": "work", "after": "play"})
            );
            assert_eq!(value["data"]["dryRun"], true);
        });
        let (code, value) = cli_json(&["client", "edit", "cursor", "--set-profiles", "play"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["profiles"]["added"], json!(["play"]));
        assert_eq!(value["data"]["profiles"]["removed"], json!(["work"]));
        assert_eq!(w.registry()["clientScopes"]["cursor"], "play");
        let config = std::fs::read_to_string(w.cursor_file()).unwrap();
        assert!(config.contains("play"), "{config}");
        assert!(config.contains("direct1"));
        assert!(config.contains("FAKE-theme"));
        let (code, value) = cli_json(&["client", "edit", "cursor", "--remove-profile", "play"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["followsActive"], "work");
        assert_eq!(w.registry()["clientScopes"]["cursor"], "");
    });
}

#[test]
fn client_edit_follows_one_profile_and_rejects_what_cannot_map() {
    world(Cursor::Scoped, |w| {
        let before = w.registry_bytes();
        let config = std::fs::read(w.cursor_file()).unwrap();
        for (list, code, kind) in [
            (
                vec!["client", "edit", "cursor", "--add-profile", "play"],
                1,
                "conflict",
            ),
            (
                vec!["client", "edit", "cursor", "--set-profiles", "play,empty"],
                2,
                "usage",
            ),
            (
                vec!["client", "edit", "cursor", "--add-profile", "ghost"],
                1,
                "not_found",
            ),
            (
                vec!["client", "edit", "ghost", "--add-profile", "play"],
                1,
                "not_found",
            ),
            (
                vec![
                    "client",
                    "edit",
                    "cursor",
                    "--add-profile",
                    "play",
                    "--remove-profile",
                    "work",
                ],
                2,
                "usage",
            ),
            (
                vec!["client", "edit", "cursor", "--add-server", "alpha"],
                2,
                "usage",
            ),
            (vec!["client", "edit", "cursor", "-e"], 2, "usage"),
            (vec!["client", "edit"], 2, "usage"),
        ] {
            let (got, value) = cli_json(&list);
            assert_eq!(
                (got, value["error"]["code"].as_str()),
                (code, Some(kind)),
                "{list:?}: {value}"
            );
        }
        assert_eq!(w.registry_bytes(), before);
        assert_eq!(std::fs::read(w.cursor_file()).unwrap(), config);
    });
}

#[test]
fn client_edit_without_options_and_with_the_current_profile_changes_nothing() {
    world(Cursor::Scoped, |w| {
        let before = w.registry_bytes();
        let config = std::fs::read(w.cursor_file()).unwrap();
        for list in [
            vec!["client", "edit", "cursor"],
            vec!["client", "edit", "cursor", "--add-profile", "work"],
            vec!["client", "edit", "cursor", "--set-profiles", "work"],
            vec!["client", "edit", "cursor", "--set-profiles", ""],
            vec!["client", "edit", "cursor", "--remove-profile", "play"],
        ] {
            let (code, value) = cli_json(&list);
            assert_eq!(code, 0, "{list:?}: {value}");
            assert_eq!(value["data"]["changed"], false, "{list:?}");
        }
        assert_eq!(w.registry_bytes(), before);
        assert_eq!(std::fs::read(w.cursor_file()).unwrap(), config);
    });
}

#[test]
fn client_edit_refuses_a_client_that_is_not_installed() {
    world(Cursor::Missing, |w| {
        std::fs::remove_dir_all(w.home.join(".cursor")).unwrap();
        let (code, value) = cli_json(&["client", "edit", "cursor", "--add-profile", "play"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found")),
            "{value}"
        );
        assert_eq!(
            value["error"]["message"],
            "Cursor installation not detected."
        );
    });
}

#[test]
fn client_edit_on_a_customized_entry_needs_force() {
    world(Cursor::Scoped, |w| {
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(w.cursor_file()).unwrap()).unwrap();
        let mut doc = doc;
        doc["mcpServers"]["toolport"]["command"] = json!("npx");
        doc["mcpServers"]["toolport"]["args"] =
            json!(["-y", "mcp-remote", "http://localhost:1/mcp"]);
        std::fs::write(w.cursor_file(), serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        let config = std::fs::read(w.cursor_file()).unwrap();
        let (code, value) = cli_json(&["client", "edit", "cursor", "--set-profiles", "play"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("conflict")),
            "{value}"
        );
        assert_eq!(std::fs::read(w.cursor_file()).unwrap(), config);
        let (code, value) = cli_json(&[
            "client",
            "edit",
            "cursor",
            "--set-profiles",
            "play",
            "--force",
        ]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(w.registry()["clientScopes"]["cursor"], "play");
    });
}

#[test]
fn client_import_previews_without_writing() {
    world(Cursor::Direct, |w| {
        untouched(w, || {
            let (code, value) = cli_json(&["client", "import", "cursor"]);
            assert_eq!(code, 0, "{value}");
            let data = &value["data"];
            assert_eq!(data["selected"], false);
            let statuses: Vec<(&str, &str)> = data["direct"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| (d["name"].as_str().unwrap(), d["status"].as_str().unwrap()))
                .collect();
            assert_eq!(
                statuses,
                [
                    ("alpha", "already"),
                    ("direct1", "importable"),
                    ("direct2", "importable")
                ]
            );
            let (_, text, _) = cli(&["client", "import", "cursor"]);
            assert!(
                text.contains("  alpha - npx -y alpha-mcp (already in the registry)"),
                "{text}"
            );
            assert!(
                text.contains("toolportctl client import cursor --all"),
                "{text}"
            );
        });
    });
}

#[test]
fn client_import_adds_the_selected_servers_and_leaves_the_client_alone() {
    world(Cursor::Direct, |w| {
        let config = std::fs::read(w.cursor_file()).unwrap();
        untouched(w, || {
            let (code, value) = cli_json(&["client", "import", "cursor", "--all", "--dry-run"]);
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["data"]["imported"].as_array().unwrap().len(), 2);
            assert_eq!(value["data"]["skipped"][0]["name"], "alpha");
        });
        let (code, value) =
            cli_json(&["client", "import", "cursor", "--select", "direct1,Direct2"]);
        assert_eq!(code, 0, "{value}");
        let names = w.server_names();
        assert_eq!(names, ["alpha", "beta", "gamma", "direct1", "direct2"]);
        assert_eq!(std::fs::read(w.cursor_file()).unwrap(), config);
        let before = w.registry_bytes();
        let (code, value) = cli_json(&["client", "import", "cursor", "--all"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["imported"], json!([]));
        assert_eq!(value["data"]["skipped"].as_array().unwrap().len(), 3);
        assert_eq!(w.registry_bytes(), before);
    });
}

#[test]
fn client_import_into_a_profile_creates_it_and_never_copies_env_values() {
    world(Cursor::Direct, |w| {
        let secret = "FAKE-never-copy-0123456789";
        let mut doc: Value =
            serde_json::from_str(&std::fs::read_to_string(w.cursor_file()).unwrap()).unwrap();
        doc["mcpServers"]["direct1"]["env"] = json!({"API_KEY": secret, "MODE": "fast"});
        std::fs::write(w.cursor_file(), serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        let (code, out, err) = cli(&[
            "client",
            "import",
            "cursor",
            "--select",
            "direct1,alpha",
            "--profile",
            "cursor",
        ]);
        assert_eq!(code, 0, "{out}{err}");
        assert!(!out.contains(secret) && !err.contains(secret));
        assert!(!String::from_utf8_lossy(&w.registry_bytes()).contains(secret));
        assert!(
            out.contains("Profile 'cursor' created with 2 server(s)."),
            "{out}"
        );
        assert!(
            out.contains("toolportctl secret set direct1 API_KEY"),
            "{out}"
        );
        let reg = w.registry();
        let profile = reg["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "cursor")
            .unwrap();
        assert_eq!(profile["enabledServerIds"].as_array().unwrap().len(), 2);
        let (code, value) = cli_json(&["client", "import", "cursor", "--select", "ghost"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found")),
            "{value}"
        );
        let (code, value) =
            cli_json(&["client", "import", "cursor", "--all", "--select", "direct1"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (2, Some("usage")),
            "{value}"
        );
    });
}

#[test]
fn client_import_refuses_a_client_without_a_config() {
    world(Cursor::Missing, |_| {
        let (code, value) = cli_json(&["client", "import", "cursor"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found")),
            "{value}"
        );
    });
}

#[test]
fn plus_handlers_run_the_same_core_as_the_ctl() {
    world(Cursor::Scoped, |w| {
        let listed = crate::plus::dispatch("plus.profile.list", json!({})).unwrap();
        assert_eq!(listed, cli_json(&["profile", "ls"]).1["data"]);
        let created = crate::plus::dispatch(
            "plus.profile.create",
            json!({"name": "demo", "dryRun": true}),
        )
        .unwrap();
        assert_eq!(created["dryRun"], true);
        assert_eq!(w.registry()["profiles"].as_array().unwrap().len(), 3);
        let edited = crate::plus::dispatch(
            "plus.profile.edit",
            json!({"profile": "work", "name": "job", "addServers": ["gamma"]}),
        )
        .unwrap();
        assert_eq!(edited["servers"]["added"], json!(["gamma"]));
        assert_eq!(w.registry()["profiles"][0]["name"], "job");
        let error = crate::plus::dispatch(
            "plus.profile.edit",
            json!({"profile": "job", "servers": "a", "addServers": "b"}),
        )
        .unwrap_err();
        assert!(error.contains("one of"), "{error}");
        let moved = crate::plus::dispatch(
            "plus.client.edit",
            json!({"client": "cursor", "setProfiles": "play"}),
        )
        .unwrap();
        assert_eq!(moved["scope"], json!({"before": "work", "after": "play"}));
        let preview =
            crate::plus::dispatch("plus.client.import", json!({"client": "cursor"})).unwrap();
        assert_eq!(preview["selected"], false);
        let removed =
            crate::plus::dispatch("plus.profile.remove", json!({"profile": "job"})).unwrap();
        assert_eq!(removed["name"], "job");
        assert_eq!(w.registry()["profiles"].as_array().unwrap().len(), 2);
        assert_eq!(
            crate::plus::dispatch("plus.profile.remove", json!({})).unwrap_err(),
            "profile is required"
        );
    });
}

#[test]
fn client_import_masks_inline_credentials_and_refuses_to_copy_them() {
    world(Cursor::Direct, |w| {
        let secrets = [
            "FAKE-inline-cred-0001",
            "FAKE-inline-cred-0002",
            "FAKE-inline-cred-0003",
            "FAKE-inline-cred-0004",
        ];
        let mut doc: Value =
            serde_json::from_str(&std::fs::read_to_string(w.cursor_file()).unwrap()).unwrap();
        doc["mcpServers"]["cred-arg"] =
            json!({"command": "tool", "args": [format!("--api-key={}", secrets[0])]});
        doc["mcpServers"]["cred-split"] =
            json!({"command": "tool", "args": ["--token", secrets[1]]});
        doc["mcpServers"]["cred-url"] =
            json!({"url": format!("https://user:{}@host.example/mcp", secrets[2])});
        doc["mcpServers"]["cred-query"] =
            json!({"url": format!("https://host.example/mcp?token={}", secrets[3])});
        std::fs::write(w.cursor_file(), serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        let before = w.registry_bytes();
        for list in [
            vec!["client", "import", "cursor"],
            vec!["client", "import", "cursor", "--all", "--dry-run"],
        ] {
            let (code, out, err) = cli(&list);
            assert_eq!(code, 0, "{list:?}: {out}{err}");
            let (_, json_out, _) = cli(&[&["--json"][..], &list[..]].concat());
            for secret in secrets {
                assert!(
                    !out.contains(secret) && !err.contains(secret) && !json_out.contains(secret),
                    "{list:?}"
                );
            }
            if list.len() == 3 {
                assert!(out.contains("cred-arg - tool <redacted> (holds"), "{out}");
                assert!(out.contains("tool --token <redacted>"), "{out}");
                assert!(out.contains("https://<redacted>@host.example/mcp"), "{out}");
                assert!(
                    out.contains("(holds an inline credential, not imported)"),
                    "{out}"
                );
                assert_eq!(w.registry_bytes(), before);
            }
        }
        let (code, value) = cli_json(&["client", "import", "cursor", "--all"]);
        assert_eq!(code, 0, "{value}");
        let imported: Vec<&str> = value["data"]["imported"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["name"].as_str().unwrap())
            .collect();
        assert_eq!(imported, ["direct1", "direct2"]);
        let skipped: Vec<&str> = value["data"]["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["reason"] == "it holds an inline credential")
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            skipped,
            ["cred-arg", "cred-query", "cred-split", "cred-url"]
        );
        let stored = String::from_utf8_lossy(&w.registry_bytes()).into_owned();
        for secret in secrets {
            assert!(!stored.contains(secret));
        }
    });
}
