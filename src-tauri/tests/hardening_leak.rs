//! `secret get --reveal` is the only sanctioned disclosure and doubles as the scanner's positive control.

#![cfg(unix)]

mod hardening_support;

use conduit_lib::plus::ctl::COMMANDS;
use hardening_support::{find_leaks, scan_tree, Canary, Run, Sandbox};
use serde_json::{json, Value};

struct World {
    sb: Sandbox,
    canary: Canary,
    repo: String,
    quiet: String,
    mcpm_root: String,
    remote: String,
    tools_file: String,
    refs_file: String,
    bundle_dir: String,
}

impl World {
    fn new() -> Self {
        let canary = Canary::new();
        let mut sb = Sandbox::new("leak");
        sb.key = canary.val("vault-key");
        sb.set_env("TOOLPORT_HTTP_TOKEN", &canary.val("http-token"));
        sb.set_env("TOOLPORT_SECRET_DELTA_TOKEN", &canary.val("env-override"));
        sb.set_env("TOOLPORT_ALLOW_BARE_SECRET_ENV", "1");
        sb.set_env("GAMMA_API_KEY", &canary.val("bare-env"));
        sb.set_env("DELTA_FILE_VALUE", &canary.val("delta-via-env"));
        sb.set_env("SYNC_PASSPHRASE_CANARY", &canary.val("sync-passphrase"));
        sb.set_env("COUNCIL_KEY_CANARY", &canary.val("council-key"));
        let leaky = |var: &str| {
            sb.script(
                &format!("leaky-{}.sh", var.to_lowercase()),
                &format!(
                    "echo \"{var}=${var}\" >&2\n\
                     echo \"master=$TOOLPORT_SECRET_KEY http=$TOOLPORT_HTTP_TOKEN\" >> \"$(dirname \"$0\")/child-env.log\"\n\
                     exit 1"
                ),
            )
        };
        let (alpha_bin, gamma_bin, delta_bin) = (
            leaky("ALPHA_API_KEY"),
            leaky("GAMMA_API_KEY"),
            leaky("DELTA_TOKEN"),
        );
        let quiet = sb.script("quiet.sh", "exit 1");
        let claude = sb.script("claude-stub.sh", "echo '[]'");
        sb.set_env("TOOLPORT_CLAUDE_BIN", &claude.to_string_lossy());
        sb.write_registry(&json!({
            "version": 1,
            "servers": [
                {"id": "alpha", "name": "alpha", "transport": "stdio",
                 "command": alpha_bin.to_string_lossy(), "args": [],
                 "env": [{"key": "ALPHA_API_KEY", "secret": true}]},
                {"id": "gamma", "name": "gamma", "transport": "stdio",
                 "command": gamma_bin.to_string_lossy(), "args": [],
                 "env": [{"key": "GAMMA_API_KEY", "secret": true}]},
                {"id": "delta", "name": "delta", "transport": "stdio",
                 "command": delta_bin.to_string_lossy(), "args": [],
                 "env": [{"key": "DELTA_TOKEN", "secret": true}]}
            ],
            "profiles": [{"id": "default", "name": "Default",
                          "enabledServerIds": ["alpha", "gamma", "delta"]}],
            "activeProfileId": "default"
        }));
        let mcpm_root = sb.mcpm_root(
            "mcpm",
            &json!({
                "import-env": {"name": "import-env", "command": "imported-server",
                               "args": ["--serve"],
                               "env": {"IMPORT_API_KEY": canary.val("import-env")}},
                "import-bearer": {"name": "import-bearer",
                                  "url": "https://bearer.example.invalid/mcp",
                                  "headers": {"Authorization":
                                      format!("Bearer {}", canary.val("import-bearer"))}},
                "import-arg": {"name": "import-arg", "command": "imported-arg",
                               "args": [format!("--api-key={}", canary.val("import-arg"))]}
            }),
            Some(&json!({"mcpServers": {
                "mcpm_import-env": {"command": "mcpm", "args": ["run", "import-env"]}
            }})),
        );
        let repo = sb.skills_repo();
        let tools_file = sb.input.join("tools.json");
        std::fs::write(
            &tools_file,
            json!({"import-env": ["alpha_tool"]}).to_string(),
        )
        .unwrap();
        let refs_file = sb.work.join("refs.md");
        std::fs::write(&refs_file, "uses mcp__mcpm_import-env__alpha_tool here\n").unwrap();
        let remote = sb.root.join("remote.git");
        let init = sb
            .command("git")
            .args(["init", "--bare", "--quiet"])
            .arg(&remote)
            .output()
            .expect("git is required");
        assert!(init.status.success(), "git init --bare failed");
        let bundle_dir = sb.input.join("bundle");
        std::fs::create_dir_all(&bundle_dir).unwrap();
        Self {
            repo: repo.to_string_lossy().into_owned(),
            quiet: quiet.to_string_lossy().into_owned(),
            mcpm_root: mcpm_root.to_string_lossy().into_owned(),
            remote: remote.to_string_lossy().into_owned(),
            tools_file: tools_file.to_string_lossy().into_owned(),
            refs_file: refs_file.to_string_lossy().into_owned(),
            bundle_dir: bundle_dir.to_string_lossy().into_owned(),
            sb,
            canary,
        }
    }

