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
use toolport_plus::selfmcp::{RESOURCES, TOOLS};

mod common;
use common::attention_world::{
    attention_access, attention_home, attention_quiet, attention_updates,
};
use common::catalog_world;
use common::compression::{
    edit_wrapped_hook, fake_proxy, fixture_hooks, lose_hooks, measure_world, rtk_home,
    silent_hooks, transcripts,
};
use common::library_world::{self, library_home};
use common::oauth_mock::{self, OauthMock};
use common::server_repo_world::{server_repo, server_repo_dirty, server_repo_peers};
use common::setup_world::{self, PASSPHRASE};
use common::taps_world;
use common::tasks_world::{scheduler_extras, tasks_home};
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

fn folder_routing(world: &World) {
    let mut registry = world.registry();
    registry["profiles"] = json!([
        {"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha"]},
        {"id": "focus", "name": "Focus", "enabledServerIds": ["srv-alpha"]},
        {"id": "wide", "name": "Wide", "enabledServerIds": ["srv-alpha", "srv-beta"]},
    ]);
    registry["folderProfiles"] = json!([{"path": "/work/proj", "profile": "wide"}]);
    world.write_registry(&registry);
}

fn a_set_that_was_deleted(world: &World) {
    let mut registry = world.registry();
    registry["clientScopes"] = json!({"claude-code": "gone"});
    world.write_registry(&registry);
}

fn launch_profile_sets(world: &World) {
    let mut registry = world.registry();
    registry["version"] = json!(3);
    registry["servers"][0]["enabled"] = json!(true);
    registry["servers"][1]["enabled"] = json!(true);
    registry["defaultAccessProfileId"] = json!("default");
    registry["profiles"] = json!([
        {"id": "default", "name": "Default", "enabledServerIds": ["srv-alpha", "srv-beta"]},
        {"id": "bare", "name": "Bare", "enabledServerIds": ["srv-alpha"]},
        {"id": "wide", "name": "Wide", "enabledServerIds": ["srv-alpha", "srv-beta"]},
        {"id": "empty", "name": "Empty", "enabledServerIds": []},
    ]);
    world.write_registry(&registry);
}

fn bare_in_use(world: &World) {
    let mut registry = world.registry();
    registry["clientScopes"] = json!({"claude-code": "bare"});
    registry["folderProfiles"] = json!([{"path": "/work/proj", "profile": "bare"}]);
    world.write_registry(&registry);
}

fn a_route_to_a_deleted_set(world: &World) {
    let mut registry = world.registry();
    registry["folderProfiles"] = json!([
        {"path": "/work/proj", "profile": "wide"},
        {"path": "/old/proj", "profile": "gone"},
    ]);
    world.write_registry(&registry);
}

fn map_the_project_folder(world: &World) {
    let mut registry = world.registry();
    registry["folderProfiles"] = json!([
        {"path": world.home.join("proj").to_string_lossy(), "profile": "wide"}
    ]);
    world.write_registry(&registry);
}

fn opt_alpha_into_the_stdio_probe(world: &World) {
    let mut registry = world.registry();
    registry["servers"][0]["plus"] = json!({"authProbe": {"kind": "stdio"}});
    world.write_registry(&registry);
}

fn claude_code_home(world: &World) {
    std::fs::write(
        world.home.join(".claude.json"),
        r#"{"theme": "dark", "mcpServers": {"legacy-lint": {"command": "lint", "args": []}}}"#,
    )
    .unwrap();
}

fn edit_the_client_file(world: &World) {
    std::fs::write(
        world.home.join(".claude.json"),
        r#"{"theme": "light", "mcpServers": {}}"#,
    )
    .unwrap();
}

fn context_files(world: &World) {
    world.context_files();
}

fn memory_files(world: &World) {
    world.memory_files();
}

fn checks_files(world: &World) {
    world.checks_files();
}

