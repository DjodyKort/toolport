use super::*;
use crate::plus::profiles::Kind;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const CANARY: &str = "FAKE-canary-secret-5d71c0";

pub(crate) struct World {
    pub(crate) home: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) launcher: PathBuf,
    _vars: Vec<crate::clients::EnvRestore>,
    _launcher: LauncherOverride,
}

impl Drop for World {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn registry_json() -> Value {
    json!({
        "version": 1,
        "servers": [
            {
                "id": "alpha", "name": "alpha", "transport": "stdio",
                "command": "alpha-mcp", "args": ["--flag"],
                "env": [{"key": "API_KEY", "secret": true}, {"key": "MODE", "value": "fast"}]
            },
            {"id": "beta", "name": "beta", "transport": "http", "url": "https://example.invalid/mcp"},
            {
                "id": "cred", "name": "cred", "transport": "stdio", "command": "cred-mcp",
                "clientCredentials": {"clientId": "FAKE-client"}
            },
            {
                "id": "rooted", "name": "rooted", "transport": "stdio", "command": "rooted-mcp",
                "cwd": "${ROOT}/sub"
            },
            {"id": "toolport", "name": "toolport", "transport": "stdio", "command": "x-mcp"}
        ],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": ["alpha"]}],
        "activeProfileId": "default"
    })
}

pub(crate) fn world(test: impl FnOnce(&World)) {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let data = crate::registry::conduit_dir().unwrap();
        let home = std::env::temp_dir().join(format!("direct-home-{}", std::process::id()));
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
        let launcher = home.join("bin").join(binary_file());
        std::fs::write(&launcher, "stand-in").unwrap();
        let w = World {
            _launcher: LauncherOverride::set(Some(&launcher.to_string_lossy())),
            home,
            data,
            launcher,
            _vars: vars,
        };
        std::fs::write(
            w.data.join("registry.json"),
            serde_json::to_string_pretty(&registry_json()).unwrap(),
        )
        .unwrap();
        test(&w);
    });
}

pub(crate) fn config_path(id: &str) -> PathBuf {
    clients::detect_clients()
        .into_iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("client {id}"))
        .config_path
        .into()
}