    fn seed_vault(&self) {
        let c = &self.canary;
        self.sb
            .ctl_in(
                &["--json", "secret", "set", "alpha", "ALPHA_API_KEY"],
                &c.val("alpha-vault"),
            )
            .assert_ok();
        self.sb
            .ctl_in(
                &["--json", "secret", "set", "gamma", "GAMMA_API_KEY"],
                &format!("{}\n", c.val("gamma-vault")),
            )
            .assert_ok();
        self.sb
            .ctl_env(
                &[
                    "--json",
                    "secret",
                    "set",
                    "delta",
                    "DELTA_TOKEN",
                    "--value-env",
                    "DELTA_FILE_VALUE",
                ],
                &[],
            )
            .assert_ok();
        let home = self.sb.home.to_string_lossy().into_owned();
        let imported = self
            .sb
            .ctl(&["--json", "import", "mcpm", &self.mcpm_root, "--home", &home]);
        imported.assert_ok();
        assert!(
            imported.data()["secrets"].as_array().unwrap().len() >= 3,
            "the mcpm fixture must put secrets into the vault: {}",
            imported.describe()
        );
    }

    fn ctl_cases(&self) -> Vec<Vec<String>> {
        let s = |list: &[&str]| list.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let home = self.sb.home.to_string_lossy().into_owned();
        let work = self.sb.work.to_string_lossy().into_owned();
        vec![
            s(&["status"]),
            s(&["doctor"]),
            s(&["server", "ls"]),
            s(&["server", "search", "--offline", "--limit", "3"]),
            s(&["server", "install", "GitHub", "--offline"]),
            s(&["server", "info", "alpha"]),
            s(&["server", "info", "import-bearer"]),
            s(&["server", "new", "newsrv", "--command", &self.quiet]),
            s(&["server", "edit", "newsrv", "--arg", "x"]),
            s(&["server", "uninstall", "newsrv", "--dry-run"]),
            s(&["server", "uninstall", "newsrv"]),
            s(&["server"]),
            s(&["inspect", "alpha"]),
            s(&["inspect", "gamma"]),
            s(&["profile", "inspect", "default"]),
            s(&["profile"]),
            s(&["client", "ls"]),
            s(&["client", "sync", "--dry-run"]),
            s(&["client", "sync"]),
            s(&["client"]),
            s(&["auth", "statusline"]),
            s(&["auth", "hook"]),
            s(&["auth"]),
            s(&["secret", "get", "alpha", "ALPHA_API_KEY"]),
            s(&["secret", "get", "gamma", "MISSING_KEY"]),
            s(&[
                "secret",
                "set",
                "alpha",
                "EXTRA_KEY",
                "--value-env",
                "DELTA_FILE_VALUE",
            ]),
            s(&["secret", "rm", "alpha", "EXTRA_KEY"]),
            s(&["secret"]),
            s(&["context", "loads", "--cwd", &work]),
            s(&["context", "folders", "--cwd", &work]),
            s(&["context", "checkpoint-status"]),
            s(&["context", "plan", "--home", &home]),
            s(&["context", "apply", "--home", &home, "--dry-run"]),
            s(&["context", "apply", "--home", &home]),
            s(&["context", "sync", "--home", &home]),
            s(&["context", "sync", "--home", &home, "--dry-run"]),
            s(&["context"]),
            s(&["compression", "status"]),
            s(&["compression", "presets"]),
            s(&["compression", "run", "--plan"]),
            s(&["compression", "verify"]),
            s(&["compression", "ledger", "summary"]),
            s(&["compression", "proxy", "down"]),
            s(&["compression", "update", "--to", "0.29.0"]),
            s(&["compression"]),
            s(&["import", "mcpm", &self.mcpm_root, "--home", &home]),
            s(&[
                "import",
                "mcpm",
                &self.mcpm_root,
                "--home",
                &home,
                "--dry-run",
            ]),
            s(&[
                "import",
                "mcpm",
                &self.mcpm_root,
                "--name-map",
                "--tools",
                &self.tools_file,
            ]),
            s(&[
                "import",
                "rename-refs",
                &self.mcpm_root,
                "--tools",
                &self.tools_file,
                "--paths",
                &self.refs_file,
                "--dry-run",
            ]),
            s(&["import"]),
            s(&["council", "tools"]),
            s(&["council", "doctor"]),
            s(&["council", "install", "--api-key-env", "COUNCIL_KEY_CANARY"]),
            s(&["council", "doctor"]),
            s(&["council", "uninstall", "--purge-key"]),
            s(&["skills", "ls", "--repo", &self.repo]),
            s(&["skills", "lint", "--repo", &self.repo]),
            s(&["skills", "diff", "--repo", &self.repo]),
            s(&["skills", "sync", "--repo", &self.repo, "--dry-run"]),
            s(&[
                "skills",
                "sync",
                "--repo",
                &self.repo,
                "--client",
                "claude-code",
            ]),
            s(&["skills", "diff", "--repo", &self.repo]),
            s(&["skills"]),
            s(&[
                "sync",
                "init",
                "--repo",
                &self.remote,
                "--passphrase-env",
                "SYNC_PASSPHRASE_CANARY",
            ]),
            s(&["sync", "status"]),
            s(&["sync", "push", "--dry-run"]),
            s(&["sync", "push"]),
            s(&["sync", "diff"]),
            s(&["sync", "pull", "--dry-run"]),
            s(&["sync", "pull"]),
            s(&["sync", "add-project", &work, "--name", "workdir"]),
            s(&["sync", "remove-project", "workdir"]),
            s(&["sync", "git-sync", "--status"]),
            s(&[
                "sync",
                "rotate-passphrase",
                "--passphrase-env",
                "SYNC_PASSPHRASE_CANARY",
            ]),
            s(&["sync", "migrate", &self.bundle_dir]),
            s(&["sync", "reset"]),
            s(&["sync"]),
            s(&["cc", "list"]),
            s(&["cc", "update", "--dry-run"]),
            s(&["update", "--check"]),
            s(&["update", "--dry-run"]),
        ]
    }

