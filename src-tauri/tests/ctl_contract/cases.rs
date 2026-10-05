//! The bulk of the contract table (MIG-GUI-13): every command that `ctl_contract.rs` does not list
//! itself. Same rules: a writer previews with `--dry-run` first, then applies on the scratch world;
//! a command with a required operand gets a `usage` step; a command that cannot run headless
//! (`direct run`, `auth login`, `compression proxy`, `compression run`) records what it prints
//! without a terminal, a browser or an engine, and says so next to its case.

use crate::ctl_fixtures::{
    bundle_drift_home, bundle_home, fork_world, fork_world_upstream_fetched, git_world,
    health_proxy, import_world, loads_home, layers_home, measure_home, skills_repo_remote_world,
    tasks_home, transcripts_world,
};
use crate::ctl_world::CtlWorld;
use crate::library_world::{self, library_home};

use super::{
    apply, case, hook, plugins_home, prepared, read, setup, usage, Case, COUNCIL_KEY, PASSPHRASE,
    VAULTED,
};

const NEW_PASSPHRASE: &str = "FAKE-sync-passphrase-31d8-rotated";
const PIPED_TASK: &str = r#"{"id":"piped-cleanup","title":"Cleanup from stdin","description":"","enabled":true,"requires":{"servers":[],"commands":["echo"]},"writesSecrets":[],"steps":[{"id":"say","title":"Say hello","type":"exec","program":"echo","args":["clean"]}],"triggers":{"manual":true,"cli":true,"selfMcp":{"enabled":false,"approval":"every-run"},"schedule":null,"onAuthFailure":[]},"createdFrom":{"kind":"manual"}}"#;
const PIPED_EDIT: &str = r#"{"id":"nightly-report","title":"Nightly report (from stdin)","description":"","enabled":true,"requires":{"servers":[],"commands":["echo"]},"writesSecrets":[],"steps":[{"id":"say","title":"Say hello","type":"exec","program":"echo","args":["report"]}],"triggers":{"manual":true,"cli":false,"selfMcp":{"enabled":false,"approval":"every-run"},"schedule":{"cron":"0 3 * * *","autoRun":true},"onAuthFailure":[]},"createdFrom":null}"#;
const STATUSLINE: &str = r#"{"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":1000,"cache_read_input_tokens":500,"cache_creation_input_tokens":0}},"model":{"id":"claude-sonnet-5"}}"#;

const ADD_PROJECT: &[&str] = &[
    "sync",
    "add-project",
    "{home}",
    "--name",
    "proj1",
    "--files",
    "a.txt",
];

const OFF_PORT_PROXY: &[&str] = &[
    "compression",
    "enable",
    "--provider",
    "headroom",
    "--port",
    "29213",
];

const PUSH_ALL: &[&str] = &["sync", "push", "--include-projects"];

const SYNC_INIT: &[&str] = &[
    "sync",
    "init",
    "--repo",
    "{base}/remote.git",
    "--machine-id",
    "m-test",
    "--passphrase-stdin",
];

const FILES_ONLY: &[(&str, &str)] = &[("TOOLPORT_CLAUDE_BIN", "/nonexistent/claude")];

fn flip_ecc(world: &CtlWorld) {
    let file = world.base.join("home/work/acme-erp/.claude/settings.local.json");
    let mut doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    doc["enabledPlugins"]["ecc@ecc"] = serde_json::json!(true);
    std::fs::write(&file, doc.to_string()).unwrap();
}

fn mcp_call_world(world: &CtlWorld) {
    fork_world(world);
    skills_repo_remote_world(world);
}