pub(crate) fn installed(id: &str) -> PathBuf {
    let path = config_path(id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let detected = clients::detect_clients()
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    if !detected.app_present && !detected.config_exists {
        std::fs::write(&path, "").unwrap();
    }
    path
}

pub(crate) fn read(id: &str) -> String {
    std::fs::read_to_string(config_path(id)).unwrap_or_default()
}

pub(crate) fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
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

fn add_ok(server: &str, client: &str) -> Outcome {
    add(server, client, false, false).unwrap_or_else(|e| panic!("{client}: {}", e.message))
}

fn entry_on_disk(client: &str, entry: &str) -> Option<McpServer> {
    clients::detect_clients()
        .into_iter()
        .find(|c| c.id == client)?
        .servers
        .into_iter()
        .find(|s| s.name == entry)
}

#[test]
fn every_client_format_takes_a_launcher_entry_and_gives_it_back() {
    world(|w| {
        let ids: Vec<String> = clients::detect_clients()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert!(ids.len() >= 30, "{ids:?}");
        for id in &ids {
            installed(id);
            let added = add_ok("alpha", id);
            assert_eq!(added.action, Action::Added, "{id}");
            let rows = list(Some(id)).unwrap();
            assert_eq!(rows.len(), 1, "{id}: {rows:?}");
            assert_eq!(rows[0].state, State::Ok, "{id}: {rows:?}");
            let on_disk = entry_on_disk(id, "alpha").unwrap();
            assert_eq!(on_disk.command.as_deref(), w.launcher.to_str(), "{id}");
            assert_eq!(on_disk.args, ["direct", "run", "alpha"], "{id}");
            let text = read(id);
            for forbidden in ["alpha-mcp", "--flag", "fast", "API_KEY"] {
                assert!(!text.contains(forbidden), "{id} leaks {forbidden}: {text}");
            }
            assert_eq!(add_ok("alpha", id).action, Action::Unchanged, "{id}");
            let gone = remove("alpha", id, false, false).unwrap();
            assert_eq!(gone.action, Action::Removed, "{id}");
            assert!(entry_on_disk(id, "alpha").is_none(), "{id}");
            assert!(list(Some(id)).unwrap().is_empty(), "{id}");
            assert_eq!(
                remove("alpha", id, false, false).unwrap().action,
                Action::Absent,
                "{id}"
            );
        }
        assert_eq!(count(&registry_ro::read().unwrap()), 0);
    });
}

#[test]
fn each_format_writes_its_own_entry_shape() {
    world(|_| {
        let shapes: [(&str, &[&str]); 8] = [
            (
                "cursor",
                &["\"mcpServers\"", "\"alpha\"", "\"command\"", "\"args\""],
            ),
            ("github-copilot-cli", &["\"tools\"", "\"*\""]),
            ("droid", &["\"type\": \"stdio\""]),
            ("vscode", &["\"servers\"", "\"alpha\""]),
            (
                "opencode",
                &["\"mcp\"", "\"type\": \"local\"", "\"enabled\": true"],
            ),
            ("codex", &["[mcp_servers.alpha]", "command = "]),
            ("goose", &["extensions:", "cmd:", "type: stdio"]),
            ("continue", &["mcpServers:", "name: alpha", "command:"]),
        ];
        for (id, needles) in shapes {
            installed(id);
            add_ok("alpha", id);
            let text = read(id);
            for needle in needles {
                assert!(text.contains(needle), "{id} lacks {needle}: {text}");
            }
        }
    });
}

#[test]
fn foreign_keys_and_servers_survive_add_and_rm_with_a_backup() {
    world(|_| {
        let seeds: [(&str, &str, &[&str]); 5] = [
            (
                "claude-code",
                r#"{"theme": "FAKE-theme", "mcpServers": {"stray": {"command": "stray-mcp"}}}"#,
                &["FAKE-theme", "stray-mcp"],
            ),
            (
                "codex",
                "# FAKE user comment\nmodel = \"fake-model\"\n\n[mcp_servers.stray]\ncommand = \"stray-mcp\"\n",
                &["# FAKE user comment", "fake-model", "stray-mcp"],
            ),
            (
                "goose",
                "GOOSE_PROVIDER: fake\nextensions:\n  developer:\n    type: builtin\n    enabled: true\n    name: developer\n",
                &["GOOSE_PROVIDER: fake", "developer"],
            ),
            (
                "hermes",
                "model: fake-model\nmcp_servers:\n  stray:\n    command: stray-mcp\n",
                &["fake-model", "stray-mcp"],
            ),
            (
                "continue",
                "name: FAKE config\nmcpServers:\n  - name: stray\n    command: stray-mcp\n",
                &["FAKE config", "stray-mcp"],
            ),
        ];
        for (id, seed, keep) in seeds {
            let path = installed(id);
            std::fs::write(&path, seed).unwrap();
            let added = add_ok("alpha", id);
            let backup = added
                .backup
                .as_deref()
                .unwrap_or_else(|| panic!("{id}: backup"));
            assert_eq!(std::fs::read_to_string(backup).unwrap(), seed, "{id}");
            let text = read(id);
            for needle in keep {
                assert!(text.contains(needle), "{id} lost {needle}: {text}");
            }
            assert!(entry_on_disk(id, "alpha").is_some(), "{id}");
            remove("alpha", id, false, false).unwrap();
            let text = read(id);
            for needle in keep {
                assert!(text.contains(needle), "{id} lost {needle} on rm: {text}");
            }
            assert!(entry_on_disk(id, "alpha").is_none(), "{id}");
        }
    });
}

#[test]
fn a_commented_jsonc_client_keeps_its_comments() {
    world(|_| {
        let path = installed("zed");
        let seed = "{\n  // FAKE keep me\n  \"theme\": \"One\",\n  \"context_servers\": {}\n}\n";
        std::fs::write(&path, seed).unwrap();
        add_ok("alpha", "zed");
        let text = read("zed");
        assert!(text.contains("// FAKE keep me"), "{text}");
        assert!(text.contains("\"alpha\""), "{text}");
        remove("alpha", "zed", false, false).unwrap();
        assert!(read("zed").contains("// FAKE keep me"));
    });
}

#[test]
fn dry_runs_write_nothing_and_say_so() {
    world(|_| {
        installed("cursor");
        let before = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        let planned = add("alpha", "cursor", false, true).unwrap();
        assert_eq!(planned.action, Action::Added);
        assert!(planned.dry_run);
        assert!(planned.text().contains("Dry run: nothing was written."));
        assert!(planned.text().contains(TRADEOFF));
        assert!(planned.to_value()["tradeoff"]
            .as_str()
            .unwrap()
            .contains("bypasses"));
        let after = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        assert_eq!(before, after);

        add_ok("alpha", "cursor");
        let before = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        let planned = remove("alpha", "cursor", false, true).unwrap();
        assert_eq!(planned.action, Action::Removed);
        assert!(planned.text().contains("Dry run: nothing was written."));
        let after = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        assert_eq!(before, after);
    });
}

#[test]
fn a_second_add_changes_nothing() {
    world(|_| {
        installed("cursor");
        add_ok("alpha", "cursor");
        let before = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        let again = add_ok("alpha", "cursor");
        assert_eq!(again.action, Action::Unchanged);
        assert!(!again.changed());
        assert_eq!(again.backup, None);
        let after = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        assert_eq!(before, after);
    });
}

#[test]
fn no_secret_value_reaches_a_client_file_or_the_registry_record() {
    world(|w| {
        crate::secrets::set_secret("alpha", "API_KEY", CANARY).unwrap();
        for id in ["cursor", "codex", "goose", "opencode", "continue"] {
            installed(id);
            let added = add_ok("alpha", id);
            assert!(!added.text().contains(CANARY));
            assert!(!added.to_value().to_string().contains(CANARY));
            assert!(!read(id).contains(CANARY), "{id}");
        }
        let registry = std::fs::read_to_string(w.data.join("registry.json")).unwrap();
        assert!(!registry.contains(CANARY));
        for (path, bytes) in tree(&crate::clients::home().unwrap()) {
            assert!(
                !String::from_utf8_lossy(&bytes).contains(CANARY),
                "{}",
                path.display()
            );
        }
    });
}

#[test]
fn remote_oauth_root_and_gateway_servers_are_refused_without_writing() {
    world(|_| {
        installed("cursor");
        let before = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        for (server, reason) in [
            ("beta", "remote"),
            ("cred", "OAuth"),
            ("rooted", "project root"),
            ("toolport", "gateway"),
        ] {
            for dry_run in [false, true] {
                let error = add(server, "cursor", true, dry_run).unwrap_err();
                assert_eq!(error.kind, Kind::Failed("refused"), "{server}");
                assert!(
                    error.message.contains(reason),
                    "{server}: {}",
                    error.message
                );
                assert!(error.message.contains("gateway"), "{server}");
            }
        }
        let after = (
            tree(&crate::registry::conduit_dir().unwrap()),
            tree(&crate::clients::home().unwrap()),
        );
        assert_eq!(before, after);
    });
}

#[test]
fn unknown_servers_and_clients_and_a_missing_launcher_are_reported() {
    world(|_| {
        installed("cursor");
        assert_eq!(
            add("nope", "cursor", false, false).unwrap_err().kind,
            Kind::NotFound
        );
        assert_eq!(
            add("alpha", "nope", false, false).unwrap_err().kind,
            Kind::NotFound
        );
        let error = add("alpha", "goose", false, false).unwrap_err();
        assert_eq!(error.kind, Kind::NotFound, "{}", error.message);
        assert!(error.message.contains("not detected"), "{}", error.message);
        let _none = LauncherOverride::set(None);
        let error = add("alpha", "cursor", false, false).unwrap_err();
        assert_eq!(error.kind, Kind::Failed("launcher_missing"));
        assert!(!config_path("cursor").exists());
    });
}

#[test]
fn a_foreign_entry_with_the_servers_name_is_never_replaced_or_removed_silently() {
    world(|_| {
        let path = installed("cursor");
        let seed = r#"{"mcpServers": {"alpha": {"command": "my-own-alpha", "args": ["x"]}}}"#;
        std::fs::write(&path, seed).unwrap();
        let error = add("alpha", "cursor", false, false).unwrap_err();
        assert_eq!(error.kind, Kind::Conflict);
        assert!(error.message.contains("did not write"), "{}", error.message);
        let error = remove("alpha", "cursor", false, false).unwrap_err();
        assert_eq!(error.kind, Kind::Conflict);
        assert_eq!(read("cursor"), seed);

        let forced = add("alpha", "cursor", true, false).unwrap();
        assert_eq!(forced.action, Action::Updated);
        assert!(forced.backup.is_some());
        assert_eq!(list(Some("cursor")).unwrap()[0].state, State::Ok);
    });
}

#[test]
fn an_edited_launcher_entry_needs_force_and_shows_as_customized() {
    world(|_| {
        let path = installed("cursor");
        add_ok("alpha", "cursor");
        let edited = read("cursor").replace("\"alpha\"\n", "\"alpha\", \"--extra\"\n");
        let edited = if edited == read("cursor") {
            read("cursor").replace("\"run\",", "\"run\", \"--extra\",")
        } else {
            edited
        };
        assert_ne!(edited, read("cursor"));
        std::fs::write(&path, &edited).unwrap();
        assert_eq!(list(Some("cursor")).unwrap()[0].state, State::Customized);
        let error = add("alpha", "cursor", false, false).unwrap_err();
        assert_eq!(error.kind, Kind::Conflict);
        assert!(
            error.message.contains("changed after Toolport wrote it"),
            "{}",
            error.message
        );
        assert_eq!(
            remove("alpha", "cursor", false, false).unwrap_err().kind,
            Kind::Conflict
        );
        assert_eq!(read("cursor"), edited);
        assert_eq!(
            remove("alpha", "cursor", true, false).unwrap().action,
            Action::Removed
        );
        assert!(list(Some("cursor")).unwrap().is_empty());
    });
}

#[test]
fn states_cover_missing_orphan_stale_and_unrecorded_entries() {
    world(|w| {
        let path = installed("cursor");
        add_ok("alpha", "cursor");
        assert_eq!(list(None).unwrap()[0].state, State::Ok);

        let kept = read("cursor");
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(list(Some("cursor")).unwrap()[0].state, State::Missing);
        std::fs::write(&path, &kept).unwrap();

        std::fs::remove_file(&w.launcher).unwrap();
        assert_eq!(list(Some("cursor")).unwrap()[0].state, State::Stale);
        std::fs::write(&w.launcher, "stand-in").unwrap();
        assert_eq!(list(Some("cursor")).unwrap()[0].state, State::Ok);

        crate::registry::update(|reg| {
            record::clear(reg, "cursor", "alpha");
            Ok(())
        })
        .unwrap();
        let rows = list(Some("cursor")).unwrap();
        assert_eq!(rows[0].state, State::Unrecorded);
        assert_eq!(rows[0].server, "alpha");
        assert_eq!(add_ok("alpha", "cursor").action, Action::Adopted);
        assert_eq!(list(Some("cursor")).unwrap()[0].state, State::Ok);

        crate::registry::update(|reg| {
            reg.servers.retain(|s| s.id != "alpha");
            Ok(())
        })
        .unwrap();
        let rows = list(Some("cursor")).unwrap();
        assert_eq!(rows[0].state, State::Orphan);
        assert_eq!(rows[0].server_name, None);
        let gone = remove("alpha", "cursor", false, false).unwrap();
        assert_eq!(gone.action, Action::Removed);
        assert!(list(None).unwrap().is_empty());
    });
}

#[test]
fn rm_forgets_a_record_whose_entry_is_gone() {
    world(|_| {
        let path = installed("cursor");
        add_ok("alpha", "cursor");
        std::fs::write(&path, "{}").unwrap();
        let done = remove("alpha", "cursor", false, false).unwrap();
        assert_eq!(done.action, Action::Forgotten);
        assert_eq!(count(&registry_ro::read().unwrap()), 0);
        assert_eq!(
            remove("alpha", "cursor", false, false).unwrap().action,
            Action::Absent
        );
        assert_eq!(
            remove("nope", "cursor", false, false).unwrap_err().kind,
            Kind::NotFound
        );
    });
}

#[test]
fn a_renamed_server_keeps_its_entry_under_the_recorded_name() {
    world(|_| {
        installed("cursor");
        add_ok("alpha", "cursor");
        crate::registry::update(|reg| {
            reg.servers
                .iter_mut()
                .find(|s| s.id == "alpha")
                .unwrap()
                .name = "renamed".into();
            Ok(())
        })
        .unwrap();
        let rows = list(Some("cursor")).unwrap();
        assert_eq!(
            (rows[0].entry.as_str(), rows[0].state),
            ("alpha", State::Ok)
        );
        assert_eq!(rows[0].server_name.as_deref(), Some("renamed"));
        let gone = remove("renamed", "cursor", false, false).unwrap();
        assert_eq!(
            (gone.entry.as_str(), gone.action),
            ("alpha", Action::Removed)
        );
    });
}

#[test]
fn a_relocated_data_dir_is_repeated_in_the_entry_but_the_secret_key_never_is() {
    world(|w| {
        installed("cursor");
        let _dir = crate::clients::EnvRestore::set("TOOLPORT_DATA_DIR", &w.data);
        let added = add_ok("alpha", "cursor");
        let launcher = added.launcher.unwrap();
        assert_eq!(
            launcher.env.get("TOOLPORT_DATA_DIR").map(String::as_str),
            w.data.to_str()
        );
        assert!(!launcher.env.contains_key("TOOLPORT_SECRET_KEY"));
        let text = read("cursor");
        assert!(text.contains("TOOLPORT_DATA_DIR"), "{text}");
        assert!(!text.contains("publisher-synthetic-vault"), "{text}");
        assert_eq!(list(Some("cursor")).unwrap()[0].state, State::Ok);
    });
}

#[test]
fn encrypted_secret_stores_are_called_out_for_servers_with_secrets() {
    world(|_| {
        installed("cursor");
        let added = add_ok("alpha", "cursor");
        assert!(
            added
                .notes
                .iter()
                .any(|n| n.contains("TOOLPORT_SECRET_KEY")),
            "{:?}",
            added.notes
        );
    });
}

#[test]
fn recorded_removal_by_server_takes_unrecorded_launchers_along() {
    world(|_| {
        installed("cursor");
        installed("windsurf");
        add_ok("alpha", "cursor");
        add_ok("alpha", "windsurf");
        crate::registry::update(|reg| {
            record::clear(reg, "windsurf", "alpha");
            Ok(())
        })
        .unwrap();
        let done = remove_recorded(Removing::Server("alpha"), false);
        assert_eq!(done.len(), 2);
        assert!(done
            .iter()
            .all(|(_, r)| r.as_ref().unwrap().action == Action::Removed));
        assert!(list(None).unwrap().is_empty());
        assert!(entry_on_disk("windsurf", "alpha").is_none());
    });
}

fn server(value: Value) -> ServerEntry {
    serde_json::from_value(value).unwrap()
}

fn envs(cmd: &std::process::Command) -> std::collections::BTreeMap<String, Option<String>> {
    cmd.get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect()
}

fn pair(key: &str, value: &str) -> (String, String) {
    (key.to_string(), value.to_string())
}

#[test]
fn the_launcher_builds_the_command_the_gateway_would_spawn() {
    let _env = crate::clients::env_test_lock();
    let entry = server(json!({
        "id": "alpha", "name": "alpha", "transport": "stdio", "command": "sh",
        "args": ["server.sh", "--flag"],
    }));
    let cmd = launcher::prepare(
        &entry,
        &entry.args,
        &[pair("API_KEY", CANARY), pair("MODE", "fast")],
    )
    .unwrap();
    assert_eq!(
        Path::new(cmd.get_program()).file_name().unwrap(),
        "sh",
        "{:?}",
        cmd.get_program()
    );
    assert_eq!(cmd.get_args().collect::<Vec<_>>(), ["server.sh", "--flag"]);
    let env = envs(&cmd);
    assert_eq!(env["API_KEY"].as_deref(), Some(CANARY));
    assert_eq!(env["MODE"].as_deref(), Some("fast"));
    #[cfg(not(windows))]
    assert!(env["PATH"].as_deref().is_some_and(|p| !p.is_empty()));
}

#[test]
fn the_launcher_strips_toolport_control_env_unless_the_server_configured_it() {
    let _env = crate::clients::env_test_lock();
    let _key = crate::clients::EnvRestore::set("TOOLPORT_SECRET_KEY", Path::new("FAKE-master-key"));
    let _dir = crate::clients::EnvRestore::set("TOOLPORT_DATA_DIR", Path::new("/fake/data"));
    let _legacy =
        crate::clients::EnvRestore::set("CONDUIT_SECRET_KEY", Path::new("FAKE-legacy-key"));
    let entry = server(json!({
        "id": "alpha", "name": "alpha", "transport": "stdio", "command": "sh", "args": [],
    }));
    let cmd = launcher::prepare(&entry, &[], &[pair("TOOLPORT_DATA_DIR", "/own/data")]).unwrap();
    let env = envs(&cmd);
    assert_eq!(env["TOOLPORT_SECRET_KEY"], None);
    assert_eq!(env["CONDUIT_SECRET_KEY"], None);
    assert_eq!(env["TOOLPORT_DATA_DIR"].as_deref(), Some("/own/data"));
}

#[test]
fn the_launcher_screens_what_it_runs_like_the_gateway() {
    let _env = crate::clients::env_test_lock();
    let entry = server(json!({
        "id": "alpha", "name": "alpha", "transport": "stdio", "command": "sh", "args": [],
    }));
    let error = launcher::prepare(&entry, &[], &[pair("LD_PRELOAD", "/tmp/evil.so")]).unwrap_err();
    assert!(error.contains("LD_PRELOAD"), "{error}");
    let env_command = server(json!({
        "id": "e", "name": "e", "transport": "stdio", "command": "env", "args": ["LD_PRELOAD=/tmp/evil.so", "sh"],
    }));
    assert!(launcher::prepare(&env_command, &env_command.args, &[]).is_err());
}

#[test]
fn the_launcher_hands_container_secrets_over_by_name_only() {
    let _env = crate::clients::env_test_lock();
    let entry = server(json!({
        "id": "box", "name": "box", "transport": "stdio", "command": "docker",
        "args": ["run", "-i", "--rm", "fake/image"],
    }));
    let cmd = launcher::prepare(&entry, &entry.args, &[pair("API_KEY", CANARY)]).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(args.windows(2).any(|w| w == ["-e", "API_KEY"]), "{args:?}");
    assert!(!args.iter().any(|a| a.contains(CANARY)), "{args:?}");
}

#[test]
fn the_launcher_pins_an_existing_working_directory_and_rejects_a_missing_one() {
    let _env = crate::clients::env_test_lock();
    let dir = std::env::temp_dir().join(format!("direct-cwd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut entry = server(json!({
        "id": "alpha", "name": "alpha", "transport": "stdio", "command": "sh", "args": [],
    }));
    entry.cwd = Some(dir.to_string_lossy().into_owned());
    let cmd = launcher::prepare(&entry, &[], &[]).unwrap();
    assert_eq!(cmd.get_current_dir(), Some(dir.as_path()));
    entry.cwd = Some(dir.join("missing").to_string_lossy().into_owned());
    assert!(launcher::prepare(&entry, &[], &[]).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_launcher_refuses_what_only_the_gateway_can_serve() {
    let _env = crate::clients::env_test_lock();
    for (id, needle) in [("beta", "remote"), ("cred", "OAuth"), ("rooted", "${ROOT}")] {
        let entry: ServerEntry = serde_json::from_value(
            registry_json()["servers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == id)
                .unwrap()
                .clone(),
        )
        .unwrap();
        let error = launcher::prepare(&entry, &entry.args, &[]).unwrap_err();
        assert!(error.contains(needle), "{id}: {error}");
    }
}

#[test]
fn a_missing_required_secret_stops_the_launcher_with_the_command_to_fix_it() {
    world(|_| {
        let reg = registry_ro::read().unwrap();
        let alpha = servers::find(&reg, "alpha").unwrap().clone();
        let failure = launcher::start(&alpha).map(|_| ()).unwrap_err();
        assert_eq!(failure.exit, 1);
        assert!(
            failure.message.contains("missing secret 'API_KEY'"),
            "{}",
            failure.message
        );
        assert!(
            failure.message.contains("toolportctl secret set alpha"),
            "{}",
            failure.message
        );
        crate::secrets::set_secret("alpha", "API_KEY", CANARY).unwrap();
        let cmd = launcher::start(&alpha).unwrap();
        assert_eq!(envs(&cmd)["API_KEY"].as_deref(), Some(CANARY));
        assert_eq!(envs(&cmd)["MODE"].as_deref(), Some("fast"));
    });
}

#[test]
fn an_unknown_server_ends_the_launcher_with_a_clear_message() {
    world(|_| {
        let failure = launcher::run("not-a-server");
        assert_eq!(failure.exit, 1);
        assert!(
            failure.message.contains("not in the registry"),
            "{}",
            failure.message
        );
    });
}

#[test]
fn records_round_trip_and_leave_no_residue() {
    let mut reg = Registry::default();
    let before = serde_json::to_value(&reg).unwrap();
    let rec = Record {
        server: "alpha".into(),
        command: "/x/toolportctl".into(),
        args: vec!["direct".into(), "run".into(), "alpha".into()],
        env: BTreeMap::from([("TOOLPORT_DATA_DIR".to_string(), "/d".to_string())]),
    };
    record::set(&mut reg, "cursor", "alpha", &rec);
    assert_eq!(record::get(&reg, "cursor", "alpha"), Some(rec.clone()));
    assert_eq!(
        record::all(&reg),
        vec![("cursor".into(), "alpha".into(), rec)]
    );
    assert!(record::clear(&mut reg, "cursor", "alpha"));
    assert!(!record::clear(&mut reg, "cursor", "alpha"));
    assert_eq!(serde_json::to_value(&reg).unwrap(), before);
}