    fn assert_clean(&self, run: &Run, context: &str) {
        run.assert_orderly();
        let leaks = find_leaks(context, run.combined().as_bytes(), &self.canary.all());
        assert!(leaks.is_empty(), "{leaks:?}\n{}", run.describe());
    }
}

fn command_prefix(args: &[String]) -> Vec<&str> {
    args.iter()
        .map(String::as_str)
        .take_while(|a| !a.starts_with('-'))
        .collect()
}

#[test]
fn every_ctl_command_is_in_the_canary_matrix() {
    let world = World::new();
    let cases = world.ctl_cases();
    for command in COMMANDS {
        let covered = cases.iter().any(|case| {
            let words = command_prefix(case);
            words.len() >= command.path.len() && words[..command.path.len()] == *command.path
        });
        assert!(
            covered,
            "toolportctl {} has no canary case: add it to ctl_cases",
            command.path.join(" ")
        );
    }
}

#[test]
fn ctl_commands_never_print_or_store_a_canary() {
    let world = World::new();
    world.seed_vault();
    let reveal = world.sb.ctl(&[
        "--json",
        "secret",
        "get",
        "alpha",
        "ALPHA_API_KEY",
        "--reveal",
    ]);
    reveal.assert_ok();
    assert_eq!(
        reveal.data()["value"],
        json!(world.canary.val("alpha-vault")),
        "positive control: --reveal prints the stored value"
    );
    assert!(
        !find_leaks("control", reveal.combined().as_bytes(), &world.canary.all()).is_empty(),
        "the scanner must see a canary it is given"
    );

    for json_mode in [true, false] {
        for case in world.ctl_cases() {
            let mut args: Vec<&str> = Vec::new();
            if json_mode {
                args.push("--json");
            }
            args.extend(case.iter().map(String::as_str));
            let stdin = if case.first().map(String::as_str) == Some("context")
                && case.get(1).map(String::as_str) == Some("checkpoint-status")
            {
                Some("{\"context_window\": {\"used_percentage\": 10}}")
            } else {
                None
            };
            let run = match stdin {
                Some(text) => world.sb.ctl_in(&args, text),
                None => world.sb.ctl(&args),
            };
            world.assert_clean(&run, &case.join(" "));
            if json_mode {
                let envelope = run.envelope();
                assert!(
                    envelope["schemaVersion"].is_number() && envelope["command"].is_string(),
                    "not an envelope: {}",
                    run.describe()
                );
            }
        }
    }

    let sync_clone = world.sb.root.join("clone");
    let _ = world
        .sb
        .command("git")
        .args(["clone", "--quiet", &world.remote])
        .arg(&sync_clone)
        .output();

    let leaks = scan_tree(&world.sb.tree(), &world.canary.all());
    assert!(leaks.is_empty(), "canary reached the disk: {leaks:#?}");
}

