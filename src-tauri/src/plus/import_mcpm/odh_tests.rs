//! MIG-ODH-1: the ODH server (`uv run --directory <repo>/mcp-server odh-mcp`) through the
//! cutover, the registry add path and spawn screening. `docs/odh-integration.md` embeds the
//! two examples checked here.

use super::*;
use crate::registry::ServerEntry;
use crate::registry_controller::ServerFields;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const REPO_ARG: &str = "<repo>/mcp-server";

fn odh_argv() -> Vec<String> {
    ["run", "--directory", REPO_ARG, "odh-mcp"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn mcpm_servers() -> Value {
    json!({
        "odh-mcp": {
            "name": "odh-mcp",
            "profile_tags": [],
            "proxy_mode": "REDACTED",
            "requires_session_pinning": false,
            "enabled": null,
            "command": "uv",
            "args": ["run", "--directory", REPO_ARG, "odh-mcp"],
            "env": {"ODOO_URL": "FAKE-url", "ODOO_API_KEY": "FAKE-key"}
        }
    })
}

fn mcpm_claude_code() -> Value {
    json!({"mcpServers": {"mcpm_odh-mcp": {"command": "mcpm", "args": ["run", "odh-mcp"]}}})
}

fn mapped_odh() -> ServerEntry {
    let input = McpmInput {
        servers: mcpm_servers().as_object().cloned().unwrap(),
        home: "{{HOME}}".into(),
        short_ids: [("odh-mcp".to_string(), "odh".to_string())].into(),
        ..Default::default()
    };
    let (mut servers, warnings) = map_servers(&input);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(servers.len(), 1);
    servers.remove(0).entry
}

#[test]
fn import_path_maps_and_accepts_the_uv_run_directory_form() {
    let entry = mapped_odh();
    assert_eq!(entry.id, "odh");
    assert_eq!(entry.transport, "stdio");
    assert_eq!(entry.command.as_deref(), Some("uv"));
    assert_eq!(entry.args, odh_argv());
    assert!(entry.launch.is_none());
    screen_entry(&entry).expect("uv run --directory <repo>/mcp-server odh-mcp passes screening");
}

#[test]
fn registry_add_path_stores_the_form_unchanged_and_launch_resolution_accepts_it() {
    let mut registry = crate::registry::Registry::default();
    let id = crate::registry_controller::apply_add_server(
        &mut registry,
        ServerFields {
            name: "odh".into(),
            transport: "stdio".into(),
            command: Some("uv".into()),
            args: odh_argv(),
            url: None,
            cwd: None,
            declare_client_capabilities: None,
            forward_instructions: None,
        },
    )
    .expect("the add path accepts the form");
    let stored = registry.servers.iter().find(|s| s.id == id).unwrap();
    assert_eq!(stored.command.as_deref(), Some("uv"));
    assert_eq!(stored.args, odh_argv());

    let mut bound = stored.clone();
    bound.args[2] = "<launch-input>".into();
    bound.launch = Some(crate::registry::LaunchConfig {
        inputs: vec![crate::registry::LaunchInput {
            key: "REPO".into(),
            label: "Repository".into(),
            secret: false,
            required: true,
            value: Some(REPO_ARG.into()),
        }],
        bindings: vec![crate::registry::ArgBinding {
            index: 2,
            parts: vec![crate::registry::ArgPart::Input { key: "REPO".into() }],
        }],
        ..Default::default()
    });
    let resolved = crate::launch_inputs::resolve_args_with(&bound, |_, _| Ok(None))
        .expect("the launch-input path screens the final argv and accepts it");
    assert_eq!(resolved.args, odh_argv());
}

#[test]
fn uv_run_and_uv_tool_run_are_download_launchers_and_get_the_long_connect_budget() {
    use crate::downstream::{is_download_launcher, stdio_connect_timeout};
    let long = std::time::Duration::from_secs(120);
    let tight = std::time::Duration::from_secs(10);
    let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    let argv = odh_argv();
    assert!(is_download_launcher("uv", &argv));
    assert_eq!(stdio_connect_timeout("uv", &argv), long);
    assert_eq!(
        stdio_connect_timeout("uv", &args(&["tool", "run", "odh-mcp"])),
        long
    );
    for command in ["/home/user/.local/bin/uv", "UV.EXE", r"C:\tools\uv.exe"] {
        assert_eq!(stdio_connect_timeout(command, &argv), long, "{command}");
    }
    assert_eq!(
        stdio_connect_timeout("uv run --directory x odh-mcp", &[]),
        long
    );
    assert_eq!(stdio_connect_timeout("uvx", &["odh-mcp".to_string()]), long);

    for rest in [
        &[][..],
        &["sync"],
        &["pip", "install", "odh-mcp"],
        &["tool", "install", "odh-mcp"],
        &["tool"],
        &["tool", "list"],
        &["python", "install"],
    ] {
        assert_eq!(
            stdio_connect_timeout("uv", &args(rest)),
            tight,
            "uv {rest:?} does not start a server"
        );
    }
    assert_eq!(
        stdio_connect_timeout("node", &["server.js".to_string()]),
        tight
    );
}

struct Cutover {
    base: PathBuf,
    home: PathBuf,
    root: PathBuf,
    short_ids: PathBuf,
    _dir: crate::registry::DataDirOverride,
    _home: HomeGuard,
}

struct HomeGuard {
    _vars: Vec<crate::clients::EnvRestore>,
}

impl HomeGuard {
    fn new(home: &Path) -> Self {
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
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = Some(home.to_path_buf()));
        HomeGuard { _vars: vars }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        crate::clients::TEST_HOME.with(|h| *h.borrow_mut() = None);
    }
}

impl Cutover {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("odh-cutover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("mcpm");
        let home = base.join("home");
        let data = base.join("data");
        for dir in [&root, &home, &data] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(root.join("servers.json"), mcpm_servers().to_string()).unwrap();
        std::fs::write(
            root.join("claude-code.json"),
            mcpm_claude_code().to_string(),
        )
        .unwrap();
        std::fs::write(home.join(".claude.json"), mcpm_claude_code().to_string()).unwrap();
        let short_ids = base.join("short-ids.json");
        std::fs::write(&short_ids, json!({"odh-mcp": "odh"}).to_string()).unwrap();
        let dir = crate::registry::DataDirOverride::set(&data);
        let guard = HomeGuard::new(&home);
        Cutover {
            base,
            home,
            root,
            short_ids,
            _dir: dir,
            _home: guard,
        }
    }

    fn run(&self) -> Plan {
        run(&RunOptions {
            root: self.root.clone(),
            short_ids_path: Some(self.short_ids.clone()),
            home: Some("{{HOME}}".into()),
            dry_run: false,
            write_clients: true,
            prune_orphans: false,
        })
        .expect("cutover run")
    }

    fn registry(&self) -> Value {
        let text = std::fs::read_to_string(self.base.join("data/registry.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

impl Drop for Cutover {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn sanitize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                match inner {
                    _ if key.ends_with("At") && (inner.is_string() || inner.is_number()) => {
                        *inner = json!("<timestamp>");
                    }
                    Value::String(command) if key == "command" => {
                        if let Some((_, leaf)) = command.rsplit_once('/') {
                            if leaf.starts_with("toolport-") {
                                *command = format!("<install-dir>/{leaf}");
                            }
                        }
                    }
                    Value::Array(items) if key == "env" => {
                        for item in items.iter_mut().filter_map(Value::as_object_mut) {
                            item.remove("value");
                        }
                    }
                    _ => sanitize(inner),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sanitize),
        _ => {}
    }
}

fn doc_block(marker: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/odh-integration.md");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let tag = format!("<!-- example:{marker} -->");
    let after = text
        .split_once(&tag)
        .unwrap_or_else(|| panic!("docs/odh-integration.md has no {tag}"))
        .1;
    let body = after
        .split_once("```json\n")
        .and_then(|(_, rest)| rest.split_once("\n```"))
        .unwrap_or_else(|| panic!("no json block after {tag}"))
        .0;
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{tag} block is not json: {e}"))
}

#[test]
fn the_documented_registry_and_client_entry_are_what_the_cutover_writes() {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let world = Cutover::new();
        let plan = world.run();
        assert!(plan.rejects.is_empty(), "{:?}", plan.rejects);

        let mut registry = world.registry();
        for key in [
            "version",
            "servers",
            "profiles",
            "activeProfileId",
            "clientScopes",
            "clientManagedEntries",
            "plus",
        ] {
            assert!(registry.get(key).is_some(), "registry.json has no {key}");
        }
        sanitize(&mut registry);
        let shown = json!({
            "servers": registry["servers"],
            "profiles": registry["profiles"],
            "clientScopes": registry["clientScopes"],
            "clientDiscovery": registry["clientDiscovery"],
            "clientManagedEntries": registry["clientManagedEntries"],
            "plus": registry["plus"],
        });
        let documented = doc_block("registry");
        assert_eq!(
            shown,
            documented,
            "docs/odh-integration.md registry example is stale; actual:\n{}",
            serde_json::to_string_pretty(&shown).unwrap()
        );

        let claude: Value = serde_json::from_str(
            &std::fs::read_to_string(world.home.join(".claude.json")).unwrap(),
        )
        .unwrap();
        let servers = claude["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 1, "{servers:?}");
        let mut client = json!({"mcpServers": {"toolport": servers["toolport"]}});
        sanitize(&mut client);
        let documented = doc_block("client-entry");
        assert_eq!(
            client,
            documented,
            "docs/odh-integration.md client example is stale; actual:\n{}",
            serde_json::to_string_pretty(&client).unwrap()
        );
    });
}

fn ctl_json(list: &[&str]) -> (i32, String, Value) {
    let mut args: Vec<String> = vec!["--json".into()];
    args.extend(list.iter().map(|s| s.to_string()));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = crate::plus::ctl::run_with(&args, &mut out, &mut err);
    let text = String::from_utf8(out).unwrap() + &String::from_utf8(err).unwrap();
    let envelope =
        serde_json::from_str(text.lines().next().unwrap_or("null")).unwrap_or(Value::Null);
    (code, text, envelope)
}

#[test]
fn ctl_server_info_names_the_launch_line_and_env_keys_but_never_env_values() {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let world = Cutover::new();
        world.run();

        let (code, text, info) = ctl_json(&["server", "info", "odh"]);
        assert_eq!(code, 0, "{text}");
        let data = &info["data"];
        assert_eq!(data["id"], "odh");
        assert_eq!(data["command"], "uv");
        assert_eq!(data["args"], json!(odh_argv()));
        assert_eq!(data["source"], "imported:mcpm");
        assert_eq!(
            data["env"],
            json!([
                {"key": "ODOO_API_KEY", "secret": true},
                {"key": "ODOO_URL", "secret": false},
            ])
        );
        assert!(data["profiles"]
            .as_array()
            .unwrap()
            .contains(&json!("claude-code")));
        assert!(
            !text.contains("FAKE-key") && !text.contains("FAKE-url"),
            "{text}"
        );

        let (code, text, status) = ctl_json(&["status"]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(status["data"]["serverCount"], 2);
        assert_eq!(status["data"]["secretsBackend"], "encrypted-file");
        assert!(
            !text.contains("FAKE-key") && !text.contains("FAKE-url"),
            "{text}"
        );
    });
}

#[test]
fn the_registry_file_keeps_non_secret_env_values_and_vaults_the_secret_ones() {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let world = Cutover::new();
        world.run();
        let raw = std::fs::read_to_string(world.base.join("data/registry.json")).unwrap();
        assert!(
            raw.contains("FAKE-url"),
            "ODOO_URL is not secret-shaped and stays in the file"
        );
        assert!(
            !raw.contains("FAKE-key"),
            "ODOO_API_KEY goes to the vault, never the file"
        );
    });
}

