use serde_json::{json, Value};
use toolport_plus_api::{
    AccessSetInfo, CatalogEntryRecord, MemoryHost, MemoryRegistry, MemorySecrets,
};

use crate::ctl::run_with;

fn host() -> MemoryHost {
    let registry = MemoryRegistry::with_servers(Vec::new());
    *registry.access_sets.borrow_mut() = vec![AccessSetInfo {
        id: "default".into(),
        name: "Default".into(),
        server_ids: vec![],
    }];
    MemoryHost {
        registry,
        secrets: MemorySecrets {
            data_dir: Some(std::env::temp_dir().join("plus-ctl-server-no-such-dir")),
            ..Default::default()
        },
        catalog: vec![CatalogEntryRecord {
            name: "Stripe".into(),
            transport: "http".into(),
            source: "curated".into(),
            env_keys: vec![],
            extra: json!({"url": "https://stripe.example.invalid/mcp"})
                .as_object()
                .unwrap()
                .clone(),
        }],
        ..Default::default()
    }
}

fn run(host: &MemoryHost, argv: &[&str]) -> (i32, Value) {
    let args: Vec<String> = ["--json"]
        .into_iter()
        .chain(argv.iter().copied())
        .map(String::from)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(host, &args, &mut out, &mut err);
    let text = String::from_utf8(out).unwrap();
    (
        code,
        serde_json::from_str(text.trim()).unwrap_or(Value::Null),
    )
}

#[test]
fn test01_new_then_info_shows_the_server_with_its_env_and_what_its_process_sees() {
    let host = host();
    let (code, added) = run(
        &host,
        &[
            "server",
            "new",
            "gamma",
            "--command",
            "gamma-mcp",
            "--arg",
            "x",
            "--env",
            "MODE=fast",
            "--inherit-env",
            "on",
        ],
    );
    assert_eq!(code, 0, "{added}");
    assert_eq!(added["data"]["id"], "gamma");
    let (_, info) = run(&host, &["server", "info", "gamma"]);
    let data = &info["data"];
    assert_eq!(data["args"], json!(["x"]));
    assert_eq!(data["inheritEnv"], true);
    assert_eq!(data["env"], json!([{"key": "MODE", "secret": false}]));
    assert_eq!(data["accessSets"], json!(["default"]));
    let (_, list) = run(&host, &["server", "ls"]);
    assert_eq!(list["data"]["servers"][0]["enabled"], true);
    assert_eq!(list["data"]["defaultAccess"], Value::Null);
}

#[test]
fn test02_a_duplicate_name_is_a_conflict_and_a_missing_endpoint_is_usage() {
    let host = host();
    run(&host, &["server", "new", "gamma", "--command", "x"]);
    let (code, again) = run(&host, &["server", "new", "GAMMA", "--command", "x"]);
    assert_eq!(
        (code, again["error"]["code"].as_str()),
        (1, Some("conflict"))
    );
    assert_eq!(again["error"]["message"], "server 'GAMMA' already exists");
    let (code, usage) = run(&host, &["server", "new", "delta"]);
    assert_eq!((code, usage["error"]["code"].as_str()), (2, Some("usage")));
    assert!(host.registry.servers.borrow().len() == 1);
}

