use super::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[path = "../../../tests/fixtures/update_source.rs"]
mod update_source;

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
        let home = std::env::temp_dir().join(format!("ctl-server-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
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
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "servers": [
                    {
                        "id": "alpha", "name": "alpha", "transport": "stdio",
                        "command": "alpha-mcp", "args": ["--flag"],
                        "env": [{"key": "API_KEY", "secret": true}, {"key": "MODE", "value": "fast"}]
                    },
                    {"id": "beta", "name": "beta", "transport": "http", "url": "https://example.invalid/mcp"}
                ],
                "profiles": [
                    {"id": "default", "name": "Default", "enabledServerIds": ["alpha"]},
                    {"id": "work", "name": "Work", "enabledServerIds": []}
                ],
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

fn seed_claude(w: &World) {
    std::fs::write(
        claude_file(w),
        serde_json::to_string_pretty(&json!({
            "theme": "FAKE-theme",
            "mcpServers": {
                "alpha": {"command": "alpha-mcp"},
                "stray": {"command": "stray-mcp"}
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

fn claude_servers(w: &World) -> Vec<String> {
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(claude_file(w)).unwrap()).unwrap();
    doc["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
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
fn server_search_offline_golden() {
    world(|_| {
        let (code, out, _) = cli(&["server", "search", "postgresql", "--offline"]);
        assert_eq!(code, 0);
        let first = out.lines().next().unwrap();
        assert!(first.starts_with("PostgreSQL "), "{out}");
        assert!(
            first.contains("stdio") && first.contains("curated"),
            "{out}"
        );
        let (code, value) = cli_json(&[
            "server",
            "search",
            "postgresql",
            "--offline",
            "--limit",
            "1",
        ]);
        assert_eq!(code, 0);
        assert_eq!(value["command"], "server search");
        assert_eq!(value["data"]["query"], "postgresql");
        assert_eq!(value["data"]["results"].as_array().unwrap().len(), 1);
        assert_eq!(value["data"]["results"][0]["name"], "PostgreSQL");
        let (_, none) = cli_json(&["server", "search", "zzzznotacatalogword", "--offline"]);
        assert_eq!(none["data"]["results"], json!([]));
        let (code, text, _) = cli(&["server", "search", "zzzznotacatalogword", "--offline"]);
        assert_eq!((code, text.trim()), (0, "No matches."));
        assert_eq!(cli(&["server", "search", "--limit", "x"]).0, 2);
    });
}

#[test]
fn server_install_from_catalog_then_conflict() {
    world(|w| {
        let (code, value) = cli_json(&["server", "install", "postgresql", "--offline"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["name"], "PostgreSQL");
        let id = value["data"]["id"].as_str().unwrap().to_string();
        let reg: Value =
            serde_json::from_str(&std::fs::read_to_string(w.data.join("registry.json")).unwrap())
                .unwrap();
        assert!(reg["servers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == id.as_str() && s["source"] == "catalog:curated"));
        let (code, value) = cli_json(&["server", "install", "PostgreSQL", "--offline"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("conflict"))
        );
        let (code, value) = cli_json(&["server", "install", "zzzznotacatalogword", "--offline"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
        assert_eq!(cli(&["server", "install"]).0, 2);
    });
}

#[test]
fn the_handshake_switches_are_set_through_server_edit_and_shown_by_info() {
    world(|w| {
        let (_, info) = cli_json(&["server", "info", "alpha"]);
        assert_eq!(info["data"]["declareClientCapabilities"], false);
        assert_eq!(info["data"]["forwardInstructions"], false);
        let (_, text, _) = cli(&["server", "info", "alpha"]);
        assert!(
            !text.contains("Declares:") && !text.contains("Forwards:"),
            "{text}"
        );

        let (code, value) = cli_json(&[
            "server",
            "edit",
            "alpha",
            "--declare-client-capabilities",
            "on",
            "--forward-instructions",
            "yes",
        ]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(
            value["data"],
            json!({"id": "alpha", "changed": ["declareClientCapabilities", "forwardInstructions"]})
        );
        let raw: Value =
            serde_json::from_str(&std::fs::read_to_string(w.data.join("registry.json")).unwrap())
                .unwrap();
        assert_eq!(raw["servers"][0]["declareClientCapabilities"], true);
        assert_eq!(raw["servers"][0]["forwardInstructions"], true);
        assert_eq!(
            raw["servers"][0]["command"], "alpha-mcp",
            "the rest of the entry is untouched"
        );
        let (_, info) = cli_json(&["server", "info", "alpha"]);
        assert_eq!(info["data"]["declareClientCapabilities"], true);
        assert_eq!(info["data"]["forwardInstructions"], true);
        let (_, text, _) = cli(&["server", "info", "alpha"]);
        assert!(
            text.contains("\nDeclares:   client capabilities\nForwards:   instructions"),
            "{text}"
        );

        let (_, same) = cli_json(&["server", "edit", "alpha", "--forward-instructions", "on"]);
        assert_eq!(same["data"]["changed"], json!([]));

        let (_, one) = cli_json(&["server", "edit", "alpha", "--forward-instructions", "off"]);
        assert_eq!(one["data"]["changed"], json!(["forwardInstructions"]));
        let (_, info) = cli_json(&["server", "info", "alpha"]);
        assert_eq!(info["data"]["declareClientCapabilities"], true);
        assert_eq!(info["data"]["forwardInstructions"], false);

        let (_, off) = cli_json(&[
            "server",
            "edit",
            "alpha",
            "--declare-client-capabilities",
            "off",
        ]);
        assert_eq!(off["data"]["changed"], json!(["declareClientCapabilities"]));
        let raw = std::fs::read_to_string(w.data.join("registry.json")).unwrap();
        assert!(
            !raw.contains("declareClientCapabilities") && !raw.contains("forwardInstructions"),
            "off leaves no field behind: {raw}"
        );

        let before = std::fs::read(w.data.join("registry.json")).unwrap();
        for bad in ["maybe", "", "2"] {
            let (code, value) =
                cli_json(&["server", "edit", "alpha", "--forward-instructions", bad]);
            assert_eq!(
                (code, value["error"]["code"].as_str()),
                (2, Some("usage")),
                "{bad:?}: {value}"
            );
            assert_eq!(
                value["error"]["message"],
                format!("--forward-instructions: '{bad}' is not on or off")
            );
        }
        let (code, _) = cli_json(&["server", "edit", "alpha", "--forward-instructions"]);
        assert_eq!(code, 2, "a switch without a value is a usage error");
        assert_eq!(std::fs::read(w.data.join("registry.json")).unwrap(), before);
    });
}

#[test]
fn server_new_info_edit_goldens() {
    world(|_| {
        let (code, value) = cli_json(&[
            "server",
            "new",
            "gamma",
            "--command",
            "gamma-mcp",
            "--arg",
            "-y",
            "--arg",
            "pkg",
        ]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"], json!({"id": "gamma", "name": "gamma"}));
        let (code, text, _) = cli(&["server", "info", "gamma"]);
        assert_eq!(code, 0);
        assert_eq!(
            text.trim_end(),
            "gamma\nId:         gamma\nTransport:  stdio\nTarget:     gamma-mcp -y pkg\nSource:     manual\nEnv keys:   -\nProfiles:   -"
        );
        let (code, value) = cli_json(&[
            "server", "edit", "gamma", "--arg", "other", "--cwd", "/tmp/x",
        ]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(
            value["data"],
            json!({"id": "gamma", "changed": ["args", "cwd"]})
        );
        let (_, info) = cli_json(&["server", "info", "gamma"]);
        assert_eq!(info["data"]["args"], json!(["other"]));
        assert_eq!(info["data"]["cwd"], "/tmp/x");
        assert_eq!(info["data"]["command"], "gamma-mcp");
        let (code, value) = cli_json(&[
            "server",
            "edit",
            "gamma",
            "--url",
            "https://example.invalid/m",
        ]);
        assert_eq!(code, 0, "{value}");
        let (_, info) = cli_json(&["server", "info", "GAMMA"]);
        assert_eq!(info["data"]["transport"], "http");
        let (_, alpha) = cli_json(&["server", "info", "alpha"]);
        assert_eq!(
            alpha["data"]["env"],
            json!([{"key": "API_KEY", "secret": true}, {"key": "MODE", "secret": false}])
        );
        assert_eq!(alpha["data"]["profiles"], json!(["default"]));
        let (code, value) = cli_json(&["server", "new", "gamma", "--command", "x"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("conflict"))
        );
        for bad in [
            &["server", "new", "delta"][..],
            &["server", "edit", "gamma"][..],
            &["server", "info"][..],
            &["server", "uninstall"][..],
        ] {
            assert_eq!(cli(bad).0, 2, "{bad:?}");
        }
        let (code, value) = cli_json(&["server", "edit", "ghost", "--name", "x"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
    });
}

#[test]
fn inspect_reports_missing_targets_and_empty_profiles() {
    world(|_| {
        let (code, value) = cli_json(&["inspect", "ghost"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
        let (code, value) = cli_json(&["profile", "inspect", "work"]);
        assert_eq!(code, 0);
        assert_eq!(value["data"], json!({"profile": "work", "servers": []}));
        let (code, text, _) = cli(&["profile", "inspect", "work"]);
        assert_eq!((code, text.trim()), (0, "No servers enabled."));
        let (code, value) = cli_json(&["profile", "inspect", "ghost"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
        let (code, value) = cli_json(&["inspect", "alpha"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("inspect"))
        );
    });
}

#[test]
fn client_sync_dry_run_writes_nothing() {
    world(|w| {
        seed_claude(w);
        let before_home = tree(&w.home);
        let before_data = tree(&w.data);
        let (code, value) = cli_json(&["client", "sync", "--client", "claude-code", "--dry-run"]);
        assert_eq!(code, 0, "{value}");
        let row = &value["data"]["clients"][0];
        assert_eq!(value["data"]["dryRun"], true);
        assert_eq!(row["gateway"], "would-install");
        assert_eq!(
            row["removed"],
            json!([
                {"name": "alpha", "reason": "redundant"},
                {"name": "stray", "reason": "orphan"}
            ])
        );
        assert_eq!(row["backups"], json!([]));
        let (code, text, _) = cli(&[
            "client",
            "sync",
            "--client",
            "claude-code",
            "--dry-run",
            "--keep-orphans",
        ]);
        assert_eq!(code, 0);
        assert_eq!(
            text.trim(),
            "claude-code: gateway would-install, would remove alpha (redundant), kept 1 orphan(s)"
        );
        assert_eq!(tree(&w.home), before_home);
        assert_eq!(tree(&w.data), before_data);
    });
}

#[test]
fn client_sync_keeps_orphans_on_request_and_backs_up() {
    world(|w| {
        seed_claude(w);
        let (code, value) = cli_json(&[
            "client",
            "sync",
            "--client",
            "claude-code",
            "--keep-orphans",
        ]);
        assert_eq!(code, 0, "{value}");
        let row = &value["data"]["clients"][0];
        assert_eq!(row["gateway"], "installed");
        assert_eq!(row["kept"], json!(["stray"]));
        let backups = row["backups"].as_array().unwrap();
        assert!(!backups.is_empty());
        for b in backups {
            assert!(Path::new(b.as_str().unwrap()).is_file());
        }
        let mut servers = claude_servers(w);
        servers.sort();
        assert_eq!(servers, ["stray", "toolport"]);
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(claude_file(w)).unwrap()).unwrap();
        assert_eq!(doc["theme"], "FAKE-theme");
        let reg: Value =
            serde_json::from_str(&std::fs::read_to_string(w.data.join("registry.json")).unwrap())
                .unwrap();
        assert!(reg["clientManagedEntries"]["claude-code"].is_object());
    });
}

#[test]
fn client_sync_prunes_orphans_and_is_idempotent() {
    world(|w| {
        seed_claude(w);
        let (code, value) = cli_json(&["client", "sync", "--client", "claude-code"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(claude_servers(w), ["toolport"]);
        let backups = value["data"]["clients"][0]["backups"].as_array().unwrap();
        assert!(backups
            .iter()
            .all(|b| Path::new(b.as_str().unwrap()).is_file()));
        let original = std::fs::read_to_string(backups.last().unwrap().as_str().unwrap()).unwrap();
        assert!(
            original.contains("stray-mcp"),
            "backup holds the pre-prune config"
        );
        let (code, again) = cli_json(&["client", "sync"]);
        assert_eq!(code, 0, "{again}");
        let row = &again["data"]["clients"][0];
        assert_eq!(row["client"], "claude-code");
        assert_eq!(row["removed"], json!([]));
        assert_eq!(row["backups"], json!([]));
        let (code, value) = cli_json(&["client", "sync", "--client", "ghost"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
        assert_eq!(cli(&["client", "sync", "--bogus"]).0, 2);
    });
}

#[test]
fn client_ls_lists_direct_entries() {
    world(|w| {
        seed_claude(w);
        let (code, value) = cli_json(&["client", "ls"]);
        assert_eq!(code, 0);
        let rows = value["data"]["clients"].as_array().unwrap();
        let row = rows.iter().find(|r| r["id"] == "claude-code").unwrap();
        assert_eq!(row["gateway"], "absent");
        assert_eq!(row["entries"], json!(["alpha", "stray"]));
        assert_eq!(row["managed"], false);
    });
}

#[test]
fn server_uninstall_dry_run_writes_nothing() {
    world(|w| {
        seed_claude(w);
        let before_home = tree(&w.home);
        let before_data = tree(&w.data);
        let (code, text, _) = cli(&["server", "uninstall", "alpha", "--dry-run"]);
        assert_eq!(code, 0);
        assert_eq!(
            text.trim(),
            "Would uninstall alpha (alpha).\n  claude-code: would remove alpha"
        );
        assert_eq!(tree(&w.home), before_home);
        assert_eq!(tree(&w.data), before_data);
    });
}

#[test]
fn server_uninstall_propagates_to_client_configs() {
    world(|w| {
        seed_claude(w);
        let (code, value) = cli_json(&["server", "uninstall", "alpha"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["clients"][0]["removed"], json!(["alpha"]));
        let backup = value["data"]["clients"][0]["backup"].as_str().unwrap();
        assert!(std::fs::read_to_string(backup)
            .unwrap()
            .contains("alpha-mcp"));
        assert_eq!(claude_servers(w), ["stray"]);
        let (_, ls) = cli_json(&["server", "ls"]);
        let ids: Vec<&str> = ls["data"]["servers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["beta"]);
        let reg: Value =
            serde_json::from_str(&std::fs::read_to_string(w.data.join("registry.json")).unwrap())
                .unwrap();
        assert_eq!(reg["profiles"][0]["enabledServerIds"], json!([]));
        let (code, value) = cli_json(&["server", "uninstall", "alpha"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (1, Some("not_found"))
        );
    });
}

#[test]
fn server_uninstall_keep_clients_leaves_configs() {
    world(|w| {
        seed_claude(w);
        let before = std::fs::read(claude_file(w)).unwrap();
        let (code, value) = cli_json(&["server", "uninstall", "alpha", "--keep-clients"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["clients"], json!([]));
        assert_eq!(std::fs::read(claude_file(w)).unwrap(), before);
    });
}

#[test]
fn server_edit_moves_the_transport_with_the_endpoint_flag() {
    world(|_| {
        let (code, value) = cli_json(&["server", "edit", "beta", "--command", "beta-mcp"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(
            value["data"],
            json!({"id": "beta", "changed": ["transport", "command"]})
        );
        let (_, info) = cli_json(&["server", "info", "beta"]);
        assert_eq!(info["data"]["transport"], "stdio");
        assert_eq!(info["data"]["url"], Value::Null);

        let (code, value) = cli_json(&[
            "server",
            "new",
            "delta",
            "--url",
            "https://example.invalid/sse",
            "--transport",
            "sse",
        ]);
        assert_eq!(code, 0, "{value}");
        let (code, value) = cli_json(&[
            "server",
            "edit",
            "delta",
            "--url",
            "https://example.invalid/s2",
        ]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["changed"], json!(["url"]));
        let (_, info) = cli_json(&["server", "info", "delta"]);
        assert_eq!(info["data"]["transport"], "sse");

        let (code, value) = cli_json(&[
            "server",
            "new",
            "epsilon",
            "--url",
            "https://example.invalid/e",
        ]);
        assert_eq!(code, 0, "{value}");
        let (_, info) = cli_json(&["server", "info", "epsilon"]);
        assert_eq!(info["data"]["transport"], "http");
    });
}

fn seed_git_meta(id: &str, meta: Value) {
    crate::registry_controller::set_server_source(id, meta.as_object().unwrap().clone()).unwrap();
}

#[test]
fn server_source_set_switches_branch_and_rejects_one_missing_on_the_remote() {
    world(|w| {
        let repo = update_source::build(&w.home.join("three-remotes"));
        seed_git_meta(
            "alpha",
            json!({"type": "git", "path": repo.work.to_string_lossy(), "remote": "fork", "branch": "main"}),
        );

        let (code, value) =
            cli_json(&["server", "source", "set", "alpha", "--branch", "feature-x"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["source"]["meta"]["branch"], "feature-x");

        let (code, value) = cli_json(&[
            "server",
            "source",
            "set",
            "alpha",
            "--branch",
            "no-such-branch",
        ]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (2, Some("usage")),
            "{value}"
        );
        assert!(
            value["error"]["message"]
                .as_str()
                .unwrap()
                .contains("no-such-branch"),
            "{value}"
        );

        let (code, value) =
            cli_json(&["server", "source", "set", "alpha", "--remote", "ghost"]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (2, Some("usage")),
            "{value}"
        );
        assert!(
            value["error"]["message"]
                .as_str()
                .unwrap()
                .contains("no such remote: ghost"),
            "{value}"
        );
    });
}

#[test]
fn server_source_set_can_add_and_validate_an_upstream() {
    world(|w| {
        let repo = update_source::build(&w.home.join("three-remotes"));
        seed_git_meta(
            "alpha",
            json!({"type": "git", "path": repo.work.to_string_lossy(), "remote": "fork", "branch": "main"}),
        );

        let (code, value) = cli_json(&[
            "server",
            "source",
            "set",
            "alpha",
            "--upstream-remote",
            "upstream",
            "--upstream-branch",
            "main",
        ]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["source"]["meta"]["upstream"]["remote"], "upstream");
        assert_eq!(value["data"]["source"]["meta"]["upstream"]["branch"], "main");

        let (code, value) = cli_json(&[
            "server",
            "source",
            "set",
            "alpha",
            "--upstream-branch",
            "no-such-branch",
        ]);
        assert_eq!(
            (code, value["error"]["code"].as_str()),
            (2, Some("usage")),
            "{value}"
        );

        let (code, value) =
            cli_json(&["server", "source", "set", "alpha", "--clear-upstream"]);
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["data"]["source"]["meta"]["upstream"], Value::Null);
    });
}

#[test]
fn server_new_refuses_a_name_taken_ignoring_case_and_padding() {
    world(|w| {
        let before = std::fs::read(w.data.join("registry.json")).unwrap();
        for name in ["ALPHA", " alpha "] {
            let (code, value) = cli_json(&["server", "new", name, "--command", "x"]);
            assert_eq!(
                (code, value["error"]["code"].as_str()),
                (1, Some("conflict")),
                "{name:?}"
            );
        }
        assert_eq!(std::fs::read(w.data.join("registry.json")).unwrap(), before);
    });
}

#[test]
fn profile_inspect_takes_a_profile_id_or_a_name_in_any_case() {
    world(|_| {
        for key in ["work", "Work", "WORK"] {
            let (code, value) = cli_json(&["profile", "inspect", key]);
            assert_eq!(code, 0, "{key}");
            assert_eq!(value["data"]["profile"], "work", "{key}");
        }
        for args in [
            &["profile", "inspect", "DEFAULT"][..],
            &["profile", "inspect"][..],
        ] {
            let (code, value) = cli_json(args);
            assert_eq!(
                (code, value["error"]["code"].as_str()),
                (1, Some("inspect")),
                "{args:?}"
            );
        }
    });
}
