use super::*;
use crate::plus::direct::{LauncherOverride, TRADEOFF};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn cli(list: &[&str]) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&args(list), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn cli_json(list: &[&str]) -> (i32, Value) {
    let mut full = vec!["--json"];
    full.extend_from_slice(list);
    let (code, out, err) = cli(&full);
    let text = if out.trim().is_empty() { err } else { out };
    (code, serde_json::from_str(text.trim()).expect(&text))
}

struct World {
    home: PathBuf,
    data: PathBuf,
    _vars: Vec<crate::clients::EnvRestore>,
    _launcher: LauncherOverride,
}

impl Drop for World {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn world(test: impl FnOnce(&World)) {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let data = crate::registry::conduit_dir().unwrap();
        let home = std::env::temp_dir().join(format!("ctl-direct-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("bin")).unwrap();
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
            "TOOLPORT_REGISTRY",
            "CONDUIT_REGISTRY",
            "CONDUIT_DATA_DIR",
        ] {
            vars.push(crate::clients::EnvRestore::set(key, empty));
        }
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.clone()));
        let launcher = home
            .join("bin")
            .join(format!("toolportctl{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&launcher, "stand-in").unwrap();
        let w = World {
            _launcher: LauncherOverride::set(Some(&launcher.to_string_lossy())),
            home,
            data,
            _vars: vars,
        };
        std::fs::write(
            w.data.join("registry.json"),
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "servers": [
                    {
                        "id": "alpha", "name": "alpha", "transport": "stdio",
                        "command": "alpha-mcp", "args": ["--flag"],
                        "env": [{"key": "API_KEY", "secret": true}, {"key": "MODE", "value": "fast"}]
                    },
                    {"id": "beta", "name": "beta", "transport": "http", "url": "https://example.invalid/mcp"},
                    {"id": "gamma", "name": "gamma", "transport": "stdio", "command": "gamma-mcp"}
                ],
                "profiles": [{"id": "default", "name": "Default", "enabledServerIds": ["alpha"]}],
                "activeProfileId": "default"
            }))
            .unwrap(),
        )
        .unwrap();
        test(&w);
    });
}

fn claude_file(w: &World) -> PathBuf {
    w.home.join(".claude.json")
}

fn codex_file(w: &World) -> PathBuf {
    w.home.join(".codex/config.toml")
}

fn seed_claude(w: &World, servers: Value) {
    std::fs::write(
        claude_file(w),
        serde_json::to_string_pretty(&json!({"theme": "FAKE-theme", "mcpServers": servers}))
            .unwrap(),
    )
    .unwrap();
}

fn seed_codex(w: &World) {
    std::fs::create_dir_all(codex_file(w).parent().unwrap()).unwrap();
    std::fs::write(
        codex_file(w),
        "# FAKE keep\nmodel = \"fake\"\n\n[mcp_servers.stray]\ncommand = \"stray-mcp\"\n",
    )
    .unwrap();
}

fn claude_servers(w: &World) -> Vec<String> {
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(claude_file(w)).unwrap()).unwrap();
    let mut names: Vec<String> = doc["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
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

#[test]
fn direct_add_ls_rm_round_trip_on_a_json_and_a_toml_client() {
    world(|w| {
        seed_claude(w, json!({"stray": {"command": "stray-mcp"}}));
        seed_codex(w);
        for client in ["claude-code", "codex"] {
            let (code, value) = cli_json(&["client", "direct", "add", "alpha", "--client", client]);
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["command"], "client direct add");
            let data = &value["data"];
            assert_eq!(data["action"], "added");
            assert_eq!(data["entry"], "alpha");
            assert_eq!(data["server"], "alpha");
            assert_eq!(data["launcher"]["args"], json!(["direct", "run", "alpha"]));
            assert!(data["tradeoff"].as_str().unwrap().contains("bypasses"));
            assert!(data["backup"].is_string());
        }
        let claude = std::fs::read_to_string(claude_file(w)).unwrap();
        assert!(claude.contains("FAKE-theme") && claude.contains("stray-mcp"));
        assert!(claude.contains("\"direct\"") && claude.contains("\"run\""));
        assert!(!claude.contains("alpha-mcp"));
        let codex = std::fs::read_to_string(codex_file(w)).unwrap();
        assert!(codex.contains("# FAKE keep") && codex.contains("[mcp_servers.alpha]"));

        let (code, value) = cli_json(&["client", "direct", "ls"]);
        assert_eq!(code, 0, "{value}");
        let rows = value["data"]["entries"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|r| r["state"] == "ok" && r["server"] == "alpha"));
        let (_, one) = cli_json(&["client", "direct", "ls", "--client", "codex"]);
        assert_eq!(one["data"]["entries"].as_array().unwrap().len(), 1);
        let (code, text, _) = cli(&["client", "direct", "ls"]);
        assert_eq!(code, 0);
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(
            text.contains("claude-code") && text.contains("ok"),
            "{text}"
        );

        for client in ["claude-code", "codex"] {
            let (code, value) = cli_json(&["client", "direct", "rm", "alpha", "--client", client]);
            assert_eq!(code, 0, "{value}");
            assert_eq!(value["data"]["action"], "removed");
        }
        assert_eq!(claude_servers(w), ["stray"]);
        assert!(!std::fs::read_to_string(codex_file(w))
            .unwrap()
            .contains("alpha"));
        let (code, text, _) = cli(&["client", "direct", "ls"]);
        assert_eq!((code, text.trim()), (0, "No direct client entries."));
    });
}

