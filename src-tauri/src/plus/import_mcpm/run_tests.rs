use super::*;
use crate::plus::ctl;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn input_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import_mcpm/input")
}

fn fakeify(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("CANARY_") {
        out.push_str(&rest[..i]);
        out.push_str("FAKE-");
        rest = &rest[i + "CANARY_".len()..];
    }
    out.push_str(rest);
    out
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

struct World {
    home: PathBuf,
    base: PathBuf,
    root: PathBuf,
    short_ids: PathBuf,
    _dir: crate::registry::DataDirOverride,
    _home: HomeGuard,
}

impl World {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("import-mcpm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("mcpm");
        std::fs::create_dir_all(&root).unwrap();
        for entry in std::fs::read_dir(input_dir()).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(root.join(path.file_name().unwrap()), fakeify(&text)).unwrap();
        }
        let short_ids = base.join("short-ids.json");
        std::fs::write(
            &short_ids,
            json!({"anna-mcp":"anna","google-docs-mcp":"gdocs","google-docs-a":"gdocs-a",
                   "google-docs-b":"gdocs-b","moodle-mcp":"moodle"})
            .to_string(),
        )
        .unwrap();
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let dir = crate::registry::DataDirOverride::set(&data);
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let guard = HomeGuard::new(&home);
        World {
            home,
            _home: guard,
            base,
            root,
            short_ids,
            _dir: dir,
        }
    }

    fn opts(&self, dry_run: bool) -> RunOptions {
        RunOptions {
            root: self.root.clone(),
            short_ids_path: Some(self.short_ids.clone()),
            home: Some("{{HOME}}".into()),
            dry_run,
            write_clients: false,
            prune_orphans: false,
        }
    }

    fn data(&self) -> PathBuf {
        self.base.join("data")
    }

    fn registry_text(&self) -> Option<String> {
        std::fs::read_to_string(self.data().join("registry.json")).ok()
    }

    fn secrets(&self) -> Vec<(String, String, String)> {
        let (input, clients) = load_input(&self.opts(true)).unwrap();
        map_all(&input, &clients)
            .secret_writes()
            .into_iter()
            .map(|s| (s.server_id.clone(), s.key.clone(), s.value.clone()))
            .collect()
    }
}

fn with_world(tag: &str, test: impl FnOnce(&World)) {
    let _env = crate::clients::env_test_lock();
    crate::secrets::tests::with_isolated_vault(|| {
        let world = World::new(tag);
        test(&world);
    });
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn assert_no_plaintext(world: &World, values: &[String]) {
    let mut files = Vec::new();
    files_under(&world.data(), &mut files);
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for v in values {
            assert!(
                !text.contains(v.as_str()),
                "{} leaked in {}",
                v,
                f.display()
            );
        }
    }
}

fn leaks(text: &str, values: &[String]) -> bool {
    values.iter().any(|v| text.contains(v.as_str()))
}

#[test]
fn dry_run_writes_nothing_and_prints_plan() {
    with_world("dry", |w| {
        let secrets = w.secrets();
        assert!(secrets.len() > 5);
        let plan = run(&w.opts(true)).unwrap();
        assert!(plan.dry_run);
        assert_eq!(plan.servers.len(), 20);
        assert!(plan.servers.iter().all(|c| c.action == Action::Created));
        assert!(plan.secrets.iter().all(|c| c.action == Action::Created));
        assert!(w.registry_text().is_none());
        let mut files = Vec::new();
        files_under(&w.data(), &mut files);
        assert!(files.is_empty(), "{files:?}");
        for (id, key, _) in &secrets {
            assert_eq!(
                crate::secrets::get_vault_secret_result(id, key).unwrap(),
                None
            );
        }
        let values: Vec<String> = secrets.iter().map(|s| s.2.clone()).collect();
        assert!(!leaks(&plan.to_value().to_string(), &values));
        assert!(!leaks(&plan.summary(), &values));
    });
}

