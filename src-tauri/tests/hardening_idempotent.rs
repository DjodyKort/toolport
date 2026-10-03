//! A second run must change nothing. Normalized: the skills lockfile `synced_at` stamp (restamped by
//! every sync, as in mcpm) and secrets.enc (fresh nonce per seal).

#![cfg(unix)]

mod hardening_support;

use hardening_support::{tree_diff, Canary, Sandbox, Tree};
use serde_json::{json, Value};

fn normalized(mut tree: Tree) -> Tree {
    tree.remove("data/secrets.enc");
    for (path, bytes) in tree.iter_mut() {
        if path.ends_with("mcpm-skills.lock") {
            let text = String::from_utf8_lossy(bytes).into_owned();
            let masked: Vec<&str> = text
                .lines()
                .map(|l| {
                    if l.trim_start().starts_with("\"synced_at\"") {
                        "\"synced_at\": <masked>"
                    } else {
                        l
                    }
                })
                .collect();
            *bytes = masked.join("\n").into_bytes();
        }
    }
    tree
}

fn assert_same(label: &str, before: &Tree, sb: &Sandbox) {
    let after = normalized(sb.tree());
    let diff = tree_diff(&normalized(before.clone()), &after);
    assert!(
        diff.is_empty(),
        "{label}: the second run changed the tree:\n{diff:#?}"
    );
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn seed_clients(sb: &Sandbox) {
    let entry = json!({"mcpServers": {
        "mcpm_import-env": {"command": "mcpm", "args": ["run", "import-env"]},
        "mcpm_import-bearer": {"command": "mcpm", "args": ["run", "import-bearer"]},
        "unrelated": {"command": "unrelated-server", "args": []}
    }})
    .to_string();
    write(&sb.home.join(".claude.json"), &entry);
    write(&sb.home.join(".cursor/mcp.json"), &entry);
}

fn mcpm_fixture(sb: &Sandbox, canary: &Canary) -> String {
    let root = sb.mcpm_root(
        "mcpm",
        &json!({
            "import-env": {"name": "import-env", "profile_tags": ["work"],
                           "command": "imported-server", "args": ["--serve"],
                           "env": {"IMPORT_API_KEY": canary.val("idem-env")}},
            "import-bearer": {"name": "import-bearer",
                              "url": "https://bearer.example.invalid/mcp",
                              "headers": {"Authorization":
                                  format!("Bearer {}", canary.val("idem-bearer"))}},
            "plain": {"name": "plain", "command": "plain-server", "args": []}
        }),
        Some(&json!({"mcpServers": {
            "mcpm_import-env": {"command": "mcpm", "args": ["run", "import-env"]}
        }})),
    );
    root.to_string_lossy().into_owned()
}

fn counts(data: &Value) -> (u64, u64) {
    let get = |k: &str| data["counts"][k].as_u64().unwrap_or(0);
    (get("created"), get("updated"))
}

#[test]
fn import_mcpm_second_run_changes_nothing() {
    let sb = Sandbox::new("idem-import");
    seed_clients(&sb);
    let root = mcpm_fixture(&sb, &Canary::new());
    let home = sb.home.to_string_lossy().into_owned();
    let args = ["--json", "import", "mcpm", &root, "--home", &home];

    let first = sb.ctl(&args);
    first.assert_ok();
    let (created, _) = counts(&first.data());
    assert!(
        created > 0,
        "the first run must write: {}",
        first.describe()
    );
    let client = std::fs::read_to_string(sb.home.join(".claude.json")).unwrap();
    assert!(
        client.contains("\"toolport\"") && !client.contains("mcpm_import-env"),
        "the first run must switch the client: {client}"
    );
    assert!(
        sb.data.join("secrets.enc").is_file(),
        "the first run must reach the vault"
    );
    let after_first = sb.tree();

    let second = sb.ctl(&args);
    second.assert_ok();
    assert_eq!(counts(&second.data()), (0, 0), "{}", second.describe());
    assert_same("import mcpm", &after_first, &sb);
}

const SELF_ID: &str = "toolport-plus-self";

fn enabled_in(sb: &Sandbox, profile: &str) -> bool {
    sb.registry()["profiles"]
        .as_array()
        .and_then(|profiles| profiles.iter().find(|p| p["id"] == profile))
        .and_then(|p| p["enabledServerIds"].as_array())
        .is_some_and(|ids| ids.iter().any(|id| id == SELF_ID))
}

fn has_self_server(sb: &Sandbox) -> bool {
    sb.server_names().iter().any(|name| name == SELF_ID)
}

fn doctor_state(sb: &Sandbox) -> (String, bool) {
    let run = sb.ctl(&["--json", "mcp", "doctor"]);
    (
        run.data()["state"].as_str().unwrap_or("").to_string(),
        run.code == Some(0),
    )
}

#[test]
fn import_mcpm_enables_the_self_server_once_and_a_user_choice_survives_reimports() {
    let sb = Sandbox::new("idem-self-choice");
    seed_clients(&sb);
    let root = mcpm_fixture(&sb, &Canary::new());
    let home = sb.home.to_string_lossy().into_owned();
    let args = ["--json", "import", "mcpm", &root, "--home", &home];

    sb.ctl(&args).assert_ok();
    for profile in ["default", "claude-code"] {
        assert!(enabled_in(&sb, profile), "{profile} after the first import");
    }
    assert_eq!(doctor_state(&sb), ("enabled".to_string(), true));

    let mut registry = sb.registry();
    for profile in registry["profiles"].as_array_mut().unwrap() {
        if profile["id"] == "claude-code" {
            profile["enabledServerIds"]
                .as_array_mut()
                .unwrap()
                .retain(|id| id != SELF_ID);
        }
    }
    sb.write_registry(&registry);
    sb.ctl(&args).assert_ok();
    let settled = sb.tree();
    assert!(!enabled_in(&sb, "claude-code"), "a re-import re-enabled it");
    assert!(enabled_in(&sb, "default"));
    assert_eq!(doctor_state(&sb), ("disabled".to_string(), true));

    let again = sb.ctl(&args);
    again.assert_ok();
    assert_eq!(counts(&again.data()), (0, 0), "{}", again.describe());
    assert_same("import after a user opt-out", &settled, &sb);
}

#[test]
fn import_mcpm_never_brings_back_an_uninstalled_self_server() {
    let sb = Sandbox::new("idem-self-uninstall");
    seed_clients(&sb);
    let root = mcpm_fixture(&sb, &Canary::new());
    let home = sb.home.to_string_lossy().into_owned();
    let args = ["--json", "import", "mcpm", &root, "--home", &home];

    sb.ctl(&args).assert_ok();
    sb.ctl(&["--json", "mcp", "uninstall"]).assert_ok();
    assert!(!has_self_server(&sb));
    sb.ctl(&args).assert_ok();
    let settled = sb.tree();
    assert!(!has_self_server(&sb));
    assert_eq!(doctor_state(&sb), ("opted-out".to_string(), true));

    let again = sb.ctl(&args);
    again.assert_ok();
    assert_eq!(counts(&again.data()), (0, 0), "{}", again.describe());
    assert_same("import after uninstall", &settled, &sb);

    sb.ctl(&["--json", "mcp", "install"]).assert_ok();
    assert!(has_self_server(&sb));
    assert_eq!(doctor_state(&sb), ("enabled".to_string(), true));
}

#[test]
fn import_mcpm_dry_run_writes_nothing() {
    let sb = Sandbox::new("idem-dry");
    seed_clients(&sb);
    let root = mcpm_fixture(&sb, &Canary::new());
    let before = sb.tree();
    let home = sb.home.to_string_lossy().into_owned();
    sb.ctl(&[
        "--json",
        "import",
        "mcpm",
        &root,
        "--home",
        &home,
        "--dry-run",
    ])
    .assert_ok();
    assert_same("import mcpm --dry-run", &before, &sb);
}

#[test]
fn client_sync_second_run_changes_nothing() {
    let sb = Sandbox::new("idem-client");
    seed_clients(&sb);
    let root = mcpm_fixture(&sb, &Canary::new());
    let home = sb.home.to_string_lossy().into_owned();
    sb.ctl(&["--json", "import", "mcpm", &root, "--home", &home])
        .assert_ok();
    write(
        &sb.home.join(".cursor/mcp.json"),
        &json!({"mcpServers": {
            "toolport": {"command": "stale"},
            "orphan-direct": {"command": "orphan", "args": []},
            "plain": {"command": "plain-server", "args": []}
        }})
        .to_string(),
    );

    let first = sb.ctl(&["--json", "client", "sync"]);
    first.assert_ok();
    let after_first = sb.tree();
    let second = sb.ctl(&["--json", "client", "sync"]);
    second.assert_ok();
    assert_same("client sync", &after_first, &sb);
    for row in second.data()["clients"].as_array().unwrap() {
        assert_eq!(
            row["removed"],
            json!([]),
            "nothing left to prune: {}",
            second.describe()
        );
    }
}

#[test]
fn context_sync_second_run_changes_nothing() {
    let sb = Sandbox::new("idem-context");
    let clients = sb.work.join("clients");
    write(&clients.join("acme/CLAUDE.local.md"), "Client notes\n");
    std::fs::create_dir_all(clients.join("acme/.git")).unwrap();
    write(
        &sb.home.join(".config/mcpm/context.json"),
        &json!({
            "profiles": {"research": {}},
            "settings": {"ensure_allow": ["Bash(ls:*)"], "ensure_ask": []},
            "wrap_default_claude": true,
            "clients_root": clients.to_string_lossy(),
        })
        .to_string(),
    );
    write(
        &sb.home.join(".claude/settings.json"),
        &json!({"permissions": {"allow": ["Read"]}}).to_string(),
    );
    let home = sb.home.to_string_lossy().into_owned();
    let args = ["--json", "context", "sync", "--home", &home, "--rules"];

    let first = sb.ctl(&args);
    first.assert_ok();
    let after_first = sb.tree();
    assert!(
        after_first.keys().any(|k| k.contains("claude-profiles")),
        "the first run must generate the profile: {:?}",
        after_first.keys().collect::<Vec<_>>()
    );
    sb.ctl(&args).assert_ok();
    assert_same("context sync", &after_first, &sb);
}

#[test]
fn compression_sync_second_run_changes_nothing() {
    let sb = Sandbox::new("idem-compression");
    std::fs::create_dir_all(sb.work.join("bin")).unwrap();
    sb.script(
        "bin/headroom",
        "case \"$1\" in\n  --version) echo 'headroom 0.29.0' ;;\n  agent-savings) echo '{\"HEADROOM_MODE\": \"token\", \"HEADROOM_MAX_ITEMS\": \"30\"}' ;;\n  *) exit 2 ;;\nesac",
    );
    let path = format!(
        "{}:/usr/bin:/bin",
        sb.work.join("bin").to_string_lossy()
    );
    sb.set_env("PATH", &path);
    write(
        &sb.home.join(".config/mcpm/compression.json"),
        &json!({"provider": "headroom", "runtime": "proxy", "active_preset": "agent"}).to_string(),
    );
    let args = ["--json", "compression", "sync"];

    let first = sb.ctl(&args);
    first.assert_ok();
    assert_eq!(first.data()["provider"], "headroom", "{}", first.describe());
    assert!(first.data()["adopted"].is_object(), "{}", first.describe());
    let after_first = sb.tree();
    for file in ["compression.json", "compression-shims.zsh", "compression-env.sh"] {
        assert!(
            after_first.contains_key(&format!("data/{file}")),
            "{file}: {:?}",
            after_first.keys().collect::<Vec<_>>()
        );
    }
    let registry = String::from_utf8_lossy(&after_first["data/registry.json"]).into_owned();
    assert!(registry.contains("plus:compression"), "{registry}");

    let second = sb.ctl(&args);
    second.assert_ok();
    assert!(second.data()["adopted"].is_null(), "{}", second.describe());
    assert_same("compression sync", &after_first, &sb);
    let raw = tree_diff(&after_first, &sb.tree());
    assert!(raw.is_empty(), "{raw:?}");
}

#[test]
fn skills_sync_second_run_changes_nothing() {
    let sb = Sandbox::new("idem-skills");
    let repo = sb.skills_repo().to_string_lossy().into_owned();
    let args = [
        "--json",
        "skills",
        "sync",
        "--repo",
        &repo,
        "--client",
        "claude-code",
    ];
    let first = sb.ctl(&args);
    first.assert_ok();
    assert!(
        sb.home.join(".claude/skills/demo/SKILL.md").is_file(),
        "{}",
        first.describe()
    );
    let after_first = sb.tree();
    let second = sb.ctl(&args);
    second.assert_ok();
    assert_eq!(second.data()["cleaned"], json!([]), "{}", second.describe());
    assert_same("skills sync", &after_first, &sb);
    let raw = tree_diff(&after_first, &sb.tree());
    assert!(
        raw.iter().all(|line| line.contains("mcpm-skills.lock")),
        "only the lockfile restamp may differ: {raw:?}"
    );
}

#[test]
fn selfmcp_content_writers_second_call_changes_nothing() {
    let sb = Sandbox::new("idem-selfmcp");
    let repo = sb.skills_repo().to_string_lossy().into_owned();
    let mut session = sb.selfmcp();
    let calls = [
        (
            "skills_sync",
            json!({"repo_path": repo, "client_keys": ["claude-code"], "global_mode": true}),
        ),
        (
            "agents_sync",
            json!({"repo_path": repo, "client_keys": ["claude-code"], "global_mode": true}),
        ),
        ("styles_sync_tier1", json!({"repo_path": repo})),
        (
            "styles_apply",
            json!({"repo_path": repo, "name": "plain", "client_keys": ["claude-code"], "confirm": true}),
        ),
    ];
    for (tool, args) in &calls {
        let reply = session.call(tool, args.clone());
        assert_eq!(reply["result"]["isError"], false, "{tool}: {reply}");
    }
    let after_first = sb.tree();
    for (tool, args) in &calls {
        let reply = session.call(tool, args.clone());
        assert_eq!(reply["result"]["isError"], false, "{tool}: {reply}");
    }
    assert_same("selfmcp content writers", &after_first, &sb);
}

#[test]
fn selfmcp_registration_second_call_changes_nothing() {
    let sb = Sandbox::new("idem-register");
    let _lock = conduit_lib::registry::data_dir_test_lock();
    let _override = conduit_lib::registry::DataDirOverride::set(&sb.data);
    let (id, first) = conduit_lib::plus::selfmcp::register::ensure_self_server().unwrap();
    assert_eq!(
        first,
        conduit_lib::plus::selfmcp::register::Ensured::Created
    );
    let after_first = sb.tree();
    let (again, second) = conduit_lib::plus::selfmcp::register::ensure_self_server().unwrap();
    assert_eq!(
        second,
        conduit_lib::plus::selfmcp::register::Ensured::Unchanged
    );
    assert_eq!(again, id);
    assert_same("selfmcp register", &after_first, &sb);
    let handled = conduit_lib::plus::dispatch("plus.selfmcp.ensure", json!({})).unwrap();
    assert_eq!(handled["action"], "unchanged");
    assert_same("plus.selfmcp.ensure", &after_first, &sb);
}

#[test]
fn council_install_second_run_changes_nothing() {
    let sb = Sandbox::new("idem-council");
    sb.set_env("COUNCIL_KEY_IDEM", &Canary::new().val("council"));
    let args = [
        "--json",
        "council",
        "install",
        "--api-key-env",
        "COUNCIL_KEY_IDEM",
    ];
    sb.ctl(&args).assert_ok();
    let after_first = sb.tree();
    sb.ctl(&args).assert_ok();
    assert_same("council install", &after_first, &sb);
}

#[test]
fn secret_set_of_the_same_value_keeps_the_vault_content() {
    let sb = Sandbox::new("idem-secret");
    let value = Canary::new().val("secret");
    sb.write_registry(&json!({
        "version": 1,
        "servers": [{"id": "alpha", "name": "alpha", "transport": "stdio",
                     "command": "alpha-server", "args": []}],
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}],
    }));
    for _ in 0..2 {
        sb.ctl_in(
            &["--json", "secret", "set", "alpha", "ALPHA_API_KEY"],
            &value,
        )
        .assert_ok();
    }
    let read = sb.ctl(&[
        "--json",
        "secret",
        "get",
        "alpha",
        "ALPHA_API_KEY",
        "--reveal",
    ]);
    assert_eq!(read.data()["value"], json!(value));
}