fn outside_link(world: &World) {
    let put = |rel: &str, text: &str| {
        let path = world.repo.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    put("shared/rubric.md", "# rubric\n");
    let skill = std::fs::read_to_string(world.repo.join("skills/acceptance/SKILL.md")).unwrap();
    put(
        "skills/acceptance/SKILL.md",
        &format!("{skill}\nAlso read the [rubric](../../shared/rubric.md).\n"),
    );
}

fn differing_setup(world: &World) {
    let path = world.home.join(".claude/skills/acceptance/SETUP.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "# my setup\nstep one\n").unwrap();
}

fn sync_home(world: &World) {
    setup_world::sync_home(world);
}

fn setup_home(world: &World) {
    setup_world::setup_home(world);
}

fn spend_data(world: &World) {
    common::spend_world::build(world);
}

fn spend_settings_conflict(world: &World) {
    common::spend_world::settings_with_a_conflict(world);
}

fn spend_settings_plain(world: &World) {
    common::spend_world::settings_without_env(world);
}

fn sources_home(world: &World) {
    common::sources_world::build_in(&world.base);
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

fn plugin_inventory(world: &World) {
    world.plugin_inventory();
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

const SPEND_CLOCK: &[(&str, &str)] = &[("TOOLPORT_PLUS_NOW", common::spend_world::NOW_SECONDS)];

const CASES: &[Case] = &[
    case(
        "attention ls",
        &[
            hook("quiet-world", attention_quiet),
            read("quiet", &["attention", "ls"]),
            hook("busy-world", attention_home),
            read("busy", &["attention", "ls"]),
            hook("access-world", attention_access),
            read("access", &["attention", "ls"]),
            usage("usage", &["attention", "ls", "extra"]),
        ],
    ),
    case(
        "attention snooze",
        &[
            hook("busy-world", attention_home),
            apply(
                "hour",
                &["attention", "snooze", "library:held", "--for", "1h"],
            ),
            read(
                "preview",
                &[
                    "attention",
                    "snooze",
                    "tasks:failed:nightly-report",
                    "--dry-run",
                ],
            ),
            apply(
                "too_long",
                &["attention", "snooze", "library:held", "--for", "8d"],
            )
            .exit(2),
            apply("unknown_row", &["attention", "snooze", "no-such-row"]).exit(1),
            usage("usage", &["attention", "snooze"]),
        ],
    ),
    case(
        "attention skip",
        &[
            hook("updates-world", attention_updates),
            setup("fetch", &["attention", "check"]),
            read("listed", &["attention", "ls"]),
            read(
                "preview",
                &[
                    "attention",
                    "skip",
                    "updates:tap:team-skills:3c5d9d8e>60abf5fe",
                    "--dry-run",
                ],
            ),
            apply(
                "skip",
                &[
                    "attention",
                    "skip",
                    "updates:tap:team-skills:3c5d9d8e>60abf5fe",
                ],
            ),
            read("hidden", &["attention", "ls", "--hidden"]),
            apply(
                "without_a_version",
                &["attention", "skip", "tasks:failed:nightly-report"],
            )
            .exit(2),
            apply("needs_you", &["attention", "skip", "library:held"]).exit(2),
        ],
    ),
    case(
        "attention dismiss",
        &[
            hook("busy-world", attention_home),
            read(
                "preview",
                &[
                    "attention",
                    "dismiss",
                    "tasks:failed:nightly-report",
                    "--dry-run",
                ],
            ),
            apply(
                "dismiss",
                &["attention", "dismiss", "tasks:failed:nightly-report"],
            ),
            read("hidden", &["attention", "ls", "--hidden"]),
            apply("needs_you", &["attention", "dismiss", "library:held"]).exit(2),
        ],
    ),
    case(
        "attention restore",
        &[
            hook("busy-world", attention_home),
            apply("not_hidden", &["attention", "restore", "library:held"]).exit(1),
            setup(
                "dismiss",
                &["attention", "dismiss", "tasks:failed:nightly-report"],
            ),
            apply(
                "restore",
                &["attention", "restore", "tasks:failed:nightly-report"],
            ),
        ],
    ),
    case(
        "attention undo",
        &[
            hook("busy-world", attention_home),
            apply("nothing", &["attention", "undo"]).exit(1),
            setup(
                "dismiss",
                &["attention", "dismiss", "tasks:failed:nightly-report"],
            ),
            apply("undo", &["attention", "undo"]),
        ],
    ),
    case(
        "attention check",
        &[
            hook("busy-world", attention_home),
            apply("no_sources", &["attention", "check"]),
            hook("updates-world", attention_updates),
            apply("one_tap", &["attention", "check"]),
        ],
    ),
    case(
        "attention settings",
        &[
            hook("busy-world", attention_home),
            read("defaults", &["attention", "settings"]),
        ],
    ),
    case(
        "attention notify",
        &[
            hook("busy-world", attention_home),
            usage("usage", &["attention", "notify"]),
        ],
    ),
    case(
        "attention set",
        &[
            hook("busy-world", attention_home),
            apply("check_off", &["attention", "set", "check.updates", "off"]),
            apply("notify_on", &["attention", "set", "notify", "on"]),
            apply("stale_days", &["attention", "set", "stale-days", "14"]),
            apply("stale_days_range", &["attention", "set", "stale-days", "0"]).exit(2),
            apply("unknown", &["attention", "set", "check.nothing", "on"]).exit(2),
            usage("usage", &["attention", "set", "notify"]),
        ],
    ),
    case(
        "spend status",
        &[
            hook("settings", spend_settings_plain),
            read("off", &["spend", "status"]).env(SPEND_CLOCK),
            hook("data", spend_data),
            read("with-data", &["spend", "status"]).env(SPEND_CLOCK),
            setup("turn-on", &["spend", "setup", "apply"]).env(SPEND_CLOCK),
            read("on", &["spend", "status"]).env(SPEND_CLOCK),
            usage("usage", &["spend", "status", "extra"]),
        ],
    ),
    case(
        "spend receiver status",
        &[
            hook("settings", spend_settings_plain),
            read("off", &["spend", "receiver", "status"]).env(SPEND_CLOCK),
            setup("turn-on", &["spend", "setup", "apply"]).env(SPEND_CLOCK),
            read("on", &["spend", "receiver", "status"]).env(SPEND_CLOCK),
        ],
    ),
    case(
        "spend summary",
        &[
            read("empty", &["spend", "summary"]).env(SPEND_CLOCK),
            hook("data", spend_data),
            read("default", &["spend", "summary"]).env(SPEND_CLOCK),
            read("today", &["spend", "summary", "--range", "today"]).env(SPEND_CLOCK),
            read(
                "month-project",
                &[
                    "spend",
                    "summary",
                    "--range",
                    "30d",
                    "--project",
                    "/work/alpha",
                ],
            )
            .env(SPEND_CLOCK),
            read(
                "month-model",
                &[
                    "spend",
                    "summary",
                    "--range",
                    "30d",
                    "--model",
                    "claude-haiku-4",
                ],
            )
            .env(SPEND_CLOCK),
            read(
                "month-task",
                &[
                    "spend",
                    "summary",
                    "--range",
                    "30d",
                    "--task",
                    "Interactive",
                ],
            )
            .env(SPEND_CLOCK),
            usage("bad-range", &["spend", "summary", "--range", "year"]).env(SPEND_CLOCK),
        ],
    ),
    case(
        "spend export-csv",
        &[
            hook("data", spend_data),
            read("month", &["spend", "export-csv", "--range", "30d"]).env(SPEND_CLOCK),
            apply(
                "saved",
                &[
                    "spend",
                    "export-csv",
                    "--range",
                    "30d",
                    "--out",
                    "{base}/spend.csv",
                ],
            )
            .env(SPEND_CLOCK),
            usage("bad-range", &["spend", "export-csv", "--range", "year"]).env(SPEND_CLOCK),
        ],
    ),
    case(
        "spend prune",
        &[
            hook("data", spend_data),
            read("dry-run", &["spend", "prune", "--dry-run"]).env(SPEND_CLOCK),
            apply("prune", &["spend", "prune"]).env(SPEND_CLOCK),
            read("again", &["spend", "prune", "--dry-run"]).env(SPEND_CLOCK),
            usage("totals-need-yes", &["spend", "prune", "--totals"]).env(SPEND_CLOCK),
            apply("totals", &["spend", "prune", "--totals", "--yes"]).env(SPEND_CLOCK),
        ],
    ),
    case(
        "spend config set",
        &[
            hook("data", spend_data),
            apply(
                "save",
                &[
                    "spend",
                    "config",
                    "set",
                    "--retention-days",
                    "14",
                    "--max-mb",
                    "32",
                    "--show-cost",
                    "off",
                ],
            )
            .env(SPEND_CLOCK),
            apply("port", &["spend", "config", "set", "--port", "4999"]).env(SPEND_CLOCK),
            usage(
                "retention-out-of-range",
                &["spend", "config", "set", "--retention-days", "3"],
            )
            .env(SPEND_CLOCK),
            usage(
                "show-cost-word",
                &["spend", "config", "set", "--show-cost", "maybe"],
            )
            .env(SPEND_CLOCK),
        ],
    ),
    case(
        "spend setup plan",
        &[
            hook("settings", spend_settings_conflict),
            read("conflict", &["spend", "setup", "plan"]).env(SPEND_CLOCK),
            read("port", &["spend", "setup", "plan", "--port", "4999"]).env(SPEND_CLOCK),
            read(
                "overwrite",
                &[
                    "spend",
                    "setup",
                    "plan",
                    "--overwrite",
                    "OTEL_METRICS_EXPORTER",
                ],
            )
            .env(SPEND_CLOCK),
            usage("low-port", &["spend", "setup", "plan", "--port", "80"]).env(SPEND_CLOCK),
            read(
                "profile",
                &["spend", "setup", "plan", "--profile", "{home}/.claude"],
            )
            .env(SPEND_CLOCK),
            usage(
                "unknown-profile",
                &["spend", "setup", "plan", "--profile", "{home}/elsewhere"],
            )
            .env(SPEND_CLOCK),
        ],
    ),
    case(
        "spend setup apply",
        &[
            hook("settings", spend_settings_conflict),
            read("dry-run", &["spend", "setup", "apply", "--dry-run"]).env(SPEND_CLOCK),
            apply("conflict", &["spend", "setup", "apply"]).env(SPEND_CLOCK),
            apply(
                "overwrite",
                &[
                    "spend",
                    "setup",
                    "apply",
                    "--overwrite",
                    "OTEL_METRICS_EXPORTER",
                ],
            )
            .env(SPEND_CLOCK),
            read("again", &["spend", "setup", "apply", "--dry-run"]).env(SPEND_CLOCK),
        ],
    ),
    case(
        "spend setup undo",
        &[
            hook("settings", spend_settings_conflict),
            read("nothing-to-undo", &["spend", "setup", "undo", "--dry-run"]).env(SPEND_CLOCK),
            setup(
                "turn-on",
                &[
                    "spend",
                    "setup",
                    "apply",
                    "--overwrite",
                    "all",
                    "--port",
                    "4999",
                ],
            )
            .env(SPEND_CLOCK),
            read("dry-run", &["spend", "setup", "undo", "--dry-run"]).env(SPEND_CLOCK),
            hook("data", spend_data),
            read(
                "dry-run-delete-data",
                &["spend", "setup", "undo", "--delete-data", "--dry-run"],
            )
            .env(SPEND_CLOCK),
            usage(
                "delete-data-needs-yes",
                &["spend", "setup", "undo", "--delete-data"],
            )
            .env(SPEND_CLOCK),
            usage(
                "unknown-profile",
                &["spend", "setup", "undo", "--profile", "{home}/elsewhere"],
            )
            .env(SPEND_CLOCK),
            apply("undo", &["spend", "setup", "undo"]).env(SPEND_CLOCK),
            apply("undo-again", &["spend", "setup", "undo"]).env(SPEND_CLOCK),
            setup(
                "turn-on-again",
                &["spend", "setup", "apply", "--overwrite", "all"],
            )
            .env(SPEND_CLOCK),
            apply(
                "undo-delete-data",
                &["spend", "setup", "undo", "--delete-data", "--yes"],
            )
            .env(SPEND_CLOCK),
            read("data-gone", &["spend", "status"]).env(SPEND_CLOCK),
        ],
    ),
    case(
        "task ls",
        &[
            hook("tasks", tasks_home),
            read("default", &["task", "ls"]),
            read("all", &["task", "ls", "--all"]),
            usage("usage", &["task", "ls", "extra"]),
            hook("scheduler", scheduler_extras),
            read("routine-and-scheduler", &["task", "ls", "--all"]),
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
            hook("scheduler", scheduler_extras),
            read("routine", &["task", "show", "refresh-reports"]),
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
            hook("scheduler", scheduler_extras),
            read("routine", &["task", "run", "refresh-reports", "--dry-run"]).exit(1),
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
            hook("scheduler", scheduler_extras),
            read("catch-up", &["task", "history", "nightly-report"]),
        ],
    ),
    case(
        "task missed",
        &[
            hook("tasks", tasks_home),
            read("none", &["task", "missed"]),
            hook("scheduler", scheduler_extras),
            read("all", &["task", "missed"]),
            read("one", &["task", "missed", "nightly-report"]),
            read("quiet-task", &["task", "missed", "draft-cleanup"]),
            usage("usage", &["task", "missed", "a", "b"]),
        ],
    ),
    case(
        "task enable",
        &[
            hook("tasks", tasks_home),
            read("preview", &["task", "enable", "draft-cleanup", "--dry-run"]),
            apply("apply", &["task", "enable", "draft-cleanup"]),
            apply("again", &["task", "enable", "draft-cleanup"]),
            read(
                "already-on",
                &["task", "enable", "nightly-report", "--dry-run"],
            ),
            apply("missing", &["task", "enable", "no-such-task"]).exit(1),
            usage("usage", &["task", "enable"]),
        ],
    ),
    case(
        "task disable",
        &[
            hook("tasks", tasks_home),
            read(
                "preview",
                &["task", "disable", "nightly-report", "--dry-run"],
            ),
            apply("apply", &["task", "disable", "nightly-report"]),
            apply("again", &["task", "disable", "nightly-report"]),
            apply("missing", &["task", "disable", "no-such-task"]).exit(1),
            usage("usage", &["task", "disable"]),
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
            apply(
                "limit",
                &[
                    "server",
                    "edit",
                    "gamma",
                    "--max-request-timeout-ms",
                    "5400000",
                ],
            ),
            read("limit-info", &["server", "info", "gamma"]),
            usage(
                "limit-refused",
                &["server", "edit", "gamma", "--max-request-timeout-ms", "0"],
            ),
            apply(
                "limit-default",
                &[
                    "server",
                    "edit",
                    "gamma",
                    "--max-request-timeout-ms",
                    "default",
                ],
            ),
            apply(
                "budget",
                &["server", "edit", "gamma", "--tool-budget", "job_wait=600"],
            ),
            read("budget-info", &["server", "info", "gamma"]),
            usage(
                "budget-refused",
                &["server", "edit", "gamma", "--tool-budget", "job_wait=0"],
            ),
            usage(
                "budget-malformed",
                &["server", "edit", "gamma", "--tool-budget", "job_wait"],
            ),
            apply(
                "budget-removed",
                &[
                    "server",
                    "edit",
                    "gamma",
                    "--tool-budget",
                    "job_wait=default",
                ],
            ),
            apply("serial", &["server", "edit", "gamma", "--serial"]),
            read("serial-info", &["server", "info", "gamma"]),
            usage(
                "serial-conflict",
                &["server", "edit", "gamma", "--serial", "--no-serial"],
            ),
            apply(
                "serial-removed",
                &["server", "edit", "gamma", "--no-serial"],
            ),
            usage("usage", &["server", "edit"]),
        ],
    ),
    case(
        "access-set ls",
        &[
            hook("sets", explicit_default_access),
            read("ls", &["access-set", "ls"]),
            hook("route", folder_routing),
            read("routed", &["access-set", "ls"]),
            usage("usage", &["access-set", "ls", "extra"]),
        ],
    ),
    case(
        "access-set add",
        &[
            hook("sets", launch_profile_sets),
            read(
                "plan",
                &[
                    "access-set",
                    "add",
                    "Deep Work",
                    "--server",
                    "alpha",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["access-set", "add", "Deep Work", "--server", "alpha"],
            ),
            read("exists", &["access-set", "add", "deep work", "--dry-run"]).exit(1),
            read(
                "unknown-server",
                &["access-set", "add", "New", "--server", "nope", "--dry-run"],
            )
            .exit(1),
            read("reserved", &["access-set", "add", "@all", "--dry-run"]).exit(2),
            read(
                "empty-set",
                &["access-set", "add", "Empty Too", "--dry-run"],
            ),
            usage("usage", &["access-set", "add"]),
        ],
    ),
    case(
        "access-set rm",
        &[
            hook("sets", launch_profile_sets),
            hook("bare-in-use", bare_in_use),
            read("in-use", &["access-set", "rm", "bare", "--dry-run"]).exit(1),
            read(
                "detach-plan",
                &["access-set", "rm", "bare", "--detach", "--dry-run"],
            ),
            read(
                "leave-plan",
                &["access-set", "rm", "bare", "--leave", "--dry-run"],
            ),
            apply("detach", &["access-set", "rm", "bare", "--detach"]),
            read("default", &["access-set", "rm", "default", "--dry-run"]).exit(1),
            read(
                "both-flags",
                &["access-set", "rm", "wide", "--detach", "--leave"],
            )
            .exit(2),
            apply("apply", &["access-set", "rm", "empty"]),
            read("gone", &["access-set", "rm", "empty"]).exit(1),
            usage("usage", &["access-set", "rm"]),
        ],
    ),
    case(
        "access-set default",
        &[
            hook("sets", launch_profile_sets),
            read("plan", &["access-set", "default", "wide", "--dry-run"]),
            apply("apply", &["access-set", "default", "wide"]),
            apply("none", &["access-set", "default", "--none"]),
            read("again", &["access-set", "default", "--none", "--dry-run"]),
            read(
                "unknown-set",
                &["access-set", "default", "nope", "--dry-run"],
            )
            .exit(1),
            read("both", &["access-set", "default", "wide", "--none"]).exit(2),
            read("nothing", &["access-set", "default"]).exit(2),
        ],
    ),
    case(
        "access-set tools",
        &[
            hook("sets", launch_profile_sets),
            read("ls", &["access-set", "tools"]),
            read(
                "plan",
                &[
                    "access-set",
                    "tools",
                    "wide",
                    "alpha",
                    "--tool",
                    "read",
                    "--tool",
                    "list",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "access-set",
                    "tools",
                    "wide",
                    "alpha",
                    "--tool",
                    "read",
                    "--tool",
                    "list",
                ],
            ),
            read("one-set", &["access-set", "tools", "wide"]),
            apply("none", &["access-set", "tools", "wide", "alpha", "--none"]),
            apply("all", &["access-set", "tools", "wide", "alpha", "--all"]),
            read(
                "not-listed",
                &["access-set", "tools", "focus", "beta", "--all", "--dry-run"],
            )
            .exit(1),
            read(
                "unknown-set",
                &["access-set", "tools", "nope", "alpha", "--all", "--dry-run"],
            )
            .exit(1),
            read(
                "unknown-server",
                &["access-set", "tools", "wide", "nope", "--all", "--dry-run"],
            )
            .exit(1),
            read(
                "both",
                &[
                    "access-set",
                    "tools",
                    "wide",
                    "alpha",
                    "--tool",
                    "x",
                    "--all",
                ],
            )
            .exit(2),
            read("nothing", &["access-set", "tools", "wide", "alpha"]).exit(2),
            read(
                "tool-without-server",
                &["access-set", "tools", "wide", "--tool", "x"],
            )
            .exit(2),
        ],
    ),
    case(
        "client scope",
        &[
            hook("sets", launch_profile_sets),
            read(
                "plan",
                &["client", "scope", "claude-code", "bare", "--dry-run"],
            ),
            apply("apply", &["client", "scope", "claude-code", "bare"]),
            hook("map-the-folder", map_the_project_folder),
            read(
                "folder-outranks",
                &["client", "scope", "claude-code", "wide", "--dry-run"],
            ),
            apply("clear", &["client", "scope", "claude-code", "--none"]),
            read(
                "unknown-client",
                &["client", "scope", "claud-code", "bare", "--dry-run"],
            )
            .exit(1),
            read(
                "unknown-set",
                &["client", "scope", "claude-code", "nope", "--dry-run"],
            )
            .exit(1),
            read("needs-a-set", &["client", "scope", "claude-code"]).exit(2),
            usage("usage", &["client", "scope"]),
        ],
    ),
    case(
        "folder route ls",
        &[
            hook("sets", launch_profile_sets),
            read("none", &["folder", "route", "ls"]),
            hook("routes", a_route_to_a_deleted_set),
            read("ls", &["folder", "route", "ls"]),
            usage("usage", &["folder", "route", "ls", "extra"]),
        ],
    ),
    case(
        "folder route add",
        &[
            hook("sets", launch_profile_sets),
            read(
                "plan",
                &["folder", "route", "add", "/work/proj", "bare", "--dry-run"],
            ),
            apply("apply", &["folder", "route", "add", "/work/proj", "bare"]),
            read(
                "again",
                &["folder", "route", "add", "/work/proj/", "bare", "--dry-run"],
            ),
            read(
                "move",
                &["folder", "route", "add", "/work/proj", "wide", "--dry-run"],
            ),
            read(
                "relative",
                &["folder", "route", "add", "proj", "bare", "--dry-run"],
            )
            .exit(2),
            read(
                "unknown-set",
                &["folder", "route", "add", "/work/proj", "gone", "--dry-run"],
            )
            .exit(1),
            usage("usage", &["folder", "route", "add", "/work/proj"]),
        ],
    ),
    case(
        "folder route rm",
        &[
            hook("sets", launch_profile_sets),
            setup("add", &["folder", "route", "add", "/work/proj", "bare"]),
            read(
                "plan",
                &["folder", "route", "rm", "/work/proj", "--dry-run"],
            ),
            apply("apply", &["folder", "route", "rm", "/work/proj"]),
            read("gone", &["folder", "route", "rm", "/work/proj"]).exit(1),
            usage("usage", &["folder", "route", "rm"]),
        ],
    ),
    case(
        "client reach",
        &[
            read("default-access", &["client", "reach"]),
            hook("explicit-access-sets", explicit_default_access),
            read("narrow-sets", &["client", "reach"]),
            read("one-server", &["client", "reach", "--server", "beta"]),
            read(
                "one-client",
                &["client", "reach", "--client", "claude-code"],
            ),
            hook("map-a-folder", folder_routing),
            read("folder-outranks", &["client", "reach"]),
            hook("delete-the-set", a_set_that_was_deleted),
            read("deleted-set", &["client", "reach"]),
            hook("home", claude_code_home),
            setup(
                "add",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            setup(
                "uninstall",
                &["server", "uninstall", "alpha", "--keep-clients"],
            ),
            read("kept-direct-entry", &["client", "reach"]),
            read("unknown-server", &["client", "reach", "--server", "nope"]).exit(1),
            read("unknown-client", &["client", "reach", "--client", "nope"]).exit(1),
            usage("usage", &["client", "reach", "alpha"]),
        ],
    ),
    case(
        "client direct add",
        &[
            hook("home", claude_code_home),
            read(
                "plan",
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
            apply(
                "unchanged",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            apply(
                "remote",
                &["client", "direct", "add", "beta", "--client", "claude-code"],
            ),
            read(
                "stale-plan",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                    "--plan-hash",
                    "0000000000000000",
                    "--force",
                ],
            )
            .exit(1),
            read(
                "unsupported",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "goose",
                    "--dry-run",
                ],
            )
            .exit(1),
            read(
                "unknown-client",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "no-such-client",
                    "--dry-run",
                ],
            )
            .exit(1),
            read(
                "unknown-server",
                &[
                    "client",
                    "direct",
                    "add",
                    "nope",
                    "--client",
                    "claude-code",
                    "--dry-run",
                ],
            )
            .exit(1),
            usage("usage", &["client", "direct", "add", "alpha"]),
        ],
    ),
    case(
        "client direct rm",
        &[
            hook("home", claude_code_home),
            setup(
                "add",
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
                "plan",
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
            apply(
                "absent",
                &["client", "direct", "rm", "alpha", "--client", "claude-code"],
            ),
            setup(
                "add-again",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            hook("edited", edit_the_client_file),
            apply(
                "forgotten",
                &["client", "direct", "rm", "alpha", "--client", "claude-code"],
            ),
            usage("usage", &["client", "direct", "rm", "alpha"]),
        ],
    ),
    case(
        "client direct ls",
        &[
            hook("home", claude_code_home),
            read("empty", &["client", "direct", "ls"]),
            setup(
                "add",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            read("entries", &["client", "direct", "ls"]),
            read(
                "one-client",
                &["client", "direct", "ls", "--client", "claude-code"],
            ),
            read(
                "unknown-client",
                &["client", "direct", "ls", "--client", "no-such-client"],
            )
            .exit(1),
        ],
    ),
    case(
        "client direct check",
        &[
            hook("home", claude_code_home),
            setup(
                "add",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            read("in-sync", &["client", "direct", "check"]),
            hook("edited", edit_the_client_file),
            read("drifted", &["client", "direct", "check"]).exit(1),
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
    case(
        "server repo status",
        &[
            hook("repo-world", server_repo),
            read("never_checked", &["server", "repo", "status"]),
            setup("fetch", &["server", "repo", "check"]),
            read("behind", &["server", "repo", "status"]),
            read("one", &["server", "repo", "status", "gamma"]),
            read("unknown", &["server", "repo", "status", "ghost"]).exit(1),
            read("no_checkout", &["server", "repo", "status", "alpha"]).exit(1),
            usage("usage", &["server", "repo", "status", "a", "b"]),
        ],
    ),
    case(
        "server repo check",
        &[
            hook("repo-world", server_repo),
            apply("fetched", &["server", "repo", "check"]),
            usage("usage", &["server", "repo", "check", "extra"]),
        ],
    ),
    case(
        "server repo update",
        &[
            hook("repo-world", server_repo),
            setup("fetch", &["server", "repo", "check"]),
            read(
                "plan",
                &[
                    "server",
                    "repo",
                    "update",
                    "gamma",
                    "--dry-run",
                    "--wait",
                    "1",
                ],
            ),
            apply(
                "update",
                &[
                    "server",
                    "repo",
                    "update",
                    "gamma",
                    "--build",
                    "echo built",
                    "--no-restart",
                ],
            ),
            apply(
                "up_to_date",
                &["server", "repo", "update", "gamma", "--no-restart"],
            ),
            hook("local-change", server_repo_dirty),
            apply(
                "dirty",
                &["server", "repo", "update", "gamma", "--no-restart"],
            )
            .exit(1),
            read(
                "dirty_plan",
                &[
                    "server",
                    "repo",
                    "update",
                    "gamma",
                    "--dry-run",
                    "--wait",
                    "1",
                ],
            ),
            apply("unknown", &["server", "repo", "update", "ghost"]).exit(1),
            usage("usage", &["server", "repo", "update"]),
            usage(
                "conflicting_restart",
                &[
                    "server",
                    "repo",
                    "update",
                    "gamma",
                    "--restart-now",
                    "--no-restart",
                ],
            ),
        ],
    ),
    case(
        "server repo switch",
        &[
            hook("repo-world", server_repo),
            hook("peers", server_repo_peers),
            apply(
                "plan_not_fetched",
                &["server", "repo", "switch", "gamma", "--dry-run"],
            ),
            apply(
                "plan",
                &["server", "repo", "switch", "gamma", "--dry-run", "--fetch"],
            ),
            apply(
                "switch",
                &[
                    "server",
                    "repo",
                    "switch",
                    "gamma",
                    "--fetch",
                    "--no-restart",
                ],
            ),
            read(
                "undo_preview",
                &["server", "repo", "switch", "gamma", "--undo", "--dry-run"],
            ),
            apply(
                "undo",
                &[
                    "server",
                    "repo",
                    "switch",
                    "gamma",
                    "--undo",
                    "--no-restart",
                ],
            ),
            apply("unknown", &["server", "repo", "switch", "ghost"]).exit(1),
            apply(
                "unknown_machine",
                &["server", "repo", "switch", "gamma", "--machine", "ghost"],
            )
            .exit(1),
            usage("usage", &["server", "repo", "switch"]),
        ],
    ),
    case(
        "server repo undo",
        &[
            hook("repo-world", server_repo),
            setup("fetch", &["server", "repo", "check"]),
            apply("nothing", &["server", "repo", "undo", "gamma"]).exit(1),
            setup(
                "update",
                &[
                    "server",
                    "repo",
                    "update",
                    "gamma",
                    "--build",
                    "echo built",
                    "--no-restart",
                ],
            ),
            read("preview", &["server", "repo", "undo", "gamma", "--dry-run"]),
            apply("undo", &["server", "repo", "undo", "gamma", "--no-restart"]),
            usage("usage", &["server", "repo", "undo"]),
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
                "applied",
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
        "context launch-profile add",
        &[
            hook("files", context_files),
            hook("sets", launch_profile_sets),
            read(
                "plan",
                &[
                    "context",
                    "launch-profile",
                    "add",
                    "bare",
                    "--set",
                    "bare",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["context", "launch-profile", "add", "bare", "--set", "bare"],
            ),
            read(
                "again",
                &["context", "launch-profile", "add", "bare", "--dry-run"],
            ),
            read(
                "unknown-set",
                &[
                    "context",
                    "launch-profile",
                    "add",
                    "bare",
                    "--set",
                    "nope",
                    "--dry-run",
                ],
            )
            .exit(1),
            read(
                "needs-a-set",
                &["context", "launch-profile", "add", "other", "--dry-run"],
            )
            .exit(2),
            usage("usage", &["context", "launch-profile", "add"]),
            hook("map-the-folder", map_the_project_folder),
            read(
                "folder-outranks",
                &[
                    "context",
                    "launch-profile",
                    "add",
                    "bare",
                    "--set",
                    "empty",
                    "--dry-run",
                ],
            ),
        ],
    ),
    case(
        "context launch-profile ls",
        &[
            hook("files", context_files),
            hook("sets", launch_profile_sets),
            read("none", &["context", "launch-profile", "ls"]),
            setup(
                "add",
                &["context", "launch-profile", "add", "bare", "--set", "bare"],
            ),
            hook("map-the-folder", map_the_project_folder),
            read("ls", &["context", "launch-profile", "ls"]),
            read(
                "ls-inside",
                &[
                    "context",
                    "launch-profile",
                    "ls",
                    "bare",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            read("ls-unknown", &["context", "launch-profile", "ls", "nope"]).exit(1),
        ],
    ),
    case(
        "context launch-profile rm",
        &[
            hook("files", context_files),
            hook("sets", launch_profile_sets),
            setup(
                "add",
                &["context", "launch-profile", "add", "bare", "--set", "bare"],
            ),
            read(
                "plan",
                &["context", "launch-profile", "rm", "bare", "--dry-run"],
            ),
            apply("apply", &["context", "launch-profile", "rm", "bare"]),
            read("gone", &["context", "launch-profile", "rm", "bare"]).exit(1),
            usage("usage", &["context", "launch-profile", "rm"]),
        ],
    ),
    case(
        "context launch-profile policy",
        &[
            hook("files", context_files),
            hook("sets", launch_profile_sets),
            hook("home", claude_code_home),
            setup(
                "direct",
                &[
                    "client",
                    "direct",
                    "add",
                    "alpha",
                    "--client",
                    "claude-code",
                ],
            ),
            setup(
                "add",
                &[
                    "context",
                    "launch-profile",
                    "add",
                    "bare",
                    "--set",
                    "empty",
                    "--deny-direct",
                    "on",
                ],
            ),
            read(
                "plan",
                &[
                    "context",
                    "launch-profile",
                    "policy",
                    "bare",
                    "--cwd",
                    "{home}/proj",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "context",
                    "launch-profile",
                    "policy",
                    "bare",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            apply(
                "off",
                &[
                    "context",
                    "launch-profile",
                    "add",
                    "bare",
                    "--deny-direct",
                    "off",
                ],
            ),
            read(
                "switched-off",
                &[
                    "context",
                    "launch-profile",
                    "policy",
                    "bare",
                    "--cwd",
                    "{home}/proj",
                ],
            ),
            read(
                "unknown-profile",
                &[
                    "context",
                    "launch-profile",
                    "policy",
                    "nope",
                    "--cwd",
                    "{home}/proj",
                ],
            )
            .exit(1),
            usage("usage", &["context", "launch-profile", "policy", "bare"]),
        ],
    ),
    case(
        "plugins ls",
        &[
            hook("files", plugin_inventory),
            read("installed", &["plugins", "ls"]),
            usage("usage", &["plugins", "ls", "extra"]),
        ],
    ),
    case(
        "plugins show",
        &[
            hook("files", plugin_inventory),
            read("one", &["plugins", "show", "ecc@ecc"]),
            read("unknown", &["plugins", "show", "ghost@x"]).exit(1),
            usage("usage", &["plugins", "show"]),
        ],
    ),
    case(
        "hooks ls",
        &[
            hook("files", plugin_inventory),
            read("all", &["hooks", "ls"]),
            read("bash", &["hooks", "ls", "--tool", "Bash"]),
            read("plugin", &["hooks", "ls", "--owner", "plugin"]),
            usage("usage", &["hooks", "ls", "extra"]),
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
        "memory ls",
        &[
            hook("files", memory_files),
            read("folder", &["memory", "ls", "--cwd", "{home}/proj-wt"]),
            read("all", &["memory", "ls", "--all"]),
            read("nothing", &["memory", "ls", "--cwd", "{home}/other-gone"]).exit(2),
            usage("usage", &["memory", "ls", "extra"]),
        ],
    ),
    case(
        "memory status",
        &[
            hook("files", memory_files),
            read("folder", &["memory", "status", "--cwd", "{home}/proj"]),
            setup(
                "switch-off",
                &["memory", "set", "off", "--cwd", "{home}/proj-wt"],
            ),
            read("off", &["memory", "status", "--cwd", "{home}/proj"]),
            read("by-hand", &["memory", "status", "--cwd", "{home}/other"]),
            usage("usage", &["memory", "status", "extra"]),
        ],
    ),
    case(
        "memory set",
        &[
            hook("files", memory_files),
            read(
                "preview",
                &[
                    "memory",
                    "set",
                    "off",
                    "--cwd",
                    "{home}/proj-wt",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["memory", "set", "off", "--cwd", "{home}/proj-wt"],
            ),
            apply(
                "default",
                &["memory", "set", "default", "--cwd", "{home}/proj"],
            ),
            read(
                "global-preview",
                &["memory", "set", "off", "--global", "--dry-run"],
            ),
            apply("global", &["memory", "set", "off", "--global"]),
            usage("usage", &["memory", "set", "maybe", "--cwd", "{home}/proj"]),
            usage(
                "both",
                &["memory", "set", "on", "--cwd", "{home}/proj", "--global"],
            ),
        ],
    ),
    case(
        "memory clear",
        &[
            hook("files", memory_files),
            read(
                "plan",
                &["memory", "clear", "--cwd", "{home}/proj-wt", "--dry-run"],
            ),
            read(
                "forever-plan",
                &[
                    "memory",
                    "clear",
                    "--cwd",
                    "{home}/proj",
                    "--forever",
                    "--dry-run",
                ],
            ),
            read(
                "forever-unconfirmed",
                &["memory", "clear", "--cwd", "{home}/proj", "--forever"],
            )
            .exit(2),
            apply(
                "apply",
                &["memory", "clear", "--cwd", "{home}/proj", "--also-off"],
            ),
            read("nothing", &["memory", "clear", "--cwd", "{home}/proj"]).exit(1),
            usage("usage", &["memory", "clear"]),
        ],
    ),
    case(
        "memory undo",
        &[
            hook("files", memory_files),
            read("nothing", &["memory", "undo", "--cwd", "{home}/proj"]).exit(1),
            setup(
                "clear",
                &["memory", "clear", "--cwd", "{home}/proj", "--also-off"],
            ),
            read(
                "preview",
                &["memory", "undo", "--cwd", "{home}/proj-wt", "--dry-run"],
            ),
            apply("apply", &["memory", "undo", "--cwd", "{home}/proj"]),
            usage("usage", &["memory", "undo", "a", "b"]),
        ],
    ),
    case(
        "memory overview",
        &[
            hook("files", memory_files),
            read("all", &["memory", "overview", "--all"]),
            read("folder", &["memory", "overview", "--cwd", "{home}/proj-wt"]),
            read(
                "filtered",
                &[
                    "memory",
                    "overview",
                    "--all",
                    "--filter",
                    "memory-off",
                    "--sort",
                    "size",
                ],
            ),
            read(
                "bad-filter",
                &["memory", "overview", "--all", "--filter", "bogus"],
            )
            .exit(2),
            usage(
                "both",
                &["memory", "overview", "--all", "--cwd", "{home}/proj"],
            ),
        ],
    ),
    case(
        "sessions ls",
        &[
            hook("files", memory_files),
            read("folder", &["sessions", "ls", "--cwd", "{home}/proj-wt"]),
            read("all", &["sessions", "ls", "--all"]),
            usage("usage", &["sessions", "ls", "extra"]),
        ],
    ),
    case(
        "sessions clear",
        &[
            hook("files", memory_files),
            read(
                "plan",
                &[
                    "sessions",
                    "clear",
                    "--cwd",
                    "{home}/proj",
                    "--older-than",
                    "7",
                    "--dry-run",
                ],
            ),
            apply(
                "trash",
                &[
                    "sessions",
                    "clear",
                    "--cwd",
                    "{home}/proj",
                    "--ids",
                    "sess-new",
                ],
            ),
            apply(
                "archive",
                &[
                    "sessions",
                    "clear",
                    "--cwd",
                    "{home}/proj",
                    "--ids",
                    "sess-wt",
                    "--to",
                    "archive",
                ],
            ),
            read(
                "forever-unconfirmed",
                &[
                    "sessions",
                    "clear",
                    "--cwd",
                    "{home}/proj",
                    "--ids",
                    "sess-old",
                    "--forever",
                ],
            )
            .exit(2),
            read(
                "unknown",
                &["sessions", "clear", "--cwd", "{home}/proj", "--ids", "nope"],
            )
            .exit(1),
            usage("usage", &["sessions", "clear", "--cwd", "{home}/proj"]),
        ],
    ),
    case(
        "sessions undo",
        &[
            hook("files", memory_files),
            read("nothing", &["sessions", "undo", "--cwd", "{home}/proj"]).exit(1),
            setup(
                "clear",
                &[
                    "sessions",
                    "clear",
                    "--cwd",
                    "{home}/proj",
                    "--ids",
                    "sess-old,sess-new",
                ],
            ),
            read(
                "preview",
                &["sessions", "undo", "--cwd", "{home}/proj", "--dry-run"],
            ),
            apply("apply", &["sessions", "undo", "--cwd", "{home}/proj"]),
            usage("usage", &["sessions", "undo", "a", "b"]),
        ],
    ),
    case(
        "sessions restore",
        &[
            hook("files", memory_files),
            setup(
                "archive",
                &[
                    "sessions",
                    "clear",
                    "--cwd",
                    "{home}/proj",
                    "--ids",
                    "sess-old",
                    "--to",
                    "archive",
                ],
            ),
            read("unknown", &["sessions", "restore", "archive-nope"]).exit(1),
            usage("usage", &["sessions", "restore"]),
        ],
    ),
    case(
        "sessions retention",
        &[
            hook("files", memory_files),
            read("current", &["sessions", "retention"]),
            read(
                "preview",
                &["sessions", "retention", "--set", "90", "--dry-run"],
            ),
            apply("apply", &["sessions", "retention", "--set", "90"]),
            read("after", &["sessions", "retention"]),
            read("zero", &["sessions", "retention", "--set", "0"]).exit(2),
            usage("not-a-number", &["sessions", "retention", "--set", "many"]),
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
        &[
            read("repo", &["skills", "ls", "--repo", "{repo}"]),
            hook("sources", sources_home),
            read("library", &["skills", "ls", "--source", "library"]),
            read("source", &["skills", "ls", "--source", "repo:odh"]),
            read("unknown", &["skills", "ls", "--source", "nope"]).exit(1),
        ],
    ),
    case(
        "sources ls",
        &[
            hook("sources", sources_home),
            read("summary", &["sources", "ls"]),
            read("items", &["sources", "ls", "--items"]),
            read("partial", &["sources", "ls", "--budget", "repo=0"]),
            read("org", &["sources", "ls", "--source", "org", "--items"]),
            usage("bad-kind", &["sources", "ls", "--kind", "nope"]),
            usage("budget", &["sources", "ls", "--budget", "repo"]),
        ],
    ),
    case(
        "sources root ls",
        &[
            hook("sources", sources_home),
            read("", &["sources", "root", "ls"]),
        ],
    ),
    case(
        "sources root add",
        &[
            hook("sources", sources_home),
            read(
                "preview",
                &["sources", "root", "add", "{home}/work", "--dry-run"],
            ),
            apply("apply", &["sources", "root", "add", "{home}/work"]),
            read("again", &["sources", "root", "add", "{home}/work"]).exit(1),
            read(
                "missing",
                &["sources", "root", "add", "{home}/nowhere", "--dry-run"],
            )
            .exit(1),
            usage("usage", &["sources", "root", "add"]),
        ],
    ),
    case(
        "sources root rm",
        &[
            hook("sources", sources_home),
            read(
                "preview",
                &["sources", "root", "rm", "{home}/dups", "--dry-run"],
            ),
            apply("apply", &["sources", "root", "rm", "{home}/dups"]),
            read("again", &["sources", "root", "rm", "{home}/dups"]).exit(1),
        ],
    ),
    case(
        "skills lint",
        &[
            read("", &["skills", "lint", "--repo", "{repo}"]),
            hook("files", checks_files),
            read("checks", &["skills", "lint", "--repo", "{repo}"]).exit(1),
        ],
    ),
    case(
        "skills files",
        &[
            read("demo", &["skills", "files", "demo", "--repo", "{repo}"]),
            hook("files", checks_files),
            read(
                "checks",
                &["skills", "files", "acceptance", "--repo", "{repo}"],
            ),
            setup(
                "sync",
                &[
                    "skills",
                    "sync",
                    "--repo",
                    "{repo}",
                    "--client",
                    "claude-code",
                ],
            ),
            read(
                "synced",
                &["skills", "files", "acceptance", "--repo", "{repo}"],
            ),
            read("unknown", &["skills", "files", "nope", "--repo", "{repo}"]).exit(1),
            usage("usage", &["skills", "files", "--repo", "{repo}"]),
        ],
    ),
    case(
        "skills fix",
        &[
            hook("files", checks_files),
            read(
                "preview",
                &["skills", "fix", "chmod", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["skills", "fix", "chmod", "--repo", "{repo}"]),
            read("again", &["skills", "fix", "chmod", "--repo", "{repo}"]),
            read(
                "unknown-skill",
                &["skills", "fix", "chmod", "nope", "--repo", "{repo}"],
            )
            .exit(1),
            usage("unknown", &["skills", "fix", "delete", "--repo", "{repo}"]),
            usage("usage", &["skills", "fix"]),
            hook("outside-link", outside_link),
            read(
                "move_link_preview",
                &[
                    "skills",
                    "fix",
                    "move-link",
                    "acceptance",
                    "../../shared/rubric.md",
                    "--repo",
                    "{repo}",
                    "--dry-run",
                ],
            ),
            apply(
                "move_link",
                &[
                    "skills",
                    "fix",
                    "move-link",
                    "acceptance",
                    "../../shared/rubric.md",
                    "--repo",
                    "{repo}",
                ],
            ),
            read(
                "move_link_again",
                &[
                    "skills",
                    "fix",
                    "move-link",
                    "acceptance",
                    "../../shared/rubric.md",
                    "--repo",
                    "{repo}",
                ],
            )
            .exit(1),
            read(
                "move_link_not_outside",
                &[
                    "skills",
                    "fix",
                    "move-link",
                    "acceptance",
                    "references/guide.md",
                    "--repo",
                    "{repo}",
                ],
            )
            .exit(1),
            usage(
                "move_link_usage",
                &[
                    "skills",
                    "fix",
                    "move-link",
                    "acceptance",
                    "--repo",
                    "{repo}",
                ],
            ),
            usage(
                "dest_for_chmod",
                &["skills", "fix", "chmod", "--dest", "x", "--repo", "{repo}"],
            ),
        ],
    ),
    case(
        "skills diff",
        &[
            read("", &["skills", "diff", "--repo", "{repo}"]).exit(1),
            hook("files", checks_files),
            hook("differs", differing_setup),
            read(
                "skill",
                &["skills", "diff", "acceptance", "--repo", "{repo}"],
            ),
            read(
                "file",
                &[
                    "skills",
                    "diff",
                    "acceptance",
                    "--file",
                    "SETUP.md",
                    "--repo",
                    "{repo}",
                ],
            ),
            read(
                "unknown_file",
                &[
                    "skills",
                    "diff",
                    "acceptance",
                    "--file",
                    "nope.md",
                    "--repo",
                    "{repo}",
                ],
            )
            .exit(1),
            read(
                "unknown_skill",
                &["skills", "diff", "nope", "--repo", "{repo}"],
            )
            .exit(1),
            usage(
                "file_alone",
                &["skills", "diff", "--file", "SETUP.md", "--repo", "{repo}"],
            ),
            usage("two_skills", &["skills", "diff", "one", "two"]),
        ],
    ),
    case(
        "skills keep",
        &[
            hook("files", checks_files),
            hook("differs", differing_setup),
            read(
                "preview",
                &[
                    "skills",
                    "keep",
                    "acceptance",
                    "SETUP.md",
                    "--repo",
                    "{repo}",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "skills",
                    "keep",
                    "acceptance",
                    "SETUP.md",
                    "--repo",
                    "{repo}",
                ],
            ),
            read(
                "kept",
                &["skills", "diff", "acceptance", "--repo", "{repo}"],
            ),
            read(
                "not_held",
                &[
                    "skills",
                    "keep",
                    "acceptance",
                    "references/guide.md",
                    "--repo",
                    "{repo}",
                ],
            )
            .exit(1),
            usage(
                "no_file",
                &["skills", "keep", "acceptance", "--repo", "{repo}"],
            ),
            usage("no_skill", &["skills", "keep"]),
        ],
    ),
    case(
        "skills audit",
        &[read("", &["skills", "audit", "--repo", "{repo}"])],
    ),
    case(
        "setup parts",
        &[
            hook("setup-world", setup_home),
            read("", &["setup", "parts", "--roots", "{home}/code"]),
            read("default-roots", &["setup", "parts"]),
            usage("usage", &["setup", "parts", "extra"]),
        ],
    ),
    case(
        "setup check",
        &[
            hook("setup-world", setup_home),
            read("", &["setup", "check", "--roots", "{home}/code"]),
            apply(
                "ticked",
                &["setup", "check", "--roots", "{home}/code", "--tick", "keys"],
            ),
            usage("unknown-item", &["setup", "check", "--tick", "nonsense"]),
        ],
    ),
    case(
        "setup inspect",
        &[
            hook("setup-world", setup_home),
            apply(
                "export",
                &[
                    "setup",
                    "export",
                    "--out",
                    "{base}/out/setup.zip",
                    "--roots",
                    "{home}/code",
                ],
            ),
            read("", &["setup", "inspect", "{base}/out/setup.zip"]),
            usage("usage", &["setup", "inspect"]),
        ],
    ),
    case(
        "setup restore",
        &[
            hook("setup-world", setup_home),
            apply(
                "export",
                &[
                    "setup",
                    "export",
                    "--out",
                    "{base}/out/setup.zip",
                    "--roots",
                    "{home}/code",
                ],
            ),
            read("plan", &["setup", "restore", "{base}/out/setup.zip"]),
            usage(
                "no-plan",
                &["setup", "restore", "{base}/out/setup.zip", "--apply"],
            ),
            usage("usage", &["setup", "restore"]),
        ],
    ),
    case(
        "sync status",
        &[
            read("unconfigured", &["sync", "status"]),
            usage("usage", &["sync", "status", "extra"]),
        ],
    ),
    case(
        "sync setup",
        &[
            hook("sync-world", sync_home),
            read(
                "preview",
                &["sync", "setup", "--folder", "{base}/shared", "--dry-run"],
            ),
            apply(
                "folder",
                &[
                    "sync",
                    "setup",
                    "--folder",
                    "{base}/shared",
                    "--passphrase-stdin",
                ],
            )
            .stdin(PASSPHRASE),
            read("configured", &["sync", "status"]),
            read(
                "hosted-service",
                &["sync", "setup", "--git", "https://teams.toolport.app/x.git"],
            )
            .exit(1),
            usage(
                "both",
                &["sync", "setup", "--git", "x", "--folder", "{base}/shared"],
            ),
            apply("short-passphrase", &["sync", "setup", "--passphrase-stdin"])
                .stdin("short\n")
                .exit(2),
        ],
    ),
    case(
        "sync push",
        &[
            hook("sync-world", sync_home),
            setup(
                "setup",
                &[
                    "sync",
                    "setup",
                    "--folder",
                    "{base}/shared",
                    "--passphrase-stdin",
                ],
            )
            .stdin(PASSPHRASE),
            read("preview", &["sync", "push", "--dry-run"]),
            apply("push", &["sync", "push"]),
            read("status", &["sync", "status"]),
        ],
    ),
    case(
        "sync plan",
        &[
            hook("sync-world", sync_home),
            setup(
                "setup",
                &[
                    "sync",
                    "setup",
                    "--folder",
                    "{base}/shared",
                    "--passphrase-stdin",
                ],
            )
            .stdin(PASSPHRASE),
            apply("empty-remote", &["sync", "plan"]),
            setup("push", &["sync", "push"]),
            apply("plan", &["sync", "plan"]),
            usage("usage", &["sync", "plan", "extra"]),
        ],
    ),
    case(
        "sync pull",
        &[
            hook("sync-world", sync_home),
            setup(
                "setup",
                &[
                    "sync",
                    "setup",
                    "--folder",
                    "{base}/shared",
                    "--passphrase-stdin",
                ],
            )
            .stdin(PASSPHRASE),
            setup("push", &["sync", "push"]),
            apply("plan", &["sync", "pull"]),
            apply("unattended", &["sync", "pull", "--unattended"]),
            usage("no-plan", &["sync", "pull", "--apply"]),
            usage(
                "unattended-apply",
                &["sync", "pull", "--unattended", "--apply", "--plan", "x"],
            ),
        ],
    ),
    case(
        "sync repos",
        &[
            hook("repo-world", server_repo),
            read("nothing_published", &["sync", "repos"]),
            hook("peers", server_repo_peers),
            read("compared", &["sync", "repos"]),
            usage("usage", &["sync", "repos", "extra"]),
        ],
    ),
    case("sync undo", &[read("nothing", &["sync", "undo"]).exit(1)]),
    case(
        "setup undo",
        &[
            hook("setup-world", setup_home),
            read("nothing", &["setup", "undo"]).exit(1),
        ],
    ),
    case(
        "setup export",
        &[
            hook("setup-world", setup_home),
            read(
                "preview",
                &[
                    "setup",
                    "export",
                    "--out",
                    "{base}/out",
                    "--roots",
                    "{home}/code",
                    "--dry-run",
                ],
            ),
            read(
                "peek",
                &[
                    "setup",
                    "export",
                    "--parts",
                    "repos",
                    "--peek",
                    "repos",
                    "--roots",
                    "{home}/code",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "setup",
                    "export",
                    "--out",
                    "{base}/out/setup.zip",
                    "--roots",
                    "{home}/code",
                ],
            ),
            apply(
                "again",
                &[
                    "setup",
                    "export",
                    "--out",
                    "{base}/out/setup.zip",
                    "--roots",
                    "{home}/code",
                ],
            )
            .exit(1),
            setup("vault", &["secret", "set", "alpha", "API_KEY"]).stdin(VAULTED),
            apply(
                "sealed",
                &[
                    "setup",
                    "export",
                    "--secrets",
                    "--sessions",
                    "--passphrase-stdin",
                    "--out",
                    "{base}/out/sealed.zip",
                    "--roots",
                    "{home}/code",
                ],
            )
            .stdin(PASSPHRASE),
            usage(
                "no-passphrase",
                &[
                    "setup",
                    "export",
                    "--secrets",
                    "--out",
                    "{base}/out/none.zip",
                ],
            ),
            usage(
                "short-passphrase",
                &[
                    "setup",
                    "export",
                    "--secrets",
                    "--passphrase-stdin",
                    "--out",
                    "{base}/out/short.zip",
                    "--roots",
                    "{home}/code",
                ],
            )
            .stdin("short\n"),
            usage(
                "bad-part",
                &["setup", "export", "--parts", "everything", "--dry-run"],
            ),
            usage("no-out", &["setup", "export", "--roots", "{home}/code"]),
        ],
    ),
    case(
        "skills status",
        &[
            read("", &["skills", "status", "--repo", "{repo}"]),
            read(
                "unknown_client",
                &["skills", "status", "--repo", "{repo}", "--client", "cursor"],
            )
            .exit(1),
        ],
    ),
    case(
        "skills sync",
        &[
            read(
                "preview",
                &["skills", "sync", "--repo", "{repo}", "--dry-run"],
            ),
            apply("apply", &["skills", "sync", "--repo", "{repo}"]),
            read(
                "unattended_preview",
                &[
                    "skills",
                    "sync",
                    "--repo",
                    "{repo}",
                    "--dry-run",
                    "--unattended",
                ],
            ),
            apply(
                "unattended",
                &["skills", "sync", "--repo", "{repo}", "--unattended"],
            ),
            setup("pause", &["skills", "auto", "pause"]),
            read(
                "unattended_paused",
                &["skills", "sync", "--repo", "{repo}", "--unattended"],
            ),
            usage(
                "unattended_apply_held",
                &[
                    "skills",
                    "sync",
                    "--repo",
                    "{repo}",
                    "--unattended",
                    "--apply-held",
                ],
            ),
            read(
                "unknown_client",
                &[
                    "skills",
                    "sync",
                    "--repo",
                    "{repo}",
                    "--client",
                    "cursor",
                    "--dry-run",
                ],
            )
            .exit(1),
            hook("files", checks_files),
            hook("differs", differing_setup),
            read(
                "take_preview",
                &[
                    "skills",
                    "sync",
                    "--repo",
                    "{repo}",
                    "--client",
                    "claude-code",
                    "--dry-run",
                    "--only",
                    "acceptance",
                    "--take",
                    "acceptance/SETUP.md",
                ],
            ),
            apply(
                "take",
                &[
                    "skills",
                    "sync",
                    "--repo",
                    "{repo}",
                    "--client",
                    "claude-code",
                    "--only",
                    "acceptance",
                    "--take",
                    "acceptance/SETUP.md",
                ],
            ),
        ],
    ),
    case(
        "skills undo",
        &[
            read("none", &["skills", "undo", "--dry-run"]).exit(1),
            setup("sync", &["skills", "sync", "--repo", "{repo}"]),
            read("preview", &["skills", "undo", "--dry-run"]),
            apply("apply", &["skills", "undo"]),
            read("again", &["skills", "undo", "--dry-run"]).exit(1),
        ],
    ),
    case(
        "skills verify",
        &[
            read("none", &["skills", "verify", "--repo", "{repo}"]).exit(1),
            setup("sync", &["skills", "sync", "--repo", "{repo}"]),
            read("clean", &["skills", "verify", "--repo", "{repo}"]),
            hook("tamper", tamper_with_installed_skill),
            read("mismatch", &["skills", "verify", "--repo", "{repo}"]).exit(1),
            read(
                "unknown_client",
                &["skills", "verify", "--repo", "{repo}", "--client", "cursor"],
            )
            .exit(1),
        ],
    ),
    case(
        "skills auto status",
        &[
            read("off", &["skills", "auto", "status"]),
            setup("switch_on", &["skills", "auto", "on"]),
            read("on", &["skills", "auto", "status"]),
            setup(
                "sync",
                &["skills", "sync", "--repo", "{repo}", "--unattended"],
            ),
            read("last_run", &["skills", "auto", "status"]),
            setup("pause", &["skills", "auto", "pause"]),
            read("paused", &["skills", "auto", "status"]),
            usage("usage", &["skills", "auto", "status", "extra"]),
        ],
    ),
    case(
        "skills auto on",
        &[
            read("preview", &["skills", "auto", "on", "--dry-run"]),
            apply("apply", &["skills", "auto", "on"]),
            apply("again", &["skills", "auto", "on"]),
            usage("usage", &["skills", "auto", "on", "extra"]),
        ],
    ),
    case(
        "skills auto off",
        &[
            read("already_off", &["skills", "auto", "off", "--dry-run"]),
            setup("on", &["skills", "auto", "on"]),
            read("preview", &["skills", "auto", "off", "--dry-run"]),
            apply("apply", &["skills", "auto", "off"]),
            usage("usage", &["skills", "auto", "off", "extra"]),
        ],
    ),
    case(
        "skills auto pause",
        &[
            apply("pause", &["skills", "auto", "pause"]),
            apply("again", &["skills", "auto", "pause"]),
        ],
    ),
    case(
        "skills auto resume",
        &[
            apply("not_paused", &["skills", "auto", "resume"]),
            setup("pause", &["skills", "auto", "pause"]),
            apply("resume", &["skills", "auto", "resume"]),
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
        &[
            read("none", &["skills", "tap", "update", "--dry-run"]),
            hook("repos", taps_world::both),
            setup(
                "setup",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/plugin-src",
                    "--name",
                    "plugins",
                ],
            ),
            setup("link", &["skills", "tap", "link", "plugins"]),
            apply("linked", &["skills", "tap", "update", "plugins"]).exit(1),
        ],
    ),
    case(
        "skills tap link",
        &[
            apply("unknown", &["skills", "tap", "link", "local"]).exit(1),
            hook("repos", taps_world::both),
            setup(
                "setup",
                &["skills", "tap", "add", "{base}/tap-src", "--name", "local"],
            ),
            apply("plain_tap", &["skills", "tap", "link", "local"]).exit(1),
            setup(
                "setup_plugins",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/plugin-src",
                    "--name",
                    "plugins",
                ],
            ),
            read("listed", &["skills", "tap", "ls"]),
            read(
                "preview",
                &["skills", "tap", "link", "plugins", "--dry-run"],
            ),
            apply("apply", &["skills", "tap", "link", "plugins"]),
            apply("again", &["skills", "tap", "link", "plugins"]),
            read("linked", &["skills", "tap", "ls"]),
            apply("remove_refused", &["skills", "tap", "remove", "plugins"]).exit(1),
            apply(
                "unknown_plugin",
                &["skills", "tap", "link", "plugins", "--plugin", "nope"],
            )
            .exit(1),
            usage("usage", &["skills", "tap", "link"]),
        ],
    ),
    case(
        "skills tap unlink",
        &[
            apply("unknown", &["skills", "tap", "unlink", "local"]).exit(1),
            hook("repos", taps_world::both),
            setup(
                "setup",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/plugin-src",
                    "--name",
                    "plugins",
                ],
            ),
            read(
                "not_linked",
                &["skills", "tap", "unlink", "plugins", "--dry-run"],
            ),
            setup("link", &["skills", "tap", "link", "plugins"]),
            read(
                "preview",
                &["skills", "tap", "unlink", "plugins", "--dry-run"],
            ),
            apply("apply", &["skills", "tap", "unlink", "plugins"]),
            read("listed", &["skills", "tap", "ls"]),
            usage("usage", &["skills", "tap", "unlink"]),
        ],
    ),
    case(
        "library select ls",
        &[
            read("empty", &["library", "select", "ls"]),
            apply("unknown", &["library", "select", "ls", "nope"]).exit(1),
            hook("repos", taps_world::select_src),
            setup(
                "setup",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/select-src",
                    "--name",
                    "picks",
                ],
            ),
            read("items", &["library", "select", "ls", "picks"]),
            setup(
                "choose",
                &["library", "select", "set", "picks", "--on", "code-review"],
            ),
            read("listed", &["library", "select", "ls"]),
        ],
    ),
    case(
        "library select set",
        &[
            hook("repos", taps_world::select_src),
            setup(
                "setup",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/select-src",
                    "--name",
                    "picks",
                ],
            ),
            usage("usage", &["library", "select", "set", "picks"]),
            apply(
                "unknown",
                &["library", "select", "set", "nope", "--on", "x"],
            )
            .exit(1),
            apply(
                "unknown_item",
                &["library", "select", "set", "picks", "--on", "nope"],
            )
            .exit(1),
            read(
                "preview",
                &[
                    "library",
                    "select",
                    "set",
                    "picks",
                    "--on",
                    "code-review",
                    "--dry-run",
                ],
            ),
            apply(
                "hook_needs_a_tick",
                &[
                    "library",
                    "select",
                    "set",
                    "picks",
                    "--on",
                    "PostToolUse:Write|Edit",
                ],
            )
            .exit(2),
            apply(
                "apply",
                &[
                    "library",
                    "select",
                    "set",
                    "picks",
                    "--on",
                    "code-review,agent:reviewer,deploy,style",
                    "--tick-hook",
                    "PostToolUse:Write|Edit",
                ],
            ),
            apply(
                "off",
                &["library", "select", "set", "picks", "--off", "deploy"],
            ),
        ],
    ),
    case(
        "library select plan",
        &[
            usage("usage", &["library", "select", "plan"]),
            hook("repos", taps_world::select_src),
            setup(
                "setup",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/select-src",
                    "--name",
                    "picks",
                ],
            ),
            setup(
                "choose",
                &[
                    "library",
                    "select",
                    "set",
                    "picks",
                    "--on",
                    "code-review,agent:reviewer",
                    "--tick-hook",
                    "PostToolUse:Write|Edit",
                ],
            ),
            read("plan", &["library", "select", "plan", "picks"]),
            read(
                "local",
                &["library", "select", "plan", "picks", "--no-fetch"],
            ),
        ],
    ),
    case(
        "library select apply",
        &[
            usage("usage", &["library", "select", "apply", "picks"]),
            hook("repos", taps_world::select_src),
            setup(
                "setup",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/select-src",
                    "--name",
                    "picks",
                ],
            ),
            setup(
                "choose",
                &[
                    "library",
                    "select",
                    "set",
                    "picks",
                    "--on",
                    "code-review,agent:reviewer",
                    "--tick-hook",
                    "PostToolUse:Write|Edit",
                ],
            ),
            apply(
                "stale_plan",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:0000000000000000",
                ],
            )
            .exit(1),
            apply(
                "apply",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:1ec014b1a33f82fe",
                ],
            ),
            read("again", &["library", "select", "plan", "picks"]),
            hook("library", taps_world::library_with_style),
            setup(
                "rules_choose",
                &["library", "select", "set", "picks", "--on", "style"],
            ),
            read("rules_conflict", &["library", "select", "plan", "picks"]),
            apply(
                "rules_needs_a_choice",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:afb479b886a4d993",
                ],
            )
            .exit(1),
            apply(
                "rules_unknown_choice",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:afb479b886a4d993",
                    "--skip",
                    "nope",
                ],
            )
            .exit(1),
            apply(
                "rules_skip",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:afb479b886a4d993",
                    "--skip",
                    "style",
                ],
            ),
            apply("rules_undo", &["library", "select", "undo", "picks"]),
            apply(
                "rules_force",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:afb479b886a4d993",
                    "--force",
                    "style",
                ],
            ),
            setup(
                "rules_off",
                &["library", "select", "set", "picks", "--off", "style"],
            ),
            read("rules_remove", &["library", "select", "plan", "picks"]),
            apply(
                "rules_removed",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:c8f5d4ff7263d84a",
                ],
            ),
        ],
    ),
    case(
        "library select undo",
        &[
            usage("usage", &["library", "select", "undo"]),
            hook("repos", taps_world::select_src),
            setup(
                "setup",
                &[
                    "skills",
                    "tap",
                    "add",
                    "{base}/select-src",
                    "--name",
                    "picks",
                ],
            ),
            apply("nothing", &["library", "select", "undo", "picks"]).exit(1),
            setup(
                "choose",
                &["library", "select", "set", "picks", "--on", "code-review"],
            ),
            setup(
                "apply",
                &[
                    "library",
                    "select",
                    "apply",
                    "picks",
                    "--plan-hash",
                    "sha256:2301a9e8f4612e06",
                ],
            ),
            apply("undo", &["library", "select", "undo", "picks"]),
        ],
    ),
    case(
        "library select check",
        &[
            usage("usage", &["library", "select", "check", "--bogus"]),
            apply("unknown", &["library", "select", "check", "picks"]).exit(1),
            apply("none", &["library", "select", "check", "--no-fetch"]),
            apply(
                "deepen",
                &["library", "select", "check", "--no-fetch", "--deepen"],
            ),
        ],
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
        "catalog status",
        &[
            read("empty", &["catalog", "status"]),
            hook("indexed-world", catalog_world::indexed),
            read("indexed", &["catalog", "status"]),
            hook("stopped-world", catalog_world::stopped),
            read("stopped", &["catalog", "status"]),
            usage("usage", &["catalog", "status", "extra"]),
        ],
    ),
    case(
        "catalog sync",
        &[usage("usage", &["catalog", "sync", "extra"])],
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
        "library open",
        &[
            hook("library-world", library_home),
            read("library", &["library", "open"]),
            read("skill", &["library", "open", "review"]),
            read("missing", &["library", "open", "nope"]).exit(1),
            read("traversal", &["library", "open", "../x"]).exit(2),
            usage("usage", &["library", "open", "a", "b"]),
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
        "library connect",
        &[
            hook("remote-world", library_world::remote_only),
            read(
                "preview",
                &[
                    "library",
                    "connect",
                    "{base}/lib-remote.git",
                    "--clone-to",
                    "{home}/lib/ai-skills",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &[
                    "library",
                    "connect",
                    "{base}/lib-remote.git",
                    "--clone-to",
                    "{home}/lib/ai-skills",
                ],
            ),
            read(
                "again",
                &[
                    "library",
                    "connect",
                    "{base}/lib-remote.git",
                    "--clone-to",
                    "{home}/lib/other",
                    "--dry-run",
                ],
            )
            .exit(1),
            usage("usage", &["library", "connect"]),
            usage(
                "credential",
                &[
                    "library",
                    "connect",
                    "https://user:pass@example.invalid/o/skills.git",
                ],
            ),
        ],
    ),
    case(
        "library remote add",
        &[
            hook("library-world", library_home),
            hook("no-remote-world", library_world::no_remote),
            read(
                "preview",
                &[
                    "library",
                    "remote",
                    "add",
                    "{base}/lib-remote.git",
                    "--dry-run",
                ],
            ),
            apply(
                "apply",
                &["library", "remote", "add", "{base}/lib-remote.git"],
            ),
            read(
                "again",
                &["library", "remote", "add", "{base}/lib-remote.git"],
            )
            .exit(1),
            usage("usage", &["library", "remote", "add"]),
        ],
    ),
    case(
        "library commit",
        &[
            hook("library-world", library_home),
            read("clean", &["library", "commit", "-m", "Nothing to do"]),
            hook("dirty-world", library_world::dirty),
            read(
                "preview",
                &["library", "commit", "-m", "Update skills", "--dry-run"],
            ),
            apply("apply", &["library", "commit", "-m", "Update skills"]),
            hook("secret-world", library_world::secret_dirty),
            read(
                "secret_preview",
                &["library", "commit", "-m", "Add leaky", "--dry-run"],
            ),
            apply("secret", &["library", "commit", "-m", "Add leaky"]).exit(1),
            usage("no_message", &["library", "commit"]),
            usage("extra", &["library", "commit", "-m", "x", "extra"]),
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
            hook("no-remote-world", library_world::no_remote),
            read("no_remote_preview", &["library", "push", "--dry-run"]),
            apply("no_remote", &["library", "push"]).exit(1),
            hook("empty-remote-world", library_world::empty_remote),
            read("first_preview", &["library", "push", "--dry-run"]),
            apply("first", &["library", "push"]),
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
            read(
                "unknown_client",
                &["skills", "clean", "--repo", "{repo}", "--client", "cursor"],
            )
            .exit(1),
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
            hook("lose", lose_hooks),
            read("lost", &["compression", "hooks", "status"]),
            read(
                "preview-reapply",
                &["compression", "hooks", "apply", "--dry-run"],
            ),
            apply("reapply", &["compression", "hooks", "apply"]),
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
            apply("for", &["compression", "hooks", "pause", "--for", "1d"]),
            read("paused-for", &["compression", "hooks", "status"]),
            usage(
                "until-too-far",
                &["compression", "hooks", "pause", "--until", "2099-01-01"],
            ),
            usage(
                "both",
                &[
                    "compression",
                    "hooks",
                    "pause",
                    "--for",
                    "1d",
                    "--until",
                    "2099-01-01",
                ],
            ),
            usage(
                "bad-duration",
                &["compression", "hooks", "pause", "--for", "soon"],
            ),
            usage(
                "too-long",
                &["compression", "hooks", "pause", "--for", "31d"],
            ),
            usage(
                "past",
                &["compression", "hooks", "pause", "--until", "2020-01-01"],
            ),
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
        "compression hooks adopt-edit",
        &[
            hook("home", rtk_home),
            setup("setup", &["compression", "hooks", "apply"]),
            hook("edit", edit_wrapped_hook),
            read("edited", &["compression", "hooks", "status"]),
            apply("apply", &["compression", "hooks", "adopt-edit", "rtk"]),
            read("adopted", &["compression", "hooks", "status"]),
            apply(
                "revert",
                &["compression", "hooks", "adopt-edit", "rtk", "--revert"],
            ),
            read("reverted", &["compression", "hooks", "status"]),
            apply("none", &["compression", "hooks", "adopt-edit", "read-once"]).exit(1),
            usage("unknown", &["compression", "hooks", "adopt-edit", "nope"]),
            usage("usage", &["compression", "hooks", "adopt-edit"]),
        ],
    ),
    case(
        "compression hooks verify",
        &[
            hook("home", fixture_hooks),
            read("foreign", &["compression", "hooks", "verify"]),
            setup("wrap", &["compression", "hooks", "apply"]),
            read("wrapped", &["compression", "hooks", "verify"]),
            read("one", &["compression", "hooks", "verify", "rtk"]),
            read("counted", &["compression", "hooks", "status"]),
            hook("silent", silent_hooks),
            read("failing", &["compression", "hooks", "verify"]).exit(1),
            usage("unknown", &["compression", "hooks", "verify", "nope"]),
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
        "compression measure",
        &[
            hook("world", measure_world),
            read(
                "no-transcripts",
                &["compression", "measure", "--transcripts", "{base}/none"],
            ),
            read(
                "measured",
                &[
                    "compression",
                    "measure",
                    "--transcripts",
                    "{home}/.claude/projects",
                ],
            ),
            read(
                "since",
                &[
                    "compression",
                    "measure",
                    "--transcripts",
                    "{home}/.claude/projects",
                    "--since",
                    "2026-11-10",
                    "--min-turns",
                    "3",
                ],
            ),
            usage(
                "bad-since",
                &["compression", "measure", "--since", "last week"],
            ),
            read(
                "csv",
                &[
                    "compression",
                    "measure",
                    "--transcripts",
                    "{home}/.claude/projects",
                    "--format",
                    "csv",
                ],
            ),
            apply(
                "csv-saved",
                &[
                    "compression",
                    "measure",
                    "--transcripts",
                    "{home}/.claude/projects",
                    "--format",
                    "csv",
                    "--out",
                    "{home}/export/measure.csv",
                ],
            ),
            usage("bad-format", &["compression", "measure", "--format", "xml"]),
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

fn tamper_with_installed_skill(world: &World) {
    let skill = world.home.join(".claude/skills/demo/SKILL.md");
    std::fs::write(skill, "edited by hand\n").unwrap();
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
            let value = json!({
                "argv": step.argv,
                "exitCode": step.exit,
                "envelope": scrub(&normalize::normalize(&world, &envelope)),
            });
            if stem == "commands" {
                golden::assert_commands_golden(&value);
            } else {
                golden::assert_golden(&stem, &value);
            }
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
        let stem = golden::stem_of_file(&name);
        assert!(owned.contains(stem), "golden {name} belongs to no case");
    }
}

#[test]
fn the_registry_lists_tools_and_commands_in_table_order() {
    let data = registry();
    let tools: Vec<&str> = data["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    let table: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
    assert_eq!(tools, table);

    let heads: Vec<String> = COMMANDS.iter().map(|c| c.path.join(" ")).collect();
    let mut next = 0;
    let mut head = "";
    for row in data["commands"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        if heads.get(next).is_some_and(|h| h == id) {
            head = id;
            next += 1;
        } else {
            assert!(
                !heads.iter().any(|h| h == id) && id.starts_with(&format!("{head} ")),
                "`{id}` does not follow its command `{head}` in table order"
            );
        }
    }
    assert_eq!(next, heads.len(), "every command row is listed once");
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
fn the_doctor_counts_the_tools_the_catalog_lists() {
    let world = World::new("doctor-count");
    let json = |argv: &[&str]| {
        let argv: Vec<String> = argv.iter().map(|w| w.to_string()).collect();
        let run = run_ctl(&world, &argv, None);
        serde_json::from_str::<Value>(run.stdout.trim()).unwrap()
    };
    json(&["mcp", "install"]);
    let doctor = json(&["mcp", "doctor"]);
    let detail = doctor["data"]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "catalog")
        .and_then(|check| check["detail"].as_str())
        .unwrap()
        .to_string();
    let listed = json(&["mcp", "tools"])["data"]["tools"]
        .as_array()
        .unwrap()
        .len();
    let resources = RESOURCES.len();
    assert_eq!(detail, format!("{listed} tools, {resources} resources"));
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
    for path in golden::json_files(&golden::root()) {
        let text = std::fs::read_to_string(&path).unwrap();
        for secret in [FAKE_SECRET, SECRET_KEY, VAULTED] {
            assert!(
                !text.contains(secret),
                "golden {} contains a secret value",
                path.display()
            );
        }
    }
}

#[test]
fn a_golden_that_is_being_blessed_is_never_read_half_written() {
    let dir = std::env::temp_dir()
        .join(format!("toolport-golden-{}", std::process::id()))
        .join("envelopes");
    let value = json!({"rows": vec!["a long enough row to span several writes"; 400]});
    golden::write_in(&dir, "status", &value);
    let stop = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut reads = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                assert_eq!(golden::read_in(&dir, "status"), Some(value.clone()));
                reads += 1;
            }
            reads
        });
        for _ in 0..300 {
            golden::write_in(&dir, "status", &value);
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0);
    });
    std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
}