#[test]
fn real_run_writes_registry_and_vault_only_secrets() {
    with_world("real", |w| {
        let secrets = w.secrets();
        let values: Vec<String> = secrets.iter().map(|s| s.2.clone()).collect();
        let plan = run(&w.opts(false)).unwrap();
        assert!(!plan.dry_run);
        assert!(plan.changed());
        let reg: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        let servers = reg["servers"].as_array().unwrap();
        assert_eq!(servers.len(), 21);
        let selfmcp: Vec<&Value> = servers
            .iter()
            .filter(|s| s["source"] == crate::plus::selfmcp::register::SELF_SOURCE)
            .collect();
        assert_eq!(selfmcp.len(), 1);
        assert_eq!(selfmcp[0]["name"], crate::plus::selfmcp::SERVER_NAME);
        assert_eq!(reg["secretsGeneration"], 1);
        assert!(!reg["clientScopes"].as_object().unwrap().is_empty());
        for (id, key, value) in &secrets {
            assert_eq!(
                crate::secrets::get_vault_secret_result(id, key)
                    .unwrap()
                    .as_deref(),
                Some(value.as_str()),
                "{id}::{key}"
            );
        }
        assert_no_plaintext(&w, &values);
        assert!(!leaks(&plan.to_value().to_string(), &values));
        assert!(plan
            .secrets
            .iter()
            .all(|c| c.id.contains("::") && c.action == Action::Created));
    });
}

#[test]
fn rerun_changes_nothing() {
    with_world("idem", |w| {
        run(&w.opts(false)).unwrap();
        let before = w.registry_text().unwrap();
        let second = run(&w.opts(false)).unwrap();
        assert!(!second.changed());
        assert_eq!(
            second.counts.get(&Action::Unchanged).copied().unwrap_or(0) > 0,
            true
        );
        assert_eq!(w.registry_text().unwrap(), before);
        let dry = run(&w.opts(true)).unwrap();
        assert!(!dry.changed());
    });
}

#[test]
fn changed_secret_and_server_report_updated() {
    with_world("upd", |w| {
        run(&w.opts(false)).unwrap();
        let path = w.root.join("servers.json");
        let text = std::fs::read_to_string(&path).unwrap();
        let (_, key, old) = w.secrets().into_iter().next().unwrap();
        std::fs::write(&path, text.replace(&old, "FAKE-rotated-0001")).unwrap();
        let plan = run(&w.opts(false)).unwrap();
        let updated: Vec<_> = plan
            .secrets
            .iter()
            .filter(|c| c.action == Action::Updated)
            .collect();
        assert_eq!(updated.len(), 1, "{key}");
        let reg: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        assert_eq!(reg["secretsGeneration"], 2);
        assert_no_plaintext(&w, &["FAKE-rotated-0001".to_string()]);
    });
}

#[test]
fn user_owned_server_with_same_id_is_a_conflict() {
    with_world("conflict", |w| {
        let mut reg = crate::registry::Registry::default();
        let mut entry = crate::registry::ServerEntry {
            id: "anna".into(),
            name: "mine".into(),
            transport: "stdio".into(),
            command: Some("true".into()),
            args: vec![],
            launch: None,
            env: vec![],
            url: None,
            cwd: None,
            source: None,
            disabled_tools: vec![],
            client_credentials: None,
            request_timeout_ms: None,
            max_request_timeout_ms: None,
            initialize_timeout_ms: None,
            unknown_fields: Default::default(),
        };
        entry.source = Some("manual".into());
        reg.servers.push(entry);
        crate::registry::save_to(&w.data().join("registry.json"), &reg).unwrap();
        let plan = run(&w.opts(false)).unwrap();
        let anna = plan.servers.iter().find(|c| c.id == "anna").unwrap();
        assert_eq!(anna.action, Action::Conflict);
        assert!(plan
            .secrets
            .iter()
            .filter(|c| c.id.starts_with("anna::"))
            .all(|c| c.action == Action::Conflict));
        assert_eq!(
            crate::secrets::get_vault_secret_result("anna", "ANNAS_SECRET_KEY").unwrap(),
            None
        );
        let after: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        let kept = after["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "anna")
            .unwrap();
        assert_eq!(kept["name"], "mine");
    });
}

#[test]
fn handler_and_cli_share_the_flow_without_leaking() {
    with_world("cli", |w| {
        let values: Vec<String> = w.secrets().iter().map(|s| s.2.clone()).collect();
        let root = w.root.to_string_lossy().to_string();
        let ids = w.short_ids.to_string_lossy().to_string();
        let args: Vec<String> = [
            "--json",
            "import",
            "mcpm",
            &root,
            "--dry-run",
            "--short-ids",
            &ids,
            "--home",
            "{{HOME}}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(ctl::run_with(&args, &mut out, &mut err), 0);
        let text = String::from_utf8(out).unwrap() + &String::from_utf8(err).unwrap();
        assert!(!leaks(&text, &values));
        let envelope: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(envelope["data"]["dryRun"], true);
        assert!(w.registry_text().is_none());

        let value = crate::plus::dispatch(
            "plus.import_mcpm.run",
            json!({"root": root, "shortIds": ids, "home": "{{HOME}}"}),
        )
        .unwrap();
        assert_eq!(value["dryRun"], false);
        assert!(!leaks(&value.to_string(), &values));
        assert!(w.registry_text().is_some());
    });
}

#[test]
fn missing_root_is_an_error() {
    with_world("missing", |w| {
        let mut opts = w.opts(true);
        opts.root = w.base.join("nope");
        assert!(run(&opts).unwrap_err().contains("servers.json"));
    });
}

fn desktop_config(w: &World) -> PathBuf {
    if cfg!(target_os = "macos") {
        w.home
            .join("Library/Application Support/Claude/claude_desktop_config.json")
    } else {
        w.home.join(".config/Claude/claude_desktop_config.json")
    }
}

fn client_files(w: &World) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("claude-code", w.home.join(".claude.json")),
        ("claude-desktop", desktop_config(w)),
        ("cursor", w.home.join(".cursor/mcp.json")),
        ("gemini-cli", w.home.join(".gemini/settings.json")),
    ]
}

