//! Contract per `toolportctl` command (D-058): one golden `--json` envelope per case under
//! `plus/fixtures/ctl-envelopes/`, produced by the real binary against the synthetic world of
//! `common/world.rs`. The GUI bridge and scripts parse these envelopes, so a change in their shape
//! must be a reviewed change: `CTL_ENVELOPE_BLESS=1 cargo test -p toolport-plus-bins --test
//! plus_ctl_contract` rewrites the goldens (rejected when `CI` is set).
//!
//! A command is classified in the command table (`Row::smoke`), not here: `every_command_is_...`
//! walks the table, so a new command fails until its row says how it is covered. Adding a case:
//! `case("<command id>", &[read(...), apply(...), usage(...)])`; a writer gets a `--dry-run` step
//! that leaves the world as it was, then the apply step.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use toolport_plus::ctl::{registry, rows, Smoke, Tier, COMMANDS, SCHEMA_VERSION};

mod common;
use common::compression::{fake_proxy, rtk_home, transcripts};
use common::library_world::{self, library_home};
use common::oauth_mock::{self, OauthMock};
use common::tasks_world::tasks_home;
use common::world::{World, FAKE_SECRET, SECRET_KEY};
use common::{golden, normalize};

#[derive(Clone, Copy)]
struct Step {
    label: &'static str,
    argv: &'static [&'static str],
    stdin: Option<&'static str>,
    exit: i32,
    writes: bool,
    golden: bool,
    /// The step prints the vaulted value on purpose; the golden holds `<VAULTED>` for it.
    reveals: bool,
    hook: Option<fn(&World)>,
    /// The step signs in through the loopback authorization server: the run plays the user's
    /// browser, and nothing is allowed to find a real one on `PATH`.
    browser: bool,
    env: &'static [(&'static str, &'static str)],
}

/// A step that must leave the world as it was: a read or a `--dry-run`.
const fn read(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        label,
        argv,
        stdin: None,
        exit: 0,
        writes: false,
        golden: true,
        reveals: false,
        hook: None,
        browser: false,
        env: &[],
    }
}

/// A step whose argv fails the parser (exit 2): nothing runs, so the world must stay as it was.
const fn usage(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        exit: 2,
        ..read(label, argv)
    }
}

const fn apply(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        writes: true,
        ..read(label, argv)
    }
}

/// Prepares the world for the steps after it with a command: checked for exit code and leaks,
/// no golden.
const fn setup(label: &'static str, argv: &'static [&'static str]) -> Step {
    Step {
        golden: false,
        ..apply(label, argv)
    }
}

/// Reshapes the world for the steps after it; runs no command and records no golden.
const fn hook(label: &'static str, prepare: fn(&World)) -> Step {
    Step {
        golden: false,
        hook: Some(prepare),
        ..read(label, &[])
    }
}

impl Step {
    const fn exit(self, exit: i32) -> Self {
        Self { exit, ..self }
    }

    const fn browser(self) -> Self {
        Self {
            browser: true,
            ..self
        }
    }

    const fn reveals(self) -> Self {
        Self {
            reveals: true,
            ..self
        }
    }

    const fn env(self, env: &'static [(&'static str, &'static str)]) -> Self {
        Self { env, ..self }
    }

    const fn stdin(self, stdin: &'static str) -> Self {
        Self {
            stdin: Some(stdin),
            ..self
        }
    }
}

struct Case {
    id: &'static str,
    steps: &'static [Step],
}

const fn case(id: &'static str, steps: &'static [Step]) -> Case {
    Case { id, steps }
}

fn gateway_without_the_seam(world: &World) {
    world.stub_gateway(b"an upstream gateway, built without the fork seam");
}

fn explicit_default_access(world: &World) {
    let mut registry = world.registry();
    registry["version"] = json!(3);
    registry["servers"][0]["enabled"] = json!(true);
    registry["servers"][1]["enabled"] = json!(true);
    registry["defaultAccessProfileId"] = json!("default");
    registry["profiles"] = json!([
        {"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha"]},
        {"id": "focus", "name": "Focus", "enabledServerIds": []},
    ]);
    registry["clientScopes"] = json!({"claude-code": "focus"});
    world.write_registry(&registry);
}

fn opt_alpha_into_the_stdio_probe(world: &World) {
    let mut registry = world.registry();
    registry["servers"][0]["plus"] = json!({"authProbe": {"kind": "stdio"}});
    world.write_registry(&registry);
}

fn context_files(world: &World) {
    world.context_files();
}

