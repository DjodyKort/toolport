//! `secret get --reveal` is the only sanctioned disclosure and doubles as the scanner's positive control.

#![cfg(unix)]

mod hardening_support;

#[path = "common/exec.rs"]
mod exec;
#[path = "common/plugins_world.rs"]
mod plugins_world;

use conduit_lib::plus::ctl::COMMANDS;
use hardening_support::{find_leaks, scan_tree, Canary, Run, Sandbox};
use serde_json::{json, Value};

fn plant_plugin_canaries(plugins: &plugins_world::PluginsWorld, canary: &Canary) {
    let settings_path = plugins.claude_home.join("settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    settings["pluginConfigs"]["ecc@ecc"]["options"]["api_token"] = json!(canary.val("plugin-option"));
    settings["hooks"]["PreToolUse"]
        .as_array_mut()
        .unwrap()
        .push(json!({"matcher": "Bash", "hooks": [{
            "type": "command",
            "command": format!(
                "SERVICE_TOKEN={} ./check.sh --api-key={}",
                canary.val("hook-env"),
                canary.val("hook-arg")
            )
        }]}));
    std::fs::write(&settings_path, settings.to_string()).unwrap();

    let mcp_path = plugins.plugin.join(".mcp.json");
    let mut mcp: Value = serde_json::from_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
    mcp["mcpServers"]["keyed"] = json!({
        "command": "node",
        "args": ["server.js", format!("--api-key={}", canary.val("mcp-arg"))],
        "env": {"SERVICE_KEY": canary.val("mcp-env")}
    });
    mcp["mcpServers"]["remote"] = json!({
        "url": format!("https://svc.example.invalid/mcp?token={}", canary.val("mcp-url")),
        "headers": {"Authorization": format!("Bearer {}", canary.val("mcp-header"))}
    });
    std::fs::write(&mcp_path, mcp.to_string()).unwrap();
}

/// The skills library with a remote URL that embeds a credential (rewritten to a local bare
/// repository by `insteadOf`, so `--fetch` runs offline), one commit ahead, found through
/// `skills_sync.json`. It lives under `input/`, which the disk scan skips: the fixture's own
/// `.git/config` has to hold the credential.
fn plant_library(sb: &Sandbox, canary: &Canary) {
    let base = sb.input.join("library-world");
    let bare = base.join("lib-remote.git");
    let lib = base.join("lib/ai-skills");
    let git = |dir: &std::path::Path, args: &[&str]| {
        let out = sb
            .command("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git is required");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::create_dir_all(&lib).unwrap();
    git(&bare, &["init", "--quiet", "--bare", "-b", "main"]);
    git(&lib, &["init", "--quiet", "-b", "main"]);
    let credentialed = format!(
        "https://library-user:{}@library.example.invalid/org/skills.git",
        canary.val("library-url")
    );
    git(&lib, &["remote", "add", "origin", &credentialed]);
    git(
        &lib,
        &[
            "config",
            "--local",
            &format!("url.{}.insteadOf", bare.display()),
            &credentialed,
        ],
    );
    for (name, push) in [("review", true), ("local-only", false)] {
        let skill = lib.join("skills").join(name).join("SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(
            skill,
            format!("---\nname: {name}\ndescription: The {name} skill\n---\nBody of {name}.\n"),
        )
        .unwrap();
        git(&lib, &["add", "-A"]);
        git(&lib, &["commit", "--quiet", "-m", &format!("Add {name}")]);
        if push {
            git(&lib, &["push", "--quiet", "-u", "origin", "main"]);
        }
    }
    std::fs::write(
        sb.data.join("skills_sync.json"),
        json!({"local_path": lib.to_string_lossy()}).to_string(),
    )
    .unwrap();
}

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
    project: String,
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
        let claude = sb.work.join("claude-stub.sh");
        let plugins = plugins_world::build_in(&sb.input.join("plugins-world"), &claude);
        plant_plugin_canaries(&plugins, &canary);
        sb.set_env("TOOLPORT_CLAUDE_BIN", &claude.to_string_lossy());
        sb.set_env("CLAUDE_CONFIG_DIR", &plugins.claude_home.to_string_lossy());
        sb.set_env("TOOLPORT_CLAUDE_MANAGED_SETTINGS", "");
        sb.write_registry(&json!({
            "version": 1,
            "servers": [
                {"id": "alpha", "name": "alpha", "transport": "stdio",
                 "command": alpha_bin.to_string_lossy(), "args": [],
                 "env": [{"key": "ALPHA_API_KEY", "secret": true}],
                 "plus": {"authProbe": {"kind": "stdio"}}},
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
        plant_library(&sb, &canary);
        Self {
            repo: repo.to_string_lossy().into_owned(),
            quiet: quiet.to_string_lossy().into_owned(),
            mcpm_root: mcpm_root.to_string_lossy().into_owned(),
            remote: remote.to_string_lossy().into_owned(),
            tools_file: tools_file.to_string_lossy().into_owned(),
            refs_file: refs_file.to_string_lossy().into_owned(),
            bundle_dir: bundle_dir.to_string_lossy().into_owned(),
            project: plugins.side.to_string_lossy().into_owned(),
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
        let fresh = format!("{work}/fresh-skills");
        let zip = format!("{work}/skills.zip");
        let unpacked = format!("{work}/unpacked-skills");
        let task_file = format!("{work}/leak-task.json");
        std::fs::write(
            &task_file,
            json!({
                "id": "leak-task", "title": "Leak check", "description": "", "enabled": true,
                "requires": {"servers": ["alpha"], "commands": []},
                "writesSecrets": [{"server": "alpha", "key": "EXTRA_KEY"}],
                "steps": [
                    {"id": "sign-in", "title": "Sign in", "type": "needs-you", "instructions": "Sign in first"},
                    {"id": "read", "title": "Read", "type": "mcp", "server": "alpha", "tool": "get", "args": {}, "capture": ["value"]},
                    {"id": "store", "title": "Store", "type": "secret-set", "server": "alpha", "key": "EXTRA_KEY", "from": "value"}
                ],
                "triggers": {"manual": true, "cli": false, "selfMcp": {"enabled": false, "approval": "every-run"}, "schedule": null, "onAuthFailure": []}
            })
            .to_string(),
        )
        .unwrap();
        vec![
            s(&["status"]),
            s(&["doctor"]),
            s(&["commands"]),
            s(&["server", "ls"]),
            s(&["server", "search", "--offline", "--limit", "3"]),
            s(&["server", "install", "GitHub", "--offline"]),
            s(&["server", "info", "alpha"]),
            s(&["server", "info", "import-bearer"]),
            s(&["server", "new", "newsrv", "--command", &self.quiet]),
            s(&["server", "edit", "newsrv", "--arg", "x"]),
            s(&["client", "direct", "add", "newsrv", "--client", "cursor", "--dry-run"]),
            s(&["client", "direct", "add", "newsrv", "--client", "cursor"]),
            s(&["client", "direct", "add", "alpha", "--client", "cursor"]),
            s(&["client", "direct", "ls"]),
            s(&["client", "direct", "ls", "--client", "cursor"]),
            s(&["direct", "run", "newsrv"]),
            s(&["client", "direct", "rm", "alpha", "--client", "cursor", "--dry-run"]),
            s(&["client", "direct", "rm", "alpha", "--client", "cursor"]),
            s(&["client", "direct", "rm", "newsrv", "--client", "cursor"]),
            s(&["client", "direct"]),
            s(&["direct"]),
            s(&["server", "uninstall", "newsrv", "--dry-run"]),
            s(&["server", "uninstall", "newsrv"]),
            s(&["server"]),
            s(&["inspect", "alpha"]),
            s(&["inspect", "gamma"]),
            s(&["profile", "inspect", "default"]),
            s(&["profile", "ls"]),
            s(&["profile", "ls", "--verbose"]),
            s(&["profile", "create", "leakcheck", "--dry-run"]),
            s(&["profile", "create", "leakcheck"]),
            s(&["profile", "edit", "leakcheck", "--add-server", "alpha", "--dry-run"]),
            s(&["profile", "edit", "leakcheck", "--name", "leakcheck2", "--add-server", "alpha,gamma"]),
            s(&["profile", "rm", "leakcheck2", "--dry-run"]),
            s(&["profile", "rm", "leakcheck2"]),
            s(&["profile"]),
            s(&["client", "ls"]),
            s(&["client", "edit", "cursor", "--set-profiles", "default", "--dry-run"]),
            s(&["client", "import", "cursor"]),
            s(&["client", "sync", "--dry-run"]),
            s(&["client", "sync"]),
            s(&["client"]),
            s(&["auth", "statusline"]),
            s(&["auth", "hook"]),
            s(&["auth", "probe"]),
            s(&["auth", "probe", "--server", "alpha", "--force"]),
            s(&["auth", "login", "alpha"]),
            s(&["auth", "login", "gamma", "--no-open"]),
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
            s(&["context", "loads", "--cwd", &work, "--no-lazy", "--measured"]),
            s(&["context", "measure", "--cwd", &work]),
            s(&["context", "measure", "--cwd", &work, "--yes"]),
            s(&["context", "measure", "--cwd", &work, "--without", "plugin:kit@market", "--force", "--yes"]),
            s(&["context", "folders", "--cwd", &work]),
            s(&["context", "checkpoint-status"]),
            s(&["context", "plan", "--home", &home]),
            s(&["context", "apply", "--home", &home, "--dry-run"]),
            s(&["context", "apply", "--home", &home]),
            s(&["context", "sync", "--home", &home]),
            s(&["task", "ls"]),
            s(&["task", "add", "leak-task", "--file", &task_file, "--dry-run"]),
            s(&["task", "add", "leak-task", "--file", &task_file]),
            s(&["task", "show", "leak-task"]),
            s(&["task", "run", "leak-task", "--dry-run"]),
            s(&["task", "history", "leak-task"]),
            s(&["task", "resume", "run-no-such"]),
            s(&["task", "cancel", "run-no-such"]),
            s(&["task", "edit", "leak-task", "--file", &task_file, "--dry-run"]),
            s(&["task", "rm", "leak-task", "--dry-run"]),
            s(&["task", "rm", "leak-task"]),
            s(&["task"]),
            s(&["context", "bundle", "ls"]),
            s(&["context", "bundle", "show", "leak-bundle"]),
            s(&["context", "bundle", "add", "leak-bundle", "--skills-off", "a-skill", "--dry-run"]),
            s(&["context", "bundle", "add", "leak-bundle", "--skills-off", "a-skill", "--plugins-off", "kit@market"]),
            s(&["context", "bundle", "show", "leak-bundle"]),
            s(&["context", "bundle", "edit", "leak-bundle", "--agents-off", "reviewer", "--dry-run"]),
            s(&["context", "bundle", "edit", "leak-bundle", "--agents-off", "reviewer"]),
            s(&["context", "bundle", "apply", "leak-bundle", "--cwd", &work, "--dry-run"]),
            s(&["context", "bundle", "apply", "leak-bundle", "--cwd", &work]),
            s(&["context", "bundle", "status", "--cwd", &work]),
            s(&["context", "bundle", "launch", "leak-bundle", "--cwd", &work]),
            s(&["context", "bundle", "config", "--auto-apply", "on"]),
            s(&["context", "bundle", "undo", "--cwd", &work, "--dry-run"]),
            s(&["context", "bundle", "undo", "--cwd", &work]),
            s(&["context", "use", "leak-bundle", "--cwd", &work, "--dry-run"]),
            s(&["context", "use", "--none", "--cwd", &work, "--dry-run"]),
            s(&["context", "bundle", "rm", "leak-bundle", "--dry-run"]),
            s(&["context", "bundle", "rm", "leak-bundle"]),
            s(&["context", "bundle"]),
            s(&["context", "sync", "--home", &home, "--dry-run"]),
            s(&["context", "init", "--home", &home, "--dry-run"]),
            s(&["context", "init", "--home", &home]),
            s(&["context", "status", "--home", &home]),
            s(&["context", "client", "add", "acme", "--home", &home, "--dry-run"]),
            s(&["context", "client", "add", "acme", "--home", &home]),
            s(&["context", "client", "edit", "acme", "--glob", "**/*.md", "--home", &home, "--dry-run"]),
            s(&["context", "client", "edit", "acme", "--glob", "**/*.md", "--home", &home]),
            s(&["context", "client", "list", "--home", &home]),
            s(&["context", "compose", "--cwd", &work]),
            s(&["context", "client", "rm", "acme", "--home", &home, "--dry-run"]),
            s(&["context", "client", "rm", "acme", "--home", &home]),
            s(&["context", "client"]),
            s(&["context", "profile", "add", "leakcheck", "--home", &home, "--dry-run"]),
            s(&["context", "profile", "add", "leakcheck", "--home", &home]),
            s(&["context", "profile", "list", "--home", &home]),
            s(&[
                "context", "profile", "remove", "leakcheck", "--purge", "--home", &home,
                "--dry-run",
            ]),
            s(&["context", "profile", "remove", "leakcheck", "--purge", "--home", &home]),
            s(&["context", "profile"]),
            s(&["context", "disable", "--purge-profiles", "--home", &home, "--dry-run"]),
            s(&["context", "disable", "--purge-profiles", "--home", &home]),
            s(&["context"]),
            s(&["compression", "status"]),
            s(&["compression", "presets"]),
            s(&["compression", "run", "--plan"]),
            s(&["compression", "verify"]),
            s(&["compression", "ledger", "summary"]),
            s(&["compression", "proxy", "down"]),
            s(&["compression", "update", "--to", "0.29.0"]),
            s(&["compression", "env"]),
            s(&["compression", "pin"]),
            s(&["compression", "doctor"]),
            s(&["compression", "seal"]),
            s(&["compression", "enable", "--provider", "rtk-only", "--dry-run"]),
            s(&["compression", "enable", "--provider", "rtk-only"]),
            s(&["compression", "use", "agent"]),
            s(&["compression", "sync"]),
            s(&["compression", "set-provider", "none", "--dry-run"]),
            s(&["compression", "set-provider", "none"]),
            s(&["compression", "disable", "--teardown", "--dry-run"]),
            s(&["compression", "disable"]),
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
            s(&["mcp", "tools"]),
            s(&["mcp", "doctor"]),
            s(&["mcp", "install"]),
            s(&["mcp", "doctor"]),
            s(&["mcp", "uninstall"]),
            s(&["skills", "ls", "--repo", &self.repo]),
            s(&["skills", "ls", "--repo", &self.repo, "--source", "library"]),
            s(&["sources", "ls"]),
            s(&["sources", "ls", "--items", "--deep", "--refresh"]),
            s(&["sources", "root", "ls"]),
            s(&["sources", "root", "add", &work, "--dry-run"]),
            s(&["sources", "root", "add", &work]),
            s(&["sources", "root", "rm", &work, "--dry-run"]),
            s(&["sources", "root", "rm", &work]),
            s(&["sources", "root"]),
            s(&["sources"]),
            s(&["attention", "ls"]),
            s(&["attention", "ls", "--level", "needs-you"]),
            s(&["attention", "dismiss", "hardening:probe", "--dry-run"]),
            s(&["attention", "dismiss", "hardening:probe", "--until", "2999-01-01"]),
            s(&["attention"]),
            s(&["library", "status"]),
            s(&["library", "status", "--fetch"]),
            s(&["library", "pull", "--dry-run"]),
            s(&["library", "pull"]),
            s(&["library", "push", "--dry-run"]),
            s(&["library", "push"]),
            s(&["library"]),
            s(&["plugins", "ls"]),
            s(&["plugins", "ls", "--refresh"]),
            s(&["plugins", "ls", "--cwd", &self.project]),
            s(&["plugins", "show", "ecc@ecc"]),
            s(&["plugins", "show", "ecc", "--cwd", &self.project]),
            s(&["plugins", "show", "missing@nowhere"]),
            s(&["plugins", "config", "ecc@ecc", "--cwd", &self.project, "--set", "hook_profile=minimal", "--set", "gateguard_exempt_globs=docs/**", "--dry-run"]),
            s(&["plugins", "config", "ecc@ecc", "--cwd", &self.project, "--set", "hook_profile=minimal", "--set", "gateguard_exempt_globs=docs/**"]),
            s(&["plugins", "config", "ecc@ecc", "--set", "hook_profile=strict", "--dry-run"]),
            s(&["plugins", "config", "ecc@ecc", "--set", "hook_profile=strict"]),
            s(&["plugins", "config", "ecc@ecc", "--set", "api_token=x"]),
            s(&["plugins", "config", "ecc@ecc", "--cwd", &self.project, "--unset", "hook_profile", "--unset", "gateguard_exempt_globs"]),
            s(&["plugins", "config"]),
            s(&["plugins", "mcp", "deny", "ecc@ecc", "keyed", "--cwd", &self.project, "--dry-run"]),
            s(&["plugins", "mcp", "deny", "ecc@ecc", "remote", "--cwd", &self.project]),
            s(&["plugins", "mcp", "allow", "ecc@ecc", "remote", "--cwd", &self.project]),
            s(&["plugins", "mcp", "deny", "ecc@ecc", "nope", "--cwd", &self.project]),
            s(&["plugins", "mcp"]),
            s(&["plugins"]),
            s(&["hooks", "ls"]),
            s(&["hooks", "ls", "--cwd", &self.project]),
            s(&["hooks", "ls", "--tool", "Bash", "--owner", "user"]),
            s(&["hooks"]),
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
            s(&["skills", "status", "--repo", &self.repo]),
            s(&["skills", "resolve", "--repo", &self.repo, "--dry-run"]),
            s(&["skills", "clean", "--repo", &self.repo, "--dry-run"]),
            s(&["skills", "uninstall", "demo", "--repo", &self.repo, "--dry-run"]),
            s(&["skills", "audit", "--repo", &self.repo]),
            s(&["skills", "init", "--path", &fresh, "--name", "fresh"]),
            s(&["skills", "add", "fresh-skill", "--path", &fresh]),
            s(&["skills", "bundle", "--repo", &self.repo, "--dry-run"]),
            s(&["skills", "bundle", "--repo", &self.repo, "--output", &zip]),
            s(&["skills", "unbundle", &zip, "--path", &unpacked, "--dry-run"]),
            s(&["skills", "unbundle", &zip, "--path", &unpacked]),
            s(&["skills", "tap", "add", "acme/skills", "--dry-run"]),
            s(&["skills", "tap", "ls"]),
            s(&["skills", "tap", "remove", "acme-skills", "--dry-run"]),
            s(&["skills", "tap", "update", "--dry-run"]),
            s(&["skills", "search", "review"]),
            s(&[
                "skills",
                "install",
                "@acme/skills",
                "--path",
                &self.repo,
                "--dry-run",
            ]),
            s(&["skills"]),
            s(&["agents", "ls", "--path", &self.repo]),
            s(&["agents", "lint", "--path", &self.repo]),
            s(&["agents", "audit", "--path", &self.repo]),
            s(&["agents", "diff", "--path", &self.repo]),
            s(&["agents", "sync", "--path", &self.repo, "--dry-run"]),
            s(&[
                "agents",
                "sync",
                "--path",
                &self.repo,
                "--client",
                "claude-code",
            ]),
            s(&["agents", "diff", "--path", &self.repo]),
            s(&["agents", "status", "--path", &self.repo]),
            s(&["agents", "clean", "--path", &self.repo, "--dry-run"]),
            s(&["agents", "uninstall", "helper", "--path", &self.repo, "--dry-run"]),
            s(&["agents", "add", "fresh-agent", "--path", &fresh, "--dry-run"]),
            s(&["agents", "add", "fresh-agent", "--path", &fresh]),
            s(&["agents"]),
            s(&["styles", "ls", "--path", &self.repo]),
            s(&["styles", "lint", "--path", &self.repo]),
            s(&["styles", "diff", "--path", &self.repo]),
            s(&["styles", "sync", "--path", &self.repo, "--dry-run"]),
            s(&[
                "styles",
                "sync",
                "--path",
                &self.repo,
                "--client",
                "claude-code",
            ]),
            s(&["styles", "diff", "--path", &self.repo]),
            s(&["styles", "status", "--path", &self.repo]),
            s(&["styles", "apply", "plain", "--path", &self.repo, "--dry-run"]),
            s(&["styles", "apply", "plain", "--path", &self.repo, "--client", "zed"]),
            s(&["styles", "remove", "--path", &self.repo, "--dry-run"]),
            s(&["styles", "remove", "--path", &self.repo]),
            s(&["styles", "clean", "--path", &self.repo, "--dry-run"]),
            s(&["styles", "add", "fresh-style", "--path", &fresh, "--dry-run"]),
            s(&["styles", "add", "fresh-style", "--path", &fresh]),
            s(&["styles"]),
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
            s(&["usage", "--root", &work]),
            s(&["usage", "--no-refresh", "--root", &work]),
            s(&["obs"]),
            s(&["obs", "otel"]),
            s(&["obs", "otel", "status", "--home", &home]),
            s(&["obs", "otel", "enable", "--home", &home, "--dry-run"]),
            s(&["obs", "otel", "enable", "--home", &home]),
            s(&["obs", "otel", "status", "--home", &home]),
            s(&["obs", "otel", "disable", "--home", &home]),
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
            let launcher = case.first().map(String::as_str) == Some("direct")
                && case.get(1).map(String::as_str) == Some("run");
            if json_mode && !launcher {
                let envelope = run.envelope();
                assert!(
                    envelope["schemaVersion"].is_number() && envelope["command"].is_string(),
                    "not an envelope: {}",
                    run.describe()
                );
            }
        }
    }

    let status = world.sb.ctl(&["--json", "library", "status", "--fetch"]);
    status.assert_ok();
    assert_eq!(
        status.data()["fetch"]["ok"],
        json!(true),
        "positive control: the library fixture is found and fetched offline"
    );
    let remote = status.data()["remote"].as_str().unwrap().to_string();
    assert!(
        remote.contains("library.example.invalid") && !remote.contains("library-user"),
        "the remote URL is shown without its userinfo: {remote}"
    );

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

#[test]
fn a_direct_client_entry_holds_no_secret_and_the_launcher_hands_none_of_ours_to_the_child() {
    let world = World::new();
    world.seed_vault();
    let cursor = world.sb.home.join(".cursor");
    std::fs::create_dir_all(&cursor).unwrap();
    let config = cursor.join("mcp.json");
    std::fs::write(&config, "{\"mcpServers\": {}}\n").unwrap();
    for case in [
        &["client", "direct", "add", "alpha", "--client", "cursor", "--dry-run"][..],
        &["client", "direct", "add", "alpha", "--client", "cursor"],
        &["client", "direct", "add", "alpha", "--client", "cursor"],
        &["client", "direct", "ls"],
    ] {
        for json_mode in [true, false] {
            let mut args: Vec<&str> = Vec::new();
            if json_mode {
                args.push("--json");
            }
            args.extend(case);
            let run = world.sb.ctl(&args);
            run.assert_ok();
            world.assert_clean(&run, &case.join(" "));
        }
    }
    let written = std::fs::read_to_string(&config).unwrap();
    let entry: Value = serde_json::from_str(&written).unwrap();
    assert_eq!(entry["mcpServers"]["alpha"]["args"], json!(["direct", "run", "alpha"]));
    let leaks = find_leaks("cursor config", written.as_bytes(), &world.canary.all());
    assert!(leaks.is_empty(), "{leaks:?}\n{written}");

    let run = world.sb.ctl(&["direct", "run", "alpha"]);
    run.assert_failed();
    assert!(
        run.stdout.is_empty(),
        "a launcher prints nothing of its own on stdout: {}",
        run.describe()
    );
    let ours = [
        world.canary.val("vault-key"),
        world.canary.val("http-token"),
        world.canary.val("env-override"),
    ];
    let log = std::fs::read_to_string(world.sb.work.join("child-env.log")).unwrap();
    for canary in &ours {
        assert!(!log.contains(canary.as_str()), "the child saw {canary}: {log}");
    }
    assert!(
        run.stderr.contains(&world.canary.val("alpha-vault")),
        "the vault value must reach the child: {}",
        run.describe()
    );
    let leaks = find_leaks("launcher stdout", run.stdout.as_bytes(), &world.canary.all());
    assert!(leaks.is_empty(), "{leaks:?}");
    let leaks = scan_tree(&world.sb.tree(), &world.canary.all());
    assert!(leaks.is_empty(), "canary reached the disk: {leaks:#?}");
}

#[test]
fn client_import_never_prints_or_stores_a_client_config_canary() {
    let world = World::new();
    let c = &world.canary;
    let cursor = world.sb.home.join(".cursor");
    std::fs::create_dir_all(&cursor).unwrap();
    std::fs::write(
        cursor.join("mcp.json"),
        json!({"mcpServers": {
            "direct-env": {"command": "direct-server", "args": ["--serve"],
                           "env": {"DIRECT_API_KEY": c.val("client-env")}},
            "direct-arg": {"command": "direct-arg",
                           "args": [format!("--api-key={}", c.val("client-arg"))]},
            "direct-split": {"command": "direct-split",
                             "args": ["--token", c.val("client-split")]},
            "direct-url": {"url": format!("https://user:{}@direct.example.invalid/mcp",
                                          c.val("client-url"))},
            "direct-query": {"url": format!("https://direct.example.invalid/mcp?token={}",
                                            c.val("client-query"))}
        }})
        .to_string(),
    )
    .unwrap();
    let cases: [&[&str]; 5] = [
        &["client", "import", "cursor"],
        &["client", "import", "cursor", "--all", "--dry-run"],
        &["client", "edit", "cursor", "--set-profiles", "default", "--dry-run"],
        &["client", "import", "cursor", "--all", "--profile", "leak-import"],
        &["client", "import", "cursor", "--select", "direct-env,direct-arg"],
    ];
    for json_mode in [true, false] {
        for case in cases {
            let mut args: Vec<&str> = Vec::new();
            if json_mode {
                args.push("--json");
            }
            args.extend(case);
            let run = world.sb.ctl(&args);
            run.assert_orderly();
            world.assert_clean(&run, &case.join(" "));
        }
    }
    let registry = world.sb.registry();
    let server = registry["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "direct-env")
        .expect("direct-env was imported");
    assert_eq!(server["env"][0]["key"], "DIRECT_API_KEY");
    assert!(server["env"][0]["value"].is_null(), "{server}");
    for name in ["direct-arg", "direct-split", "direct-url", "direct-query"] {
        assert!(
            registry["servers"].as_array().unwrap().iter().all(|s| s["name"] != name),
            "{name} holds an inline credential and must not be imported"
        );
    }
    std::fs::remove_dir_all(&cursor).unwrap();
    let leaks = scan_tree(&world.sb.tree(), &world.canary.all());
    assert!(leaks.is_empty(), "canary reached the disk: {leaks:#?}");
}

fn leak_bundle(world: &World) -> String {
    world
        .sb
        .work
        .join("selfmcp-leak-bundle.zip")
        .to_string_lossy()
        .into_owned()
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
        "skills_bundle" => set(
            &mut args,
            &[("output", json!(leak_bundle(world))), ("dry_run", json!(false))],
        ),
        "skills_unbundle" => set(
            &mut args,
            &[("bundle_path", json!(leak_bundle(world))), ("dry_run", json!(false))],
        ),
        "compression_enable" => set(
            &mut args,
            &[
                ("provider", json!("rtk-only")),
                ("port", json!(9511)),
                ("dry_run", json!(false)),
            ],
        ),
        "compression_set_provider" => set(
            &mut args,
            &[("provider", json!("none")), ("dry_run", json!(false))],
        ),
        "compression_use" => set(
            &mut args,
            &[("preset", json!("agent")), ("dry_run", json!(false))],
        ),
        "compression_disable" => set(&mut args, &[("dry_run", json!(false))]),
        "compression_sync" => set(
            &mut args,
            &[
                ("dry_run", json!(false)),
                ("mcpm_root", json!(world.sb.work.to_string_lossy())),
            ],
        ),
        "clients_sync" | "sync_push" => set(&mut args, &[("dry_run", json!(false))]),
        "skills_clean" | "skills_resolve" | "agents_clean" | "styles_clean" => {
            set(&mut args, &[("dry_run", json!(false))])
        }
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
                ("dry_run", json!(false)),
            ],
        ),
        "styles_apply" | "styles_remove" | "styles_sync_tier1" => set(
            &mut args,
            &[
                ("client_keys", json!(["claude-code"])),
                ("dry_run", json!(false)),
            ],
        ),
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
        "client_direct_add" | "client_direct_rm" => set(
            &mut args,
            &[
                ("server", json!("alpha")),
                ("client", json!("cursor")),
                ("dry_run", json!(false)),
            ],
        ),
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
    let cursor = world.sb.home.join(".cursor");
    std::fs::create_dir_all(&cursor).unwrap();
    std::fs::write(cursor.join("mcp.json"), "{\"mcpServers\": {}}\n").unwrap();
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
    let repo = json!(world.repo);
    for (name, args) in [
        ("plugins_ls", json!({"cwd": world.project})),
        ("plugins_show", json!({"id": "ecc@ecc", "cwd": world.project})),
        ("plugins_config", json!({"id": "ecc@ecc", "cwd": world.project, "set": {"hook_profile": "minimal"}})),
        ("plugins_config", json!({"id": "ecc@ecc", "cwd": world.project, "set": {"hook_profile": "minimal"}, "dry_run": false})),
        ("plugins_config", json!({"id": "ecc@ecc", "cwd": world.project, "unset": ["hook_profile"], "dry_run": false})),
        ("plugins_mcp", json!({"action": "deny", "id": "ecc@ecc", "server": "keyed", "cwd": world.project})),
        ("plugins_mcp", json!({"action": "deny", "id": "ecc@ecc", "server": "remote", "cwd": world.project, "dry_run": false})),
        ("plugins_mcp", json!({"action": "allow", "id": "ecc@ecc", "server": "remote", "cwd": world.project, "dry_run": false})),
        ("hooks_ls", json!({"cwd": world.project})),
        ("hooks_ls", json!({"tool": "Bash"})),
        ("attention_ls", json!({})),
        ("attention_ls", json!({"level": "look"})),
        ("library_status", json!({"fetch": true})),
        ("library_pull", json!({"dry_run": false})),
    ] {
        let reply = session.call(name, args.clone());
        assert_eq!(reply["result"]["isError"], false, "{name} {args} -> {reply}");
    }
    for (name, args) in [
        ("skills_scaffold", json!({"name": "throwaway", "repo_path": repo})),
        (
            "skills_uninstall",
            json!({"name": "throwaway", "repo_path": repo, "dry_run": false, "confirm": true}),
        ),
        ("agents_scaffold", json!({"name": "throwaway", "repo_path": repo})),
        (
            "agents_uninstall",
            json!({"name": "throwaway", "repo_path": repo, "dry_run": false, "confirm": true}),
        ),
    ] {
        let reply = session.call(name, args.clone());
        assert_eq!(reply["result"]["isError"], false, "{name} {args} -> {reply}");
    }
    for tree in ["skills/throwaway", "agents/throwaway"] {
        assert!(!std::path::Path::new(&world.repo).join(tree).exists());
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