#[test]
fn direct_add_and_rm_text_names_the_trade_off_and_honours_dry_run() {
    world(|w| {
        seed_claude(w, json!({}));
        let (home, data) = (tree(&w.home), tree(&w.data));
        let (code, text, _) = cli(&[
            "client",
            "direct",
            "add",
            "alpha",
            "--client",
            "claude-code",
            "--dry-run",
        ]);
        assert_eq!(code, 0);
        assert!(text.contains("Would add direct entry 'alpha'"), "{text}");
        assert!(text.contains(TRADEOFF), "{text}");
        assert!(text.contains("Dry run: nothing was written."), "{text}");
        let (_, value) = cli_json(&[
            "client",
            "direct",
            "add",
            "alpha",
            "--client",
            "claude-code",
            "--dry-run",
        ]);
        assert_eq!(
            (&value["data"]["dryRun"], &value["data"]["changed"]),
            (&json!(true), &json!(true))
        );
        assert_eq!((tree(&w.home), tree(&w.data)), (home, data));

        let (code, text, _) = cli(&[
            "client",
            "direct",
            "add",
            "alpha",
            "--client",
            "claude-code",
        ]);
        assert_eq!(code, 0);
        assert!(
            text.starts_with("Added direct entry 'alpha' for alpha to Claude Code"),
            "{text}"
        );
        assert!(
            text.contains(TRADEOFF) && text.contains("Restart Claude Code"),
            "{text}"
        );
        let (home, data) = (tree(&w.home), tree(&w.data));
        let (code, text, _) = cli(&[
            "client",
            "direct",
            "rm",
            "alpha",
            "--client",
            "claude-code",
            "--dry-run",
        ]);
        assert_eq!(code, 0);
        assert!(text.contains("Would remove direct entry 'alpha'"), "{text}");
        assert_eq!((tree(&w.home), tree(&w.data)), (home, data));
    });
}