#[test]
fn ctl_reads_the_file_toolport_registry_names_like_the_gateway() {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let world = Cutover::new();
        world.run();
        let elsewhere = world.base.join("elsewhere.json");
        let mut other = crate::registry::Registry::default();
        let mut entry = mapped_odh();
        entry.id = "elsewhere-only".into();
        entry.name = "elsewhere-only".into();
        other.servers = vec![entry];
        crate::registry::save_to(&elsewhere, &other).unwrap();
        let data_file = world.base.join("data/registry.json");

        {
            let _restore = crate::clients::EnvRestore::set("TOOLPORT_REGISTRY", &elsewhere);
            assert_eq!(crate::registry::resolved_path(), Some(elsewhere.clone()));

            let (code, text, status) = ctl_json(&["status"]);
            assert_eq!(code, 0, "{text}");
            assert_eq!(status["data"]["serverCount"], 1, "{text}");
            assert_eq!(
                status["data"]["registry"]["path"],
                json!(elsewhere.to_string_lossy()),
                "{text}"
            );
            assert_eq!(
                status["data"]["dataDir"],
                json!(world.base.join("data").to_string_lossy()),
                "the data dir is still the data dir: {text}"
            );

            let (code, text, listed) = ctl_json(&["server", "ls"]);
            assert_eq!(code, 0, "{text}");
            let names: Vec<_> = listed["data"]["servers"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap())
                .collect();
            assert_eq!(names, ["elsewhere-only"], "{text}");
            let (code, _, _) = ctl_json(&["server", "info", "odh"]);
            assert_ne!(code, 0, "odh lives only in the data-dir file");
        }

        let (code, text, status) = ctl_json(&["status"]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(status["data"]["serverCount"], 2, "{text}");
        assert_eq!(
            status["data"]["registry"]["path"],
            json!(data_file.to_string_lossy()),
            "without the override ctl keeps reading <data dir>/registry.json: {text}"
        );
    });
}