pub const MORE: &[Case] = &[
    // mcp call (contract section 15): one step per tool that no command covers, a read, a
    // refusal without `confirm` or a dry run, and an apply on the scratch world for the writers
    prepared(
        "mcp call",
        mcp_call_world,
        &[
            read("agents_get", &["mcp", "call", "agents_get", "--args", r#"{"name":"helper","repo_path":"{repo}"}"#]),
            read("agents_list_transpilers", &["mcp", "call", "agents_list_transpilers"]),
            read("flow_diagram", &["mcp", "call", "flow_diagram", "--args", "{}"]),
            read("skills_get", &["mcp", "call", "skills_get", "--args", r#"{"name":"demo","repo_path":"{repo}"}"#]),
            read("skills_list_transpilers", &["mcp", "call", "skills_list_transpilers"]),
            read("styles_active", &["mcp", "call", "styles_active", "--args", r#"{"repo_path":"{repo}"}"#]),
            read("styles_get", &["mcp", "call", "styles_get", "--args", r#"{"name":"plain","repo_path":"{repo}"}"#]),
            read("styles_list_transpilers", &["mcp", "call", "styles_list_transpilers"]),
            read("where_am_i", &["mcp", "call", "where_am_i"]),
            read("servers_check_updates", &["mcp", "call", "servers_check_updates", "--args", r#"{"name":"alpha"}"#]),
            read("servers_detect_source", &["mcp", "call", "servers_detect_source", "--args", r#"{"name":"forked"}"#]),
            read("servers_git_status", &["mcp", "call", "servers_git_status", "--args", r#"{"name":"forked"}"#]),
            read("missing", &["mcp", "call", "skills_get", "--args", r#"{"name":"nope","repo_path":"{repo}"}"#]).exit(1),
            read("invalid", &["mcp", "call", "skills_get", "--args", "{}"]).exit(1),
            usage("unknown", &["mcp", "call", "skils_get"]),
            usage("operand", &["mcp", "call"]),
            usage("secret", &["mcp", "call", "servers_install", "--args", r#"{"config":{"env":{"API_KEY":"FAKE-inline-secret-9d41"}}}"#]),
            read("agents_edit_body.refused", &["mcp", "call", "agents_edit_body", "--args", r#"{"name":"helper","new_body":"Changed prompt\n","repo_path":"{repo}"}"#]).exit(1),
            apply("agents_edit_body", &["mcp", "call", "agents_edit_body", "--args", r#"{"name":"helper","new_body":"Changed prompt\n","repo_path":"{repo}","confirm":true}"#]),
            read("skills_edit_body.refused", &["mcp", "call", "skills_edit_body", "--args", r#"{"name":"demo","new_body":"Changed body\n","repo_path":"{repo}"}"#]).exit(1),
            apply("skills_edit_body", &["mcp", "call", "skills_edit_body", "--args", r#"{"name":"demo","new_body":"Changed body\n","repo_path":"{repo}","confirm":true}"#]),
            apply("skills_edit_frontmatter", &["mcp", "call", "skills_edit_frontmatter", "--args", r#"{"name":"demo","patch":{"description":"Edited description"},"repo_path":"{repo}","confirm":true}"#]),
            apply("styles_edit_body", &["mcp", "call", "styles_edit_body", "--args", r#"{"name":"plain","new_body":"Short\n","repo_path":"{repo}","confirm":true}"#]),
            apply("servers_add_profile_tag", &["mcp", "call", "servers_add_profile_tag", "--args", r#"{"name":"beta","profile_tag":"Default"}"#]),
            apply("servers_remove_profile_tag", &["mcp", "call", "servers_remove_profile_tag", "--args", r#"{"name":"beta","profile_tag":"Default"}"#]),
            read("servers_set_mode.refused", &["mcp", "call", "servers_set_mode", "--args", r#"{"name":"alpha","mode":"direct"}"#]).exit(1),
            apply("servers_set_mode", &["mcp", "call", "servers_set_mode", "--args", r#"{"name":"alpha","mode":"direct","confirm":true}"#]),
            apply("servers_apply_update", &["mcp", "call", "servers_apply_update", "--args", r#"{"name":"alpha","confirm":true}"#]),
            read("servers_fork_sync.refused", &["mcp", "call", "servers_fork_sync", "--args", r#"{"name":"forked"}"#]).exit(1),
            apply("servers_fork_sync", &["mcp", "call", "servers_fork_sync", "--args", r#"{"name":"forked","target_branch":"main-synced","confirm":true}"#]),
            read("skills_delete.refused", &["mcp", "call", "skills_delete", "--args", r#"{"name":"demo","repo_path":"{repo}"}"#]).exit(1),
            apply("skills_delete", &["mcp", "call", "skills_delete", "--args", r#"{"name":"demo","repo_path":"{repo}","confirm":true}"#]),
            read("skills_git_push.refused", &["mcp", "call", "skills_git_push", "--args", r#"{"commit_message":"add extra","repo_path":"{repo}"}"#]).exit(1),
            apply("skills_git_push", &["mcp", "call", "skills_git_push", "--args", r#"{"commit_message":"add extra","repo_path":"{repo}","confirm":true}"#]),
            apply("stdin", &["mcp", "call", "where_am_i", "--args-stdin"]).stdin("{}"),
        ],
    ),
    // library (MIG-SRC-3): a bare remote, the clone found through the sync config and a hook per
    // state the clone can be in; the library never talks to the network except with `--fetch`
    prepared(
        "library status",
        library_home,
        &[
            hook("ahead-world", library_world::ahead_one),
            read("ahead", &["library", "status"]),
            hook("duplicate-world", library_world::add_copy),
            read("duplicate", &["library", "status"]),
            hook("behind-world", library_world::behind_two),
            read("behind", &["library", "status"]),
            read("fetch", &["library", "status", "--fetch"]),
            hook("dirty-world", library_world::dirty),
            read("dirty", &["library", "status"]),
            hook("no-remote-world", library_world::no_remote),
            read("no-remote", &["library", "status"]),
            usage("usage", &["library", "status", "extra"]),
        ],
    ),
    prepared(
        "library pull",
        library_home,
        &[
            hook("behind-world", library_world::behind_two),
            hook("dirty-world", library_world::dirty),
            read("dirty-preview", &["library", "pull", "--dry-run"]).exit(1),
            apply("dirty", &["library", "pull"]).exit(1),
            hook("clean-world", library_world::clean),
            read("preview", &["library", "pull", "--dry-run"]),
            apply("apply", &["library", "pull"]),
            read("current", &["library", "pull", "--dry-run"]),
            usage("usage", &["library", "pull", "extra"]),
        ],
    ),
    prepared(
        "library push",
        library_home,
        &[
            hook("ahead-world", library_world::ahead_one),
            read("dry-run", &["library", "push", "--dry-run"]),
            hook("secret-world", library_world::secret_commit),
            read("secret-preview", &["library", "push", "--dry-run"]),
            apply("secret", &["library", "push"]).exit(1),
            hook("secret-dropped", library_world::drop_last_commit),
            apply("apply", &["library", "push"]),
            read("current", &["library", "push", "--dry-run"]),
            usage("usage", &["library", "push", "extra"]),
        ],
    ),
    // plugins and hooks (contract section 14): the recorded `claude` stub, and the same world with
    // `claude` missing, where the CLI-only figures are null
    prepared(
        "plugins ls",
        plugins_home,
        &[
            read("cli", &["plugins", "ls"]),
            read("files", &["plugins", "ls"]).env(FILES_ONLY),
            read("folder", &["plugins", "ls", "--cwd", "{home}/work/side-project"]),
            read("measured", &["plugins", "ls", "--cwd", "{home}/work/acme-erp"]),
            read("refresh", &["plugins", "ls", "--refresh"]),
            usage("usage", &["plugins", "ls", "extra"]),
            usage("badcwd", &["plugins", "ls", "--cwd", "{home}/work/nowhere"]),
        ],
    ),
    prepared(
        "plugins show",
        plugins_home,
        &[
            read("cli", &["plugins", "show", "ecc@ecc", "--cwd", "{home}/work/acme-erp"]),
            read("files", &["plugins", "show", "ecc@ecc", "--cwd", "{home}/work/acme-erp"])
                .env(FILES_ONLY),
            read("denied", &["plugins", "show", "ecc", "--cwd", "{home}/work/side-project"]),
            read("unknown", &["plugins", "show", "nope@nowhere"]).exit(1),
            usage("usage", &["plugins", "show"]),
        ],
    ),
    prepared(
        "plugins config",
        plugins_home,
        &[
            read("folder.plan", &["plugins", "config", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--set", "hook_profile=minimal", "--set", "gateguard=off", "--set", "gateguard_exempt_globs=docs/**,scripts/*.sh", "--dry-run"]),
            apply("folder.apply", &["plugins", "config", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--set", "hook_profile=minimal", "--set", "gateguard=off", "--set", "gateguard_exempt_globs=docs/**,scripts/*.sh"]),
            read("folder.unset-plan", &["plugins", "config", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--unset", "gateguard", "--unset", "hook_profile", "--dry-run"]),
            apply("folder.unset", &["plugins", "config", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--unset", "gateguard", "--unset", "hook_profile", "--unset", "gateguard_exempt_globs"]),
            read("global.plan", &["plugins", "config", "ecc@ecc", "--set", "hook_profile=strict", "--set", "hooks_enabled=false", "--dry-run"]),
            apply("global.apply", &["plugins", "config", "ecc@ecc", "--set", "hook_profile=strict", "--set", "hooks_enabled=false"]),
            usage("unknown-knob", &["plugins", "config", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--set", "no_such_knob=1"]),
            usage("folder-only", &["plugins", "config", "ecc@ecc", "--set", "gateguard_exempt_globs=docs/**"]),
            usage("bad-value", &["plugins", "config", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--set", "hook_profile=loud"]),
            usage("nothing", &["plugins", "config", "ecc@ecc"]),
            usage("usage", &["plugins", "config"]),
            read("unknown", &["plugins", "config", "nope@nowhere", "--cwd", "{home}/work/acme-erp", "--set", "gateguard=off", "--dry-run"]).exit(1),
        ],
    ),
    prepared(
        "plugins mcp",
        plugins_home,
        &[
            read("deny.plan", &["plugins", "mcp", "deny", "ecc@ecc", "chrome-devtools", "--cwd", "{home}/work/acme-erp", "--dry-run"]),
            apply("deny.apply", &["plugins", "mcp", "deny", "ecc@ecc", "chrome-devtools", "--cwd", "{home}/work/acme-erp"]),
            read("allow.plan", &["plugins", "mcp", "allow", "ecc@ecc", "chrome-devtools", "--cwd", "{home}/work/acme-erp", "--dry-run"]),
            apply("allow.apply", &["plugins", "mcp", "allow", "ecc@ecc", "chrome-devtools", "--cwd", "{home}/work/acme-erp"]),
            read("deny.foreign", &["plugins", "mcp", "deny", "ecc@ecc", "chrome-devtools", "--cwd", "{home}/work/side-project", "--dry-run"]),
            read("unknown-server", &["plugins", "mcp", "deny", "ecc@ecc", "nope", "--cwd", "{home}/work/acme-erp", "--dry-run"]).exit(1),
            read("unknown", &["plugins", "mcp", "deny", "nope@nowhere", "x", "--cwd", "{home}/work/acme-erp", "--dry-run"]).exit(1),
            usage("no-cwd", &["plugins", "mcp", "deny", "ecc@ecc", "chrome-devtools"]),
            usage("bad-action", &["plugins", "mcp", "block", "ecc@ecc", "chrome-devtools", "--cwd", "{home}/work/acme-erp"]),
            usage("usage", &["plugins", "mcp", "deny"]),
        ],
    ),
    prepared(
        "plugins off",
        plugins_home,
        &[
            read("plan", &["plugins", "off", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--dry-run"]),
            apply("apply", &["plugins", "off", "ecc@ecc", "--cwd", "{home}/work/acme-erp"]),
            read("again", &["plugins", "off", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--dry-run"]),
            read("foreign", &["plugins", "off", "ecc@ecc", "--cwd", "{home}/work/side-project", "--dry-run"]),
            read("unknown", &["plugins", "off", "nope@nowhere", "--cwd", "{home}/work/acme-erp", "--dry-run"]).exit(1),
            usage("no-cwd", &["plugins", "off", "ecc@ecc"]),
            usage("bad-cwd", &["plugins", "off", "ecc@ecc", "--cwd", "{home}/work/nowhere"]),
            usage("usage", &["plugins", "off"]),
        ],
    ),
    prepared(
        "plugins on",
        plugins_home,
        &[
            read("nothing", &["plugins", "on", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--dry-run"]),
            apply("setup", &["plugins", "off", "ecc@ecc", "--cwd", "{home}/work/acme-erp"]),
            read("plan", &["plugins", "on", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--dry-run"]),
            apply("apply", &["plugins", "on", "ecc@ecc", "--cwd", "{home}/work/acme-erp"]),
            read("foreign", &["plugins", "on", "ecc@ecc", "--cwd", "{home}/work/side-project", "--dry-run"]),
            apply("setup-conflict", &["plugins", "off", "ecc@ecc", "--cwd", "{home}/work/acme-erp"]),
            hook("flip", flip_ecc),
            read("conflict.plan", &["plugins", "on", "ecc@ecc", "--cwd", "{home}/work/acme-erp", "--dry-run"]),
            apply("conflict", &["plugins", "on", "ecc@ecc", "--cwd", "{home}/work/acme-erp"]),
            read("unknown", &["plugins", "on", "nope@nowhere", "--cwd", "{home}/work/acme-erp", "--dry-run"]).exit(1),
            usage("no-cwd", &["plugins", "on", "ecc@ecc"]),
            usage("usage", &["plugins", "on"]),
        ],
    ),
    prepared(
        "plugins disable",
        plugins_home,
        &[
            read("plan", &["plugins", "disable", "ecc@ecc", "--dry-run"]),
            apply("apply", &["plugins", "disable", "ecc@ecc"]),
            read("no-claude", &["plugins", "disable", "ecc@ecc", "--dry-run"]).env(FILES_ONLY).exit(1),
            read("unknown", &["plugins", "disable", "nope@nowhere", "--dry-run"]).exit(1),
            usage("usage", &["plugins", "disable"]),
        ],
    ),
    prepared(
        "plugins enable",
        plugins_home,
        &[
            read("plan", &["plugins", "enable", "ecc@ecc", "--dry-run"]),
            apply("apply", &["plugins", "enable", "ecc@ecc"]),
            read("no-claude", &["plugins", "enable", "ecc@ecc", "--dry-run"]).env(FILES_ONLY).exit(1),
            read("unknown", &["plugins", "enable", "nope@nowhere", "--dry-run"]).exit(1),
            usage("usage", &["plugins", "enable"]),
        ],
    ),
    prepared(
        "hooks ls",
        plugins_home,
        &[
            read("full", &["hooks", "ls", "--cwd", "{home}/work/acme-erp"]),
            read("bash", &["hooks", "ls", "--cwd", "{home}/work/acme-erp", "--tool", "Bash"]),
            read("skill", &["hooks", "ls", "--owner", "skill"]),
            read("project", &["hooks", "ls", "--cwd", "{home}/work/side-project"]),
            read("disabled", &["hooks", "ls", "--cwd", "{home}/work/quiet"]),
            usage("usage", &["hooks", "ls", "--owner", "nope"]),
        ],
    ),
    // server
    case(
        "server install",
        &[
            apply("apply", &["server", "install", "Stripe", "--offline"]),
            apply("again", &["server", "install", "Stripe", "--offline"]).exit(1),
            apply(
                "unknown",
                &["server", "install", "no-such-server", "--offline"],
            )
            .exit(1),
            usage("usage", &["server", "install"]),
        ],
    ),
    case(
        "server new",
        &[
            apply(
                "apply",
                &[
                    "server",
                    "new",
                    "gamma",
                    "--command",
                    "/bin/true",
                    "--arg",
                    "x",
                ],
            ),
            apply(
                "remote",
                &[
                    "server",
                    "new",
                    "remote1",
                    "--url",
                    "https://example.invalid/mcp2",
                    "--transport",
                    "http",
                ],
            ),
            read("after", &["server", "ls"]),
            usage("usage", &["server", "new"]),
        ],
    ),
    case(
        "server edit",
        &[
            setup(
                "setup",
                &["server", "new", "gamma", "--command", "/bin/true"],
            ),
            apply(
                "apply",
                &[
                    "server",
                    "edit",
                    "gamma",
                    "--arg",
                    "y",
                    "--forward-instructions",
                    "off",
                ],
            ),
            read("after", &["server", "info", "gamma"]),
            apply(
                "unknown",
                &["server", "edit", "no-such-server", "--arg", "q"],
            )
            .exit(1),
            usage("usage", &["server", "edit"]),
        ],
    ),
    prepared(
        "server source set",
        fork_world_upstream_fetched,
        &[
            apply(
                "apply",
                &["server", "source", "set", "forked", "--remote", "upstream"],
            ),
            read(
                "after",
                &[
                    "mcp",
                    "call",
                    "servers_detect_source",
                    "--args",
                    r#"{"name":"forked"}"#,
                ],
            ),
            apply(
                "unknown",
                &["server", "source", "set", "no-such-server", "--remote", "upstream"],
            )
            .exit(1),
            usage("usage", &["server", "source", "set"]),
        ],
    ),
    // client
    case(
        "client edit",
        &[
            read(
                "preview",
                &[
                    "client",
                    "edit",
                    "cursor",
                    "--set-profiles",
                    "default",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["client", "edit", "cursor", "--set-profiles", "default"],
            ),
            usage("usage", &["client", "edit"]),
        ],
    ),
    case(
        "client import",
        &[
            read(
                "preview",
                &["client", "import", "cursor", "--all", "--dry-run"],
            ),
            apply(
                "apply",
                &[
                    "client",
                    "import",
                    "cursor",
                    "--all",
                    "--profile",
                    "cursor-import",
                ],
            ),
            usage("usage", &["client", "import"]),
        ],
    ),
    case(
        "client direct add",
        &[
            read(
                "preview",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            read("after", &["client", "direct", "ls"]),
            usage("usage", &["client", "direct", "add"]),
        ],
    ),
    case(
        "client direct rm",
        &[
            setup(
                "setup",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            read(
                "preview",
                &[
                    "client",
                    "direct",
                    "rm",
                    "alpha",
                    "--client",
                    "claude-code",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["client", "direct", "rm", "alpha", "--client", "claude-code"],
            ),
            usage("usage", &["client", "direct", "rm"]),
        ],
    ),
    // terminal surface: the command replaces itself with the server process, so only its refusals
    // print an envelope
    case(
        "direct run",
        &[
            apply("unknown", &["direct", "run", "no-such-server"]).exit(1),
            usage("usage", &["direct", "run"]),
        ],
    ),
    // auth and secrets
    case(
        "auth probe",
        &[
            apply("apply", &["auth", "probe"]),
            apply("unknown", &["auth", "probe", "--server", "alpha"]).exit(1),
        ],
    ),
    // a stdio server signs in without a browser; the browser flow of an http server needs a
    // resolvable authorization server and is not part of a headless contract run
    case(
        "auth login",
        &[
            apply("signed-in", &["auth", "login", "alpha", "--no-open"]),
            usage("usage", &["auth", "login"]),
        ],
    ),
    case(
        "secret get",
        &[
            setup("setup", &["secret", "set", "alpha", "API_TOKEN"]).stdin(VAULTED),
            read("set", &["secret", "get", "alpha", "API_TOKEN"]),
            read(
                "reveal",
                &["secret", "get", "alpha", "API_TOKEN", "--reveal"],
            )
            .reveals(),
            read("unset", &["secret", "get", "alpha", "NO_SUCH_KEY"]).exit(1),
            usage("usage", &["secret", "get"]),
        ],
    ),
    case(
        "secret rm",
        &[
            setup("setup", &["secret", "set", "alpha", "API_TOKEN"]).stdin(VAULTED),
            apply("apply", &["secret", "rm", "alpha", "API_TOKEN"]),
            read("after", &["secret", "get", "alpha", "API_TOKEN"]).exit(1),
            usage("usage", &["secret", "rm"]),
        ],
    ),
    // context
    prepared(
        "context loads",
        loads_home,
        &[
            // the folder itself, not `/`: the search for nested CLAUDE.md files walks down from
            // it, and below `/` it would list the machine's own files
            read("home", &["context", "loads", "--cwd", "{home}"]),
            read(
                "profile",
                &["context", "loads", "--profile", "no-such-profile"],
            )
            .exit(1),
            read(
                "folder",
                &["context", "loads", "--cwd", "{home}/work/erp/clients/acme-erp"],
            ),
            read(
                "no-lazy",
                &[
                    "context",
                    "loads",
                    "--cwd",
                    "{home}/work/erp/clients/acme-erp",
                    "--no-lazy",
                ],
            ),
        ],
    ),
    prepared(
        "context measure",
        measure_home,
        &[
            read(
                "measured",
                &[
                    "context",
                    "measure",
                    "--cwd",
                    "{home}/work/erp/clients/acme-erp",
                    "--without",
                    "plugin:kit@market",
                    "--yes",
                ],
            ),
            read(
                "cached",
                &[
                    "context",
                    "measure",
                    "--cwd",
                    "{home}/work/erp/clients/acme-erp",
                    "--without",
                    "plugin:kit@market",
                ],
            ),
            read(
                "loads",
                &[
                    "context",
                    "loads",
                    "--cwd",
                    "{home}/work/erp/clients/acme-erp",
                    "--measured",
                ],
            ),
            read(
                "unconfirmed",
                &[
                    "context",
                    "measure",
                    "--cwd",
                    "{home}/work/erp/clients/acme-erp",
                    "--model",
                    "sonnet",
                ],
            )
            .exit(1),
        ],
    ),
    case(
        "context checkpoint-status",
        &[
            read(
                "status",
                &["context", "checkpoint-status", "--checkpoint-at", "50000"],
            )
            .stdin(STATUSLINE),
            read("empty", &["context", "checkpoint-status"]).exit(1),
        ],
    ),
    case("context plan", &[read("", &["context", "plan"])]),
    case(
        "context apply",
        &[
            read("preview", &["context", "apply", "--dry-run"]),
            apply("apply", &["context", "apply"]),
        ],
    ),
    case(
        "context sync",
        &[
            read("preview", &["context", "sync", "--dry-run"]),
            apply("apply", &["context", "sync"]),
        ],
    ),
    case(
        "context init",
        &[
            read("preview", &["context", "init", "--dry-run"]),
            apply("apply", &["context", "init"]),
        ],
    ),
    prepared(
        "context client add",
        layers_home,
        &[
            read("preview", &["context", "client", "add", "acme", "--dry-run"]),
            apply("apply", &["context", "client", "add", "acme"]),
            read(
                "folder-preview",
                &[
                    "context", "client", "add", "acme-kb", "--scope", "folder", "--folder",
                    "{home}/work/other", "--import", "{home}/kb/CLAUDE.md", "--dry-run",
                ],
            ),
            apply(
                "folder",
                &[
                    "context", "client", "add", "acme-kb", "--scope", "folder", "--folder",
                    "{home}/work/other", "--import", "{home}/kb/CLAUDE.md",
                ],
            ),
            read("scaffold", &["context", "client", "add", "tree-knowledge", "--dry-run"]),
            read("exists", &["context", "client", "add", "acme-erp", "--dry-run"]),
            read(
                "no-folder",
                &["context", "client", "add", "acme-nowhere", "--scope", "folder", "--dry-run"],
            )
            .exit(1),
            usage(
                "bad-scope",
                &["context", "client", "add", "acme-bad", "--scope", "sideways"],
            )
            .exit(2),
            read("list", &["context", "client", "list"]),
            usage("usage", &["context", "client", "add"]),
        ],
    ),
    prepared(
        "context client edit",
        layers_home,
        &[
            read(
                "preview",
                &[
                    "context", "client", "edit", "client-erp-knowledge", "--folder",
                    "{home}/work/erp/clients/acme-two", "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context", "client", "edit", "client-erp-knowledge", "--folder",
                    "{home}/work/erp/clients/acme-two",
                ],
            ),
            read(
                "delivery",
                &["context", "client", "edit", "erp-knowledge", "--delivery", "import", "--dry-run"],
            ),
            read("unchanged", &["context", "client", "edit", "client-chain", "--dry-run"]),
            read(
                "no-folder",
                &["context", "client", "edit", "client-chain", "--folder", "", "--dry-run"],
            )
            .exit(1),
            read("missing", &["context", "client", "edit", "nope", "--scope", "global", "--dry-run"]).exit(1),
            read("list", &["context", "client", "list"]),
            usage("usage", &["context", "client", "edit"]),
        ],
    ),
    prepared(
        "context client rm",
        layers_home,
        &[
            setup("deploy", &["context", "sync"]),
            read("preview", &["context", "client", "rm", "client-chain", "--dry-run"]),
            apply("apply", &["context", "client", "rm", "chain"]),
            read("shared", &["context", "client", "rm", "client-linked", "--dry-run"]),
            read("missing", &["context", "client", "rm", "nope", "--dry-run"]).exit(1),
            read("not-a-client", &["context", "client", "rm", "personal", "--dry-run"]).exit(2),
            read("list", &["context", "client", "list"]),
            usage("usage", &["context", "client", "rm"]),
        ],
    ),
    prepared(
        "context compose",
        layers_home,
        &[
            read("before", &["context", "compose", "--cwd", "{home}/work/erp/clients/acme-two"]),
            setup("deploy", &["context", "sync"]),
            read("client", &["context", "compose", "--cwd", "{home}/work/erp/clients/acme-two"]),
            read("layers", &["context", "compose", "--cwd", "{home}/work/erp/clients/acme-erp"]),
            read("outside", &["context", "compose", "--cwd", "{home}/work/other"]),
            read("list", &["context", "client", "list"]),
            read("missing", &["context", "compose", "--cwd", "{home}/no/such/folder"]).exit(2),
            usage("usage", &["context", "compose", "extra"]),
        ],
    ),
    case(
        "context profile add",
        &[
            read(
                "preview",
                &[
                    "context",
                    "profile",
                    "add",
                    "work",
                    "--rules",
                    "none",
                    "--servers",
                    "none",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context",
                    "profile",
                    "add",
                    "work",
                    "--rules",
                    "none",
                    "--servers",
                    "none",
                ],
            ),
            usage("usage", &["context", "profile", "add"]),
        ],
    ),
    case(
        "context profile remove",
        &[
            setup(
                "setup",
                &[
                    "context",
                    "profile",
                    "add",
                    "work",
                    "--rules",
                    "none",
                    "--servers",
                    "none",
                ],
            ),
            read(
                "preview",
                &[
                    "context",
                    "profile",
                    "remove",
                    "work",
                    "--purge",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["context", "profile", "remove", "work", "--purge"],
            ),
            usage("usage", &["context", "profile", "remove"]),
        ],
    ),
    case(
        "context disable",
        &[
            setup(
                "setup",
                &[
                    "context",
                    "profile",
                    "add",
                    "work",
                    "--rules",
                    "none",
                    "--servers",
                    "none",
                ],
            ),
            read(
                "preview",
                &["context", "disable", "--purge-profiles", "--dry-run"],
            ),
            apply("apply", &["context", "disable", "--purge-profiles"]),
        ],
    ),
    // compression
    case(
        "compression enable",
        &[
            read(
                "preview",
                &[
                    "compression",
                    "enable",
                    "--provider",
                    "rtk-only",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["compression", "enable", "--provider", "rtk-only"],
            ),
        ],
    ),
    case(
        "compression use",
        &[
            read("preview", &["compression", "use", "agent", "--dry-run"]),
            apply("apply", &["compression", "use", "agent"]),
            usage("usage", &["compression", "use"]),
        ],
    ),
    case(
        "compression set-provider",
        &[
            read(
                "preview",
                &["compression", "set-provider", "headroom", "--dry-run"],
            ),
            apply("apply", &["compression", "set-provider", "headroom"]),
            usage("usage", &["compression", "set-provider"]),
        ],
    ),
    case(
        "compression sync",
        &[
            read("preview", &["compression", "sync", "--dry-run"]),
            apply("apply", &["compression", "sync"]),
        ],
    ),
    // reads the proxy's /health; the preset moves to a port nothing listens on, then to a port
    // that answers like the proxy, so a proxy of the machine that runs the test is never touched
    prepared(
        "compression seal",
        |_| health_proxy(29214),
        &[
            setup("setup", OFF_PORT_PROXY),
            read("no-proxy", &["compression", "seal"]).exit(1),
            setup(
                "proxy",
                &[
                    "compression",
                    "enable",
                    "--provider",
                    "headroom",
                    "--port",
                    "29214",
                ],
            ),
            read("preview", &["compression", "seal", "--dry-run"]),
            apply("apply", &["compression", "seal", "--apply"]),
            apply("again", &["compression", "seal", "--apply"]),
        ],
    ),
    case(
        "compression disable",
        &[
            read(
                "preview",
                &["compression", "disable", "--teardown", "--dry-run"],
            ),
            apply("apply", &["compression", "disable", "--teardown"]),
        ],
    ),
    // terminal surface: `--plan` prints what would run instead of replacing the process
    case(
        "compression run",
        &[read("plan", &["compression", "run", "--plan", "claude"])],
    ),
    prepared(
        "compression verify",
        transcripts_world,
        &[
            read(
                "no-transcripts",
                &["compression", "verify", "--transcripts", "{base}/none"],
            )
            .exit(1),
            read(
                "measured",
                &[
                    "compression",
                    "verify",
                    "--transcripts",
                    "{home}/.claude/projects",
                    "--min-turns",
                    "1",
                ],
            ),
        ],
    ),
    case(
        "compression ledger record",
        &[
            apply(
                "apply",
                &[
                    "compression",
                    "ledger",
                    "record",
                    "--provider",
                    "rtk-only",
                    "--before",
                    "1000",
                    "--after",
                    "400",
                    "--source",
                    "contract",
                    "--session",
                    "s1",
                ],
            ),
            usage("usage", &["compression", "ledger", "record"]),
        ],
    ),
    // the proxy needs the engine on PATH, which the contract world does not have
    case(
        "compression proxy up",
        &[
            setup("setup", OFF_PORT_PROXY),
            apply("no-engine", &["compression", "proxy", "up"]).exit(1),
        ],
    ),
    case(
        "compression proxy down",
        &[
            setup("setup", OFF_PORT_PROXY),
            apply("no-proxy", &["compression", "proxy", "down"]).exit(1),
        ],
    ),
    case(
        "compression proxy restart",
        &[
            setup("setup", OFF_PORT_PROXY),
            apply("no-engine", &["compression", "proxy", "restart"]).exit(1),
        ],
    ),
    // `--accept` would install the engine with uv and the latest version comes from the network,
    // so only the preview of an explicit target is recorded
    case(
        "compression update",
        &[read(
            "preview",
            &["compression", "update", "--to", "0.30.0"],
        )],
    ),
    // import
    prepared(
        "import mcpm",
        import_world,
        &[
            read("preview", &["import", "mcpm", "{base}/mcpm", "--dry-run"]),
            read(
                "name-map",
                &[
                    "import",
                    "mcpm",
                    "{base}/mcpm",
                    "--dry-run",
                    "--tools",
                    "{base}/tools.json",
                    "--name-map",
                ],
            ),
            apply("apply", &["import", "mcpm", "{base}/mcpm"]),
            usage("usage", &["import", "mcpm"]),
        ],
    ),
    prepared(
        "import rename-refs",
        import_world,
        &[
            read(
                "preview",
                &[
                    "import",
                    "rename-refs",
                    "{base}/mcpm",
                    "--tools",
                    "{base}/tools.json",
                    "--paths",
                    "{base}/refs",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "import",
                    "rename-refs",
                    "{base}/mcpm",
                    "--tools",
                    "{base}/tools.json",
                    "--paths",
                    "{base}/refs",
                ],
            ),
            usage("usage", &["import", "rename-refs"]),
        ],
    ),
    // council and the self-management server
    case(
        "council install",
        &[
            apply("apply", &["council", "install"]),
            apply(
                "key",
                &["council", "install", "--api-key-env", "COUNCIL_TEST_KEY"],
            )
            .env(&[("COUNCIL_TEST_KEY", COUNCIL_KEY)]),
        ],
    ),
    case(
        "council uninstall",
        &[
            setup("setup", &["council", "install"]),
            apply("apply", &["council", "uninstall"]),
            apply("again", &["council", "uninstall", "--purge-key"]),
        ],
    ),
    case(
        "mcp install",
        &[
            apply("apply", &["mcp", "install"]),
            apply("profile", &["mcp", "install", "--profile", "default"]),
        ],
    ),
    case(
        "mcp uninstall",
        &[
            setup("setup", &["mcp", "install"]),
            apply("apply", &["mcp", "uninstall"]),
        ],
    ),
    // skills
    case(
        "skills init",
        &[
            read(
                "preview",
                &[
                    "skills",
                    "init",
                    "--path",
                    "{base}/fresh",
                    "--name",
                    "contract",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "skills",
                    "init",
                    "--path",
                    "{base}/fresh",
                    "--name",
                    "contract",
                ],
            ),
        ],
    ),
    case(
        "skills add",
        &[
            setup(
                "setup",
                &[
                    "skills",
                    "init",
                    "--path",
                    "{base}/fresh",
                    "--name",
                    "contract",
                ],
            ),
            read(
                "preview",
                &[
                    "skills",
                    "add",
                    "fresh-skill",
                    "--path",
                    "{base}/fresh",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["skills", "add", "fresh-skill", "--path", "{base}/fresh"],
            ),
            usage("usage", &["skills", "add"]),
        ],
    ),
    case(
        "skills bundle",
        &[
            read(
                "preview",
                &["skills", "bundle", "--repo", "{repo}", "--dry-run"],
            ),
            apply(
                "apply",
                &[
                    "skills",
                    "bundle",
                    "--repo",
                    "{repo}",
                    "--output",
                    "{base}/demo.zip",
                ],
            ),
        ],
    ),
    case(
        "skills unbundle",
        &[
            setup(
                "bundle",
                &[
                    "skills",
                    "bundle",
                    "--repo",
                    "{repo}",
                    "--output",
                    "{base}/demo.zip",
                ],
            ),
            setup(
                "init",
                &[
                    "skills",
                    "init",
                    "--path",
                    "{base}/fresh",
                    "--name",
                    "contract",
                ],
            ),
            read(
                "preview",
                &[
                    "skills",
                    "unbundle",
                    "{base}/demo.zip",
                    "--path",
                    "{base}/fresh",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "skills",
                    "unbundle",
                    "{base}/demo.zip",
                    "--path",
                    "{base}/fresh",
                ],
            ),
            usage("usage", &["skills", "unbundle"]),
        ],
    ),
    case(
        "skills clean",
        &[
            setup("setup", &["skills", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &["skills", "clean", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["skills", "clean", "--repo", "{repo}"]),
        ],
    ),
    case(
        "skills uninstall",
        &[
            setup("setup", &["skills", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &[
                    "skills",
                    "uninstall",
                    "demo",
                    "--repo",
                    "{repo}",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["skills", "uninstall", "demo", "--repo", "{repo}"],
            ),
            apply(
                "unknown",
                &["skills", "uninstall", "demo", "--repo", "{repo}"],
            )
            .exit(1),
            usage("usage", &["skills", "uninstall"]),
        ],
    ),
    case(
        "skills resolve",
        &[
            setup("setup", &["skills", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &["skills", "resolve", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["skills", "resolve", "--repo", "{repo}"]),
        ],
    ),
    prepared(
        "skills tap add",
        git_world,
        &[
            read(
                "preview",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/tap-src",
                    "--name",
                    "local",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read("after", &["skills", "tap", "ls"]),
            usage("usage", &["skills", "tap", "add"]),
        ],
    ),
    prepared(
        "skills tap remove",
        git_world,
        &[
            setup(
                "setup",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read(
                "preview",
                &["skills", "tap", "remove", "local", "--dry-run"],
            ),
            apply("apply", &["skills", "tap", "remove", "local"]),
            apply("unknown", &["skills", "tap", "remove", "local"]).exit(1),
            usage("usage", &["skills", "tap", "remove"]),
        ],
    ),
    prepared(
        "skills tap update",
        git_world,
        &[
            read("none", &["skills", "tap", "update", "--dry-run"]),
            setup(
                "setup",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read("preview", &["skills", "tap", "update", "--dry-run"]),
            apply("apply", &["skills", "tap", "update", "local"]),
        ],
    ),
    prepared(
        "skills search",
        git_world,
        &[
            read("empty", &["skills", "search", "tapskill"]),
            setup(
                "setup",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            read("hit", &["skills", "search", "tapskill"]),
            usage("usage", &["skills", "search"]),
        ],
    ),
    // the clone url of an `@user/repo` spec is github, so only the dry-run plan is recorded
    case(
        "skills install",
        &[
            read(
                "preview",
                &["skills", "install", "@acme/skills", "--dry-run"],
            ),
            usage("usage", &["skills", "install"]),
        ],
    ),
    // agents
    case(
        "agents add",
        &[
            read(
                "preview",
                &["agents", "add", "scout", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["agents", "add", "scout", "--repo", "{repo}"]),
            usage("usage", &["agents", "add"]),
        ],
    ),
    case(
        "agents audit",
        &[read("", &["agents", "audit", "--repo", "{repo}"])],
    ),
    case(
        "agents sync",
        &[
            read(
                "preview",
                &["agents", "sync", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["agents", "sync", "--repo", "{repo}"]),
        ],
    ),
    case(
        "agents clean",
        &[
            setup("setup", &["agents", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &["agents", "clean", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["agents", "clean", "--repo", "{repo}"]),
        ],
    ),
    case(
        "agents uninstall",
        &[
            setup("setup", &["agents", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &[
                    "agents",
                    "uninstall",
                    "helper",
                    "--repo",
                    "{repo}",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["agents", "uninstall", "helper", "--repo", "{repo}"],
            ),
            usage("usage", &["agents", "uninstall"]),
        ],
    ),
    // styles
    case(
        "styles add",
        &[
            read(
                "preview",
                &["styles", "add", "terse", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["styles", "add", "terse", "--repo", "{repo}"]),
            usage("usage", &["styles", "add"]),
        ],
    ),
    case(
        "styles sync",
        &[
            read(
                "preview",
                &["styles", "sync", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["styles", "sync", "--repo", "{repo}"]),
        ],
    ),
    case(
        "styles apply",
        &[
            read(
                "preview",
                &["styles", "apply", "plain", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["styles", "apply", "plain", "--repo", "{repo}"]),
            usage("usage", &["styles", "apply"]),
        ],
    ),
    case(
        "styles remove",
        &[
            setup("setup", &["styles", "apply", "plain", "--repo", "{repo}"]),
            read(
                "preview",
                &["styles", "remove", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["styles", "remove", "--repo", "{repo}"]),
        ],
    ),
    case(
        "styles clean",
        &[
            setup("setup", &["styles", "sync", "--repo", "{repo}"]),
            read(
                "preview",
                &["styles", "clean", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["styles", "clean", "--repo", "{repo}"]),
        ],
    ),
    // sync: the remote is a local bare repository
    prepared(
        "sync init",
        git_world,
        &[
            apply("apply", SYNC_INIT).stdin(PASSPHRASE),
            read("after", &["sync", "status"]),
            usage("usage", &["sync", "init"]),
            usage(
                "passphrase",
                &["sync", "init", "--repo", "{base}/remote.git"],
            ),
        ],
    ),
    prepared(
        "sync push",
        git_world,
        &[
            apply("unconfigured", PUSH_ALL).exit(1),
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            read(
                "preview",
                &["sync", "push", "--include-projects", "--dry-run"],
            ),
            apply("apply", PUSH_ALL),
        ],
    ),
    prepared(
        "sync pull",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            setup("push", PUSH_ALL),
            read(
                "preview",
                &["sync", "pull", "--include-projects", "--dry-run"],
            ),
            apply("apply", &["sync", "pull", "--include-projects"]),
        ],
    ),
    prepared(
        "sync diff",
        git_world,
        &[
            read("unconfigured", &["sync", "diff"]).exit(1),
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            setup("push", PUSH_ALL),
            read("configured", &["sync", "diff"]),
        ],
    ),
    prepared(
        "sync reset",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            apply("apply", &["sync", "reset"]),
            read("after", &["sync", "status"]),
        ],
    ),
    prepared(
        "sync rotate-passphrase",
        git_world,
        &[
            apply(
                "unconfigured",
                &["sync", "rotate-passphrase", "--passphrase-stdin"],
            )
            .stdin(NEW_PASSPHRASE)
            .exit(1),
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            setup("push", PUSH_ALL),
            apply(
                "apply",
                &["sync", "rotate-passphrase", "--passphrase-stdin"],
            )
            .stdin(NEW_PASSPHRASE),
            usage("usage", &["sync", "rotate-passphrase"]),
        ],
    ),
    prepared(
        "sync add-project",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            apply("apply", ADD_PROJECT),
            read("after", &["sync", "status"]),
            usage("usage", &["sync", "add-project"]),
        ],
    ),
    prepared(
        "sync remove-project",
        git_world,
        &[
            setup("init", SYNC_INIT).stdin(PASSPHRASE),
            setup("project", ADD_PROJECT),
            apply("apply", &["sync", "remove-project", "proj1"]),
            read("after", &["sync", "status"]),
            usage("usage", &["sync", "remove-project"]),
        ],
    ),
    prepared(
        "sync git-sync",
        git_world,
        &[
            apply("status", &["sync", "git-sync", "--status"]),
            apply(
                "configure",
                &[
                    "sync",
                    "git-sync",
                    "--repo",
                    "{base}/tap-src",
                    "--branch",
                    "main",
                ],
            ),
            apply("clear", &["sync", "git-sync", "--clear"]),
        ],
    ),
    // a real migration needs a bundle in the legacy mcpm format; the refusals are recorded
    prepared(
        "sync migrate",
        git_world,
        &[
            apply(
                "no-bundle",
                &["sync", "migrate", "{base}", "--passphrase-stdin"],
            )
            .stdin(PASSPHRASE)
            .exit(1),
            usage("usage", &["sync", "migrate"]),
        ],
    ),
    // plugins, usage and telemetry
    case(
        "cc update",
        &[
            read("preview", &["cc", "update", "--dry-run"]),
            read("one", &["cc", "update", "demo-plugin", "--dry-run"]),
            apply("apply", &["cc", "update", "demo-plugin"]),
        ],
    ),
    prepared(
        "usage",
        transcripts_world,
        &[
            apply("apply", &["usage"]),
            apply("cached", &["usage", "--no-refresh"]),
        ],
    ),
    case(
        "obs otel enable",
        &[
            read(
                "preview",
                &["obs", "otel", "enable", "--port", "4999", "--dry-run"],
            ),
            apply("apply", &["obs", "otel", "enable", "--port", "4999"]),
        ],
    ),
    case(
        "obs otel disable",
        &[
            setup("setup", &["obs", "otel", "enable", "--port", "4999"]),
            read("preview", &["obs", "otel", "disable", "--dry-run"]),
            apply("apply", &["obs", "otel", "disable"]),
        ],
    ),
    // context bundles (MIG-CTX-10): the library of `bundle_home` holds acme-dev, the legacy
    // `default` list and a broken definition; the client repository has a foreign
    // `settings.local.json`, the workspace repository above it files that nothing may touch
    prepared(
        "context bundle ls",
        bundle_home,
        &[
            read("library", &["context", "bundle", "ls"]),
            setup(
                "setup",
                &["context", "bundle", "apply", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp"],
            ),
            read("applied", &["context", "bundle", "ls"]),
        ],
    ),
    prepared(
        "context bundle show",
        bundle_home,
        &[
            read("bundle", &["context", "bundle", "show", "acme-dev"]),
            read("legacy", &["context", "bundle", "show", "default"]),
            read("broken", &["context", "bundle", "show", "broken"]).exit(1),
            read("missing", &["context", "bundle", "show", "no-such-bundle"]).exit(1),
            usage("usage", &["context", "bundle", "show"]),
        ],
    ),
    prepared(
        "context bundle add",
        bundle_home,
        &[
            read(
                "preview",
                &[
                    "context", "bundle", "add", "ops-lite", "--description", "Operations work",
                    "--skills-off", "scratch-one,scratch-two", "--plugins-off",
                    "tools-pack@tools-market", "--layers-add", "acme-knowledge", "--bind",
                    "~/work/erp/clients/*", "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context", "bundle", "add", "ops-lite", "--description", "Operations work",
                    "--skills-off", "scratch-one,scratch-two", "--plugins-off",
                    "tools-pack@tools-market", "--layers-add", "acme-knowledge", "--bind",
                    "~/work/erp/clients/*",
                ],
            ),
            read("exists", &["context", "bundle", "add", "ops-lite", "--dry-run"]).exit(1),
            read(
                "from-folder",
                &["context", "bundle", "add", "from-client", "--from-folder", "{home}/work/erp/clients/acme-erp", "--dry-run"],
            ),
            read("show", &["context", "bundle", "show", "ops-lite"]),
            usage("usage", &["context", "bundle", "add"]),
        ],
    ),
    prepared(
        "context bundle edit",
        bundle_home,
        &[
            read(
                "preview",
                &[
                    "context", "bundle", "edit", "acme-dev", "--agents-off", "reviewer-bot,planner",
                    "--description", "ERP work", "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context", "bundle", "edit", "acme-dev", "--agents-off", "reviewer-bot,planner",
                    "--description", "ERP work",
                ],
            ),
            read("show", &["context", "bundle", "show", "acme-dev"]),
            read("missing", &["context", "bundle", "edit", "no-such-bundle", "--bind", "x", "--dry-run"]).exit(1),
            usage("usage", &["context", "bundle", "edit"]),
        ],
    ),
    prepared(
        "context bundle rm",
        bundle_home,
        &[
            setup(
                "setup",
                &["context", "bundle", "apply", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp"],
            ),
            read("refused", &["context", "bundle", "rm", "acme-dev", "--dry-run"]).exit(1),
            read("forced", &["context", "bundle", "rm", "acme-dev", "--force", "--dry-run"]),
            read("preview", &["context", "bundle", "rm", "default", "--dry-run"]),
            apply("apply", &["context", "bundle", "rm", "default"]),
            read("missing", &["context", "bundle", "rm", "default", "--dry-run"]).exit(1),
            usage("usage", &["context", "bundle", "rm"]),
        ],
    ),
    prepared(
        "context bundle apply",
        bundle_home,
        &[
            read(
                "plan",
                &["context", "bundle", "apply", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp", "--dry-run"],
            ),
            apply(
                "result",
                &["context", "bundle", "apply", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp"],
            ),
            read(
                "again",
                &["context", "bundle", "apply", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp", "--dry-run"],
            ),
            read(
                "legacy",
                &["context", "bundle", "apply", "default", "--cwd", "{home}/work/erp/clients/acme-two", "--dry-run"],
            ),
            usage(
                "no-folder",
                &["context", "bundle", "apply", "acme-dev", "--cwd", "{home}/no-such-folder", "--dry-run"],
            ),
            read(
                "missing",
                &["context", "bundle", "apply", "no-such-bundle", "--cwd", "{home}/work/erp/clients/acme-erp", "--dry-run"],
            )
            .exit(1),
            usage("usage", &["context", "bundle", "apply"]),
        ],
    ),
    prepared(
        "context bundle undo",
        bundle_drift_home,
        &[
            read("plan", &["context", "bundle", "undo", "--cwd", "{home}/work/erp/clients/acme-erp", "--dry-run"]),
            apply("result", &["context", "bundle", "undo", "--cwd", "{home}/work/erp/clients/acme-erp"]),
            read("conflicts", &["context", "bundle", "undo", "--cwd", "{home}/work/erp/clients/acme-two", "--dry-run"]),
            apply("conflicted", &["context", "bundle", "undo", "--cwd", "{home}/work/erp/clients/acme-two"]),
            read("none", &["context", "bundle", "undo", "--cwd", "{home}/work/erp/clients/acme-erp", "--dry-run"]).exit(1),
        ],
    ),
    prepared(
        "context bundle status",
        bundle_drift_home,
        &[
            read("clean", &["context", "bundle", "status", "--cwd", "{home}/work/erp/clients/acme-erp"]),
            read("drift", &["context", "bundle", "status", "--cwd", "{home}/work/erp/clients/acme-two"]),
            read("none", &["context", "bundle", "status", "--cwd", "{home}/work/erp"]),
            read("default", &["context", "bundle", "status"]),
        ],
    ),
    prepared(
        "context bundle launch",
        bundle_home,
        &[
            apply("apply", &["context", "bundle", "launch", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp"]),
            apply("legacy", &["context", "bundle", "launch", "default"]),
            apply("missing", &["context", "bundle", "launch", "no-such-bundle"]).exit(1),
            usage("usage", &["context", "bundle", "launch"]),
        ],
    ),
    prepared(
        "context bundle config",
        bundle_home,
        &[
            read("show", &["context", "bundle", "config"]),
            apply("on", &["context", "bundle", "config", "--auto-apply", "on"]),
            read("after", &["context", "bundle", "config"]),
            apply("off", &["context", "bundle", "config", "--auto-apply", "off"]),
            usage("usage", &["context", "bundle", "config", "--auto-apply", "sometimes"]),
        ],
    ),
    prepared(
        "context use",
        bundle_home,
        &[
            read("off", &["context", "use", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp", "--dry-run"]),
            apply("bundle", &["context", "use", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-erp"]),
            setup("enable", &["context", "folders", "--enable"]),
            read("routed", &["context", "use", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-two", "--dry-run"]),
            apply("routed-apply", &["context", "use", "acme-dev", "--cwd", "{home}/work/erp/clients/acme-two"]),
            read("none", &["context", "use", "--none", "--cwd", "{home}/work/erp/clients/acme-two", "--dry-run"]),
            apply("none-apply", &["context", "use", "--none", "--cwd", "{home}/work/erp/clients/acme-two"]),
            read("none-again", &["context", "use", "--none", "--cwd", "{home}/work/erp/clients/acme-two", "--dry-run"]).exit(1),
            read("missing", &["context", "use", "no-such-name", "--cwd", "{home}/work/erp/clients/acme-two", "--dry-run"]).exit(1),
            usage("usage", &["context", "use", "--cwd", "{home}/work/erp/clients/acme-two"]),
        ],
    ),
    // tasks (MIG-AUTO-1): `tasks_home` holds a task that needs the user and writes one secret, a
    // scheduled one, a disabled draft and a broken file, plus run records with fixed ids; a
    // started run spawns `/usr/bin/true` as its runner, so no step of a golden ever runs
    // attention (MIG-GUI-11): the tasks world gives a run that waits for you and one that failed;
    // every other feed reads local state of the same synthetic home
    prepared(
        "attention ls",
        tasks_home,
        &[
            read("default", &["attention", "ls"]),
            read("needs-you", &["attention", "ls", "--level", "needs-you"]),
            read("fyi", &["attention", "ls", "--level", "fyi"]),
            usage("usage", &["attention", "ls", "--level", "urgent"]),
        ],
    ),
    prepared(
        "attention dismiss",
        tasks_home,
        &[
            read("preview", &["attention", "dismiss", "tasks:portal-token:waiting", "--dry-run"]),
            apply("apply", &["attention", "dismiss", "tasks:portal-token:waiting", "--until", "2099-01-01"]),
            read("hidden", &["attention", "ls"]),
            read("until-preview", &["attention", "dismiss", "later:feed", "--until", "2099-12-31", "--dry-run"]),
            apply("forever", &["attention", "dismiss", "later:feed"]),
            usage("usage", &["attention", "dismiss", "tasks:portal-token:waiting", "--until", "soon"]),
        ],
    ),
    prepared(
        "task ls",
        tasks_home,
        &[
            read("default", &["task", "ls"]),
            read("all", &["task", "ls", "--all"]),
            usage("usage", &["task", "ls", "extra"]),
        ],
    ),
    prepared(
        "task show",
        tasks_home,
        &[
            read("waiting", &["task", "show", "portal-token"]),
            read("scheduled", &["task", "show", "nightly-report"]),
            read("missing", &["task", "show", "no-such-task"]).exit(1),
            usage("usage", &["task", "show"]),
        ],
    ),
    prepared(
        "task run",
        tasks_home,
        &[
            read("preview", &["task", "run", "portal-token", "--dry-run"]),
            read("disabled", &["task", "run", "draft-cleanup", "--dry-run"]),
            apply("busy", &["task", "run", "portal-token"]).exit(1),
            apply("refused", &["task", "run", "draft-cleanup"]).exit(1),
            apply("missing", &["task", "run", "no-such-task"]).exit(1),
            apply("apply", &["task", "run", "nightly-report"]).env(&[("TOOLPORT_CTL_BIN", "/usr/bin/true")]),
            usage("usage", &["task", "run"]),
        ],
    ),
    prepared(
        "task resume",
        tasks_home,
        &[
            apply("apply", &["task", "resume", "run-fixture-waiting"]),
            apply("finished", &["task", "resume", "run-fixture-ok"]).exit(1),
            apply("not-waiting", &["task", "resume", "run-fixture-stale"]).exit(1),
            apply("missing", &["task", "resume", "run-no-such"]).exit(1),
            usage("usage", &["task", "resume"]),
        ],
    ),
    prepared(
        "task cancel",
        tasks_home,
        &[
            apply("apply", &["task", "cancel", "run-fixture-stale"]),
            apply("again", &["task", "cancel", "run-fixture-stale"]).exit(1),
            apply("finished", &["task", "cancel", "run-fixture-ok"]).exit(1),
            apply("missing", &["task", "cancel", "run-no-such"]).exit(1),
            usage("usage", &["task", "cancel"]),
        ],
    ),
    prepared(
        "task add",
        tasks_home,
        &[
            read("preview", &["task", "add", "weekly-cleanup", "--file", "{home}/defs/weekly-cleanup.json", "--dry-run"]),
            apply("apply", &["task", "add", "weekly-cleanup", "--file", "{home}/defs/weekly-cleanup.json"]),
            read("exists", &["task", "add", "weekly-cleanup", "--file", "{home}/defs/weekly-cleanup.json", "--dry-run"]).exit(1),
            read("command-preview", &["task", "add", "refresh-login", "--from-command", "{home}/defs/refresh-login.md", "--dry-run"]),
            apply("command", &["task", "add", "refresh-login", "--from-command", "{home}/defs/refresh-login.md"]),
            read("stdin-preview", &["task", "add", "piped-cleanup", "--file", "-", "--dry-run"]).stdin(PIPED_TASK),
            apply("stdin", &["task", "add", "piped-cleanup", "--file", "-"]).stdin(PIPED_TASK),
            usage("stdin-mismatch", &["task", "add", "other-name", "--file", "-", "--dry-run"]).stdin(PIPED_TASK),
            read("invalid", &["task", "add", "portal-token", "--file", "{home}/defs/undeclared.json", "--dry-run"]).exit(1),
            read("unreadable", &["task", "add", "x", "--file", "{home}/defs/none.json", "--dry-run"]).exit(1),
            usage("mismatch", &["task", "add", "other-name", "--file", "{home}/defs/weekly-cleanup.json", "--dry-run"]),
            usage("usage", &["task", "add", "x"]),
        ],
    ),
    prepared(
        "task edit",
        tasks_home,
        &[
            read("preview", &["task", "edit", "nightly-report", "--file", "{home}/defs/nightly-report.json", "--dry-run"]),
            apply("apply", &["task", "edit", "nightly-report", "--file", "{home}/defs/nightly-report.json"]),
            read("stdin-preview", &["task", "edit", "nightly-report", "--file", "-", "--dry-run"]).stdin(PIPED_EDIT),
            apply("stdin", &["task", "edit", "nightly-report", "--file", "-"]).stdin(PIPED_EDIT),
            read("stdin-empty", &["task", "edit", "nightly-report", "--file", "-", "--dry-run"]).stdin("").exit(1),
            read("invalid", &["task", "edit", "portal-token", "--file", "{home}/defs/undeclared.json", "--dry-run"]).exit(1),
            read("missing", &["task", "edit", "weekly-cleanup", "--file", "{home}/defs/weekly-cleanup.json", "--dry-run"]).exit(1),
            usage("usage", &["task", "edit", "nightly-report"]),
        ],
    ),
    prepared(
        "task rm",
        tasks_home,
        &[
            read("preview", &["task", "rm", "nightly-report", "--dry-run"]),
            apply("apply", &["task", "rm", "nightly-report"]),
            read("busy", &["task", "rm", "portal-token", "--dry-run"]).exit(1),
            read("missing", &["task", "rm", "no-such-task", "--dry-run"]).exit(1),
            usage("usage", &["task", "rm"]),
        ],
    ),
    prepared(
        "task history",
        tasks_home,
        &[
            read("all", &["task", "history"]),
            read("task", &["task", "history", "portal-token"]),
            read("limit", &["task", "history", "--limit", "1"]),
            read("run", &["task", "history", "--run", "run-fixture-ok"]),
            read("other-task", &["task", "history", "portal-token", "--run", "run-fixture-ok"]).exit(1),
            read("missing-run", &["task", "history", "--run", "run-no-such"]).exit(1),
            read("missing-task", &["task", "history", "no-such-task"]).exit(1),
            usage("bad-limit", &["task", "history", "--limit", "many"]),
        ],
    ),
];