#[test]
fn direct_commands_report_usage_and_refusals_with_the_documented_exit_codes() {
    world(|w| {
        seed_claude(w, json!({}));
        let (home, data) = (tree(&w.home), tree(&w.data));
        for bad in [
            &["client", "direct", "add", "alpha"][..],
            &["client", "direct", "add", "--client", "claude-code"][..],
            &[
                "client",
                "direct",
                "add",
                "alpha",
                "beta",
                "--client",
                "claude-code",
            ][..],
            &["client", "direct", "rm", "alpha"][..],
            &["client", "direct", "ls", "extra"][..],
            &[
                "client",
                "direct",
                "add",
                "alpha",
                "--client",
                "claude-code",
                "--bogus",
            ][..],
            &["client", "direct"][..],
            &["direct"][..],
            &["direct", "run"][..],
        ] {
            let (code, value) = cli_json(bad);
            assert_eq!(
                (code, value["error"]["code"].as_str()),
                (2, Some("usage")),
                "{bad:?}"
            );
        }
        let (code, _, err) = cli(&["client", "direct"]);
        assert_eq!(code, 2);
        assert!(err.contains("usage: client direct add|rm|ls"), "{err}");

        let (code, value) =
            cli_json(&["client", "direct", "add", "beta", "--client", "claude-code"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("refused")),
            "{value}"
        );
        let message = value["error"]["message"].as_str().unwrap();
        assert!(
            message.contains("remote") && message.contains("gateway"),
            "{message}"
        );
        let (code, value) =
            cli_json(&["client", "direct", "add", "nope", "--client", "claude-code"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
        let (code, value) = cli_json(&["client", "direct", "add", "alpha", "--client", "nope"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
        let (code, value) =
            cli_json(&["client", "direct", "rm", "alpha", "--client", "claude-code"]);
        assert_eq!(
            (code, &value["data"]["action"]),
            (0, &json!("absent")),
            "{value}"
        );
        assert_eq!((tree(&w.home), tree(&w.data)), (home, data));
    });
}

#[test]
fn client_sync_leaves_a_direct_entry_alone_and_still_prunes_foreign_ones() {
    world(|w| {
        seed_claude(
            w,
            json!({"stray": {"command": "stray-mcp"}, "gamma": {"command": "gamma-mcp"}}),
        );
        cli_json(&[
            "client",
            "direct",
            "add",
            "alpha",
            "--client",
            "claude-code",
        ]);
        assert_eq!(claude_servers(w), ["alpha", "gamma", "stray"]);
        let (code, value) = cli_json(&["client", "sync", "--client", "claude-code", "--dry-run"]);
        assert_eq!(code, 0, "{value}");
        let row = &value["data"]["clients"][0];
        assert_eq!(
            row["removed"],
            json!([
                {"name": "gamma", "reason": "redundant"},
                {"name": "stray", "reason": "orphan"}
            ])
        );
        assert_eq!(row["direct"], json!(["alpha"]));

        let (code, value) = cli_json(&["client", "sync", "--client", "claude-code"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(claude_servers(w), ["alpha", "toolport"]);
        assert_eq!(value["data"]["clients"][0]["direct"], json!(["alpha"]));
        let (code, text, _) = cli(&["client", "sync", "--client", "claude-code"]);
        assert_eq!(code, 0);
        assert!(
            text.contains("left 1 direct launcher entr(ies) alone"),
            "{text}"
        );
        assert_eq!(claude_servers(w), ["alpha", "toolport"]);
        let (_, listed) = cli_json(&["client", "direct", "ls"]);
        assert_eq!(listed["data"]["entries"][0]["state"], "ok");
    });
}

#[test]
fn client_sync_removes_a_launcher_entry_whose_server_is_gone_unless_orphans_are_kept() {
    world(|w| {
        seed_claude(w, json!({}));
        cli_json(&[
            "client",
            "direct",
            "add",
            "gamma",
            "--client",
            "claude-code",
        ]);
        cli_json(&["server", "uninstall", "gamma", "--keep-clients"]);
        let (_, listed) = cli_json(&["client", "direct", "ls"]);
        assert_eq!(listed["data"]["entries"][0]["state"], "orphan");

        let (_, kept) = cli_json(&[
            "client",
            "sync",
            "--client",
            "claude-code",
            "--keep-orphans",
        ]);
        assert_eq!(kept["data"]["clients"][0]["kept"], json!(["gamma"]));
        assert!(claude_servers(w).contains(&"gamma".to_string()));

        let (code, value) = cli_json(&["client", "sync", "--client", "claude-code"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(
            value["data"]["clients"][0]["removed"],
            json!([{"name": "gamma", "reason": "orphan"}])
        );
        assert_eq!(claude_servers(w), ["toolport"]);
        let (_, listed) = cli_json(&["client", "direct", "ls"]);
        assert_eq!(listed["data"]["entries"], json!([]));
    });
}

#[test]
fn server_uninstall_removes_its_direct_entries_in_every_client() {
    world(|w| {
        seed_claude(w, json!({"stray": {"command": "stray-mcp"}}));
        seed_codex(w);
        for client in ["claude-code", "codex"] {
            cli_json(&["client", "direct", "add", "alpha", "--client", client]);
        }
        cli_json(&[
            "client",
            "direct",
            "add",
            "gamma",
            "--client",
            "claude-code",
        ]);

        let (home, data) = (tree(&w.home), tree(&w.data));
        let (code, value) = cli_json(&["server", "uninstall", "alpha", "--dry-run"]);
        assert_eq!(code, 0, "{value}");
        let clients = value["data"]["clients"].as_array().unwrap();
        assert_eq!(clients.len(), 2);
        assert!(clients.iter().all(|c| c["removed"] == json!(["alpha"])));
        assert_eq!((tree(&w.home), tree(&w.data)), (home, data));

        let (code, text, _) = cli(&["server", "uninstall", "alpha"]);
        assert_eq!(code, 0, "{text}");
        assert!(
            text.contains("claude-code: removed alpha") && text.contains("codex: removed alpha"),
            "{text}"
        );
        assert_eq!(claude_servers(w), ["gamma", "stray"]);
        assert!(!std::fs::read_to_string(codex_file(w))
            .unwrap()
            .contains("alpha"));
        let (_, listed) = cli_json(&["client", "direct", "ls"]);
        let rows = listed["data"]["entries"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["server"], "gamma");
    });
}

#[test]
fn server_uninstall_keeps_direct_entries_with_keep_clients() {
    world(|w| {
        seed_claude(w, json!({}));
        cli_json(&[
            "client",
            "direct",
            "add",
            "alpha",
            "--client",
            "claude-code",
        ]);
        let (code, value) = cli_json(&["server", "uninstall", "alpha", "--keep-clients"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["clients"], json!([]));
        assert_eq!(claude_servers(w), ["alpha"]);
    });
}

#[test]
fn client_ls_status_and_doctor_report_direct_entries() {
    world(|w| {
        seed_claude(w, json!({"stray": {"command": "stray-mcp"}}));
        let (_, status) = cli_json(&["status"]);
        assert!(status["data"].get("directEntries").is_none());
        let (_, text, _) = cli(&["status"]);
        assert!(!text.contains("Direct entries"), "{text}");
        let (_, doctor) = cli_json(&["doctor"]);
        assert!(doctor["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["name"] != "directEntries"));

        cli_json(&[
            "client",
            "direct",
            "add",
            "alpha",
            "--client",
            "claude-code",
        ]);
        let (_, listed) = cli_json(&["client", "ls"]);
        let claude = listed["data"]["clients"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "claude-code")
            .unwrap();
        assert_eq!(claude["entries"], json!(["stray"]));
        assert_eq!(claude["launchers"][0]["entry"], "alpha");
        assert_eq!(claude["launchers"][0]["state"], "ok");
        let (_, text, _) = cli(&["client", "ls"]);
        assert!(
            text.contains("1 direct entries, 1 direct launcher entries"),
            "{text}"
        );

        let (_, status) = cli_json(&["status"]);
        assert_eq!(status["data"]["directEntries"], 1);
        let (_, text, _) = cli(&["status"]);
        assert!(text.contains("Direct entries:  1"), "{text}");
        let (code, doctor) = cli_json(&["doctor"]);
        assert_eq!(code, 0);
        let check = doctor["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "directEntries")
            .unwrap();
        assert_eq!(check["status"], "ok");

        std::fs::write(claude_file(w), "{}").unwrap();
        let (code, doctor) = cli_json(&["doctor"]);
        assert_eq!(
            code, 0,
            "a drifted direct entry warns, it does not fail the run"
        );
        let check = doctor["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "directEntries")
            .unwrap();
        assert_eq!(check["status"], "warn");
        assert!(check["detail"]
            .as_str()
            .unwrap()
            .contains("claude-code/alpha missing"));
    });
}

#[test]
fn client_import_does_not_offer_a_launcher_entry_back_as_a_server() {
    world(|w| {
        seed_claude(w, json!({"stray": {"command": "stray-mcp"}}));
        cli_json(&[
            "client",
            "direct",
            "add",
            "alpha",
            "--client",
            "claude-code",
        ]);
        let (code, value) = cli_json(&["client", "import", "claude-code"]);
        assert_eq!(code, 0, "{value}");
        let direct = value["data"]["direct"].as_array().unwrap();
        assert_eq!(direct.len(), 1, "{value}");
        assert_eq!(direct[0]["name"], "stray");
    });
}