fn plugin_files(world: &World) {
    world.context_files();
    let put = |rel: &str, text: &str| {
        let path = world.home.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    put(
        ".claude/plugins/installed_plugins.json",
        "{\"version\": 2, \"plugins\": {\"ecc@ecc\": [{\"scope\": \"user\"}]}}",
    );
    put(
        ".claude/settings.json",
        "{\"enabledPlugins\": {\"ecc@ecc\": true}}",
    );
}

fn point_beta_at_a_loopback_authorization_server(world: &World) {
    let idp = OauthMock::start();
    let mut registry = world.registry();
    registry["servers"][1]["url"] = json!(idp.mcp_url());
    world.write_registry(&registry);
    *world.idp.borrow_mut() = Some(idp);
}

fn unreadable_registry(world: &World) {
    std::fs::write(world.data.join("registry.json"), "{ not json").unwrap();
}

const VAULTED: &str = "FAKE-vaulted-value-1c9e";

const OFF_PORT_PROXY: &[&str] = &[
    "compression",
    "enable",
    "--provider",
    "headroom",
    "--port",
    "29213",
];

const PIPED_TASK: &str = r#"{"id":"piped-cleanup","title":"Cleanup from stdin","description":"","enabled":true,"requires":{"servers":[],"commands":["echo"]},"writesSecrets":[],"steps":[{"id":"say","title":"Say hello","type":"exec","program":"echo","args":["clean"]}],"triggers":{"manual":true,"cli":true,"selfMcp":{"enabled":false,"approval":"every-run"},"schedule":null,"onAuthFailure":[]},"createdFrom":{"kind":"manual"}}"#;
const PIPED_EDIT: &str = r#"{"id":"nightly-report","title":"Nightly report (from stdin)","description":"","enabled":true,"requires":{"servers":[],"commands":["echo"]},"writesSecrets":[],"steps":[{"id":"say","title":"Say hello","type":"exec","program":"echo","args":["report"]}],"triggers":{"manual":true,"cli":false,"selfMcp":{"enabled":false,"approval":"every-run"},"schedule":{"cron":"0 3 * * *","autoRun":true},"onAuthFailure":[]},"createdFrom":null}"#;

const CASES: &[Case] = &[
    case(
        "task ls",
        &[
            hook("tasks", tasks_home),
            read("default", &["task", "ls"]),
            read("all", &["task", "ls", "--all"]),
            usage("usage", &["task", "ls", "extra"]),
        ],
    ),
    case(
        "task show",
        &[
            hook("tasks", tasks_home),
            read("waiting", &["task", "show", "portal-token"]),
            read("scheduled", &["task", "show", "nightly-report"]),
            read("missing", &["task", "show", "no-such-task"]).exit(1),
            usage("usage", &["task", "show"]),
        ],
    ),
    case(
        "task run",
        &[
            hook("tasks", tasks_home),
            read("preview", &["task", "run", "portal-token", "--dry-run"]),
            read("disabled", &["task", "run", "draft-cleanup", "--dry-run"]),
            apply("busy", &["task", "run", "portal-token"]).exit(1),
            apply("refused", &["task", "run", "draft-cleanup"]).exit(1),
            apply("missing", &["task", "run", "no-such-task"]).exit(1),
            apply("apply", &["task", "run", "nightly-report"])
                .env(&[("TOOLPORT_CTL_BIN", "/usr/bin/true")]),
            usage("usage", &["task", "run"]),
        ],
    ),
    case(
        "task resume",
        &[
            hook("tasks", tasks_home),
            apply("apply", &["task", "resume", "run-fixture-waiting"]),
            apply("finished", &["task", "resume", "run-fixture-ok"]).exit(1),
            apply("not-waiting", &["task", "resume", "run-fixture-stale"]).exit(1),
            apply("missing", &["task", "resume", "run-no-such"]).exit(1),
            usage("usage", &["task", "resume"]),
        ],
    ),
    case(
        "task cancel",
        &[
            hook("tasks", tasks_home),
            apply("apply", &["task", "cancel", "run-fixture-stale"]),
            apply("again", &["task", "cancel", "run-fixture-stale"]).exit(1),
            apply("finished", &["task", "cancel", "run-fixture-ok"]).exit(1),
            apply("missing", &["task", "cancel", "run-no-such"]).exit(1),
            usage("usage", &["task", "cancel"]),
        ],
    ),
    case(
        "task add",
        &[
            hook("tasks", tasks_home),
            read(
                "preview",
                &[
                    "task",
                    "add",
                    "weekly-cleanup",
                    "--file",
                    "{home}/defs/weekly-cleanup.json",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "task",
                    "add",
                    "weekly-cleanup",
                    "--file",
                    "{home}/defs/weekly-cleanup.json",
                ],
            ),
            read(
                "exists",
                &[
                    "task",
                    "add",
                    "weekly-cleanup",
                    "--file",
                    "{home}/defs/weekly-cleanup.json",
                    "--dry-run",
                ],
            )
            .exit(1),
            read(
                "command-preview",
                &[
                    "task",
                    "add",
                    "refresh-login",
                    "--from-command",
                    "{home}/defs/refresh-login.md",
                    "--dry-run",
                ],
            ),
            apply(
                "command",
                &[
                    "task",
                    "add",
                    "refresh-login",
                    "--from-command",
                    "{home}/defs/refresh-login.md",
                ],
            ),
            read(
                "stdin-preview",
                &["task", "add", "piped-cleanup", "--file", "-", "--dry-run"],
            )
            .stdin(PIPED_TASK),
            apply("stdin", &["task", "add", "piped-cleanup", "--file", "-"]).stdin(PIPED_TASK),
            usage(
                "stdin-mismatch",
                &["task", "add", "other-name", "--file", "-", "--dry-run"],
            )
            .stdin(PIPED_TASK),
            read(
                "invalid",
                &[
                    "task",
                    "add",
                    "portal-token",
                    "--file",
                    "{home}/defs/undeclared.json",
                    "--dry-run",
                ],
            )
            .exit(1),
            read(
                "unreadable",
                &[
                    "task",
                    "add",
                    "x",
                    "--file",
                    "{home}/defs/none.json",
                    "--dry-run",
                ],
            )
            .exit(1),
            usage(
                "mismatch",
                &[
                    "task",
                    "add",
                    "other-name",
                    "--file",
                    "{home}/defs/weekly-cleanup.json",
                    "--dry-run",
                ],
            ),
            usage("usage", &["task", "add", "x"]),
        ],
    ),
    case(
        "task edit",
        &[
            hook("tasks", tasks_home),
            read(
                "preview",
                &[
                    "task",
                    "edit",
                    "nightly-report",
                    "--file",
                    "{home}/defs/nightly-report.json",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "task",
                    "edit",
                    "nightly-report",
                    "--file",
                    "{home}/defs/nightly-report.json",
                ],
            ),
            read(
                "stdin-preview",
                &["task", "edit", "nightly-report", "--file", "-", "--dry-run"],
            )
            .stdin(PIPED_EDIT),
            apply("stdin", &["task", "edit", "nightly-report", "--file", "-"]).stdin(PIPED_EDIT),
            read(
                "stdin-empty",
                &["task", "edit", "nightly-report", "--file", "-", "--dry-run"],
            )
            .stdin("")
            .exit(1),
            read(
                "invalid",
                &[
                    "task",
                    "edit",
                    "portal-token",
                    "--file",
                    "{home}/defs/undeclared.json",
                    "--dry-run",
                ],
            )
            .exit(1),
            read(
                "missing",
                &[
                    "task",
                    "edit",
                    "weekly-cleanup",
                    "--file",
                    "{home}/defs/weekly-cleanup.json",
                    "--dry-run",
                ],
            )
            .exit(1),
            usage("usage", &["task", "edit", "nightly-report"]),
        ],
    ),
    case(
        "task rm",
        &[
            hook("tasks", tasks_home),
            read("preview", &["task", "rm", "nightly-report", "--dry-run"]),
            apply("apply", &["task", "rm", "nightly-report"]),
            read("busy", &["task", "rm", "portal-token", "--dry-run"]).exit(1),
            read("missing", &["task", "rm", "no-such-task", "--dry-run"]).exit(1),
            usage("usage", &["task", "rm"]),
        ],
    ),
    case(
        "task history",
        &[
            hook("tasks", tasks_home),
            read("all", &["task", "history"]),
            read("task", &["task", "history", "portal-token"]),
            read("limit", &["task", "history", "--limit", "1"]),
            read("run", &["task", "history", "--run", "run-fixture-ok"]),
            read(
                "other-task",
                &["task", "history", "portal-token", "--run", "run-fixture-ok"],
            )
            .exit(1),
            read("missing-run", &["task", "history", "--run", "run-no-such"]).exit(1),
            read("missing-task", &["task", "history", "no-such-task"]).exit(1),
            usage("bad-limit", &["task", "history", "--limit", "many"]),
        ],
    ),
    case("commands", &[read("", &["commands"])]),
    case(
        "status",
        &[
            read("", &["status"]),
            hook("explicit-access-sets", explicit_default_access),
            read("access-sets", &["status"]),
            hook("unreadable", unreadable_registry),
            read("unreadable-registry", &["status"]),
        ],
    ),
    case(
        "doctor",
        &[
            read("", &["doctor"]),
            hook("no-seam", gateway_without_the_seam),
            read("gateway-without-seam", &["doctor"]),
            hook("unreadable", unreadable_registry),
            read("unreadable-registry", &["doctor"]).exit(1),
        ],
    ),
    case("server ls", &[read("", &["server", "ls"])]),
    case("server info", &[read("", &["server", "info", "alpha"])]),
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
                "env",
                &[
                    "server",
                    "edit",
                    "gamma",
                    "--env",
                    "JAVA_HOME=/opt/jdk",
                    "--env",
                    "MODE=fast",
                    "--unset-env",
                    "NEVER_SET",
                ],
            ),
            apply(
                "env-refused",
                &["server", "edit", "gamma", "--env", "API_KEY=plain"],
            )
            .exit(1),
            apply(
                "unknown",
                &["server", "edit", "no-such-server", "--arg", "q"],
            )
            .exit(1),
            usage("usage", &["server", "edit"]),
        ],
    ),
    case(
        "server mode",
        &[
            apply("apply", &["server", "mode", "alpha", "search"]),
            apply("unchanged", &["server", "mode", "alpha", "search"]),
            read("info", &["server", "info", "alpha"]),
            apply("back", &["server", "mode", "alpha", "auto"]),
            usage("retired", &["server", "mode", "alpha", "direct"]),
            usage("usage", &["server", "mode", "alpha"]),
            read("unknown", &["server", "mode", "no-such-server", "auto"]).exit(1),
        ],
    ),
    case(
        "server uninstall",
        &[read(
            "preview",
            &["server", "uninstall", "alpha", "--dry-run"],
        )],
    ),
    case(
        "server restart",
        &[
            read("preview", &["server", "restart", "alpha", "--dry-run"]),
            apply(
                "not-running",
                &["server", "restart", "alpha", "--wait", "1"],
            ),
        ],
    ),
    case("auth statusline", &[read("", &["auth", "statusline"])]),
    case("auth hook", &[read("", &["auth", "hook"])]),
    case(
        "auth probe",
        &[
            hook("opt-in", opt_alpha_into_the_stdio_probe),
            apply("apply", &["auth", "probe"]),
            read("unknown", &["auth", "probe", "--server", "beta"]).exit(1),
            usage("usage", &["auth", "probe", "extra"]),
        ],
    ),
    case(
        "auth login",
        &[
            read("signed-in", &["auth", "login", "alpha", "--no-open"]),
            hook(
                "loopback-idp",
                point_beta_at_a_loopback_authorization_server,
            ),
            apply("remote-no-open", &["auth", "login", "beta", "--no-open"]).browser(),
            usage("usage", &["auth", "login"]),
        ],
    ),
    case(
        "secret set",
        &[
            apply("apply", &["secret", "set", "alpha", "API_TOKEN"]).stdin(VAULTED),
            read("get", &["secret", "get", "alpha", "API_TOKEN"]),
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
    case("mcp doctor", &[read("", &["mcp", "doctor"]).exit(1)]),
    case("mcp tools", &[read("", &["mcp", "tools"])]),
    case(
        "mcp install",
        &[
            apply("apply", &["mcp", "install"]),
            apply("profile", &["mcp", "install", "--profile", "default"]),
            hook("explicit-access-sets", explicit_default_access),
            apply("access-set", &["mcp", "install", "--access-set", "focus"]),
            read("doctor", &["mcp", "doctor"]),
            usage("bogus", &["mcp", "install", "--bogus"]),
            read(
                "unknown-access-set",
                &["mcp", "install", "--access-set", "nope"],
            )
            .exit(1),
        ],
    ),
    case(
        "mcp uninstall",
        &[
            apply("setup", &["mcp", "install"]),
            apply("apply", &["mcp", "uninstall"]),
            apply("again", &["mcp", "uninstall"]),
            read("doctor", &["mcp", "doctor"]),
        ],
    ),
    case(
        "mcp call",
        &[
            read("where_am_i", &["mcp", "call", "where_am_i"]),
            read(
                "flow_diagram",
                &["mcp", "call", "flow_diagram", "--args", "{}"],
            ),
            read("doctor", &["mcp", "call", "doctor"]),
            read(
                "invalid",
                &["mcp", "call", "where_am_i", "--args", r#"{"depth":2}"#],
            )
            .exit(1),
            usage("unknown", &["mcp", "call", "where_am"]),
            usage("operand", &["mcp", "call"]),
            usage(
                "secret",
                &[
                    "mcp",
                    "call",
                    "where_am_i",
                    "--args",
                    r#"{"config":{"env":{"API_KEY":"FAKE-inline-secret-9d41"}}}"#,
                ],
            ),
            read("stdin", &["mcp", "call", "where_am_i", "--args-stdin"]).stdin("{}"),
            apply(
                "servers_add_profile_tag",
                &[
                    "mcp",
                    "call",
                    "servers_add_profile_tag",
                    "--args",
                    r#"{"name":"beta","profile_tag":"Default"}"#,
                ],
            ),
            apply(
                "servers_remove_profile_tag",
                &[
                    "mcp",
                    "call",
                    "servers_remove_profile_tag",
                    "--args",
                    r#"{"name":"beta","profile_tag":"Default"}"#,
                ],
            ),
            read(
                "servers_set_mode.refused",
                &[
                    "mcp",
                    "call",
                    "servers_set_mode",
                    "--args",
                    r#"{"name":"alpha","mode":"search"}"#,
                ],
            )
            .exit(1),
            apply(
                "servers_set_mode",
                &[
                    "mcp",
                    "call",
                    "servers_set_mode",
                    "--args",
                    r#"{"name":"alpha","mode":"search","confirm":true}"#,
                ],
            ),
        ],
    ),
    case(
        "context bundle ls",
        &[
            hook("files", context_files),
            read("", &["context", "bundle", "ls"]),
        ],
    ),
    case(
        "context bundle show",
        &[
            hook("files", context_files),
            read("bundle", &["context", "bundle", "show", "acme-dev"]),
            read("missing", &["context", "bundle", "show", "nope"]).exit(1),
            usage("usage", &["context", "bundle", "show"]),
        ],
    ),
    case(
        "context bundle add",
        &[
            hook("files", context_files),
            read(
                "preview",
                &[
                    "context",
                    "bundle",
                    "add",
                    "lean",
                    "--skills-off",
                    "scratch-1",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context",
                    "bundle",
                    "add",
                    "lean",
                    "--skills-off",
                    "scratch-1",
                ],
            ),
            read("exists", &["context", "bundle", "add", "lean"]).exit(1),
            read("show", &["context", "bundle", "show", "lean"]),
        ],
    ),
    case(
        "context bundle edit",
        &[
            hook("files", context_files),
            read(
                "preview",
                &[
                    "context",
                    "bundle",
                    "edit",
                    "acme-dev",
                    "--plugins-off",
                    "tools-pack@tools-market",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context",
                    "bundle",
                    "edit",
                    "acme-dev",
                    "--plugins-off",
                    "tools-pack@tools-market",
                ],
            ),
            read(
                "missing",
                &["context", "bundle", "edit", "nope", "--description", "x"],
            )
            .exit(1),
        ],
    ),
    case(
        "context bundle rm",
        &[
            hook("files", context_files),
            read(
                "preview",
                &["context", "bundle", "rm", "acme-dev", "--dry-run"],
            ),
            apply("apply", &["context", "bundle", "rm", "acme-dev"]),
            read("missing", &["context", "bundle", "rm", "acme-dev"]).exit(1),
        ],
    ),
    case(
        "context bundle apply",
        &[
            hook("files", context_files),
            read(
                "plan",
                &[
                    "context",
                    "bundle",
                    "apply",
                    "acme-dev",
                    "--cwd",
                    "{home}/proj",
                    "--dry-run",
                ],
            ),
            apply(
                "result",
                &[
                    "context",
                    "bundle",
                    "apply",
                    "acme-dev",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            read(
                "status",
                &["context", "bundle", "status", "--cwd", "{home}/proj"],
            ),
            read(
                "missing",
                &["context", "bundle", "apply", "nope", "--cwd", "{home}/proj"],
            )
            .exit(1),
            read(
                "no-folder",
                &[
                    "context",
                    "bundle",
                    "apply",
                    "acme-dev",
                    "--cwd",
                    "{home}/gone",
                ],
            )
            .exit(2),
            usage("usage", &["context", "bundle", "apply"]),
        ],
    ),
    case(
        "context bundle undo",
        &[
            hook("files", context_files),
            setup(
                "setup",
                &[
                    "context",
                    "bundle",
                    "apply",
                    "acme-dev",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            read(
                "plan",
                &[
                    "context",
                    "bundle",
                    "undo",
                    "--cwd",
                    "{home}/proj",
                    "--dry-run",
                ],
            ),
            apply(
                "result",
                &["context", "bundle", "undo", "--cwd", "{home}/proj"],
            ),
            read(
                "none",
                &["context", "bundle", "undo", "--cwd", "{home}/proj"],
            )
            .exit(1),
        ],
    ),
    case(
        "context bundle status",
        &[
            hook("files", context_files),
            read(
                "none",
                &["context", "bundle", "status", "--cwd", "{home}/proj"],
            ),
        ],
    ),
    case(
        "context bundle launch",
        &[
            hook("files", context_files),
            apply(
                "launch",
                &[
                    "context",
                    "bundle",
                    "launch",
                    "acme-dev",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            read("missing", &["context", "bundle", "launch", "nope"]).exit(1),
        ],
    ),
    case(
        "context bundle config",
        &[
            read("show", &["context", "bundle", "config"]),
            apply("on", &["context", "bundle", "config", "--auto-apply", "on"]),
            read("after", &["context", "bundle", "config"]),
            usage(
                "bad",
                &["context", "bundle", "config", "--auto-apply", "maybe"],
            ),
        ],
    ),
    case(
        "context use",
        &[
            hook("files", context_files),
            read(
                "plan",
                &[
                    "context",
                    "use",
                    "acme-dev",
                    "--cwd",
                    "{home}/proj",
                    "--dry-run",
                ],
            ),
            apply(
                "routed",
                &["context", "use", "acme-dev", "--cwd", "{home}/proj"],
            ),
            apply(
                "none",
                &["context", "use", "--none", "--cwd", "{home}/proj"],
            ),
            read(
                "missing",
                &["context", "use", "nope", "--cwd", "{home}/proj"],
            )
            .exit(1),
            usage("usage", &["context", "use"]),
        ],
    ),
    case(
        "context leave-out",
        &[
            hook("files", context_files),
            read(
                "plan",
                &[
                    "context",
                    "leave-out",
                    "add",
                    "{home}/.claude/CLAUDE.md",
                    "--cwd",
                    "{home}/proj",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context",
                    "leave-out",
                    "add",
                    "{home}/.claude/CLAUDE.md",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            apply(
                "memory",
                &[
                    "context",
                    "leave-out",
                    "add",
                    "--memory",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            apply(
                "back",
                &[
                    "context",
                    "leave-out",
                    "rm",
                    "{home}/.claude/CLAUDE.md",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            read(
                "not-an-instruction-file",
                &[
                    "context",
                    "leave-out",
                    "add",
                    "{home}/proj/.git/info/exclude",
                    "--cwd",
                    "{home}/proj",
                ],
            )
            .exit(1),
            usage("usage", &["context", "leave-out", "add"]),
        ],
    ),
    case(
        "plugins off",
        &[
            hook("files", plugin_files),
            read(
                "plan",
                &[
                    "plugins",
                    "off",
                    "ecc@ecc",
                    "--cwd",
                    "{home}/proj",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["plugins", "off", "ecc@ecc", "--cwd", "{home}/proj"],
            ),
            apply(
                "again",
                &["plugins", "off", "ecc@ecc", "--cwd", "{home}/proj"],
            ),
            read(
                "unknown-plugin",
                &["plugins", "off", "ghost@x", "--cwd", "{home}/proj"],
            )
            .exit(1),
            usage("usage", &["plugins", "off"]),
        ],
    ),
    case(
        "plugins on",
        &[
            hook("files", plugin_files),
            read(
                "nothing-to-remove",
                &["plugins", "on", "ecc@ecc", "--cwd", "{home}/proj"],
            ),
            apply(
                "off",
                &["plugins", "off", "ecc@ecc", "--cwd", "{home}/proj"],
            ),
            apply("on", &["plugins", "on", "ecc@ecc", "--cwd", "{home}/proj"]),
            usage("usage", &["plugins", "on"]),
        ],
    ),
    case(
        "context loads",
        &[
            hook("files", context_files),
            read("folder", &["context", "loads", "--cwd", "{home}/proj"]),
            read(
                "measured",
                &["context", "loads", "--cwd", "{home}/proj", "--measured"],
            ),
            read("no-folder", &["context", "loads", "--cwd", "{home}/gone"]).exit(2),
        ],
    ),
    case(
        "context compose",
        &[
            hook("files", context_files),
            read("folder", &["context", "compose", "--cwd", "{home}/proj"]),
            usage("usage", &["context", "compose", "extra"]),
        ],
    ),
    case(
        "context sync",
        &[
            hook("files", context_files),
            read("preview", &["context", "sync", "--dry-run"]),
            apply("off", &["context", "sync"]),
            setup(
                "auto",
                &["context", "bundle", "config", "--auto-apply", "on"],
            ),
            read("auto-preview", &["context", "sync", "--dry-run"]),
        ],
    ),
    case(
        "context measure",
        &[
            hook("files", context_files),
            read("plan", &["context", "measure", "--cwd", "{home}/proj"]),
            read("no-folder", &["context", "measure", "--cwd", "{home}/gone"]).exit(2),
            read(
                "variants",
                &[
                    "context",
                    "measure",
                    "--cwd",
                    "{home}/proj",
                    "--without",
                    "plugin:kit@market",
                    "--without",
                    "skill:scratch-*",
                    "--bundle",
                    "acme-dev",
                ],
            ),
            read(
                "bad-variant",
                &[
                    "context",
                    "measure",
                    "--cwd",
                    "{home}/proj",
                    "--without",
                    "nonsense",
                ],
            )
            .exit(2),
        ],
    ),
    case(
        "context status",
        &[
            hook("files", context_files),
            read("folder", &["context", "status", "--cwd", "{home}/proj"]),
            apply(
                "left-out",
                &[
                    "context",
                    "leave-out",
                    "add",
                    "--memory",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            read("after", &["context", "status", "--cwd", "{home}/proj"]),
            read("no-folder", &["context", "status", "--cwd", "{home}/gone"]).exit(2),
            usage("usage", &["context", "status", "extra"]),
        ],
    ),
    case(
        "skills ls",
        &[read("repo", &["skills", "ls", "--repo", "{repo}"])],
    ),
    case(
        "skills lint",
        &[read("", &["skills", "lint", "--repo", "{repo}"])],
    ),
    case(
        "skills diff",
        &[read("", &["skills", "diff", "--repo", "{repo}"]).exit(1)],
    ),
    case(
        "skills audit",
        &[read("", &["skills", "audit", "--repo", "{repo}"])],
    ),
    case(
        "skills status",
        &[read("", &["skills", "status", "--repo", "{repo}"])],
    ),
    case(
        "skills sync",
        &[
            read(
                "preview",
                &["skills", "sync", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["skills", "sync", "--repo", "{repo}"]),
        ],
    ),
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
        "skills tap add",
        &[usage("usage", &["skills", "tap", "add"])],
    ),
    case("skills tap ls", &[read("", &["skills", "tap", "ls"])]),
    case(
        "skills tap remove",
        &[
            apply("unknown", &["skills", "tap", "remove", "local"]).exit(1),
            usage("usage", &["skills", "tap", "remove"]),
        ],
    ),
    case(
        "skills tap update",
        &[read("none", &["skills", "tap", "update", "--dry-run"])],
    ),
    case(
        "skills search",
        &[
            read("empty", &["skills", "search", "tapskill"]),
            usage("usage", &["skills", "search"]),
        ],
    ),
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
    case(
        "agents ls",
        &[read("", &["agents", "ls", "--repo", "{repo}"])],
    ),
    case(
        "agents lint",
        &[read("", &["agents", "lint", "--repo", "{repo}"])],
    ),
    case(
        "agents diff",
        &[read("", &["agents", "diff", "--repo", "{repo}"])],
    ),
    case(
        "agents status",
        &[read("", &["agents", "status", "--repo", "{repo}"])],
    ),
    case(
        "styles ls",
        &[read("", &["styles", "ls", "--repo", "{repo}"])],
    ),
    case(
        "styles lint",
        &[read("", &["styles", "lint", "--repo", "{repo}"])],
    ),
    case(
        "styles diff",
        &[read("", &["styles", "diff", "--repo", "{repo}"])],
    ),
    case(
        "styles status",
        &[read("", &["styles", "status", "--repo", "{repo}"])],
    ),
    case(
        "library status",
        &[
            hook("library-world", library_home),
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
    case(
        "library pull",
        &[
            hook("library-world", library_home),
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
    case(
        "library push",
        &[
            hook("library-world", library_home),
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
    case(
        "compression hooks status",
        &[
            hook("home", rtk_home),
            read("", &["compression", "hooks", "status"]),
        ],
    ),
    case(
        "compression hooks apply",
        &[
            hook("home", rtk_home),
            read("preview", &["compression", "hooks", "apply", "--dry-run"]),
            apply("apply", &["compression", "hooks", "apply"]),
            apply("again", &["compression", "hooks", "apply"]),
            read("after", &["compression", "hooks", "status"]),
            usage(
                "unknown-provider",
                &["compression", "hooks", "apply", "--provider", "nope"],
            ),
        ],
    ),
    case(
        "compression hooks undo",
        &[
            hook("home", rtk_home),
            setup("setup", &["compression", "hooks", "apply"]),
            read("preview", &["compression", "hooks", "undo", "--dry-run"]),
            apply("apply", &["compression", "hooks", "undo"]),
            read("after", &["compression", "hooks", "status"]),
        ],
    ),
    case(
        "compression hooks pause",
        &[
            apply("pause", &["compression", "hooks", "pause"]),
            apply("again", &["compression", "hooks", "pause"]),
            read("paused", &["compression", "hooks", "status"]),
        ],
    ),
    case(
        "compression hooks resume",
        &[
            setup("setup", &["compression", "hooks", "pause"]),
            apply("apply", &["compression", "hooks", "resume"]),
            apply("again", &["compression", "hooks", "resume"]),
        ],
    ),
    case(
        "compression hook",
        &[usage("usage", &["compression", "hook", "rtk"])],
    ),
    case(
        "compression status",
        &[read("", &["compression", "status"])],
    ),
    case(
        "compression presets",
        &[read("", &["compression", "presets"])],
    ),
    case("compression pin", &[read("", &["compression", "pin"])]),
    case("compression env", &[read("", &["compression", "env"])]),
    case(
        "compression doctor",
        &[read("", &["compression", "doctor"])],
    ),
    case(
        "compression ledger summary",
        &[read("", &["compression", "ledger", "summary"])],
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
            usage(
                "parsec-refused",
                &["compression", "enable", "--provider", "parsec", "--dry-run"],
            ),
            usage(
                "parsec-wrong-name",
                &[
                    "compression",
                    "enable",
                    "--provider",
                    "parsec",
                    "--accept-egress",
                    "headroom",
                    "--dry-run",
                ],
            ),
            read(
                "parsec-preview",
                &[
                    "compression",
                    "enable",
                    "--provider",
                    "parsec",
                    "--accept-egress",
                    "parsec",
                    "--dry-run",
                ],
            ),
            apply(
                "parsec-apply",
                &[
                    "compression",
                    "enable",
                    "--provider",
                    "parsec",
                    "--accept-egress",
                    "parsec",
                ],
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
    case(
        "compression seal",
        &[
            hook("fake-proxy", fake_proxy),
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
    case(
        "compression verify",
        &[
            hook("transcripts", transcripts),
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
];

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn expand(world: &World, word: &str) -> String {
    word.replace("{home}", &world.path(&world.home))
        .replace("{data}", &world.path(&world.data))
        .replace("{base}", &world.path(&world.base))
        .replace("{repo}", &world.path(&world.repo))
}

fn run_ctl(world: &World, argv: &[String], stdin: Option<&str>) -> Run {
    run_ctl_as(world, argv, stdin, false, &[])
}

fn run_ctl_as(
    world: &World,
    argv: &[String],
    stdin: Option<&str>,
    browser: bool,
    env: &[(&str, &str)],
) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .arg("--json")
        .args(argv)
        .env_clear()
        .current_dir(&world.home)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in world.env() {
        command.env(key, value);
    }
    if browser {
        let empty = world.base.join("no-browser-on-path");
        std::fs::create_dir_all(&empty).unwrap();
        command.env("PATH", &empty);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn toolportctl");
    if let Some(text) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    let user = browser.then(|| {
        let stderr = child.stderr.take().unwrap();
        let prefix = world
            .idp
            .borrow()
            .as_ref()
            .expect("a browser step needs the loopback authorization server")
            .authorize_prefix();
        std::thread::spawn(move || {
            let mut seen = String::new();
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                seen.push_str(&line);
                seen.push('\n');
                if line.trim().starts_with(&prefix) {
                    oauth_mock::approve_in_browser(line.trim());
                }
            }
            seen
        })
    });
    let output = child.wait_with_output().expect("wait for toolportctl");
    Run {
        code: output.status.code().expect("exit code, not a signal"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: match user {
            Some(user) => user.join().expect("the browser stand-in finished"),
            None => String::from_utf8_lossy(&output.stderr).into_owned(),
        },
    }
}

fn scrub(envelope: &Value) -> Value {
    let text = serde_json::to_string(envelope).unwrap();
    serde_json::from_str(&text.replace(VAULTED, "<VAULTED>")).unwrap()
}

fn stem(case: &Case, step: &Step) -> String {
    if step.label.is_empty() {
        golden::file_stem(case.id)
    } else {
        format!("{}.{}", golden::file_stem(case.id), step.label)
    }
}

fn changed_files(
    world: &World,
    before: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
) -> Vec<String> {
    let after = world.snapshot();
    after
        .iter()
        .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
        .map(|(path, _)| path.display().to_string())
        .chain(
            before
                .keys()
                .filter(|p| !after.contains_key(*p))
                .map(|p| format!("removed {}", p.display())),
        )
        .map(|p| {
            normalize::world_roots(world)
                .iter()
                .fold(p, |acc, root| acc.replace(root, "<WORLD>"))
        })
        .collect()
}

fn run_case(case: &Case) {
    let world = World::new(&golden::file_stem(case.id));
    for step in case.steps {
        if let Some(prepare) = step.hook {
            prepare(&world);
            continue;
        }
        let argv: Vec<String> = step.argv.iter().map(|w| expand(&world, w)).collect();
        let before = world.snapshot();
        let run = run_ctl_as(&world, &argv, step.stdin, step.browser, step.env);
        let name = format!("{} {}", case.id, step.label);

        assert_eq!(
            run.code, step.exit,
            "{name}: exit code\nstdout: {}\nstderr: {}",
            run.stdout, run.stderr
        );
        let lines: Vec<&str> = run.stdout.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "{name}: stdout is exactly one envelope line, got {:?}",
            run.stdout
        );
        let envelope: Value = serde_json::from_str(lines[0])
            .unwrap_or_else(|e| panic!("{name}: stdout is not JSON: {e}"));
        assert_eq!(envelope["schemaVersion"], SCHEMA_VERSION, "{name}");
        assert_eq!(
            envelope["ok"],
            step.exit == 0,
            "{name}: ok follows the exit code"
        );
        assert!(envelope["command"].is_string(), "{name}");
        if step.exit == 0 {
            assert!(
                envelope.get("data").is_some(),
                "{name}: ok envelope without data"
            );
        } else {
            assert!(
                envelope["error"]["code"].is_string() && envelope["error"]["message"].is_string(),
                "{name}: failed envelope without error code and message"
            );
        }
        let canaries: &[&str] = if step.reveals {
            &[
                FAKE_SECRET,
                SECRET_KEY,
                oauth_mock::ACCESS_TOKEN,
                oauth_mock::REFRESH_TOKEN,
            ]
        } else {
            &[
                FAKE_SECRET,
                SECRET_KEY,
                VAULTED,
                oauth_mock::ACCESS_TOKEN,
                oauth_mock::REFRESH_TOKEN,
            ]
        };
        for canary in canaries {
            assert!(
                !run.stdout.contains(canary) && !run.stderr.contains(canary),
                "{name}: leaked a secret value"
            );
        }
        for (path, bytes) in world.snapshot() {
            assert!(
                !bytes
                    .windows(VAULTED.len())
                    .any(|window| window == VAULTED.as_bytes()),
                "{name}: {} holds the vaulted value in plain text",
                path.display()
            );
        }
        if !step.writes {
            let changed = changed_files(&world, &before);
            assert!(
                changed.is_empty(),
                "{name}: a read or dry-run step changed the world: {changed:?}"
            );
        }
        let stem = stem(case, step);
        if step.golden {
            golden::assert_golden(
                &stem,
                &json!({
                    "argv": step.argv,
                    "exitCode": step.exit,
                    "envelope": scrub(&normalize::normalize(&world, &envelope)),
                }),
            );
        }
    }
}

#[test]
fn every_case_matches_its_golden_envelope() {
    let mut failures = Vec::new();
    for case in CASES {
        if let Err(payload) = std::panic::catch_unwind(|| run_case(case)) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            failures.push(format!("{}: {message}", case.id));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn command_ids() -> Vec<String> {
    registry()["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "command")
        .map(|row| row["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn every_case_names_a_command_and_every_golden_belongs_to_a_case() {
    let ids: BTreeSet<String> = command_ids().into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut owned: BTreeSet<String> = BTreeSet::new();
    for case in CASES {
        assert!(
            ids.contains(case.id),
            "case `{}` is not a registry command",
            case.id
        );
        assert!(seen.insert(case.id), "case `{}` appears twice", case.id);
        let labels: BTreeSet<&str> = case.steps.iter().map(|s| s.label).collect();
        assert_eq!(
            labels.len(),
            case.steps.len(),
            "{}: step labels repeat",
            case.id
        );
        for step in case.steps.iter().filter(|step| step.golden) {
            owned.insert(stem(case, step));
        }
        for step in case.steps.iter().filter(|step| step.hook.is_none()) {
            assert!(
                ids.iter().any(|id| {
                    let words: Vec<&str> = id.split(' ').collect();
                    step.argv.len() >= words.len() && step.argv[..words.len()] == words[..]
                }),
                "{} {}: argv starts with no registry command",
                case.id,
                step.label
            );
        }
    }
    for entry in std::fs::read_dir(golden::root()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = name.strip_suffix(".json").unwrap_or(&name);
        assert!(owned.contains(stem), "golden {name} belongs to no case");
    }
}

#[test]
fn contract_coverage() {
    let ids = command_ids();
    let with_case: BTreeSet<&str> = CASES.iter().map(|case| case.id).collect();
    let missing: Vec<&String> = ids
        .iter()
        .filter(|id| !with_case.contains(id.as_str()))
        .collect();
    eprintln!(
        "ctl contract: {} commands, {} with a golden case, {} uncovered",
        ids.len(),
        ids.len() - missing.len(),
        missing.len()
    );
    assert!(missing.is_empty(), "no golden envelope for: {missing:?}");
}

#[test]
fn every_command_is_either_read_only_tested_or_given_a_reason() {
    let table: BTreeSet<String> = COMMANDS.iter().map(|c| c.path.join(" ")).collect();
    assert_eq!(table.len(), COMMANDS.len(), "command paths are unique");
    let classified = rows();
    let leaves: BTreeSet<String> = command_ids().into_iter().collect();
    let in_rows: BTreeSet<String> = classified.iter().map(|r| r.id.to_string()).collect();
    assert_eq!(
        leaves, in_rows,
        "every leaf command has one row and no row is stale"
    );
    let world = World::new("smoke");
    let before = world.snapshot();
    for row in &classified {
        match row.smoke {
            Smoke::Skip(reason) => {
                assert!(
                    !reason.trim().is_empty(),
                    "{}: not run by the smoke test and no reason given",
                    row.id
                );
                assert_ne!(
                    row.tier,
                    Tier::Read,
                    "{}: a read command is run, not skipped",
                    row.id
                );
            }
            Smoke::Read(extra) => {
                let mut argv: Vec<String> = row.id.split(' ').map(String::from).collect();
                argv.extend(extra.iter().map(|w| w.to_string()));
                let run = run_ctl(&world, &argv, None);
                assert!(
                    run.code == 0 || run.code == 1,
                    "{}: exit {} {:?}",
                    row.id,
                    run.code,
                    run.stderr
                );
                let envelope: Value = serde_json::from_str(run.stdout.trim())
                    .unwrap_or_else(|e| panic!("{}: not one envelope: {e}", row.id));
                assert_eq!(envelope["ok"], run.code == 0, "{}", row.id);
                assert_eq!(
                    changed_files(&world, &before),
                    Vec::<String>::new(),
                    "{}: the smoke run changed the world",
                    row.id
                );
            }
        }
    }
}

#[test]
fn help_lists_every_command_and_exits_zero_without_writing() {
    let world = World::new("help");
    let before = world.snapshot();
    for command in COMMANDS {
        for flag in ["--help", "-h"] {
            let mut argv: Vec<String> = command.path.iter().map(|w| w.to_string()).collect();
            argv.push(flag.into());
            let run = run_ctl_plain(&world, &argv);
            assert_eq!(run.code, 0, "{argv:?}: {:?}", run.stderr);
            assert!(run.stdout.contains("Usage: toolportctl"), "{argv:?}");
            assert!(run.stdout.contains(command.summary), "{argv:?}");
        }
    }
    assert_eq!(changed_files(&world, &before), Vec::<String>::new());
}

fn run_ctl_plain(world: &World, argv: &[String]) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .args(argv)
        .env_clear()
        .current_dir(&world.home)
        .stdin(Stdio::null());
    for (key, value) in world.env() {
        command.env(key, value);
    }
    let output = command.output().expect("run toolportctl");
    Run {
        code: output.status.code().expect("exit code"),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn exit_codes_and_the_version_envelope_follow_the_documented_contract() {
    let world = World::new("exit-codes");
    let json = |argv: &[&str]| {
        let argv: Vec<String> = argv.iter().map(|w| w.to_string()).collect();
        let run = run_ctl(&world, &argv, None);
        let envelope: Value = serde_json::from_str(run.stdout.trim()).unwrap();
        (run, envelope)
    };
    let (run, version) = json(&["--version"]);
    assert_eq!(run.code, 0);
    assert_eq!(version["command"], "version");
    assert_eq!(version["data"]["name"], "toolportctl");
    for argv in [
        vec!["nope"],
        vec!["status", "extra"],
        vec!["--bogus"],
        vec!["mcp", "serve"],
    ] {
        let (run, envelope) = json(&argv);
        assert_eq!(run.code, 2, "{argv:?}: {envelope}");
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["error"]["code"], "usage");
    }
    let bare = run_ctl_plain(&world, &[]);
    assert_eq!(bare.code, 2);
    assert!(bare.stdout.contains("Usage: toolportctl"));
    let (run, unhealthy) = json(&["mcp", "doctor"]);
    assert_eq!(run.code, 1);
    assert_eq!(unhealthy["error"]["code"], "unhealthy");
    assert!(unhealthy["data"]["checks"].is_array());
}

#[test]
fn data_dir_flag_points_the_command_at_another_data_dir() {
    let world = World::new("data-dir");
    let elsewhere = world.base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_toolportctl"));
    command
        .args(["--json", "--data-dir"])
        .arg(&elsewhere)
        .arg("status")
        .env_clear()
        .current_dir(&world.home);
    for (key, value) in world
        .env()
        .into_iter()
        .filter(|(k, _)| *k != "TOOLPORT_DATA_DIR")
    {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["data"]["registry"]["exists"], false);
    assert_eq!(
        envelope["data"]["dataDir"],
        elsewhere.to_string_lossy().as_ref()
    );
}

#[test]
fn a_secret_key_never_shows_in_any_golden() {
    for entry in std::fs::read_dir(golden::root()).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for secret in [FAKE_SECRET, SECRET_KEY, VAULTED] {
            assert!(
                !text.contains(secret),
                "golden {} contains a secret value",
                entry.path().display()
            );
        }
    }
}