struct Cutover<'a> {
    sb: &'a Sandbox,
    root: std::path::PathBuf,
    tools: String,
    home: String,
}

const CUTOVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/cutover/cutover.sh");
const ROLLBACK: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../scripts/cutover/rollback.sh"
);
const BACKUPS: &str = ".toolport-cutover-backups";

impl<'a> Cutover<'a> {
    fn new(sb: &'a Sandbox) -> Self {
        let root = sb.home.join(".config/mcpm");
        let launcher = json!({"mcpServers": {
            "mcpm_alpha": {"command": "mcpm", "args": ["run", "alpha"]}
        }});
        write(
            &root.join("servers.json"),
            &json!({
                "alpha": {"name": "alpha", "profile_tags": [], "command": "alpha-server",
                          "args": [], "env": {"ALPHA_API_KEY": Canary::new().val("cutover")}}
            })
            .to_string(),
        );
        write(&root.join("claude-code.json"), &launcher.to_string());
        write(
            &root.join("context.json"),
            &json!({"wrap_default_claude": true,
                    "settings": {"ensure_allow": ["mcp__mcpm_alpha"], "ensure_ask": []}})
            .to_string(),
        );
        write(&sb.home.join(".claude.json"), &launcher.to_string());
        write(
            &sb.home.join(".claude/settings.json"),
            &json!({"permissions": {"allow": ["mcp__mcpm_alpha__ping"]}}).to_string(),
        );
        let tools = sb.input.join("tools.json");
        write(&tools, &json!({"alpha": ["ping"]}).to_string());
        Self {
            sb,
            root,
            tools: tools.to_string_lossy().into_owned(),
            home: sb.home.to_string_lossy().into_owned(),
        }
    }