#[test]
fn test03_edit_reports_what_changed_and_refuses_a_credential_as_plain_env() {
    let host = host();
    run(&host, &["server", "new", "gamma", "--command", "x"]);
    let (_, edited) = run(
        &host,
        &[
            "server",
            "edit",
            "gamma",
            "--arg",
            "y",
            "--inherit-env",
            "on",
            "--env",
            "JAVA_HOME=/opt/jdk",
            "--unset-env",
            "NEVER_SET",
        ],
    );
    assert_eq!(
        edited["data"]["changed"],
        json!(["args", "inheritEnv", "env.JAVA_HOME"])
    );
    let (_, same) = run(&host, &["server", "edit", "gamma", "--arg", "y"]);
    assert_eq!(same["data"]["changed"], json!([]));
    let (code, refused) = run(
        &host,
        &["server", "edit", "gamma", "--env", "API_KEY=plain"],
    );
    assert_eq!(
        (code, refused["error"]["code"].as_str()),
        (1, Some("input"))
    );
    assert!(refused["error"]["message"]
        .as_str()
        .unwrap()
        .contains("rule credential-name"));
    let (code, missing) = run(&host, &["server", "edit", "ghost", "--arg", "q"]);
    assert_eq!(
        (code, missing["error"]["code"].as_str()),
        (1, Some("not_found"))
    );
    let (code, bare) = run(&host, &["server", "edit", "gamma"]);
    assert_eq!((code, bare["error"]["code"].as_str()), (2, Some("usage")));
    let (code, bad) = run(
        &host,
        &["server", "edit", "gamma", "--inherit-env", "maybe"],
    );
    assert_eq!(code, 2, "{bad}");
}

#[test]
fn test04_install_adds_a_curated_entry_once_and_names_a_missing_one() {
    let host = host();
    let (code, done) = run(&host, &["server", "install", "stripe", "--offline"]);
    assert_eq!(code, 0, "{done}");
    assert_eq!(
        done["data"],
        json!({"id": "stripe", "name": "Stripe", "envKeys": [], "source": "curated"})
    );
    let (code, again) = run(&host, &["server", "install", "Stripe"]);
    assert_eq!(
        (code, again["error"]["code"].as_str()),
        (1, Some("conflict"))
    );
    let (code, none) = run(&host, &["server", "install", "nope"]);
    assert_eq!(
        (code, none["error"]["message"].as_str()),
        (1, Some("no catalog entry named 'nope'"))
    );
}

#[test]
fn test05_uninstall_previews_then_removes_and_a_restart_preview_asks_no_gateway() {
    let host = host();
    run(&host, &["server", "new", "gamma", "--command", "x"]);
    let (_, preview) = run(&host, &["server", "uninstall", "gamma", "--dry-run"]);
    assert_eq!(preview["data"]["dryRun"], true);
    assert_eq!(host.registry.servers.borrow().len(), 1);
    let (_, restart) = run(&host, &["server", "restart", "gamma", "--dry-run"]);
    assert_eq!(restart["data"]["status"], "dry_run");
    assert!(!std::env::temp_dir()
        .join("plus-ctl-server-no-such-dir")
        .exists());
    let (code, done) = run(&host, &["server", "uninstall", "gamma"]);
    assert_eq!((code, done["data"]["dryRun"].as_bool()), (0, Some(false)));
    assert!(host.registry.servers.borrow().is_empty());
    let (code, gone) = run(&host, &["server", "uninstall", "gamma"]);
    assert_eq!(
        (code, gone["error"]["code"].as_str()),
        (1, Some("not_found"))
    );
}

#[test]
fn test06_restart_with_no_gateway_and_no_wait_asks_and_returns() {
    let host = host();
    run(&host, &["server", "new", "gamma", "--command", "x"]);
    let dir = std::env::temp_dir().join(format!("plus-ctl-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let host = MemoryHost {
        secrets: MemorySecrets {
            data_dir: Some(dir.clone()),
            ..Default::default()
        },
        ..host
    };
    let (code, asked) = run(&host, &["server", "restart", "gamma", "--wait", "0"]);
    assert_eq!(
        (code, asked["data"]["status"].as_str()),
        (0, Some("requested"))
    );
    let left = std::fs::read_dir(dir.join("server-restarts"))
        .unwrap()
        .count();
    assert_eq!(left, 1, "the request waits for a gateway tick");
    let (code, usage) = run(&host, &["server", "restart", "gamma", "--wait", "soon"]);
    assert_eq!(code, 2, "{usage}");
    let _ = std::fs::remove_dir_all(&dir);
}