fn seed_clients(w: &World) {
    for (id, path) in client_files(w) {
        let mut doc: Value = serde_json::from_str(
            &std::fs::read_to_string(w.root.join(format!("{id}.json"))).unwrap(),
        )
        .unwrap();
        doc["theme"] = json!("FAKE-theme");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn live_opts(w: &World, dry_run: bool) -> RunOptions {
    RunOptions {
        write_clients: true,
        ..w.opts(dry_run)
    }
}

#[test]
fn clients_get_one_toolport_entry_bound_to_their_profile() {
    with_world("clients", |w| {
        seed_clients(w);
        let plan = run(&live_opts(w, false)).unwrap();
        assert_eq!(plan.clients.len(), 4);
        assert!(plan.clients.iter().all(|c| c.action == Action::Created));
        let reg: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        for (id, path) in client_files(w) {
            let doc = read_json(&path);
            let servers = doc["mcpServers"].as_object().unwrap();
            assert!(servers.contains_key("toolport"), "{id}");
            let env = &servers["toolport"]["env"];
            assert_eq!(env["TOOLPORT_CLIENT_ID"], id);
            assert_eq!(env["TOOLPORT_PROFILE"], id);
            assert!(
                servers.keys().all(|k| !k.starts_with("mcpm_")),
                "{id}: {:?}",
                servers.keys().collect::<Vec<_>>()
            );
            assert_eq!(doc["theme"], "FAKE-theme");
            assert_eq!(reg["clientScopes"][id], id);
            assert!(reg["clientManagedEntries"][id].is_object(), "{id}");
        }
        let code = read_json(&w.home.join(".claude.json"));
        let keys: Vec<&str> = code["mcpServers"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert!(keys.contains(&"mcpm-mcp"));
        let desktop = read_json(&desktop_config(w));
        assert!(desktop["mcpServers"]["context7"].is_object());
        assert!(desktop["mcpServers"]["playwright"].is_object());
        let profiles = reg["profiles"].as_array().unwrap();
        let ids = |id: &str| -> Vec<Value> {
            profiles.iter().find(|p| p["id"] == id).unwrap()["enabledServerIds"]
                .as_array()
                .unwrap()
                .clone()
        };
        let count = |id: &str| {
            ids(id)
                .iter()
                .filter(|s| *s != "toolport-plus-self")
                .count()
        };
        assert_eq!(count("claude-code"), 18);
        assert_eq!(count("claude-desktop"), 16);
        for id in ["claude-code", "claude-desktop"] {
            assert!(ids(id).contains(&json!("toolport-plus-self")), "{id}");
        }
        assert_eq!(reg["clientDiscovery"]["claude-code"], "full");
        assert_eq!(reg["clientDiscovery"]["claude-desktop"], "lazy");
        assert_eq!(reg["clientDiscovery"]["cursor"], "lazy");
        assert_eq!(reg["clientDiscovery"]["gemini-cli"], "lazy");
        let orphans: Vec<_> = plan
            .clients
            .iter()
            .flat_map(|c| c.orphans.iter().cloned())
            .collect();
        assert_eq!(orphans.len(), 3, "{orphans:?}");
    });
}

#[test]
fn client_apply_is_idempotent_and_dry_run_writes_nothing() {
    with_world("clients-idem", |w| {
        seed_clients(w);
        let before: Vec<String> = client_files(w)
            .iter()
            .map(|(_, p)| std::fs::read_to_string(p).unwrap())
            .collect();
        let dry = run(&live_opts(w, true)).unwrap();
        assert!(dry.clients.iter().all(|c| c.action == Action::Created));
        assert!(dry.clients.iter().all(|c| !c.removed.is_empty()));
        let after: Vec<String> = client_files(w)
            .iter()
            .map(|(_, p)| std::fs::read_to_string(p).unwrap())
            .collect();
        assert_eq!(before, after);
        assert!(w.registry_text().is_none());

        run(&live_opts(w, false)).unwrap();
        let written: Vec<String> = client_files(w)
            .iter()
            .map(|(_, p)| std::fs::read_to_string(p).unwrap())
            .collect();
        let registry = w.registry_text().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let second = run(&live_opts(w, false)).unwrap();
        let detail = {
            let reg: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
            let live = read_json(&w.home.join(".claude.json"));
            format!(
                "{}\n{}\n{}",
                json!(second.clients),
                reg["clientManagedEntries"]["claude-code"],
                live["mcpServers"]["toolport"]
            )
        };
        assert!(!second.changed(), "{}\n{detail}", second.summary());
        assert!(second.clients.iter().all(|c| c.action == Action::Unchanged));
        let again: Vec<String> = client_files(w)
            .iter()
            .map(|(_, p)| std::fs::read_to_string(p).unwrap())
            .collect();
        assert_eq!(written, again);
        assert_eq!(w.registry_text().unwrap(), registry);
    });
}

#[test]
fn prune_orphans_removes_unmanaged_entries() {
    with_world("clients-prune", |w| {
        seed_clients(w);
        let opts = RunOptions {
            prune_orphans: true,
            ..live_opts(w, false)
        };
        let plan = run(&opts).unwrap();
        assert!(plan.clients.iter().all(|c| c.orphans.is_empty()));
        let code = read_json(&w.home.join(".claude.json"));
        let keys: Vec<&String> = code["mcpServers"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["toolport"]);
        let desktop = read_json(&desktop_config(w));
        assert_eq!(desktop["mcpServers"].as_object().unwrap().len(), 1);
    });
}

#[test]
fn customized_toolport_entry_is_left_alone() {
    with_world("clients-custom", |w| {
        seed_clients(w);
        let path = w.home.join(".cursor/mcp.json");
        let mut doc = read_json(&path);
        doc["mcpServers"]["toolport"] =
            json!({"command": "/usr/bin/env", "args": ["FAKE-wrapper"]});
        std::fs::write(&path, doc.to_string()).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let plan = run(&live_opts(w, false)).unwrap();
        let cursor = plan.clients.iter().find(|c| c.id == "cursor").unwrap();
        assert_eq!(cursor.action, Action::Conflict);
        assert!(cursor.error.is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        let other = plan.clients.iter().find(|c| c.id == "claude-code").unwrap();
        assert_eq!(other.action, Action::Created);
    });
}

#[test]
fn mcpm_client_names_map_to_toolport_ids() {
    with_world("aliases", |w| {
        for (file, key) in [
            ("qwen-cli", "mcpServers"),
            ("codex-cli", "mcpServers"),
            ("vscode", "servers"),
        ] {
            std::fs::write(
                w.root.join(format!("{file}.json")),
                json!({ key: {"mcpm_context7": {"command": "mcpm", "args": ["run", "context7"]}} })
                    .to_string(),
            )
            .unwrap();
        }
        let (_, clients) = load_input(&w.opts(true)).unwrap();
        let ids: Vec<&str> = clients.iter().map(|c| c.client_id.as_str()).collect();
        for id in ["qwen-code", "codex", "vscode"] {
            assert!(ids.contains(&id), "{ids:?}");
        }
        assert!(clients.iter().all(|c| !c.servers.is_empty()));
    });
}

#[test]
fn desktop_remotes_go_through_the_gateway_without_proxy_shims() {
    with_world("desktop-remotes", |w| {
        seed_clients(w);
        run(&live_opts(w, false)).unwrap();
        let desktop = read_json(&desktop_config(w));
        let entries = desktop["mcpServers"].as_object().unwrap();
        assert!(entries.contains_key("toolport"));
        for (key, entry) in entries {
            let args = entry["args"].to_string();
            assert!(!args.contains("mcp-proxy"), "{key}");
            assert!(entry.get("url").is_none(), "{key}");
            assert!(!key.starts_with("mcpm_"), "{key}");
        }
        let reg: Value = serde_json::from_str(&w.registry_text().unwrap()).unwrap();
        let server = |id: &str| {
            reg["servers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["id"] == id)
                .unwrap_or_else(|| panic!("{id}"))
                .clone()
        };
        let desktop_profile = reg["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "claude-desktop")
            .unwrap()["enabledServerIds"]
            .clone();
        for id in ["figma", "clickup", "miro"] {
            let s = server(id);
            assert_eq!(s["transport"], "http", "{id}");
            assert!(s["url"].as_str().unwrap().starts_with("https://"), "{id}");
            assert!(s.get("command").is_none_or(Value::is_null), "{id}");
            assert_eq!(s["mcpmOauth"], true, "{id}");
            assert!(
                desktop_profile.as_array().unwrap().contains(&json!(id)),
                "{id}"
            );
            assert!(w.secrets().iter().all(|(sid, _, _)| sid != id), "{id}");
        }
        let slack = server("slack");
        assert_eq!(slack["transport"], "http");
        assert!(slack.get("mcpmOauth").is_none_or(Value::is_null));
        assert!(w
            .secrets()
            .iter()
            .any(|(sid, key, _)| sid == "slack" && key == "__http_auth__"));
    });
}

fn launch_world(w: &World) -> RunOptions {
    let bin = w.home.join(".config/mcpm/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("helper.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    let home = w.home.to_string_lossy().into_owned();
    std::fs::write(
        w.root.join("servers.json"),
        json!({
            "helper": {"name": "helper", "command": "bash",
                       "args": [format!("{home}/.config/mcpm/bin/helper.sh")]},
            "ghost": {"name": "ghost", "command": "bash",
                      "args": [format!("{home}/.config/mcpm/bin/absent.sh")]},
            "wrapped": {"name": "wrapped", "command": "sudo", "args": ["true"]},
            "plain": {"name": "plain", "command": "node", "args": ["/srv/plain/index.js"]}
        })
        .to_string(),
    )
    .unwrap();
    for entry in std::fs::read_dir(&w.root).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() != "servers.json" {
            std::fs::remove_file(path).unwrap();
        }
    }
    RunOptions {
        root: w.root.clone(),
        short_ids_path: None,
        home: Some(home),
        dry_run: false,
        write_clients: false,
        prune_orphans: false,
    }
}

#[test]
fn launch_specs_are_relocated_screened_and_scripts_copied() {
    with_world("launch", |w| {
        let opts = launch_world(w);
        let plan = run(&opts).unwrap();
        assert_eq!(plan.rejects.len(), 1, "{:?}", plan.rejects);
        assert_eq!(plan.rejects[0].id, "wrapped");
        let ids: Vec<&str> = plan.servers.iter().map(|c| c.id.as_str()).collect();
        assert!(!ids.contains(&"wrapped"));
        assert!(ids.contains(&"plain"));
        let copied = w.data().join("imported-scripts/config_mcpm_bin/helper.sh");
        assert!(copied.is_file());
        assert_eq!(plan.scripts.len(), 2);
        let text = w.registry_text().unwrap();
        assert!(text.contains("imported-scripts"));
        assert!(!text.contains(".config/mcpm/bin/helper.sh\""));
        assert!(!text.contains("\"wrapped\""));
        assert!(plan.summary().contains("rejected wrapped"));
        let warnings = plan.warnings.to_string();
        assert!(warnings.contains("script-missing"));
        assert!(warnings.contains("rejected-launch"));
    });
}

#[test]
fn dry_run_plans_scripts_without_copying() {
    with_world("launch-dry", |w| {
        let mut opts = launch_world(w);
        opts.dry_run = true;
        let plan = run(&opts).unwrap();
        assert_eq!(plan.rejects.len(), 1);
        assert!(!w.data().join("imported-scripts").exists());
        assert!(w.registry_text().is_none());
    });
}

#[test]
fn action_names_match_as_str_and_counts_serialize_in_name_order() {
    let all = [
        Action::Created,
        Action::Updated,
        Action::Unchanged,
        Action::Conflict,
    ];
    for action in all {
        assert_eq!(serde_json::to_value(action).unwrap(), action.as_str());
    }
    let counts: BTreeMap<Action, usize> = all.into_iter().map(|a| (a, 1)).collect();
    assert_eq!(
        serde_json::to_string(&counts).unwrap(),
        r#"{"conflict":1,"created":1,"unchanged":1,"updated":1}"#
    );
}