    fn bash(&self, label: &str, args: &[&str]) {
        let out = self
            .sb
            .command("bash")
            .args(args)
            .output()
            .expect("bash is required");
        assert!(
            out.status.success(),
            "{label}: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn run(&self, label: &str) {
        self.bash(
            label,
            &[
                CUTOVER,
                "--home",
                &self.home,
                "--tools",
                &self.tools,
                "--ctl",
                hardening_support::CTL,
            ],
        );
    }

    fn home_tree(&self) -> Tree {
        hardening_support::tree(&self.sb.home, &[BACKUPS])
    }
}

#[test]
fn cutover_second_run_changes_nothing() {
    let sb = Sandbox::new("idem-cutover");
    let cutover = Cutover::new(&sb);
    let settled = || {
        normalized(sb.tree())
            .into_iter()
            .filter(|(path, _)| !path.contains(BACKUPS))
            .collect::<Tree>()
    };
    cutover.run("first cutover");
    let client = std::fs::read_to_string(sb.home.join(".claude.json")).unwrap();
    assert!(
        client.contains("\"toolport\"") && !client.contains("mcpm_alpha"),
        "the first cutover must switch the client: {client}"
    );
    assert!(
        std::fs::read_to_string(sb.home.join(".claude/settings.json"))
            .unwrap()
            .contains("mcp__toolport__alpha__ping"),
        "the first cutover must rewrite permission references"
    );
    let context: Value =
        serde_json::from_str(&std::fs::read_to_string(cutover.root.join("context.json")).unwrap())
            .unwrap();
    assert_eq!(context["wrap_default_claude"], json!(false));
    assert_eq!(
        context["settings"]["ensure_allow"],
        json!([]),
        "the mcpm context policy must stop re-adding mcpm permissions: {context}"
    );
    let after_first = settled();
    cutover.run("second cutover");
    let diff = tree_diff(&after_first, &settled());
    assert!(diff.is_empty(), "the second cutover changed: {diff:#?}");
}

#[test]
fn cutover_then_rollback_restores_the_home_byte_for_byte() {
    let sb = Sandbox::new("idem-rollback");
    let cutover = Cutover::new(&sb);
    let original = cutover.home_tree();
    cutover.run("cutover");
    assert_ne!(
        cutover.home_tree(),
        original,
        "the cutover must change the home"
    );
    cutover.bash("rollback", &[ROLLBACK, "--home", &cutover.home]);
    let diff = tree_diff(&original, &cutover.home_tree());
    assert!(diff.is_empty(), "the rollback left a difference: {diff:#?}");
    cutover.bash("second rollback", &[ROLLBACK, "--home", &cutover.home]);
    let diff = tree_diff(&original, &cutover.home_tree());
    assert!(
        diff.is_empty(),
        "the second rollback changed the home: {diff:#?}"
    );
}