fn tool_args(name: &str, schema: &Value, world: &World) -> Value {
    let props = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut args = serde_json::Map::new();
    let required: Vec<&str> = schema["required"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for key in &required {
        let ty = props[*key]["type"].as_str().unwrap_or("string");
        args.insert(
            (*key).to_string(),
            match ty {
                "boolean" => json!(true),
                "object" => json!({}),
                "array" => json!(["x"]),
                _ => json!("x"),
            },
        );
    }
    if props.contains_key("repo_path") {
        args.insert("repo_path".into(), json!(world.repo));
    }
    if props.contains_key("confirm") {
        args.insert("confirm".into(), json!(true));
    }
    let set = |args: &mut serde_json::Map<String, Value>, pairs: &[(&str, Value)]| {
        for (key, value) in pairs {
            args.insert((*key).to_string(), value.clone());
        }
    };
    let entity = if name.starts_with("agents_") {
        "helper"
    } else if name.starts_with("styles_") {
        "plain"
    } else if name.starts_with("servers_") {
        "alpha"
    } else {
        "demo"
    };
    if props.contains_key("name") {
        args.insert("name".into(), json!(entity));
    }
    match name {
        "skills_scaffold" => set(&mut args, &[("name", json!("fresh"))]),
        "skills_delete" => set(&mut args, &[("name", json!("fresh"))]),
        "agents_scaffold" => set(&mut args, &[("name", json!("fresh-agent"))]),
        "styles_scaffold" => set(&mut args, &[("name", json!("fresh-style"))]),
        "skills_edit_body" | "agents_edit_body" | "styles_edit_body" => {
            set(&mut args, &[("new_body", json!("Replacement body"))])
        }
        "skills_edit_frontmatter" => set(&mut args, &[("patch", json!({"description": "Edited"}))]),
        "skills_git_push" => set(&mut args, &[("commit_message", json!("sync"))]),
        "skills_sync" | "agents_sync" => set(
            &mut args,
            &[
                ("client_keys", json!(["claude-code"])),
                ("global_mode", json!(true)),
            ],
        ),
        "styles_apply" | "styles_remove" | "styles_sync_tier1" => {
            set(&mut args, &[("client_keys", json!(["claude-code"]))])
        }
        "servers_install" => set(
            &mut args,
            &[
                ("name", json!("newsrv")),
                (
                    "config",
                    json!({"command": world.quiet, "transport": "stdio"}),
                ),
            ],
        ),
        "servers_update_config" => set(&mut args, &[("patch", json!({"cwd": "/"}))]),
        "servers_set_mode" => set(&mut args, &[("mode", json!("auto"))]),
        "servers_add_profile_tag" | "servers_remove_profile_tag" => {
            set(&mut args, &[("profile_tag", json!("default"))])
        }
        "servers_uninstall" => set(&mut args, &[("name", json!("delta"))]),
        "servers_check_updates" => {
            args.remove("name");
        }
        _ => {}
    }
    Value::Object(args)
}

#[test]
fn selfmcp_tools_and_resources_never_return_or_store_a_canary() {
    let world = World::new();
    world.seed_vault();
    let mut session = world.sb.selfmcp();
    let listed = session.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap().clone();
    assert!(!tools.is_empty());

    let catalog: Vec<&str> = conduit_lib::plus::selfmcp::TOOLS
        .iter()
        .map(|t| t.name)
        .collect();
    let mut called = Vec::new();
    for tool in &tools {
        let name = tool["name"].as_str().unwrap();
        let args = tool_args(name, &tool["inputSchema"], &world);
        let reply = session.call(name, args.clone());
        assert!(reply["result"].is_object(), "{name} {args} -> {reply}");
        called.push(name.to_string());
    }
    for name in catalog {
        assert!(called.iter().any(|c| c == name), "{name} was never called");
    }
    let resources = session.request("resources/list", json!({}));
    for resource in resources["result"]["resources"].as_array().unwrap() {
        let uri = resource["uri"].as_str().unwrap();
        let reply = session.read(uri);
        assert!(
            reply["result"].is_object() || reply["error"].is_object(),
            "{uri} -> {reply}"
        );
    }

    let leaks = find_leaks(
        "selfmcp transcript",
        session.transcript.as_bytes(),
        &world.canary.all(),
    );
    assert!(leaks.is_empty(), "{leaks:#?}");
    let leaks = find_leaks(
        "selfmcp stderr",
        session.stderr().as_bytes(),
        &world.canary.all(),
    );
    assert!(leaks.is_empty(), "{leaks:#?}");
    drop(session);
    let leaks = scan_tree(&world.sb.tree(), &world.canary.all());
    assert!(leaks.is_empty(), "canary reached the disk: {leaks:#?}");
}
